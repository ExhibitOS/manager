// SPDX-License-Identifier: Apache-2.0
//! Original-plan encrypted restoration into its durably reserved fresh candidate.
use super::*;
use crate::{Result as LifecycleResult, err};
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRestorationReceipt {
    pub operation_id: String,
    pub candidate_id: String,
    pub authority_generation: u64,
    pub active_instance: String,
    pub restoration: crate::restoration::RestorationReceipt,
    pub original_selection_preserved: bool,
    pub rollback_completed: bool,
}
fn candidate(store: &Store, registry: &installations::Registry) -> LifecycleResult<String> {
    let i = store.intent().ok_or_else(|| err("UPDATE_INTENT_MISSING"))?;
    if i.update.stage() != crate::update::Stage::Restoring {
        return Err(err("UPDATE_ROLLBACK_STAGE_INVALID"));
    }
    let id = i
        .update
        .restore_candidate()
        .ok_or_else(|| err("UPDATE_ROLLBACK_CANDIDATE_MISSING"))?;
    let p = i.update.plan();
    if !installations::uuid(id)
        || id == p.source_instance
        || id == p.target_instance
        || !store.used_instances.contains(id)
        || !registry
            .installations
            .iter()
            .any(|e| e.id == id && e.kind == "recovery")
    {
        return Err(err("UPDATE_ROLLBACK_CANDIDATE_MISSING"));
    }
    let bound = store.bound_source_id(registry)?;
    if registry.active_id != bound || ![&p.source_instance, &p.target_instance].contains(&&bound) {
        return Err(err("UPDATE_SOURCE_MISMATCH"));
    }
    Ok(id.to_owned())
}
fn restore_receipt(
    plan: &crate::update::Plan,
    id: &str,
    r: &crate::restoration::RestorationReceipt,
) -> LifecycleResult<crate::update::RestoreReceipt> {
    if r.operation != "restored-and-running" || !installations::uuid(&r.id) {
        return Err(err("UPDATE_RESTORE_BINDING_MISMATCH"));
    }
    crate::restoration::RestorationBinding::from_plan(plan)?.check_receipt(r)?;
    Ok(crate::update::RestoreReceipt {
        operation_id: plan.operation_id.clone(),
        backup_id: plan.backup_id.clone(),
        backup_manifest: plan.backup_manifest.clone(),
        inventory_digest: plan.source_inventory.clone(),
        candidate_id: id.to_owned(),
        schema: plan.source_schema.clone(),
        inventory_verified: true,
        separate_candidate: true,
    })
}
fn reserve_space(root: &Path, archive: &Path, required: u64) -> LifecycleResult<()> {
    if !archive.is_absolute() || fs::canonicalize(archive).ok().as_deref() != Some(archive) {
        return Err(err("BACKUP_PATH_INVALID"));
    }
    let mut bytes = 0u64;
    let mut count = 0usize;
    let mut pending = vec![archive.to_owned()];
    while let Some(path) = pending.pop() {
        let m = fs::symlink_metadata(&path).map_err(|_| err("BACKUP_PATH_INVALID"))?;
        if m.file_type().is_symlink() {
            return Err(err("BACKUP_PATH_INVALID"));
        }
        if m.is_dir() {
            for e in fs::read_dir(path).map_err(|_| err("BACKUP_PATH_INVALID"))? {
                count += 1;
                if count > 20000 {
                    return Err(err("STORAGE_QUOTA"));
                }
                pending.push(e.map_err(|_| err("BACKUP_PATH_INVALID"))?.path());
            }
        } else if m.is_file() {
            use std::os::unix::fs::MetadataExt;
            if m.nlink() != 1 || m.uid() != unsafe { libc::geteuid() } {
                return Err(err("BACKUP_PATH_INVALID"));
            }
            bytes = bytes
                .checked_add(m.len())
                .ok_or_else(|| err("STORAGE_QUOTA"))?;
        } else {
            return Err(err("BACKUP_PATH_INVALID"));
        }
    }
    // Authentication/deployment image copies plus restore growth and6GiB floor.
    let peak = bytes
        .checked_mul(3)
        .and_then(|n| n.checked_add(required))
        .and_then(|n| n.checked_add(6 * 1024 * 1024 * 1024))
        .ok_or_else(|| err("STORAGE_QUOTA"))?;
    if fs2::available_space(root).map_err(|_| err("STORAGE_UNAVAILABLE"))? < peak {
        return Err(err("RESTORE_SPACE_REQUIRED"));
    }
    Ok(())
}
impl Store {
    /// Restore only the ID reserved by this live Store's BeginRestore. The exact
    /// original backup, schema, image and inventory come from its immutable plan.
    /// Holds profile/source/changed-target/new-candidate guards through the final
    /// authority publication. No selection activation or caller health evidence.
    pub fn restore_registered_rollback_candidate(
        &mut self,
        image: &str,
        key: &Path,
        archive: &Path,
        port: u16,
        external_writers_quiesced: bool,
    ) -> LifecycleResult<RollbackRestorationReceipt> {
        if !external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        self.check_root().map_err(|e| err(e.code()))?;
        self.require_authority_recovery()
            .map_err(|e| err(e.code()))?;
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| err("UPDATE_FENCE_UNAVAILABLE"))?;
        let _session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let controller = crate::LifecycleService::open_retry_diagnostics(self.profile.clone())?;
        let _profile = installations::profile_lock(&controller)?;
        let (registry, previous) =
            installations::load(&self.profile)?.ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
        let id = candidate(self, &registry)?;
        let plan = self.intent().unwrap().update.plan().clone();
        let binding = crate::restoration::RestorationBinding::from_plan(&plan)?;
        let generation = self.current.generation;
        let root = self.profile.join("installations").join(&id);
        installations::private_directory(&root)?;
        let root_identity =
            fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?;
        let parent_identity = fs::symlink_metadata(self.profile.join("installations"))
            .map_err(|_| err("UPDATE_TARGET_CHANGED"))?;
        let mut protected = Vec::new();
        for source in [&plan.source_instance, &plan.target_instance] {
            let entry = registry
                .installations
                .iter()
                .find(|e| &e.id == source)
                .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
            let p = installations::root(&self.profile, entry);
            installations::private_directory(&p)?;
            protected.push(crate::LifecycleService::open_retry_diagnostics(p)?);
        }
        protected.sort_by(|a, b| a.root.cmp(&b.root));
        let guards = protected
            .iter()
            .map(|s| s.lock())
            .collect::<LifecycleResult<Vec<_>>>()?;
        let target = crate::LifecycleService::open_retry_diagnostics(root.clone())?;
        let guard = target.lock()?;
        // No-overwrite refusal before any Engine check/resource creation.
        crate::restoration::fresh_root(&root)?;
        reserve_space(&root, archive, plan.required_free_bytes)?;
        let check = |store: &Store| -> LifecycleResult<()> {
            store.check_root().map_err(|e| err(e.code()))?;
            if store.current.generation != generation
                || candidate(store, &registry)? != id
                || installations::load(&store.profile)?.map(|(_, b)| b) != Some(previous.clone())
                || !identity(
                    &root_identity,
                    &fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
                )
                || !identity(
                    &parent_identity,
                    &fs::symlink_metadata(store.profile.join("installations"))
                        .map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
                )
            {
                return Err(err("UPDATE_ROLLBACK_CHANGED"));
            }
            target.check_restoration_guard(&guard)?;
            for (service, guard) in protected.iter().zip(&guards) {
                service.check_restoration_guard(guard)?;
            }
            Ok(())
        };
        check(self)?;
        let result = (|| -> LifecycleResult<crate::restoration::RestorationReceipt> {
            let r = target
                .restore_update_candidate_locked(image, key, archive, port, &binding, &guard)?;
            check(self)?;
            let source_entry = registry
                .installations
                .iter()
                .find(|e| e.id == plan.source_instance)
                .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
            owned_execution::check_restored_source_files(
                &installations::root(&self.profile, source_entry),
                &root.join(format!("restore-{}", r.id)),
                &r,
                &plan,
            )?;
            let proof = restore_receipt(&plan, &id, &r)?;
            // Actual bounded service-backup restore verified the inventory and
            // original runtime image before this event; health completion remains
            // separate and must be reobserved before selected-host activation.
            self.restore_finished(
                &plan.operation_id,
                generation,
                proof,
                owned_execution::release_now()?,
            )
            .map_err(|e| err(e.code()))?;
            if self.intent().is_none_or(|i| {
                i.update.stage() != crate::update::Stage::AwaitingRollbackHealth
                    || i.update.restore_candidate() != Some(id.as_str())
            }) || installations::load(&self.profile)?.map(|(_, b)| b) != Some(previous.clone())
            {
                return Err(err("UPDATE_ROLLBACK_UNCERTAIN"));
            }
            target.check_restoration_guard(&guard)?;
            for (service, guard) in protected.iter().zip(&guards) {
                service.check_restoration_guard(guard)?;
            }
            Ok(r)
        })();
        match result {
            Ok(restoration) => Ok(RollbackRestorationReceipt {
                operation_id: plan.operation_id,
                candidate_id: id,
                authority_generation: self.current.generation,
                active_instance: registry.active_id,
                restoration,
                original_selection_preserved: true,
                rollback_completed: false,
            }),
            Err(error) => {
                if self.current.generation == generation
                    && self
                        .intent()
                        .is_some_and(|i| i.update.stage() == crate::update::Stage::Restoring)
                {
                    self.recovery_failed(
                        &plan.operation_id,
                        generation,
                        owned_execution::release_now()?,
                    )
                    .map_err(|_| err("UPDATE_ROLLBACK_UNCERTAIN"))?;
                }
                Err(error)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_restoration_binding_never_accepts_target_schema_or_unbound_receipt() {
        let mut p = super::super::tests::plan();
        p.backup_id = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let r = crate::restoration::RestorationReceipt {
            id: uuid::Uuid::new_v4().to_string(),
            operation: "restored-and-running".into(),
            backup_id: p.backup_id.clone(),
            authenticated_manifest_sha256: p.backup_manifest.clone(),
            bundle_id: uuid::Uuid::new_v4().to_string(),
            project_name: "synthetic".into(),
            open_url: "http://127.0.0.1:48080".into(),
            at: 1,
            source_verification: Some(crate::restoration::RestorationProof {
                inventory_sha256: p.source_inventory.clone(),
                schema_sha256: p.source_schema.clone(),
                runtime_image_sha256: p.source_image.clone(),
            }),
        };
        assert_eq!(restore_receipt(&p, &id, &r).unwrap().candidate_id, id);
        for change in 0..5 {
            let mut r = serde_json::from_value::<crate::restoration::RestorationReceipt>(
                serde_json::to_value(&r).unwrap(),
            )
            .unwrap();
            match change {
                0 => {
                    r.source_verification.as_mut().unwrap().schema_sha256 = p.target_schema.clone()
                }
                1 => {
                    r.source_verification.as_mut().unwrap().runtime_image_sha256 =
                        p.target_image.clone()
                }
                2 => r.source_verification = None,
                3 => r.authenticated_manifest_sha256 = "f".repeat(64),
                _ => r.operation = "completed".into(),
            }
            assert!(restore_receipt(&p, &id, &r).is_err());
        }
    }
    #[test]
    fn rollback_restoration_refuses_ack_stage_and_existing_candidate_before_engine() {
        let (p, mut s) = super::super::rollback_registration::tests::fixture();
        let generation = s.current.generation;
        assert_eq!(
            s.restore_registered_rollback_candidate(
                "untrusted",
                Path::new("missing"),
                Path::new("missing"),
                48080,
                false
            )
            .unwrap_err()
            .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            s.restore_registered_rollback_candidate(
                "untrusted",
                Path::new("missing"),
                Path::new("missing"),
                48080,
                true
            )
            .unwrap_err()
            .code,
            "UPDATE_ROLLBACK_STAGE_INVALID"
        );
        assert_eq!(s.current.generation, generation);
        let registered = s.register_rollback_candidate(true).unwrap();
        fs::write(registered.candidate_path.join("retain"), b"operator bytes").unwrap();
        let generation = s.current.generation;
        assert_eq!(
            s.restore_registered_rollback_candidate(
                "untrusted",
                Path::new("missing"),
                Path::new("missing"),
                48080,
                true
            )
            .unwrap_err()
            .code,
            "RESTORE_FRESH_ROOT_REQUIRED"
        );
        assert_eq!(s.current.generation, generation);
        assert_eq!(
            fs::read(registered.candidate_path.join("retain")).unwrap(),
            b"operator bytes"
        );
        assert_eq!(
            installations::load(&p).unwrap().unwrap().0.active_id,
            registered.active_instance
        );
        drop(s);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn rollback_restoration_changed_selection_and_missing_reserved_entry_never_create_resources() {
        let (p, mut s) = super::super::rollback_registration::tests::fixture();
        let registered = s.register_rollback_candidate(true).unwrap();
        let generation = s.current.generation;
        let (mut registry, bytes) = installations::load(&p).unwrap().unwrap();
        registry.active_id = s.intent().unwrap().update.plan().target_instance.clone();
        installations::save(&p, &registry, Some(&bytes)).unwrap();
        assert_eq!(
            s.restore_registered_rollback_candidate(
                "untrusted",
                Path::new("missing"),
                Path::new("missing"),
                48080,
                true
            )
            .unwrap_err()
            .code,
            "UPDATE_SOURCE_MISMATCH"
        );
        let bytes = installations::load(&p).unwrap().unwrap().1;
        registry.active_id = registered.active_instance;
        registry
            .installations
            .retain(|e| e.id != registered.candidate_id);
        installations::save(&p, &registry, Some(&bytes)).unwrap();
        assert_eq!(
            s.restore_registered_rollback_candidate(
                "untrusted",
                Path::new("missing"),
                Path::new("missing"),
                48080,
                true
            )
            .unwrap_err()
            .code,
            "UPDATE_ROLLBACK_CANDIDATE_MISSING"
        );
        assert_eq!(s.current.generation, generation);
        assert!(
            fs::read_dir(&registered.candidate_path)
                .unwrap()
                .next()
                .is_none()
        );
        drop(s);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn rollback_restoration_foreign_operation_guard_refuses_before_engine() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("rollback-guard-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&root).unwrap();
        let a = crate::LifecycleService::new(root.join("a")).unwrap();
        let b = crate::LifecycleService::new(root.join("b")).unwrap();
        let guard = a.lock().unwrap();
        a.check_restoration_guard(&guard).unwrap();
        assert_eq!(
            b.check_restoration_guard(&guard).unwrap_err().code,
            "UPDATE_FENCE_UNAVAILABLE"
        );
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rollback_restoration_archive_alias_and_budget_overflow_preserve_inputs() {
        use std::os::unix::fs::symlink;
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-rollback-budget-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&parent).unwrap();
        let archive = parent.join("archive");
        installations::new_directory(&archive).unwrap();
        let data = archive.join("cipher.bin");
        crate::restoration::private_bytes(&data, b"synthetic encrypted bytes").unwrap();
        let before = fs::read(&data).unwrap();
        assert_eq!(
            reserve_space(&parent, &archive, u64::MAX).unwrap_err().code,
            "STORAGE_QUOTA"
        );
        let alias = parent.join("alias");
        symlink(&archive, &alias).unwrap();
        assert_eq!(
            reserve_space(&parent, &alias, 1).unwrap_err().code,
            "BACKUP_PATH_INVALID"
        );
        fs::hard_link(&data, parent.join("outside-link")).unwrap();
        assert_eq!(
            reserve_space(&parent, &archive, 1).unwrap_err().code,
            "BACKUP_PATH_INVALID"
        );
        assert_eq!(fs::read(&data).unwrap(), before);
        fs::remove_dir_all(&parent).unwrap();
    }
    #[test]
    #[ignore = "explicit local Docker qualification; reads retained synthetic backup and creates only new profile/candidate"]
    fn rollback_restoration_actual_encrypted_source_backup_into_new_fixture() {
        let fixture =
            fs::canonicalize(std::env::var("EXHIBITOS_ROLLBACK_BACKUP_FIXTURE").unwrap()).unwrap();
        let plan_report = PathBuf::from(std::env::var("EXHIBITOS_ROLLBACK_PLAN_REPORT").unwrap());
        let original = fixture.join("retained-source-manager");
        let creation: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.join("backup-creation-report.json")).unwrap())
                .unwrap();
        let archive = original.join(format!(
            "backup-creation-{}/archive",
            creation["receipt"]["id"].as_str().unwrap()
        ));
        let key_file = fixture.join("key.bin");
        let image = creation["receipt"]["image"].as_str().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(plan_report).unwrap()).unwrap();
        let mut pl: crate::update::Plan =
            serde_json::from_value(report["binding"]["intent"]["update"]["plan"].clone()).unwrap();
        // Fixture-only failed journal: actual restore qualification, not a claim
        // that native admission/apply or changed migration was executed.
        pl.operation_id = uuid::Uuid::new_v4().to_string();
        pl.source_instance = uuid::Uuid::new_v4().to_string();
        pl.target_instance = uuid::Uuid::new_v4().to_string();
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-rollback-actual-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&root).unwrap();
        println!("PRIVATE_QUALIFICATION_ROOT={}", root.display());
        let profile = root.join("profile");
        installations::new_directory(&profile).unwrap();
        let files = [
            "installed.json",
            "engine.json",
            "runtime.env",
            "bundle/manifest.json",
            "bundle/compose.yaml",
        ];
        let source = profile.join("local-runtime");
        installations::new_directory(&source).unwrap();
        installations::new_directory(&source.join("bundle")).unwrap();
        let before = files
            .iter()
            .map(|name| (*name, hash(&fs::read(original.join(name)).unwrap())))
            .collect::<Vec<_>>();
        for name in &files {
            crate::restoration::private_bytes(
                &source.join(name),
                &fs::read(original.join(name)).unwrap(),
            )
            .unwrap();
        }
        installations::new_directory(&profile.join("installations")).unwrap();
        installations::new_directory(&profile.join("installations").join(&pl.target_instance))
            .unwrap();
        installations::save(
            &profile,
            &installations::Registry {
                format: 1,
                active_id: pl.source_instance.clone(),
                installations: vec![
                    installations::Entry {
                        id: pl.source_instance.clone(),
                        kind: "default".into(),
                        created_at: 1,
                    },
                    installations::Entry {
                        id: pl.target_instance.clone(),
                        kind: "recovery".into(),
                        created_at: 2,
                    },
                ],
            },
            None,
        )
        .unwrap();
        let (unused, signing, mut policy, mut release) = super::super::tests::fixture();
        fs::remove_dir_all(unused.parent().unwrap()).unwrap();
        let now = owned_execution::release_now().unwrap();
        policy.source_schema_sha256 = pl.source_schema.clone();
        policy.minimum_issued_at = now - 10;
        release.source_schemas = vec![pl.source_schema.clone()];
        release.artifact.runtime_image_sha256 = pl.target_image.clone();
        release.artifact.schema_sha256 = pl.target_schema.clone();
        release.issued_at = now - 1;
        release.expires_at = now + 3600;
        let raw = super::super::tests::seal(&signing, &release);
        let mut store = Store::provision(&profile, "default", policy, now).unwrap();
        let mut verified = store.verify_for_preparation(&raw, now).unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        store
            .prepare_update(&raw, &verified, pl.clone(), now)
            .unwrap();
        store
            .enroll_authority_recovery(&root.join("vault"), now)
            .unwrap();
        let mut evidence = super::super::tests::observations();
        evidence.plan = pl.clone();
        evidence.available_free_bytes = fs2::available_space(&root).unwrap();
        store.begin_update(evidence, &verified, now).unwrap();
        store
            .update_failed(&pl.operation_id, store.current.generation, now)
            .unwrap();
        let registered = store.register_rollback_candidate(true).unwrap();
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let restored = store
            .restore_registered_rollback_candidate(image, &key_file, &archive, port, true)
            .unwrap();
        assert!(!restored.rollback_completed && restored.original_selection_preserved);
        assert_eq!(restored.candidate_id, registered.candidate_id);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::AwaitingRollbackHealth
        );
        assert_eq!(
            installations::load(&profile).unwrap().unwrap().0.active_id,
            pl.source_instance
        );
        for (name, digest) in before {
            assert_eq!(hash(&fs::read(original.join(name)).unwrap()), digest);
            assert_eq!(hash(&fs::read(source.join(name)).unwrap()), digest);
        }
        let target =
            crate::LifecycleService::open_retry_diagnostics(registered.candidate_path.clone())
                .unwrap();
        let guard = target.lock().unwrap();
        let m = target.manifest().unwrap();
        target.validate_ownership(&m, "docker").unwrap();
        target.validate_volumes(&m, "docker").unwrap();
        assert!(crate::readiness(&m).ready);
        let (_, app) =
            crate::backup_creation::one_container(&target, &m, "docker", "platform").unwrap();
        assert_eq!(app["Image"], format!("sha256:{}", pl.source_image));
        crate::restoration::private_bytes(&root.join("report.json"),&serde_json::to_vec_pretty(&serde_json::json!({
            "status":"PASS","classification":"Actual original encrypted service restore with synthetic failed journal; not native full update/migration/rollback activation proof", "restoration":restored,"targetRoot":registered.candidate_path,
            "currentSelectionPreserved":true,"oldSourceImageObserved":true,"nativeReadiness":true,
            "changedMigrationVerified":false,"rollbackCompleted":false
        })).unwrap()).unwrap();
        // Preserve this fresh coherent qualification target for independent next
        // health/selection validation. No original, volume or archive deletion.
        crate::run(
            "docker",
            &crate::compose_args(&m, &["stop", "--timeout", "30"]),
            Some(&target.root.join("bundle")),
            180,
        )
        .unwrap();
        target.check_restoration_guard(&guard).unwrap();
        println!("PASS_ACTUAL_ROLLBACK_RESTORATION");
    }
}
