// SPDX-License-Identifier: Apache-2.0
//! Current stopped recovery-candidate data, never cached restoration success.
use super::*;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateInventoryReceipt {
    pub source_instance: String,
    pub candidate_instance: String,
    pub candidate_bundle_id: String,
    pub candidate_database_volume: String,
    pub candidate_blob_volume: String,
    pub snapshot_volume: String,
    pub repeated_snapshot_volume: String,
    pub candidate_content_sha256: String,
    pub inventory: source_inventory::InventoryProof,
    pub observed_at: u64,
    pub configuration_verified: bool,
    pub image_bytes_verified: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
fn receipt_matches(
    r: &crate::restoration::RestorationReceipt,
    job: &str,
    m: &crate::BundleManifest,
    plan: &crate::update::Plan,
) -> crate::Result<()> {
    let proof = r
        .source_verification
        .as_ref()
        .ok_or_else(|| crate::err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
    if r.id != job
        || r.operation != "restored-and-running"
        || r.backup_id != plan.backup_id
        || r.authenticated_manifest_sha256 != plan.backup_manifest
        || r.bundle_id != m.bundle_id
        || r.project_name != m.project_name
        || r.open_url != m.open_url
        || !proof.valid()
        || proof.inventory_sha256 != plan.source_inventory
        || proof.schema_sha256 != plan.source_schema
        || proof.runtime_image_sha256 != plan.source_image
    {
        return Err(crate::err("UPDATE_CANDIDATE_PROOF_MISSING"));
    }
    Ok(())
}
fn same_stopped(a: &SourceStoppedReceipt, b: &SourceStoppedReceipt) -> bool {
    a.source_instance == b.source_instance
        && a.bundle_id == b.bundle_id
        && a.platform_container == b.platform_container
        && a.database_container == b.database_container
        && a.runtime_image_sha256 == b.runtime_image_sha256
        && a.database_volume == b.database_volume
        && a.blob_volume == b.blob_volume
        && a.configuration_volume == b.configuration_volume
}
fn isolated(a: &SourceStoppedReceipt, b: &SourceStoppedReceipt) -> bool {
    let original = [
        a.database_volume.as_str(),
        a.blob_volume.as_str(),
        a.configuration_volume.as_str(),
    ];
    let candidate = [
        b.database_volume.as_str(),
        b.blob_volume.as_str(),
        b.configuration_volume.as_str(),
    ];
    a.bundle_id != b.bundle_id
        && a.source_instance != b.source_instance
        && a.platform_container != b.platform_container
        && a.database_container != b.database_container
        && !candidate.iter().any(|name| original.contains(name))
}
pub(super) fn copies_match(
    a: &source_database::DatabaseCopyProof,
    b: &source_database::DatabaseCopyProof,
) -> bool {
    a.system_identifier == b.system_identifier
        && a.content_sha256 == b.content_sha256
        && a.bytes == b.bytes
        && a.files == b.files
        && a.entries == b.entries
        && a.postgres_major == b.postgres_major
        && a.pgdata == b.pgdata
        && a.clean_shutdown
        && b.clean_shutdown
}
pub(super) fn observe_copy(
    ctx: &CandidateContext<'_>,
    image: &str,
) -> crate::Result<(
    String,
    source_database::DatabaseCopyProof,
    source_inventory::InventoryProof,
)> {
    let (volume, physical) = source_database::copy(image, &ctx.candidate_before.database_volume)?;
    let logical = source_inventory::compare_candidate(
        image,
        &volume,
        &ctx.candidate_before.blob_volume,
        ctx.manifest_path,
        &ctx.receipt.authenticated_manifest_sha256,
        &physical.system_identifier,
    )?;
    source_inventory::matched_candidate(&logical, ctx.plan)?;
    Ok((volume, physical, logical))
}
pub(super) struct CandidateContext<'a> {
    pub target: &'a LifecycleService,
    pub root: &'a Path,
    pub workspace: &'a Path,
    pub raw: &'a [u8],
    pub manifest_path: &'a Path,
    pub manifest: &'a crate::BundleManifest,
    pub receipt: &'a crate::restoration::RestorationReceipt,
    pub plan: &'a crate::update::Plan,
    pub source_before: &'a SourceStoppedReceipt,
    pub candidate_before: &'a SourceStoppedReceipt,
}
impl ExecutionSession<'_> {
    pub(super) fn inspect_restored_candidate<T>(
        &self,
        acknowledged: bool,
        work: impl FnOnce(&CandidateContext<'_>) -> crate::Result<T>,
    ) -> crate::Result<T> {
        self.inspect_restored_candidate_finalized(acknowledged, work, |_, _| Ok(()))
    }
    /// The finalizer runs only after all common checks, while both operation
    /// locks and the parent exclusive profile/trust session are still held.
    pub(super) fn inspect_restored_candidate_finalized<T>(
        &self,
        acknowledged: bool,
        work: impl FnOnce(&CandidateContext<'_>) -> crate::Result<T>,
        finalize: impl FnOnce(&CandidateContext<'_>, &mut T) -> crate::Result<()>,
    ) -> crate::Result<T> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source = self.source.lock()?;
        let _target = target.lock()?;
        let root_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let job = target
            .restoration_status()?
            .filter(|j| j.state == "completed")
            .ok_or_else(|| crate::err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
        let workspace = root.join(format!("restore-{}", job.id));
        let receipt_path = crate::checked_path(&workspace, "receipt.json")?;
        let receipt_bytes = crate::installation_backup::source_bytes(
            &workspace,
            "receipt.json",
            1024 * 1024,
            true,
        )?;
        let receipt: crate::restoration::RestorationReceipt =
            serde_json::from_slice(&receipt_bytes)
                .map_err(|_| crate::err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
        let manifest = target.manifest()?;
        receipt_matches(&receipt, &job.id, &manifest, plan)?;
        let authenticated = workspace.join("authenticated");
        let raw = crate::installation_backup::source_bytes(
            &authenticated,
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )?;
        binding.authenticated(
            &receipt.backup_id,
            &receipt.authenticated_manifest_sha256,
            &raw,
        )?;
        let manifest_path = crate::checked_path(&authenticated, "manifest.json")?;
        let source_before = source_stopped::observe(&self.source, plan)?;
        let mut candidate_plan = plan.clone();
        candidate_plan.source_instance = plan.target_instance.clone();
        let candidate_before = source_stopped::observe(&target, &candidate_plan)?;
        if !isolated(&source_before, &candidate_before) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let context = CandidateContext {
            target: &target,
            root: &root,
            workspace: &workspace,
            raw: &raw,
            manifest_path: &manifest_path,
            manifest: &manifest,
            receipt: &receipt,
            plan,
            source_before: &source_before,
            candidate_before: &candidate_before,
        };
        let mut result = work(&context)?;
        let source_after = source_stopped::observe(&self.source, plan)?;
        let candidate_after = source_stopped::observe(&target, &candidate_plan)?;
        self.check()?;
        if !same_stopped(&source_before, &source_after)
            || !same_stopped(&candidate_before, &candidate_after)
            || !identity(
                &root_before,
                &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
            )
            || crate::installation_backup::source_bytes(
                &workspace,
                "receipt.json",
                1024 * 1024,
                true,
            )? != receipt_bytes
            || crate::installation_backup::source_bytes(
                &authenticated,
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )? != raw
            || crate::checked_path(&workspace, "receipt.json")? != receipt_path
            || serde_json::to_vec(&target.manifest()?)
                .map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?
                != serde_json::to_vec(&manifest).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        finalize(&context, &mut result)?;
        self.check()?;
        Ok(result)
    }
    /// Does not start/activate a candidate or trust a caller-supplied success flag.
    /// Both source and candidate must be stopped. Fresh copied DB volumes remain
    /// private evidence under the existing snapshot policy; originals are readonly.
    pub fn verify_restored_candidate_inventory(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<CandidateInventoryReceipt> {
        self.inspect_restored_candidate(acknowledged, |ctx| {
            let CandidateContext {
                root,
                manifest_path,
                receipt,
                plan,
                candidate_before,
                manifest,
                ..
            } = ctx;
            if fs2::available_space(root).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 12 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
            let (snapshot_volume, first) =
                source_database::copy(image, &candidate_before.database_volume)?;
            let inventory = source_inventory::compare_candidate(
                image,
                &snapshot_volume,
                &candidate_before.blob_volume,
                manifest_path,
                &receipt.authenticated_manifest_sha256,
                &first.system_identifier,
            )?;
            source_inventory::matched_candidate(&inventory, plan)?;
            let (repeated_snapshot_volume, repeated) =
                source_database::copy(image, &candidate_before.database_volume)?;
            if !copies_match(&first, &repeated) {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
            Ok(CandidateInventoryReceipt {
                source_instance: plan.source_instance.clone(),
                candidate_instance: plan.target_instance.clone(),
                candidate_bundle_id: manifest.bundle_id.clone(),
                candidate_database_volume: candidate_before.database_volume.clone(),
                candidate_blob_volume: candidate_before.blob_volume.clone(),
                snapshot_volume,
                repeated_snapshot_volume,
                candidate_content_sha256: first.content_sha256,
                inventory,
                observed_at: crate::now(),
                configuration_verified: false,
                image_bytes_verified: false,
                preflight_verified: false,
                update_executed: false,
            })
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn fixture() -> (
        crate::BundleManifest,
        crate::update::Plan,
        crate::restoration::RestorationReceipt,
    ) {
        let plan = super::super::super::tests::plan();
        let manifest = crate::BundleManifest {
            schema_version: "1".into(),
            bundle_id: "66b55f61-83fa-47dd-a591-8b70eeef3ab4".into(),
            version: "0.1.0".into(),
            protocol_version: "1".into(),
            compose_sha256: "a".repeat(64),
            preferred_engine: Some("docker".into()),
            project_name: "exhibitos-candidate".into(),
            services: vec!["database".into(), "platform".into()],
            images: vec![],
            ports: vec![13200],
            open_url: "http://127.0.0.1:13200".into(),
            readiness_url: "http://127.0.0.1:13200/api/v1/readiness".into(),
            minimum_free_bytes: 1,
        };
        let receipt = crate::restoration::RestorationReceipt {
            id: "test-job".into(),
            operation: "restored-and-running".into(),
            backup_id: plan.backup_id.clone(),
            authenticated_manifest_sha256: plan.backup_manifest.clone(),
            bundle_id: manifest.bundle_id.clone(),
            project_name: manifest.project_name.clone(),
            open_url: manifest.open_url.clone(),
            at: 1,
            source_verification: Some(crate::restoration::RestorationProof {
                inventory_sha256: plan.source_inventory.clone(),
                schema_sha256: plan.source_schema.clone(),
                runtime_image_sha256: plan.source_image.clone(),
            }),
        };
        (manifest, plan, receipt)
    }
    #[test]
    fn cached_restore_receipt_cannot_replace_exact_candidate_and_backup_provenance() {
        let (m, p, r) = fixture();
        assert!(receipt_matches(&r, "test-job", &m, &p).is_ok());
        let original = serde_json::to_vec(&r).unwrap();
        for field in [
            "id",
            "operation",
            "backup_id",
            "manifest",
            "bundle",
            "project",
            "url",
            "inventory",
            "schema",
            "image",
            "missing",
        ] {
            let mut changed: crate::restoration::RestorationReceipt =
                serde_json::from_slice(&original).unwrap();
            match field {
                "id" => changed.id = "other".into(),
                "operation" => changed.operation = "verified".into(),
                "backup_id" => changed.backup_id = "other".into(),
                "manifest" => changed.authenticated_manifest_sha256 = "0".repeat(64),
                "bundle" => changed.bundle_id = "other".into(),
                "project" => changed.project_name = "other".into(),
                "url" => changed.open_url = "http://127.0.0.1:13201".into(),
                "inventory" => {
                    changed
                        .source_verification
                        .as_mut()
                        .unwrap()
                        .inventory_sha256 = "0".repeat(64)
                }
                "schema" => {
                    changed.source_verification.as_mut().unwrap().schema_sha256 = "0".repeat(64)
                }
                "image" => {
                    changed
                        .source_verification
                        .as_mut()
                        .unwrap()
                        .runtime_image_sha256 = "0".repeat(64)
                }
                _ => changed.source_verification = None,
            }
            assert_eq!(
                receipt_matches(&changed, "test-job", &m, &p)
                    .unwrap_err()
                    .code,
                "UPDATE_CANDIDATE_PROOF_MISSING"
            );
        }
        assert_eq!(serde_json::to_vec(&r).unwrap(), original);
    }
    fn observation(instance: &str) -> SourceStoppedReceipt {
        SourceStoppedReceipt {
            source_instance: instance.into(),
            bundle_id: instance.into(),
            platform_container: format!("{instance}-app"),
            database_container: format!("{instance}-db"),
            runtime_image_sha256: "a".repeat(64),
            blob_volume: format!("{instance}-objects"),
            configuration_volume: format!("{instance}-config"),
            database_volume: format!("{instance}-data"),
            observed_at: 1,
        }
    }
    #[test]
    fn every_candidate_volume_is_separate_and_repeated_native_identity_is_exact() {
        let a = observation("source");
        let b = observation("candidate");
        assert!(isolated(&a, &b));
        assert!(!isolated(&a, &a));
        for shared in [&a.database_volume, &a.blob_volume, &a.configuration_volume] {
            let mut b = observation("candidate");
            b.database_volume = shared.clone();
            assert!(!isolated(&a, &b));
        }
        let mut repeated = observation("candidate");
        repeated.observed_at = 2;
        assert!(same_stopped(&b, &repeated));
        repeated.database_container = "replaced-db".into();
        assert!(!same_stopped(&b, &repeated));
    }
}
