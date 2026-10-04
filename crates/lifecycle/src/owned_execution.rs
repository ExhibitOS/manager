// SPDX-License-Identifier: Apache-2.0
//! Existing registered source adapters under the borrowed exclusive trust fence.
use super::*;
use crate::{LifecycleService, Status, maintenance::VerificationReceipt};
#[path = "source_stopped.rs"]
mod source_stopped;
pub use source_stopped::SourceStoppedReceipt;

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
impl ExecutionSession<'_> {
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
        let after_source = source_stopped::observe(&self.source, plan)?;
        if before_source.platform_container != after_source.platform_container
            || before_source.database_container != after_source.database_container
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        self.check()?;
        installations::private_directory(&root)?;
        if !identity(
            &before,
            &fs::symlink_metadata(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
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
        assert!(!root.join("engine.json").exists());
        assert!(!root.join("installed.json").exists());
        drop(lease);
        assert_eq!(store.receipt().generation, 2);
    }
}
