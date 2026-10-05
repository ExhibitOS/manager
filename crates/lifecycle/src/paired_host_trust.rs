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
    pub pair_binding_verified: bool,
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
        self.restore_host_pair(host_archive, trust_archive, key_file, None, apps_closed)
    }
    /// Also authenticate that host and retained-current trust belong to one pair.
    pub fn restore_bound_missing_host(
        &self,
        host_archive: &Path,
        trust_archive: &Path,
        key_file: &Path,
        pair_binding: &Path,
        apps_closed: bool,
    ) -> crate::Result<MissingHostRecoveryReceipt> {
        self.restore_host_pair(
            host_archive,
            trust_archive,
            key_file,
            Some(pair_binding),
            apps_closed,
        )
    }
    fn restore_host_pair(
        &self,
        host_archive: &Path,
        trust_archive: &Path,
        key_file: &Path,
        pair_binding: Option<&Path>,
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
            let binding = pair_binding
                .map(|catalog| {
                    self.verify_recovery_pair(
                        catalog,
                        host_archive,
                        trust_archive,
                        key.as_slice()
                            .try_into()
                            .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                    )
                })
                .transpose()?;
            let original = self
                .checkpoint_records()
                .map_err(|e| crate::err(e.code()))?;
            let parent = self
                .profile
                .parent()
                .ok_or_else(|| crate::err("PROFILE_PATH_INVALID"))?;
            installations::private_directory(parent)?;
            let stage = parent.join(format!(".missing-host-recovery-{}", uuid::Uuid::new_v4()));
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let mut builder = builder;
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
            if let Some(bound) = &binding {
                if host.manifest_sha256 != bound.host_manifest_sha256 {
                    return Err(crate::err("RECOVERY_PAIR_INVALID"));
                }
                let repeated = self.verify_recovery_pair(
                    pair_binding.ok_or_else(|| crate::err("RECOVERY_PAIR_INVALID"))?,
                    host_archive,
                    trust_archive,
                    key.as_slice()
                        .try_into()
                        .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                )?;
                if &repeated != bound {
                    return Err(crate::err("RECOVERY_PAIR_INVALID"));
                }
            }
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
                pair_binding_verified: binding.is_some(),
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
            self.seal_recovery_pair(
                &stage,
                key.as_slice()
                    .try_into()
                    .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                &host,
                None,
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
    fn observed_binding_writes_only_new_catalog_and_preserves_original_pair_and_authority() {
        let (profile, store, _key, pair) = recovery_fixture();
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        let original = pair.join("pair-binding.bin");
        let output = pair.join("observed-binding.bin");
        let proof = store
            .verify_checkpoint_pair(&original, &host, &trust, &[4; 32])
            .unwrap();
        assert!(!proof.receipt().source_plan_bound);
        let before = (
            fs::read(&host).unwrap(),
            fs::read(&trust).unwrap(),
            fs::read(&original).unwrap(),
            store.current_sha256.clone(),
        );
        let bound = store
            .bind_observed_checkpoint(&proof, &output, &[4; 32])
            .unwrap();
        assert!(bound.receipt().source_plan_bound);
        store
            .recheck_checkpoint_pair(&bound, &output, &host, &trust, &[4; 32])
            .unwrap();
        let bytes = fs::read(&output).unwrap();
        assert!(
            store
                .bind_observed_checkpoint(&proof, &output, &[4; 32])
                .is_err()
        );
        assert_eq!(fs::read(&output).unwrap(), bytes);
        assert!(
            store
                .verify_checkpoint_pair(&output, &host, &trust, &[5; 32])
                .is_err()
        );
        assert_eq!(
            (
                fs::read(&host).unwrap(),
                fs::read(&trust).unwrap(),
                fs::read(&original).unwrap(),
                store.current_sha256.clone()
            ),
            before
        );
        assert_eq!(fs::read_dir(&pair).unwrap().count(), 5); // host,trust,binding,receipt,newcatalog
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
    #[test]
    fn checkpoint_reader_does_not_transition_inflight_intent_and_normal_open_still_recovers() {
        let (profile, mut store, verified) = super::super::super::tests::prepared_fixture();
        store
            .begin_update(super::super::super::tests::observations(), &verified, 21)
            .unwrap();
        let root = store.root.clone();
        let generation = store.current.generation;
        let record = fs::read(root.join("00000000000000000003.json")).unwrap();
        drop(store);
        let reader = super::binding::CheckpointVerifier::open(&profile, "default").unwrap();
        assert_eq!(reader.authority_receipt().generation, generation);
        assert!(!root.join("00000000000000000004.json").exists());
        assert_eq!(
            fs::read(root.join("00000000000000000003.json")).unwrap(),
            record
        );
        assert!(matches!(
            Store::open(&profile, "default"),
            Err(Error::TrustBusy)
        ));
        drop(reader);
        let normal = Store::open(&profile, "default").unwrap();
        assert_eq!(normal.receipt().generation, generation + 1);
        assert_eq!(
            normal.intent().unwrap().update().stage(),
            crate::update::Stage::RecoveryRequired
        );
        let parent = profile.parent().unwrap().to_path_buf();
        drop(normal);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn opaque_current_pair_recheck_refuses_changed_ciphertext_pair_key_and_authority() {
        let (profile, mut store, key_file, pair) = recovery_fixture();
        let key = [4u8; 32];
        let catalog = pair.join("pair-binding.bin");
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        let head = store.current_sha256.clone();
        let original = fs::read(profile.join("witness.bin")).unwrap();
        let proof = store
            .verify_checkpoint_pair(&catalog, &host, &trust, &key)
            .unwrap();
        assert_eq!(proof.receipt().generation, store.current.generation);
        assert!(!proof.receipt().source_plan_bound);
        store
            .recheck_checkpoint_pair(&proof, &catalog, &host, &trust, &key)
            .unwrap();
        assert!(
            store
                .verify_checkpoint_pair(&catalog, &host, &trust, &[5u8; 32])
                .is_err()
        );
        let second = pair.with_extension("proof-second");
        fs::write(profile.join("witness.bin"), b"changed host snapshot").unwrap();
        store
            .checkpoint_host_trust(&key_file, &second, true)
            .unwrap();
        assert!(
            store
                .recheck_checkpoint_pair(
                    &proof,
                    &second.join("pair-binding.bin"),
                    &second.join("host.bin"),
                    &second.join("trust.bin"),
                    &key
                )
                .is_err()
        );
        let mut tampered = fs::read(&host).unwrap();
        let last = tampered.last_mut().unwrap();
        *last ^= 1;
        let bad = pair.join("tampered-host.bin");
        private_file(&bad, true)
            .unwrap()
            .write_all(&tampered)
            .unwrap();
        assert!(
            store
                .recheck_checkpoint_pair(&proof, &catalog, &bad, &trust, &key)
                .is_err()
        );
        assert_eq!(store.current_sha256, head);
        fs::write(profile.join("witness.bin"), &original).unwrap();
        let policy = store.policy().clone();
        store
            .replace_policy(policy, store.current.policy_generation, 22)
            .unwrap();
        assert!(
            store
                .recheck_checkpoint_pair(&proof, &catalog, &host, &trust, &key)
                .is_err()
        );
        assert_eq!(fs::read(profile.join("witness.bin")).unwrap(), original);
        let parent = profile.parent().unwrap().to_path_buf();
        drop(store);
        fs::remove_dir_all(parent).unwrap();
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
    fn bound_pair_refuses_mixed_ciphertexts_tamper_missing_binding_and_new_trust_before_publication()
     {
        let (profile, mut store, key, pair) = recovery_fixture();
        let second = pair.with_extension("second");
        fs::write(profile.join("witness.bin"), b"later host snapshot").unwrap();
        store.checkpoint_host_trust(&key, &second, true).unwrap();
        let original = fs::read(profile.join("witness.bin")).unwrap();
        let old = profile.with_extension("quarantined");
        fs::rename(&profile, &old).unwrap();
        let binding = pair.join("pair-binding.bin");
        let call = |host: &Path, trust: &Path, catalog: &Path| {
            store.restore_bound_missing_host(host, trust, &key, catalog, true)
        };
        for (host, trust) in [
            (second.join("host.bin"), pair.join("trust.bin")),
            (pair.join("host.bin"), second.join("trust.bin")),
        ] {
            assert_eq!(
                call(&host, &trust, &binding).unwrap_err().code,
                "RECOVERY_PAIR_INVALID"
            );
            assert!(!profile.exists());
        }
        let raw = fs::read(&binding).unwrap();
        // Correctly authenticated but unsupported schema is still rejected.
        use aes_gcm::{
            Aes256Gcm, Nonce,
            aead::{Aead, KeyInit, Payload},
        };
        let domain = b"ExhibitOS-recovery-pair-v1\0";
        let cipher = Aes256Gcm::new_from_slice(&[4u8; 32]).unwrap();
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&raw[domain.len()..domain.len() + 12]),
                Payload {
                    msg: &raw[domain.len() + 12..],
                    aad: domain,
                },
            )
            .unwrap();
        let mut unsupported: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        unsupported["unrecognizedField"] = serde_json::json!(true);
        let encoded = serde_json::to_vec(&unsupported).unwrap();
        let nonce = [1u8; 12]; // unique within this fresh synthetic key fixture
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &encoded,
                    aad: domain,
                },
            )
            .unwrap();
        let bad_schema = pair.join("unknown-schema");
        let mut file = private_file(&bad_schema, true).unwrap();
        file.write_all(domain).unwrap();
        file.write_all(&nonce).unwrap();
        file.write_all(&sealed).unwrap();
        drop(file);
        assert_eq!(
            call(&pair.join("host.bin"), &pair.join("trust.bin"), &bad_schema)
                .unwrap_err()
                .code,
            "RECOVERY_PAIR_INVALID"
        );
        assert!(!profile.exists());
        for kind in ["tamper", "truncate", "extra", "domain"] {
            let bad = pair.join(format!("binding-{kind}"));
            let mut bytes = raw.clone();
            match kind {
                "tamper" => *bytes.last_mut().unwrap() ^= 1,
                "truncate" => {
                    bytes.pop();
                }
                "extra" => bytes.push(0),
                _ => bytes[0] ^= 1,
            }
            private_file(&bad, true).unwrap().write_all(&bytes).unwrap();
            assert_eq!(
                call(&pair.join("host.bin"), &pair.join("trust.bin"), &bad)
                    .unwrap_err()
                    .code,
                "RECOVERY_PAIR_INVALID"
            );
            assert!(!profile.exists());
        }
        assert_eq!(
            call(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &pair.join("missing")
            )
            .unwrap_err()
            .code,
            "RECOVERY_PAIR_INVALID"
        );
        assert!(
            store
                .verify_recovery_pair(
                    &binding,
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                    &[9u8; 32]
                )
                .is_err()
        );
        let alias = pair.join("linked-binding");
        fs::hard_link(&binding, &alias).unwrap();
        assert!(
            store
                .verify_recovery_pair(
                    &alias,
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                    &[4u8; 32]
                )
                .is_err()
        );
        fs::remove_file(alias).unwrap();
        let parent = profile.parent().unwrap().to_owned();
        assert_eq!(fs::read(old.join("witness.bin")).unwrap(), original);
        store
            .replace_policy(
                store.current.policy.clone(),
                store.current.policy_generation,
                22,
            )
            .unwrap();
        assert_eq!(
            store
                .restore_bound_missing_host(
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                    &key,
                    &binding,
                    true
                )
                .unwrap_err()
                .code,
            "RECOVERY_PAIR_INVALID"
        );
        assert!(!profile.exists());
        drop(store);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn bound_pair_restores_exact_host_and_refuses_wrong_source_provenance() {
        let (profile, store, key, pair) = recovery_fixture();
        let original = store.current_sha256.clone();
        let bound = store
            .verify_recovery_pair(
                &pair.join("pair-binding.bin"),
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &[4u8; 32],
            )
            .unwrap();
        let bad = pair.with_extension("wrong-source");
        installations::new_directory(&bad).unwrap();
        fs::copy(pair.join("host.bin"), bad.join("host.bin")).unwrap();
        fs::copy(pair.join("trust.bin"), bad.join("trust.bin")).unwrap();
        let mut plan = store.intent().unwrap().update.plan().clone();
        plan.source_inventory = "0".repeat(64);
        let host = profile_backup::HostReceipt {
            id: "test".into(),
            operation: "host-profile-checkpoint".into(),
            files: 2,
            bytes: 1,
            manifest_sha256: bound.host_manifest_sha256,
            external_volumes_saved: false,
            host_writer_quiescence: "operator-acknowledged".into(),
        };
        store
            .seal_recovery_pair(
                &bad,
                &[4u8; 32],
                &host,
                Some(super::super::binding::SourceBinding::from_plan(&plan)),
            )
            .unwrap();
        let old = profile.with_extension("quarantined");
        fs::rename(&profile, &old).unwrap();
        assert_eq!(
            store
                .restore_bound_missing_host(
                    &bad.join("host.bin"),
                    &bad.join("trust.bin"),
                    &key,
                    &bad.join("pair-binding.bin"),
                    true
                )
                .unwrap_err()
                .code,
            "RECOVERY_PAIR_INVALID"
        );
        assert!(!profile.exists());
        let receipt = store
            .restore_bound_missing_host(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &key,
                &pair.join("pair-binding.bin"),
                true,
            )
            .unwrap();
        assert!(receipt.pair_binding_verified);
        assert!(receipt.host_profile_restored);
        assert!(!receipt.runtime_data_restored);
        assert_eq!(store.current_sha256, original);
        assert_eq!(
            fs::read(profile.join("witness.bin")).unwrap(),
            fs::read(old.join("witness.bin")).unwrap()
        );
        let parent = profile.parent().unwrap().to_owned();
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
