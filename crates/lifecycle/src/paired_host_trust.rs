// SPDX-License-Identifier: Apache-2.0
//! Paired inactive host+trust archives, cooperative fences; external data excluded.
use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostTrustReceipt {
    pub host: profile_backup::HostReceipt,
    pub trust: TrustCheckpointReceipt,
    pub external_volumes_saved: bool,
    pub live_authority_restored: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingHostRecoveryReceipt {
    pub host: profile_backup::HostReceipt,
    pub retained_trust: TrustCheckpointReceipt,
    pub host_profile_restored: bool,
    pub current_trust_preserved: bool,
    pub trust_authority_restored: bool,
    pub runtime_data_restored: bool,
    pub runtime_started: bool,
    pub recovery_workspace: PathBuf,
}
impl Store {
    /// Recover only absent host bytes using independently retained current trust.
    /// Lost trust authority and external runtime volumes are separate recovery gates.
    pub fn restore_missing_host(
        &self,
        host_archive: &Path,
        trust_archive: &Path,
        key_file: &Path,
        apps_closed: bool,
    ) -> crate::Result<MissingHostRecoveryReceipt> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        if !apps_closed {
            return Err(crate::err("HOST_WRITER_ACK_REQUIRED"));
        }
        match fs::symlink_metadata(&self.profile) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(crate::err("HOST_RESTORE_TARGET_EXISTS")),
        }
        if !key_file.is_absolute()
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file)
            || key_file.starts_with(&self.profile)
            || key_file.starts_with(&self.root)
        {
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let mut key = read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?;
        if key.len() != 32 {
            key.fill(0);
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let result = (|| {
            let original = self
                .checkpoint_records()
                .map_err(|e| crate::err(e.code()))?;
            let parent = self
                .profile
                .parent()
                .ok_or_else(|| crate::err("PROFILE_PATH_INVALID"))?;
            installations::private_directory(parent)?;
            let stage = parent.join(format!(".missing-host-recovery-{}", uuid::Uuid::new_v4()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(&stage)
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            let trust = self
                .extract_trust_checkpoint(
                    trust_archive,
                    &stage.join("trust"),
                    key.as_slice()
                        .try_into()
                        .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                )
                .map_err(|e| crate::err(e.code()))?;
            let host = profile_backup::extract_host(
                &self.profile,
                key_file,
                host_archive,
                &stage.join("host"),
                true,
            )?;
            self.verify_trust_checkpoint(&stage.join("trust"))
                .map_err(|e| crate::err(e.code()))?;
            if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key
                || self
                    .checkpoint_records()
                    .map_err(|e| crate::err(e.code()))?
                    != original
            {
                return Err(crate::err("HOST_CHECKPOINT_INVALID"));
            }
            let receipt = MissingHostRecoveryReceipt {
                host,
                retained_trust: trust,
                host_profile_restored: true,
                current_trust_preserved: true,
                trust_authority_restored: false,
                runtime_data_restored: false,
                runtime_started: false,
                recovery_workspace: stage.clone(),
            };
            let mut pending = private_file(&stage.join("publication-intent.json"), true)
                .map_err(|e| crate::err(e.code()))?;
            pending
                .write_all(
                    &serde_json::to_vec(
                        &serde_json::json!({"state":"publication_pending","recovery":&receipt}),
                    )
                    .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?,
                )
                .and_then(|_| pending.sync_all())
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            sync_dir(&stage).map_err(|e| crate::err(e.code()))?;
            profile_backup::activate_missing_host(
                &self.profile,
                &stage.join("host"),
                &receipt.host,
            )?;
            self.check_root()
                .map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?;
            if self
                .checkpoint_records()
                .map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?
                != original
            {
                return Err(crate::err("HOST_RESTORE_UNCERTAIN"));
            }
            let mut completed = private_file(&stage.join("completed.json"), true)
                .map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?;
            completed
                .write_all(
                    &serde_json::to_vec(&receipt)
                        .map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?,
                )
                .and_then(|_| completed.sync_all())
                .map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?;
            sync_dir(&stage).map_err(|_| crate::err("HOST_RESTORE_UNCERTAIN"))?;
            Ok(receipt)
        })();
        key.fill(0);
        result
    }

    /// The same borrowed anchor remains exclusive while legacy profile/root locks
    /// span both archives and host inventory re-observation. No nested re-lock.
    pub fn checkpoint_host_trust(
        &self,
        key_file: &Path,
        target: &Path,
        writers_stopped: bool,
    ) -> crate::Result<HostTrustReceipt> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        if !writers_stopped {
            return Err(crate::err("HOST_WRITER_ACK_REQUIRED"));
        }
        let parent = target
            .parent()
            .ok_or_else(|| crate::err("PROFILE_PATH_INVALID"))?;
        if !target.is_absolute()
            || fs::canonicalize(parent).ok().as_deref() != Some(parent)
            || target.starts_with(&self.profile)
            || target.starts_with(&self.root)
            || target.exists()
        {
            return Err(crate::err("PROFILE_PATH_INVALID"));
        }
        installations::private_directory(parent)?;
        // Same private leaf/identity guards used by trust records; key is external.
        if key_file.starts_with(&self.root)
            || key_file.starts_with(&self.profile)
            || !key_file.is_absolute()
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file)
        {
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let mut key = read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?;
        if key.len() != 32 {
            key.fill(0);
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let result = (|| {
            let stage = parent.join(format!("pending-host-trust-{}", uuid::Uuid::new_v4()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(&stage)
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            let anchor = self
                ._anchor
                .try_clone()
                .map_err(|_| crate::err("UPDATE_FENCE_UNAVAILABLE"))?;
            let (host, trust) = profile_backup::checkpoint_host_anchored(
                &self.profile,
                key_file,
                &stage.join("host.bin"),
                true,
                anchor,
                || {
                    if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key
                    {
                        return Err(crate::err("PROFILE_KEY_INVALID"));
                    }
                    self.archive_trust_checkpoint(
                        &stage.join("trust.bin"),
                        key.as_slice()
                            .try_into()
                            .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                    )
                    .map_err(|e| crate::err(e.code()))
                },
            )?;
            if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key {
                return Err(crate::err("PROFILE_KEY_INVALID"));
            }
            let receipt = HostTrustReceipt {
                host,
                trust,
                external_volumes_saved: false,
                live_authority_restored: false,
            };
            let mut marker = private_file(&stage.join("verified.json"), true)
                .map_err(|e| crate::err(e.code()))?;
            let bytes =
                serde_json::to_vec(&receipt).map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            marker
                .write_all(&bytes)
                .and_then(|_| marker.sync_all())
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            sync_dir(&stage).map_err(|e| crate::err(e.code()))?;
            self.check_root().map_err(|e| crate::err(e.code()))?;
            publish(&stage, target).map_err(|e| crate::err(e.code()))?;
            sync_dir(parent).map_err(|e| crate::err(e.code()))?;
            Ok(receipt)
        })();
        key.fill(0);
        result
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn recovery_fixture() -> (PathBuf, Store, PathBuf, PathBuf) {
        let (profile, store, _) = super::super::super::tests::prepared_fixture();
        drop(store);
        let controller =
            crate::installations::InstallationController::new(profile.clone(), None).unwrap();
        controller.context().unwrap();
        drop(controller);
        fs::write(profile.join("witness.bin"), b"original host witness").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            profile.join("witness.bin"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let store = Store::open(&profile, "default").unwrap();
        let parent = profile.parent().unwrap();
        let key = parent.join("recovery-key");
        private_file(&key, true)
            .unwrap()
            .write_all(&[4u8; 32])
            .unwrap();
        let pair = parent.join("recovery-pair");
        store.checkpoint_host_trust(&key, &pair, true).unwrap();
        (profile, store, key, pair)
    }
    #[test]
    fn absent_host_recovers_original_namespace_and_keeps_current_authority() {
        let (profile, store, key, pair) = recovery_fixture();
        let old = profile.with_extension("quarantined");
        let head = store.current_sha256.clone();
        let intent = serde_json::to_vec(&store.current).unwrap();
        let registry = fs::read(profile.join("installation-selection.json")).unwrap();
        fs::rename(&profile, &old).unwrap();
        let receipt = store
            .restore_missing_host(&pair.join("host.bin"), &pair.join("trust.bin"), &key, true)
            .unwrap();
        assert!(receipt.host_profile_restored && receipt.current_trust_preserved);
        assert!(
            !receipt.trust_authority_restored
                && !receipt.runtime_data_restored
                && !receipt.runtime_started
        );
        assert_eq!(
            fs::read(profile.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        assert_eq!(
            fs::read(old.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        assert_eq!(
            fs::read(profile.join("installation-selection.json")).unwrap(),
            registry
        );
        assert_eq!(store.current_sha256, head);
        assert_eq!(serde_json::to_vec(&store.current).unwrap(), intent);
        assert!(receipt.recovery_workspace.join("completed.json").is_file());
        assert!(
            !receipt
                .recovery_workspace
                .join("host/payload.pending")
                .exists()
        );
        let parent = profile.parent().unwrap().to_path_buf();
        drop(store);
        let controller =
            crate::installations::InstallationController::new(profile.clone(), None).unwrap();
        controller.context().unwrap();
        drop(controller);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn existing_or_symlink_host_and_missing_ack_are_never_overwritten() {
        let (profile, store, key, pair) = recovery_fixture();
        let run = |ack| {
            store.restore_missing_host(&pair.join("host.bin"), &pair.join("trust.bin"), &key, ack)
        };
        assert_eq!(run(false).unwrap_err().code, "HOST_WRITER_ACK_REQUIRED");
        assert_eq!(run(true).unwrap_err().code, "HOST_RESTORE_TARGET_EXISTS");
        let old = profile.with_extension("quarantined");
        fs::rename(&profile, &old).unwrap();
        std::os::unix::fs::symlink(profile.with_extension("absent"), &profile).unwrap();
        assert_eq!(run(true).unwrap_err().code, "HOST_RESTORE_TARGET_EXISTS");
        assert_eq!(
            fs::read(old.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        let parent = profile.parent().unwrap().to_path_buf();
        drop(store);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn older_trust_or_tampered_host_keeps_namespace_absent_and_new_floors() {
        let (profile, mut store, key, pair) = recovery_fixture();
        let old = profile.with_extension("quarantined");
        fs::rename(&profile, &old).unwrap();
        store
            .replace_policy(
                store.current.policy.clone(),
                store.current.policy_generation,
                22,
            )
            .unwrap();
        let head = store.current_sha256.clone();
        assert!(
            store
                .restore_missing_host(&pair.join("host.bin"), &pair.join("trust.bin"), &key, true)
                .is_err()
        );
        assert!(!profile.exists());
        assert_eq!(store.current_sha256, head);
        let fresh = pair.join("latest-trust.bin");
        store.archive_trust_checkpoint(&fresh, &[4u8; 32]).unwrap();
        let bad = pair.join("bad-host.bin");
        let mut bytes = fs::read(pair.join("host.bin")).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        private_file(&bad, true).unwrap().write_all(&bytes).unwrap();
        assert!(
            store
                .restore_missing_host(&bad, &fresh, &key, true)
                .is_err()
        );
        assert!(!profile.exists());
        assert_eq!(store.current_sha256, head);
        assert_eq!(
            fs::read(old.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        let parent = profile.parent().unwrap().to_path_buf();
        drop(store);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn paired_archives_borrow_anchor_and_preserve_prepared_history() {
        let (profile, store, _) = super::super::super::tests::prepared_fixture();
        let selection =
            match crate::installations::InstallationController::new(profile.clone(), None) {
                Err(e) => e,
                Ok(_) => panic!("held Store fence acquired"),
            };
        assert_eq!(selection.code, "PROFILE_BUSY");
        // Use a valid controller registry, created only after the Store fence drops.
        drop(store);
        let controller =
            crate::installations::InstallationController::new(profile.clone(), None).unwrap();
        let _ = controller.context().unwrap();
        drop(controller);
        let store = Store::open(&profile, "default").unwrap();
        let parent = profile.parent().unwrap();
        let key = parent.join("pair-key");
        private_file(&key, true)
            .unwrap()
            .write_all(&[3u8; 32])
            .unwrap();
        let target = parent.join("pair");
        assert!(store.checkpoint_host_trust(&key, &target, false).is_err());
        assert!(!target.exists());
        let original = store.current_sha256.clone();
        let receipt = store.checkpoint_host_trust(&key, &target, true).unwrap();
        assert!(!receipt.external_volumes_saved);
        assert!(!receipt.live_authority_restored);
        assert_eq!(store.current_sha256, original);
        assert!(target.join("host.bin").is_file());
        assert!(target.join("trust.bin").is_file());
        assert!(target.join("verified.json").is_file());
        assert!(store.checkpoint_host_trust(&key, &target, true).is_err());
    }
}
