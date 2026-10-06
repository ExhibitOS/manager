// SPDX-License-Identifier: Apache-2.0
//! Reserve rollback identity before any fresh host publication. No Engine work.
use super::*;
use crate::{Result as LifecycleResult, err};

/// Registration only: never a restore, inventory, health or activation permit.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRollbackCandidate {
    pub operation_id: String,
    pub candidate_id: String,
    pub candidate_path: PathBuf,
    pub active_instance: String,
    pub authority_generation: u64,
    pub registry_sha256: String,
    pub runtime_data_restored: bool,
    pub runtime_started: bool,
    pub activated: bool,
}

impl Store {
    /// Create only a NEW private, inactive rollback namespace for RecoveryRequired.
    /// Caller supplies consent, never an ID, path, plan or verified receipt. The
    /// original authority reserves the ID durably BEFORE folder/registry writes.
    /// On interruption, reopen requires recovery and keeps the ID reserved; no
    /// original, failed candidate or recorded selection is adopted or overwritten.
    pub fn register_rollback_candidate(
        &mut self,
        preserve_active: bool,
    ) -> LifecycleResult<RegisteredRollbackCandidate> {
        self.register_rollback_candidate_inner(preserve_active, |_, _| Ok(()))
    }

    fn register_rollback_candidate_inner(
        &mut self,
        preserve_active: bool,
        mut observed: impl FnMut(&str, &Path) -> LifecycleResult<()>,
    ) -> LifecycleResult<RegisteredRollbackCandidate> {
        if !preserve_active {
            return Err(err("UPDATE_ROLLBACK_ACK_REQUIRED"));
        }
        self.check_root().map_err(|e| err(e.code()))?;
        self.require_authority_recovery()
            .map_err(|e| err(e.code()))?;
        let intent = self.intent().ok_or_else(|| err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::RecoveryRequired {
            return Err(err("UPDATE_ROLLBACK_STAGE_INVALID"));
        }
        let plan = intent.update.plan().clone();
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| err("UPDATE_FENCE_UNAVAILABLE"))?;
        let _session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let controller = crate::LifecycleService::open_retry_diagnostics(self.profile.clone())?;
        let _profile = installations::profile_lock(&controller)?;
        let (mut registry, previous) =
            installations::load(&self.profile)?.ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
        let bound = self.bound_source_id(&registry)?;
        if registry.active_id != bound
            || ![&plan.source_instance, &plan.target_instance].contains(&&bound)
        {
            return Err(err("UPDATE_SOURCE_MISMATCH"));
        }
        if registry.installations.len() >= 128 {
            return Err(err("INSTALLATION_SELECTION_LIMIT"));
        }
        // Both old source and changed target operation namespaces remain fenced.
        let mut services = Vec::new();
        for id in [&plan.source_instance, &plan.target_instance] {
            let entry = registry
                .installations
                .iter()
                .find(|e| &e.id == id)
                .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
            let root = installations::root(&self.profile, entry);
            installations::private_directory(&root)?;
            services.push(crate::LifecycleService::open_retry_diagnostics(root)?);
        }
        services.sort_by(|a, b| a.root.cmp(&b.root));
        let _operations = services
            .iter()
            .map(|s| s.lock())
            .collect::<LifecycleResult<Vec<_>>>()?;
        let parent = self.profile.join("installations");
        installations::private_directory(&parent)?;
        let parent_identity =
            fs::symlink_metadata(&parent).map_err(|_| err("UPDATE_TARGET_CHANGED"))?;
        let entry = installations::Entry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "recovery".into(),
            created_at: crate::now(),
        };
        let root = installations::root(&self.profile, &entry);
        if self.used_instances.contains(&entry.id)
            || registry.installations.iter().any(|e| e.id == entry.id)
            || fs::symlink_metadata(&root).is_ok()
        {
            return Err(err("UPDATE_IDENTITY_REUSED"));
        }
        let active = registry.active_id.clone();
        let id = entry.id.clone();
        self.begin_restore(
            &plan.operation_id,
            self.current.generation,
            id.clone(),
            owned_execution::release_now()?,
        )
        .map_err(|e| err(e.code()))?;
        observed("reserved", &root)?;
        if !identity(
            &parent_identity,
            &fs::symlink_metadata(&parent).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(err("UPDATE_TARGET_CHANGED"));
        }
        installations::new_directory(&root)?;
        let root_identity =
            fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?;
        observed("created", &root)?;
        if !identity(
            &root_identity,
            &fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
        ) || !identity(
            &parent_identity,
            &fs::symlink_metadata(&parent).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(err("UPDATE_TARGET_CHANGED"));
        }
        // A reserved namespace must not adopt any bytes supplied after creation.
        installations::private_directory(&root)?;
        if fs::read_dir(&root)
            .map_err(|_| err("STATE_UNAVAILABLE"))?
            .next()
            .is_some()
        {
            return Err(err("UPDATE_ROLLBACK_CANDIDATE_CHANGED"));
        }
        registry.installations.push(entry);
        if installations::load(&self.profile)?.map(|(_, b)| b) != Some(previous.clone()) {
            return Err(err("UPDATE_SOURCE_CHANGED"));
        }
        self.check_root().map_err(|e| err(e.code()))?;
        installations::save(&self.profile, &registry, Some(&previous))?;
        observed("published", &root)?;
        if !identity(
            &root_identity,
            &fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(err("UPDATE_TARGET_CHANGED"));
        }
        let (current, bytes) = installations::load(&self.profile)?
            .ok_or_else(|| err("INSTALLATION_SELECTION_UNCERTAIN"))?;
        if current.active_id != active
            || current.installations.len() != registry.installations.len()
            || !current
                .installations
                .iter()
                .any(|e| e.id == id && e.kind == "recovery")
        {
            return Err(err("INSTALLATION_SELECTION_UNCERTAIN"));
        }
        Ok(RegisteredRollbackCandidate {
            operation_id: plan.operation_id,
            candidate_id: id,
            candidate_path: root,
            active_instance: active,
            authority_generation: self.current.generation,
            registry_sha256: hash(&bytes),
            runtime_data_restored: false,
            runtime_started: false,
            activated: false,
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::tests::{fixture as trust_fixture, observations, plan, seal};
    use super::*;
    const SOURCE: &str = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
    const TARGET: &str = "cc414c3c-dd99-45e2-8307-131a49f72d68";
    pub(in super::super) fn fixture() -> (PathBuf, Store) {
        let (p, key, policy, release) = trust_fixture();
        let mut s = Store::provision(&p, "default", policy, 10).unwrap();
        let raw = seal(&key, &release);
        let mut verified = s.verify_for_preparation(&raw, 20).unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        let mut pl = plan();
        pl.source_instance = SOURCE.into();
        pl.target_instance = TARGET.into();
        pl.backup_id = uuid::Uuid::new_v4().to_string();
        s.prepare_update(&raw, &verified, pl.clone(), 20).unwrap();
        s.enroll_authority_recovery(&p.parent().unwrap().join("vault"), 20)
            .unwrap();
        installations::save(
            &p,
            &installations::Registry {
                format: 1,
                active_id: SOURCE.into(),
                installations: vec![
                    installations::Entry {
                        id: SOURCE.into(),
                        kind: "default".into(),
                        created_at: 0,
                    },
                    installations::Entry {
                        id: TARGET.into(),
                        kind: "recovery".into(),
                        created_at: 1,
                    },
                ],
            },
            None,
        )
        .unwrap();
        installations::new_directory(&p.join("local-runtime")).unwrap();
        installations::new_directory(&p.join("installations")).unwrap();
        installations::new_directory(&p.join("installations").join(TARGET)).unwrap();
        let mut e = observations();
        e.plan = pl;
        s.begin_update(e, &verified, 21).unwrap();
        s.update_failed("update-1", s.current.generation, 22)
            .unwrap();
        (p, s)
    }
    fn retire(p: PathBuf, s: Store) {
        drop(s);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn rollback_registration_reserves_before_creation_and_preserves_selection_and_history() {
        let (p, mut s) = fixture();
        let prefix = s.current.generation;
        let records: Vec<_> = (1..=prefix)
            .map(|g| (g, fs::read(s.root.join(format!("{g:020}.json"))).unwrap()))
            .collect();
        let result = s.register_rollback_candidate(true).unwrap();
        assert_eq!(result.authority_generation, prefix + 1);
        assert_eq!(
            s.intent().unwrap().update.stage(),
            crate::update::Stage::Restoring
        );
        assert!(s.used_instances.contains(&result.candidate_id));
        assert!(!result.runtime_data_restored && !result.runtime_started && !result.activated);
        assert!(
            fs::read_dir(&result.candidate_path)
                .unwrap()
                .next()
                .is_none()
        );
        let (reg, _) = installations::load(&p).unwrap().unwrap();
        assert_eq!(reg.active_id, SOURCE);
        assert_eq!(reg.installations.len(), 3);
        for (g, raw) in records {
            assert_eq!(fs::read(s.root.join(format!("{g:020}.json"))).unwrap(), raw);
        }
        let id = result.candidate_id;
        drop(s);
        let s = Store::open(&p, "default").unwrap();
        assert_eq!(
            s.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        assert!(s.used_instances.contains(&id));
        assert_eq!(
            installations::load(&p).unwrap().unwrap().0.active_id,
            SOURCE
        );
        retire(p, s);
    }
    #[test]
    fn rollback_registration_interruptions_never_reuse_identity_or_adopt_candidate_bytes() {
        for boundary in ["reserved", "created", "published"] {
            let (p, mut s) = fixture();
            let previous = installations::load(&p).unwrap().unwrap().1;
            let mut old_root = None;
            assert!(
                s.register_rollback_candidate_inner(true, |stage, root| {
                    if stage == boundary {
                        old_root = Some(root.to_owned());
                        return Err(err("SYNTHETIC_INTERRUPT"));
                    }
                    Ok(())
                })
                .is_err()
            );
            let root = old_root.unwrap();
            let id = root.file_name().unwrap().to_str().unwrap().to_owned();
            assert!(s.used_instances.contains(&id));
            if boundary != "published" {
                assert_eq!(installations::load(&p).unwrap().unwrap().1, previous);
            }
            let saved = if root.exists() {
                fs::write(root.join("failed-witness"), b"preserve").unwrap();
                true
            } else {
                false
            };
            drop(s);
            let mut s = Store::open(&p, "default").unwrap();
            let next = s.register_rollback_candidate(true).unwrap();
            assert_ne!(next.candidate_id, id);
            assert!(s.used_instances.contains(&id));
            if saved {
                assert_eq!(fs::read(root.join("failed-witness")).unwrap(), b"preserve");
            }
            assert_eq!(
                installations::load(&p).unwrap().unwrap().0.active_id,
                SOURCE
            );
            retire(p, s);
        }
    }
    #[test]
    fn rollback_registration_refuses_unacknowledged_stage_selection_and_foreign_candidate() {
        let (p, mut s) = fixture();
        let generation = s.current.generation;
        let old = installations::load(&p).unwrap().unwrap().1;
        assert_eq!(
            s.register_rollback_candidate(false).unwrap_err().code,
            "UPDATE_ROLLBACK_ACK_REQUIRED"
        );
        assert_eq!(s.current.generation, generation);
        let (mut reg, raw) = installations::load(&p).unwrap().unwrap();
        reg.active_id = TARGET.into();
        installations::save(&p, &reg, Some(&raw)).unwrap();
        assert_eq!(
            s.register_rollback_candidate(true).unwrap_err().code,
            "UPDATE_SOURCE_MISMATCH"
        );
        assert_eq!(s.current.generation, generation);
        reg.active_id = SOURCE.into();
        let raw = installations::load(&p).unwrap().unwrap().1;
        installations::save(&p, &reg, Some(&raw)).unwrap();
        assert_eq!(installations::load(&p).unwrap().unwrap().1, old);
        let mut root = None;
        assert_eq!(
            s.register_rollback_candidate_inner(true, |stage, p| {
                if stage == "created" {
                    fs::write(p.join("foreign"), b"preserve").unwrap();
                    root = Some(p.to_owned());
                }
                Ok(())
            })
            .unwrap_err()
            .code,
            "UPDATE_ROLLBACK_CANDIDATE_CHANGED"
        );
        assert_eq!(
            fs::read(root.unwrap().join("foreign")).unwrap(),
            b"preserve"
        );
        assert_eq!(installations::load(&p).unwrap().unwrap().1, old);
        assert_eq!(
            s.register_rollback_candidate(true).unwrap_err().code,
            "UPDATE_ROLLBACK_STAGE_INVALID"
        );
        retire(p, s);
    }
    #[test]
    fn rollback_registration_refuses_replaced_empty_candidate_without_adoption() {
        let (p, mut s) = fixture();
        let old = installations::load(&p).unwrap().unwrap().1;
        let error = s
            .register_rollback_candidate_inner(true, |stage, root| {
                if stage == "created" {
                    fs::rename(root, root.with_extension("retained")).unwrap();
                    installations::new_directory(root).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, "UPDATE_TARGET_CHANGED");
        assert_eq!(installations::load(&p).unwrap().unwrap().1, old);
        assert_eq!(
            s.intent().unwrap().update.stage(),
            crate::update::Stage::Restoring
        );
        retire(p, s);
    }
    #[test]
    #[ignore = "abrupt child worker; parent invokes it in three fresh synthetic namespaces"]
    fn rollback_registration_crash_worker() {
        let parent = PathBuf::from(std::env::var("EXHIBITOS_ROLLBACK_CRASH_PARENT").unwrap());
        assert_eq!(fs::canonicalize(&parent).unwrap(), parent);
        assert!(
            parent
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("exhibitos-release-trust-")
        );
        installations::private_directory(&parent).unwrap();
        let boundary = std::env::var("EXHIBITOS_ROLLBACK_CRASH_BOUNDARY").unwrap();
        assert!(["reserved", "created", "published"].contains(&boundary.as_str()));
        let mut s = Store::open(&parent.join("profile"), "default").unwrap();
        let _ = s.register_rollback_candidate_inner(true, |stage, _| {
            if stage == boundary {
                std::process::exit(77);
            }
            Ok(())
        });
        panic!("child did not reach requested boundary");
    }
    #[test]
    fn rollback_registration_abrupt_process_exit_preserves_reservations_and_selection() {
        for boundary in ["reserved", "created", "published"] {
            let (p, s) = fixture();
            let generation = s.current.generation;
            let root = s.root.clone();
            let prefix: Vec<_> = (1..=generation)
                .map(|g| (g, fs::read(root.join(format!("{g:020}.json"))).unwrap()))
                .collect();
            drop(s);
            let status=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","signed_release::trust::rollback_registration::tests::rollback_registration_crash_worker","--ignored","--nocapture"])
                .env("EXHIBITOS_ROLLBACK_CRASH_PARENT",p.parent().unwrap())
                .env("EXHIBITOS_ROLLBACK_CRASH_BOUNDARY",boundary)
                .status().unwrap();
            assert_eq!(status.code(), Some(77));
            let raw = fs::read(root.join(format!("{:020}.json", generation + 1))).unwrap();
            let record: Record = serde_json::from_slice(&raw).unwrap();
            let reserved =
                serde_json::to_value(&record.intent.unwrap().update).unwrap()["restoreCandidate"]
                    .as_str()
                    .unwrap()
                    .to_owned();
            let candidate = p.join("installations").join(&reserved);
            assert_eq!(candidate.exists(), boundary != "reserved");
            let mut s = Store::open(&p, "default").unwrap();
            assert_eq!(
                s.intent().unwrap().update.stage(),
                crate::update::Stage::RecoveryRequired
            );
            assert!(s.used_instances.contains(&reserved));
            for (g, bytes) in prefix {
                assert_eq!(fs::read(root.join(format!("{g:020}.json"))).unwrap(), bytes);
            }
            assert_eq!(
                fs::read(root.join(format!("{:020}.json", generation + 1))).unwrap(),
                raw
            );
            assert_eq!(
                installations::load(&p).unwrap().unwrap().0.active_id,
                SOURCE
            );
            let next = s.register_rollback_candidate(true).unwrap();
            assert_ne!(next.candidate_id, reserved);
            assert!(s.used_instances.contains(&reserved));
            assert_eq!(candidate.exists(), boundary != "reserved");
            retire(p, s);
        }
    }
}
