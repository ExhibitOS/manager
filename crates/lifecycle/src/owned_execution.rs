// SPDX-License-Identifier: Apache-2.0
//! Existing registered source adapters under the borrowed exclusive trust fence.
use super::*;
use crate::{LifecycleService, Status, maintenance::VerificationReceipt};

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
        let (p, k, policy, r) = fixture();
        let mut s = Store::provision(&p, scope, policy, 10).unwrap();
        let e = seal(&k, &r);
        let mut v = s.verify_for_preparation(&e, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut plan = plan();
        plan.source_instance = SOURCE.into();
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
}
