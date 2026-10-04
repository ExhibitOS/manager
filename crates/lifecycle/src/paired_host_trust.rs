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
impl Store {
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
