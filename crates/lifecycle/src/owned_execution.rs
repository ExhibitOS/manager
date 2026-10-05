// SPDX-License-Identifier: Apache-2.0
//! Existing registered source adapters under the borrowed exclusive trust fence.
use super::*;
#[path = "prepared_oci.rs"]
mod prepared_oci;
use crate::{LifecycleService, Status, maintenance::VerificationReceipt};
pub use prepared_oci::PreparedOciReceipt;
#[path = "source_stopped.rs"]
mod source_stopped;
pub use source_stopped::SourceStoppedReceipt;
#[path = "source_deployment.rs"]
mod source_deployment;
pub use source_deployment::SourceDeploymentReceipt;
#[path = "source_configuration.rs"]
mod source_configuration;
pub use source_configuration::NativeConfigurationReceipt;
#[path = "source_images.rs"]
mod source_images;
pub use source_images::SourceImageReceipt;
#[path = "source_database.rs"]
mod source_database;
pub use source_database::DatabaseSnapshotReceipt;
#[path = "source_inventory.rs"]
mod source_inventory;
pub use source_inventory::SourceInventoryReceipt;
#[path = "source_full_configuration.rs"]
mod source_full_configuration;
#[path = "source_image_bytes.rs"]
mod source_image_bytes;
pub use source_full_configuration::ConfigurationInventoryReceipt;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecoveryReceipt {
    pub inventory: SourceInventoryReceipt,
    pub configuration: ConfigurationInventoryReceipt,
    pub repeated_inventory: SourceInventoryReceipt,
    pub preflight_verified: bool,
    pub update_executed: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHostTrustReceipt {
    pub host: profile_backup::HostReceipt,
    pub trust: TrustCheckpointReceipt,
    pub source: SourceRecoveryReceipt,
    pub after_archive_inventory: SourceInventoryReceipt,
    pub after_archive_configuration: ConfigurationInventoryReceipt,
    pub external_volumes_saved: bool,
    pub live_authority_restored: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
/// Holds the Store borrow and both profile fences. No arbitrary root, controller
/// bootstrap, activation, journal transition, or externally supplied success flag.
pub struct ExecutionSession<'a> {
    store: &'a mut Store,
    source: LifecycleService,
    profile_identity: Metadata,
    source_identity: Metadata,
    registry: Vec<u8>,
    _session: profile_backup::ProfileSession,
}
impl Store {
    pub fn execution(&mut self) -> crate::Result<ExecutionSession<'_>> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        installations::private_directory(&self.profile)?;
        // Clone the existing open file description; never re-lock, downgrade or
        // explicitly unlock it. The Store borrow keeps the exclusive owner alive.
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| crate::err("UPDATE_FENCE_UNAVAILABLE"))?;
        let session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let (registry, bytes) = installations::load(&self.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| {
                if self.installation == "default" {
                    e.kind == "default"
                } else {
                    e.id == self.installation
                }
            })
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let intent = self
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if entry.id != intent.update.plan().source_instance {
            return Err(crate::err("UPDATE_SOURCE_MISMATCH"));
        }
        let root = installations::root(&self.profile, entry);
        installations::private_directory(&root)?;
        let profile_identity =
            fs::symlink_metadata(&self.profile).map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let source_identity =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let source = LifecycleService::open_retry_diagnostics(root)?;
        let lease = ExecutionSession {
            store: self,
            source,
            profile_identity,
            source_identity,
            registry: bytes,
            _session: session,
        };
        lease.check()?;
        Ok(lease)
    }
}
/// Opaque retained bytes for the executor. This is not a preflight or apply permit.
pub struct PreparedArtifact {
    staged: crate::signed_release::artifact::StagedArtifact,
    verified: VerifiedRelease,
    plan: crate::update::Plan,
    generation: u64,
    envelope: String,
    profile: PathBuf,
    profile_identity: Metadata,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedArtifactReceipt {
    pub operation_id: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub staged_path: PathBuf,
    pub trust_generation: u64,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
// Lifecycle log timestamps use milliseconds; signed release times are Unix seconds.
fn release_now() -> crate::Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| crate::err("UPDATE_CLOCK_UNVERIFIED"))
}
impl ExecutionSession<'_> {
    /// Stage the current intent's exact signed artifact under the borrowed execution
    /// fence. No caller policy/envelope/plan or success booleans are accepted.
    pub fn stage_prepared_artifact(
        &self,
        source: &Path,
        parent: &Path,
    ) -> crate::Result<PreparedArtifact> {
        let mut artifact = self.stage_prepared_artifact_at(source, parent, release_now()?)?;
        self.reverify_prepared_artifact(&mut artifact)?;
        Ok(artifact)
    }
    fn stage_prepared_artifact_at(
        &self,
        source: &Path,
        parent: &Path,
        now: u64,
    ) -> crate::Result<PreparedArtifact> {
        self.check()?;
        if parent.starts_with(&self.store.profile) || parent.starts_with(&self.store.root) {
            return Err(crate::err("UPDATE_ARTIFACT_UNAVAILABLE"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan().clone();
        let envelope = intent.envelope.clone();
        let mut verified = self
            .store
            .verify_for_preparation(envelope.as_bytes(), now)
            .map_err(|e| crate::err(e.code()))?;
        if !verified.binds(&plan)
            || verified.payload_sha256 != intent.payload_sha256
            || verified.key_id != intent.key_id
            || verified.release.artifact.sha256 != intent.artifact_sha256
        {
            return Err(crate::err("UPDATE_PLAN_MISMATCH"));
        }
        let _source_lock = self.source.lock()?;
        let staged = crate::signed_release::artifact::stage(&mut verified, source, parent, now)
            .map_err(|e| crate::err(e.code()))?;
        let mut result = PreparedArtifact {
            staged,
            verified,
            plan,
            generation: self.store.receipt().generation,
            envelope,
            profile: self.store.profile.clone(),
            profile_identity: self.profile_identity.clone(),
        };
        self.reverify_prepared_artifact_at(&mut result, now)?;
        self.check()?;
        Ok(result)
    }
    /// Revalidate current trust/time/plan and the retained handle before later use.
    /// A returned receipt deliberately never authorizes an Applying transition.
    pub fn reverify_prepared_artifact(
        &self,
        artifact: &mut PreparedArtifact,
    ) -> crate::Result<PreparedArtifactReceipt> {
        self.reverify_prepared_artifact_at(artifact, release_now()?)
    }
    fn reverify_prepared_artifact_at(
        &self,
        artifact: &mut PreparedArtifact,
        now: u64,
    ) -> crate::Result<PreparedArtifactReceipt> {
        self.check()?;
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if artifact.profile != self.store.profile
            || !identity(&artifact.profile_identity, &self.profile_identity)
            || intent.update.stage() != crate::update::Stage::Prepared
            || intent.update.plan() != &artifact.plan
            || intent.envelope != artifact.envelope
            || self.store.receipt().generation != artifact.generation
        {
            return Err(crate::err("UPDATE_OPERATION_STALE"));
        }
        let mut fresh = self
            .store
            .verify_for_preparation(intent.envelope.as_bytes(), now)
            .map_err(|e| crate::err(e.code()))?;
        if !fresh.binds(&artifact.plan)
            || fresh.payload_sha256 != artifact.verified.payload_sha256
            || fresh.key_id != artifact.verified.key_id
        {
            return Err(crate::err("UPDATE_PLAN_MISMATCH"));
        }
        artifact
            .staged
            .reverify(&mut fresh, now)
            .map_err(|e| crate::err(e.code()))?;
        self.check()?;
        artifact.verified = fresh;
        Ok(PreparedArtifactReceipt {
            operation_id: artifact.plan.operation_id.clone(),
            artifact_sha256: artifact.verified.release.artifact.sha256.clone(),
            artifact_bytes: artifact.verified.release.artifact.bytes,
            staged_path: artifact.staged.path().to_owned(),
            trust_generation: artifact.generation,
            preflight_verified: false,
            update_executed: false,
        })
    }
    fn check(&self) -> crate::Result<()> {
        self.store.check_root().map_err(|e| crate::err(e.code()))?;
        installations::private_directory(&self.store.profile)?;
        installations::private_directory(&self.source.root)?;
        let profile = fs::symlink_metadata(&self.store.profile)
            .map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let source = fs::symlink_metadata(&self.source.root)
            .map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let (_, bytes) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        if !identity(&profile, &self.profile_identity)
            || !identity(&source, &self.source_identity)
            || bytes != self.registry
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        Ok(())
    }
    /// Observed service status, not plan-bound update health or preflight proof.
    pub fn source_status(&self) -> crate::Result<Status> {
        self.check()?;
        let result = {
            let _lock = self.source.lock()?;
            self.source.status()
        };
        self.check()?;
        result
    }
    /// Fresh direct engine observations; not a current data snapshot or full preflight.
    pub fn verify_source_stopped(
        &self,
        external_writers_quiesced: bool,
    ) -> crate::Result<SourceStoppedReceipt> {
        self.check()?;
        if !external_writers_quiesced {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let _lock = self.source.lock()?;
        let result = source_stopped::observe(
            &self.source,
            self.store
                .intent()
                .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                .update
                .plan(),
        );
        self.check()?;
        result
    }
    /// Current host deployment bytes against the exact authenticated restored backup.
    /// DB/blob/config-volume equality and full preflight remain separate.
    pub fn verify_source_deployment(
        &self,
        acknowledged: bool,
    ) -> crate::Result<SourceDeploymentReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceDeploymentReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                files,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Current native freeze key only; full configuration/image/data gates remain.
    pub fn verify_source_configuration(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<NativeConfigurationReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let expected = source_configuration::expected(&raw)?;
            let observed =
                source_configuration::observe(image, &before.configuration_volume, &expected)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(NativeConfigurationReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                configuration_volume: before.configuration_volume,
                maintenance_image: image.into(),
                file: observed,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Complete supported configuration scope with newly exported current image bytes.
    pub fn verify_configuration_inventory(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_scoped(image, acknowledged, false)
    }
    /// Fresh full verification with an explicit successful-export retirement policy.
    /// Failure candidates and small verified image receipts remain private.
    pub fn verify_configuration_inventory_transient(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_at(image, acknowledged, false, None, false)
    }
    fn verify_configuration_inventory_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_at(image, acknowledged, roots_held, None, true)
    }
    fn verify_configuration_inventory_at(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
        export_parent: Option<&Path>,
        retain_images: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target_lock = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let manifest = source_full_configuration::scope(&raw)?;
            let expected = source_configuration::expected(&raw)?;
            let observed =
                source_configuration::observe(image, &before.configuration_volume, &expected)?;
            let mappings = source_images::observe(&self.source, &workspace, &raw)?;
            let inventory_bytes = crate::installation_backup::source_bytes(
                &workspace.join("configuration"),
                "manager-image-inventory.json",
                1048576,
                true,
            )?;
            let preserved = source_images::bound_inventory(
                &inventory_bytes,
                &manifest,
                &self.source.manifest()?,
            )?;
            let exported = export_parent
                .unwrap_or(&self.source.root)
                .join(format!("source-image-observation-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&exported).map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&exported, fs::Permissions::from_mode(0o700))
                    .map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
            }
            installations::private_directory(&exported)?;
            let images = source_image_bytes::export(&exported, &preserved)?;
            let generated = source_full_configuration::generated_inventory(&images, &manifest)?;
            if source_configuration::observe(image, &before.configuration_volume, &expected)?
                != observed
                || source_images::observe(&self.source, &workspace, &raw)? != mappings
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            let full_files = source_full_configuration::files(&files, observed, generated)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(ConfigurationInventoryReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                configuration_volume: before.configuration_volume,
                files: full_files,
                images,
                export_workspace: exported.to_string_lossy().into_owned(),
                image_archives_retained: true,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let mut receipt = result?;
        if !retain_images {
            source_image_bytes::retire_verified(
                Path::new(&receipt.export_workspace),
                &receipt.images,
            )?;
            receipt.image_archives_retained = false;
        }
        Ok(receipt)
    }
    /// DB/blob before and after full configuration/image bytes under the same
    /// source+registered-target locks. Does not authorize Applying or prove recovery.
    pub fn verify_source_recovery_bundle(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.verify_source_recovery_scoped(image, acknowledged, false, true)
    }
    /// Full DB/blob/configuration observation; successful fresh image exports are
    /// retired only after the repeated data inventory and all root checks pass.
    pub fn verify_source_recovery_transient(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.verify_source_recovery_scoped(image, acknowledged, false, false)
    }
    fn verify_source_recovery_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
        retain_images: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let plan = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
            .update
            .plan();
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&root)?;
        let original =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let inventory = self.verify_source_inventory_scoped(image, true, true)?;
        let mut configuration = self.verify_configuration_inventory_scoped(image, true, true)?;
        let repeated_inventory = self.verify_source_inventory_scoped(image, true, true)?;
        if inventory.source_content_sha256 != repeated_inventory.source_content_sha256
            || inventory.inventory.inventory_sha256 != repeated_inventory.inventory.inventory_sha256
            || inventory.inventory.schema_sha256 != repeated_inventory.inventory.schema_sha256
            || inventory.inventory.authenticated_manifest_sha256
                != configuration.authenticated_manifest_sha256
            || inventory.source_instance != configuration.source_instance
            || inventory.target_instance != configuration.target_instance
            || inventory.source_database_volume != repeated_inventory.source_database_volume
            || inventory.source_blob_volume != repeated_inventory.source_blob_volume
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &original,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        if !retain_images {
            source_image_bytes::retire_verified(
                Path::new(&configuration.export_workspace),
                &configuration.images,
            )?;
            configuration.image_archives_retained = false;
        }
        Ok(SourceRecoveryReceipt {
            inventory,
            configuration,
            repeated_inventory,
            preflight_verified: false,
            update_executed: false,
        })
    }
    /// Cooperative host/root fences span fresh source observations, host+trust
    /// archives and a final read-only source comparison. External volumes are
    /// observed against the existing backup, not newly archived by this method.
    /// The completed pre-archive observation retires its fresh image exports;
    /// final recovery exports remain retained outside the host profile.
    pub fn checkpoint_source_host_trust(
        &self,
        image: &str,
        key_file: &Path,
        target: &Path,
        external_writers_quiesced: bool,
        host_writers_stopped: bool,
    ) -> crate::Result<SourceHostTrustReceipt> {
        self.check()?;
        if !external_writers_quiesced {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !host_writers_stopped {
            return Err(crate::err("HOST_WRITER_ACK_REQUIRED"));
        }
        let parent = target
            .parent()
            .ok_or_else(|| crate::err("PROFILE_PATH_INVALID"))?;
        if !target.is_absolute()
            || fs::canonicalize(parent).ok().as_deref() != Some(parent)
            || target.starts_with(&self.store.profile)
            || target.starts_with(&self.store.root)
            || target.exists()
        {
            return Err(crate::err("PROFILE_PATH_INVALID"));
        }
        installations::private_directory(parent)?;
        if !key_file.is_absolute()
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file)
            || key_file.starts_with(&self.store.profile)
            || key_file.starts_with(&self.store.root)
        {
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let mut key = read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?;
        if key.len() != 32 {
            key.fill(0);
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let result = (|| {
            let stage = parent.join(format!(
                "pending-source-host-trust-{}",
                uuid::Uuid::new_v4()
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(&stage)
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            let source = std::cell::RefCell::new(None);
            let (host, (), (trust, source, after_archive_inventory, after_archive_configuration)) =
                profile_backup::checkpoint_host_borrowed(
                    &self.store.profile,
                    key_file,
                    &stage.join("host.bin"),
                    &self._session,
                    || {
                        *source.borrow_mut() =
                            Some(self.verify_source_recovery_scoped(image, true, true, false)?);
                        Ok(())
                    },
                    || {
                        self.check()?;
                        if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?
                            != key
                        {
                            return Err(crate::err("PROFILE_KEY_INVALID"));
                        }
                        let trust = self
                            .store
                            .archive_trust_checkpoint(
                                &stage.join("trust.bin"),
                                key.as_slice()
                                    .try_into()
                                    .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                            )
                            .map_err(|e| crate::err(e.code()))?;
                        let after = self.verify_source_inventory_scoped(image, true, true)?;
                        let source = source
                            .borrow_mut()
                            .take()
                            .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
                        let before = &source.repeated_inventory;
                        if before.source_content_sha256 != after.source_content_sha256
                            || before.inventory.inventory_sha256 != after.inventory.inventory_sha256
                            || before.inventory.schema_sha256 != after.inventory.schema_sha256
                            || before.inventory.authenticated_manifest_sha256
                                != after.inventory.authenticated_manifest_sha256
                            || before.source_instance != after.source_instance
                            || before.target_instance != after.target_instance
                            || before.source_database_volume != after.source_database_volume
                            || before.source_blob_volume != after.source_blob_volume
                        {
                            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
                        }
                        let after_configuration = self.verify_configuration_inventory_at(
                            image,
                            true,
                            true,
                            Some(&stage),
                            true,
                        )?;
                        let before_configuration = &source.configuration;
                        if before_configuration.configuration_volume
                            != after_configuration.configuration_volume
                            || serde_json::to_value(&before_configuration.files).ok()
                                != serde_json::to_value(&after_configuration.files).ok()
                            || serde_json::to_value(&before_configuration.images).ok()
                                != serde_json::to_value(&after_configuration.images).ok()
                            || before_configuration.authenticated_manifest_sha256
                                != after_configuration.authenticated_manifest_sha256
                        {
                            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
                        }
                        Ok((trust, source, after, after_configuration))
                    },
                )?;
            self.check()?;
            if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key {
                return Err(crate::err("PROFILE_KEY_INVALID"));
            }
            let mut after_archive_configuration = after_archive_configuration;
            let export_relative = Path::new(&after_archive_configuration.export_workspace)
                .strip_prefix(&stage)
                .map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
            after_archive_configuration.export_workspace =
                target.join(export_relative).to_string_lossy().into_owned();
            let receipt = SourceHostTrustReceipt {
                host,
                trust,
                source,
                after_archive_inventory,
                after_archive_configuration,
                external_volumes_saved: false,
                live_authority_restored: false,
                preflight_verified: false,
                update_executed: false,
            };
            let mut marker = private_file(&stage.join("verified.json"), true)
                .map_err(|e| crate::err(e.code()))?;
            marker
                .write_all(
                    &serde_json::to_vec(&receipt)
                        .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?,
                )
                .and_then(|_| marker.sync_all())
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            sync_dir(&stage).map_err(|e| crate::err(e.code()))?;
            publish(&stage, target).map_err(|e| crate::err(e.code()))?;
            sync_dir(parent).map_err(|e| crate::err(e.code()))?;
            Ok(receipt)
        })();
        key.fill(0);
        result
    }
    /// Retained physical stopped-source copy; no logical inventory or full preflight proof.
    pub fn snapshot_source_database(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<DatabaseSnapshotReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            if fs2::available_space(&self.source.root)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 4 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
            let (snapshot_volume, proof) = source_database::copy(image, &before.database_volume)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(DatabaseSnapshotReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                source_volume: before.database_volume,
                snapshot_volume,
                maintenance_image: image.into(),
                proof,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Current logical DB/blob inventory from isolated source snapshots; full preflight remains.
    pub fn verify_source_inventory(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceInventoryReceipt> {
        self.verify_source_inventory_scoped(image, acknowledged, false)
    }
    fn verify_source_inventory_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
    ) -> crate::Result<SourceInventoryReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target_lock = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            if fs2::available_space(&self.source.root)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 6 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
            let (snapshot_volume, proof) = source_database::copy(image, &before.database_volume)?;
            let manifest_path =
                crate::checked_path(&workspace.join("authenticated"), "manifest.json")?;
            let inventory = source_inventory::compare(
                image,
                &snapshot_volume,
                &before.blob_volume,
                &manifest_path,
                &receipt.authenticated_manifest_sha256,
            )?;
            source_inventory::matched(&inventory, plan)?;
            let (repeated_snapshot_volume, repeated_copy) =
                source_database::copy(image, &before.database_volume)?;
            if proof.content_sha256 != repeated_copy.content_sha256
                || proof.bytes != repeated_copy.bytes
                || proof.entries != repeated_copy.entries
                || proof.files != repeated_copy.files
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceInventoryReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                source_database_volume: before.database_volume,
                source_blob_volume: before.blob_volume,
                snapshot_volume,
                repeated_snapshot_volume,
                source_content_sha256: proof.content_sha256,
                inventory,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Fresh current Engine image references; not image byte or full preflight proof.
    pub fn verify_source_images(&self, acknowledged: bool) -> crate::Result<SourceImageReceipt> {
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
        crate::restoration::RestorationBinding::from_plan(plan)?;
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
        let identity_before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let observed = source_images::observe(&self.source, &workspace, &raw)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceImageReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                images: observed,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &identity_before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Restore the exact planned source backup into the already-registered fresh
    /// target. Prepared intent has reserved this target identity durably; no
    /// journal transition, signed-update activation or selection change occurs here.
    pub fn prepare_target_candidate(
        &self,
        image: &str,
        key: &Path,
        archive: &Path,
        port: u16,
        acknowledged: bool,
    ) -> crate::Result<crate::restoration::RestorationReceipt> {
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
        let before =
            fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        // Preserve no-overwrite precedence even when the source is unavailable.
        {
            let _lock = target.lock()?;
            crate::restoration::fresh_root(&root)?;
        }
        let before_source = source_stopped::observe(&self.source, plan)?;
        let result = target.restore_update_candidate(image, key, archive, port, &binding);
        let post_check = (|| -> crate::Result<()> {
            if let Ok(receipt) = &result {
                let _target_lock = target.lock()?;
                source_deployment::authenticated_files(
                    &self.source.root,
                    &root.join(format!("restore-{}", receipt.id)),
                    receipt,
                    plan,
                )?;
            }
            Ok(())
        })();
        let after_source = source_stopped::observe(&self.source, plan);
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let after_source = after_source?;
        if before_source.platform_container != after_source.platform_container
            || before_source.database_container != after_source.database_container
            || before_source.blob_volume != after_source.blob_volume
            || before_source.configuration_volume != after_source.configuration_volume
            || before_source.database_volume != after_source.database_volume
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        post_check?;
        result
    }
    /// Actual authenticated decryption, bound to the prepared manifest hash.
    /// Authentication alone cannot authorize update or prove complete restoration.
    pub fn verify_backup(
        &self,
        image: &str,
        key: &Path,
        archive: &Path,
    ) -> crate::Result<VerificationReceipt> {
        self.check()?;
        let result = self.source.verify_backup(image, key, archive);
        self.check()?;
        let receipt = result?;
        if receipt.authenticated_manifest_sha256
            != self
                .store
                .intent()
                .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                .update
                .plan()
                .backup_manifest
        {
            return Err(crate::err("UPDATE_BACKUP_MISMATCH"));
        }
        Ok(receipt)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{fixture, plan, seal};
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    const SOURCE: &str = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
    fn prepared(scope: &str) -> (PathBuf, Store) {
        prepared_with_budget(scope, 1024)
    }
    fn prepared_with_budget(scope: &str, budget: u64) -> (PathBuf, Store) {
        let (p, k, policy, r) = fixture();
        let mut s = Store::provision(&p, scope, policy, 10).unwrap();
        let e = seal(&k, &r);
        let mut v = s.verify_for_preparation(&e, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut plan = plan();
        plan.source_instance = SOURCE.into();
        plan.required_free_bytes = budget;
        plan.target_instance = "cc414c3c-dd99-45e2-8307-131a49f72d68".into();
        plan.backup_id = "45ec39e9-5c19-47e6-9aaf-176978521a73".into();
        s.prepare_update(&e, &v, plan, 20).unwrap();
        (p, s)
    }
    fn registered(p: &Path, id: &str, kind: &str) -> PathBuf {
        let mut entries = vec![installations::Entry {
            id: SOURCE.into(),
            kind: "default".into(),
            created_at: 0,
        }];
        if kind != "default" {
            entries.push(installations::Entry {
                id: id.into(),
                kind: kind.into(),
                created_at: 0,
            });
        } else {
            entries[0].id = id.into();
        }
        let r = installations::Registry {
            format: 1,
            active_id: entries[0].id.clone(),
            installations: entries,
        };
        let path = p.join("installation-selection.json");
        fs::write(&path, serde_json::to_vec(&r).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        let root = if kind == "default" {
            p.join("local-runtime")
        } else {
            fs::create_dir(p.join("installations")).unwrap();
            fs::set_permissions(p.join("installations"), fs::Permissions::from_mode(0o700))
                .unwrap();
            p.join("installations").join(id)
        };
        installations::new_directory(&root).unwrap();
        root
    }
    #[test]
    fn owned_artifact_rechecks_expiry_and_retained_path_without_transition() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-owned-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"fixture").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let original = store.current_sha256.clone();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&source, &parent, 21)
            .unwrap();
        let receipt = session
            .reverify_prepared_artifact_at(&mut artifact, 22)
            .unwrap();
        assert!(!receipt.preflight_verified && !receipt.update_executed);
        assert_eq!(
            session
                .reverify_prepared_artifact_at(&mut artifact, 100)
                .unwrap_err()
                .code,
            "UPDATE_RELEASE_EXPIRED"
        );
        let staged = artifact.staged.path().to_owned();
        fs::rename(&staged, staged.with_extension("original")).unwrap();
        fs::write(&staged, b"fixture").unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(
            session
                .reverify_prepared_artifact_at(&mut artifact, 22)
                .unwrap_err()
                .code,
            "UPDATE_ARTIFACT_MISMATCH"
        );
        drop(session);
        assert_eq!(store.current_sha256, original);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
    }
    #[test]
    fn identical_plan_in_another_profile_cannot_reuse_retained_artifact() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-scope-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"fixture").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&source, &parent, 21)
            .unwrap();
        drop(session);
        let (other, mut second) = prepared("default");
        registered(&other, SOURCE, "default");
        let second_session = second.execution().unwrap();
        assert_eq!(
            second_session
                .reverify_prepared_artifact_at(&mut artifact, 21)
                .unwrap_err()
                .code,
            "UPDATE_OPERATION_STALE"
        );
        assert_eq!(
            second_session.store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
    }
    #[test]
    fn owned_artifact_rejects_source_bytes_and_profile_staging() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-refusal-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"changed").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let original = store.current_sha256.clone();
        let session = store.execution().unwrap();
        assert_eq!(
            session
                .stage_prepared_artifact_at(&source, &p, 21)
                .err()
                .unwrap()
                .code,
            "UPDATE_ARTIFACT_UNAVAILABLE"
        );
        assert_eq!(
            session
                .stage_prepared_artifact_at(&source, &parent, 21)
                .err()
                .unwrap()
                .code,
            "UPDATE_ARTIFACT_MISMATCH"
        );
        drop(session);
        assert_eq!(store.current_sha256, original);
    }
    #[test]
    fn missing_registry_or_wrong_source_cannot_bootstrap_execution() {
        let (p, mut s) = prepared("default");
        assert_eq!(
            s.execution().err().unwrap().code,
            "UPDATE_SOURCE_UNREGISTERED"
        );
        assert!(!p.join("local-runtime").exists());
        registered(&p, "e7980e6c-cbae-454f-906b-1f4924cc2a4e", "default");
        assert_eq!(s.execution().err().unwrap().code, "UPDATE_SOURCE_MISMATCH");
        assert_eq!(s.receipt().generation, 2);
    }
    #[test]
    fn owned_fence_excludes_other_sessions_without_downgrade_and_keeps_prepared() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        {
            let lease = s.execution().unwrap();
            assert_eq!(
                profile_backup::session_lock(&p, false).err().unwrap().code,
                "PROFILE_BUSY"
            );
            assert!(matches!(Store::open(&p, "default"), Err(Error::TrustBusy)));
            assert!(!lease.source_status().unwrap().installed);
        }
        assert_eq!(
            profile_backup::session_lock(&p, false).err().unwrap().code,
            "PROFILE_BUSY"
        );
        assert_eq!(
            s.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        assert_eq!(s.receipt().generation, 2);
        drop(s);
        assert!(profile_backup::session_lock(&p, false).is_ok());
    }
    #[test]
    fn registry_and_source_replacements_invalidate_existing_lease() {
        for registry in [true, false] {
            let (p, mut s) = prepared("default");
            let root = registered(&p, SOURCE, "default");
            let lease = s.execution().unwrap();
            if registry {
                let path = p.join("installation-selection.json");
                let mut b = fs::read(&path).unwrap();
                b.push(b' ');
                fs::write(path, b).unwrap();
            } else {
                fs::rename(&root, p.join("retained-source")).unwrap();
                installations::new_directory(&root).unwrap();
            }
            assert_eq!(
                lease.source_status().unwrap_err().code,
                "UPDATE_SOURCE_CHANGED"
            );
        }
    }
    #[test]
    fn alias_and_unregistered_explicit_scope_are_rejected() {
        let (p, mut s) = prepared("default");
        let root = registered(&p, SOURCE, "default");
        let retained = p.join("retained-source");
        fs::rename(&root, &retained).unwrap();
        symlink(&retained, &root).unwrap();
        assert!(s.execution().is_err());
        let (p, mut s) = prepared("e7980e6c-cbae-454f-906b-1f4924cc2a4e");
        registered(&p, SOURCE, "default");
        assert_eq!(
            s.execution().err().unwrap().code,
            "UPDATE_SOURCE_UNREGISTERED"
        );
    }
    #[test]
    fn explicit_recovery_scope_does_not_change_active_selection() {
        let (p, mut s) = prepared(SOURCE);
        let root = registered(&p, SOURCE, "recovery");
        // The helper's duplicate default ID must be replaced by a distinct UUID.
        let path = p.join("installation-selection.json");
        let mut r: installations::Registry =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        r.installations[0].id = "e7980e6c-cbae-454f-906b-1f4924cc2a4e".into();
        r.active_id = r.installations[0].id.clone();
        fs::write(&path, serde_json::to_vec(&r).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(lease.source.root, root);
        assert!(!lease.source_status().unwrap().installed);
        assert_eq!(fs::read(path).unwrap(), before);
    }
    fn register_target(p: &Path) -> PathBuf {
        let (mut r, _) = installations::load(p).unwrap().unwrap();
        let id = "cc414c3c-dd99-45e2-8307-131a49f72d68";
        r.installations.push(installations::Entry {
            id: id.into(),
            kind: "recovery".into(),
            created_at: 0,
        });
        fs::write(
            p.join("installation-selection.json"),
            serde_json::to_vec(&r).unwrap(),
        )
        .unwrap();
        let parent = p.join("installations");
        installations::new_directory(&parent).unwrap();
        let target = parent.join(id);
        installations::new_directory(&target).unwrap();
        target
    }
    #[test]
    fn candidate_requires_ack_existing_registered_target_and_private_namespace() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert!(!p.join("installations").exists());
        drop(lease);
        assert_eq!(s.receipt().generation, 2);
        let target = register_target(&p);
        let retained = p.join("retained-target");
        fs::rename(&target, &retained).unwrap();
        symlink(&retained, &target).unwrap();
        let lease = s.execution().unwrap();
        assert!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .is_err()
        );
        assert_eq!(fs::read_dir(retained).unwrap().count(), 0);
    }
    #[test]
    fn candidate_preparation_rejects_applying_before_any_target_mutation() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        let envelope = s.intent().unwrap().envelope.as_bytes().to_vec();
        let mut v = s.verify_for_preparation(&envelope, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut observations = super::super::tests::observations();
        observations.plan = s.intent().unwrap().update.plan().clone();
        s.begin_update(observations, &v, 21).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .unwrap_err()
                .code,
            "UPDATE_CANDIDATE_STAGE_INVALID"
        );
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }
    #[test]
    fn existing_target_is_refused_before_an_unsatisfiable_new_operation_budget() {
        let (p, mut s) = prepared_with_budget("default", u64::MAX);
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        fs::write(target.join("retained-candidate"), b"keep").unwrap();
        let key = p.parent().unwrap().join("synthetic-test-key.bin");
        fs::write(&key, [7u8; 32]).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let archive = p.parent().unwrap().join("synthetic-archive");
        installations::new_directory(&archive).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate(
                    &format!("sha256:{}", "a".repeat(64)),
                    &key,
                    &archive,
                    13200,
                    true
                )
                .unwrap_err()
                .code,
            "RESTORE_FRESH_ROOT_REQUIRED"
        );
        assert_eq!(
            fs::read(target.join("retained-candidate")).unwrap(),
            b"keep"
        );
        assert!(!target.join("restoration.json").exists());
        drop(lease);
        assert_eq!(s.receipt().generation, 2);
    }
    #[test]
    fn fresh_target_registration_preserves_active_source_intent_and_existing_bytes() {
        let (p, store) = prepared("default");
        let root = registered(&p, SOURCE, "default");
        fs::write(root.join("original-witness"), b"keep source").unwrap();
        let trust = serde_json::to_vec(&store.receipt()).unwrap();
        let intent = serde_json::to_vec(&store.intent()).unwrap();
        assert_eq!(
            store.register_update_target(false).unwrap_err().code,
            "UPDATE_TARGET_ACK_REQUIRED"
        );
        assert!(!p.join("installations").exists());
        let first = store.register_update_target(true).unwrap();
        let second = store.register_update_target(true).unwrap();
        assert_eq!(first.source_instance, SOURCE);
        assert_eq!(first.active_instance, SOURCE);
        assert_ne!(first.target_instance, second.target_instance);
        assert!(!first.activated && !first.runtime_started);
        assert_eq!(
            fs::read(root.join("original-witness")).unwrap(),
            b"keep source"
        );
        assert_eq!(fs::read_dir(&first.target_path).unwrap().count(), 0);
        assert_eq!(serde_json::to_vec(&store.receipt()).unwrap(), trust);
        assert_eq!(serde_json::to_vec(&store.intent()).unwrap(), intent);
        let (registry, _) = installations::load(&p).unwrap().unwrap();
        assert_eq!(registry.active_id, SOURCE);
        assert_eq!(registry.installations.len(), 3);
    }
    #[test]
    fn fresh_registration_refuses_source_mismatch_and_aliased_namespace() {
        let (p, store) = prepared("default");
        registered(&p, "e7980e6c-cbae-454f-906b-1f4924cc2a4e", "default");
        assert_eq!(
            store.register_update_target(true).unwrap_err().code,
            "UPDATE_SOURCE_MISMATCH"
        );
        let (p, store) = prepared("default");
        registered(&p, SOURCE, "default");
        let other = p.parent().unwrap().join("retained-other");
        installations::new_directory(&other).unwrap();
        symlink(&other, p.join("installations")).unwrap();
        assert!(store.register_update_target(true).is_err());
        assert_eq!(fs::read_dir(other).unwrap().count(), 0);
    }
    #[test]
    fn integrated_checkpoint_refuses_both_missing_acknowledgements_before_writes() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let session = store.execution().unwrap();
        let destination = p.parent().unwrap().join("unpublished-pair");
        let key = p.parent().unwrap().join("absent-key");
        let image = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            session
                .checkpoint_source_host_trust(&image, &key, &destination, false, true)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .checkpoint_source_host_trust(&image, &key, &destination, true, false)
                .unwrap_err()
                .code,
            "HOST_WRITER_ACK_REQUIRED"
        );
        assert!(!destination.exists());
    }
    #[test]
    fn combined_recovery_refuses_missing_ack_and_candidate_without_engine_work() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let session = store.execution().unwrap();
        assert_eq!(
            session
                .verify_source_recovery_bundle(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_source_recovery_bundle(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert_eq!(
            session
                .verify_source_recovery_transient(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_source_recovery_transient(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        drop(session);
        assert_eq!(store.receipt().generation, 2);
    }
}

#[cfg(all(test, unix))]
mod source_stop_tests {
    use super::super::tests::{fixture, plan, seal};
    use super::*;
    #[test]
    fn missing_installed_source_and_ack_refuse_without_engine_bootstrap() {
        let (p, k, policy, r) = fixture();
        let mut store = Store::provision(&p, "default", policy, 10).unwrap();
        let envelope = seal(&k, &r);
        let mut v = store.verify_for_preparation(&envelope, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut planned = plan();
        let id = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
        planned.source_instance = id.into();
        store.prepare_update(&envelope, &v, planned, 20).unwrap();
        fs::write(p.join("installation-selection.json"),serde_json::to_vec(&serde_json::json!({"format":1,"activeId":id,"installations":[{"id":id,"kind":"default","createdAt":0}]})).unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            p.join("installation-selection.json"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let root = p.join("local-runtime");
        installations::new_directory(&root).unwrap();
        let lease = store.execution().unwrap();
        assert_eq!(
            lease.verify_source_stopped(false).unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease.verify_source_stopped(true).unwrap_err().code,
            "UPDATE_SOURCE_NOT_INSTALLED"
        );
        assert_eq!(
            lease
                .verify_source_configuration(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease
                .verify_source_configuration(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_RESTORE_BINDING_INVALID"
        );
        assert_eq!(
            lease.verify_source_images(false).unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease.verify_source_images(true).unwrap_err().code,
            "UPDATE_RESTORE_BINDING_INVALID"
        );
        assert!(!root.join("engine.json").exists());
        assert!(!root.join("installed.json").exists());
        drop(lease);
        assert_eq!(store.receipt().generation, 2);
    }
}
