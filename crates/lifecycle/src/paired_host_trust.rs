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
    pub historical_checkpoint: Option<super::binding::RollbackCheckpointReceipt>,
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
        self.restore_host_pair(
            host_archive,
            trust_archive,
            key_file,
            None,
            false,
            apps_closed,
        )
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
            false,
            apps_closed,
        )
    }
    /// Restore absent host bytes from a same-plan historical Prepared checkpoint,
    /// retaining the independently present latest authority without rewinding it.
    /// Runtime data and lost-authority recovery remain separate operations.
    pub fn restore_rollback_missing_host(
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
            true,
            apps_closed,
        )
    }
    fn restore_host_pair(
        &self,
        host_archive: &Path,
        trust_archive: &Path,
        key_file: &Path,
        pair_binding: Option<&Path>,
        historical: bool,
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
            let key_array: &[u8; 32] = key
                .as_slice()
                .try_into()
                .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?;
            let rollback = if historical {
                Some(self.verify_rollback_checkpoint_pair(
                    pair_binding.ok_or_else(|| crate::err("RECOVERY_PAIR_INVALID"))?,
                    host_archive,
                    trust_archive,
                    key_array,
                )?)
            } else {
                None
            };
            let binding = pair_binding
                .filter(|_| !historical)
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
            // The old trust archive is authenticated provenance, never the authority
            // to activate. Independently retained latest records supply this new small
            // checkpoint and exact inactive extraction under the same Store fence.
            let latest_archive = if rollback.is_some() {
                let path = stage.join("latest-trust.bin");
                self.archive_trust_checkpoint(&path, key_array)
                    .map_err(|e| crate::err(e.code()))?;
                Some(path)
            } else {
                None
            };
            let trust = self
                .extract_trust_checkpoint(
                    latest_archive.as_deref().unwrap_or(trust_archive),
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
            if let Some(proof) = &rollback {
                if host.manifest_sha256 != proof.receipt().checkpoint.host_manifest_sha256 {
                    return Err(crate::err("RECOVERY_PAIR_INVALID"));
                }
                self.recheck_rollback_checkpoint_pair(
                    proof,
                    pair_binding.ok_or_else(|| crate::err("RECOVERY_PAIR_INVALID"))?,
                    host_archive,
                    trust_archive,
                    key_array,
                )?;
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
                pair_binding_verified: binding.is_some() || rollback.is_some(),
                trust_authority_restored: false,
                runtime_data_restored: false,
                runtime_started: false,
                recovery_workspace: stage.clone(),
                historical_checkpoint: rollback.as_ref().map(|proof| proof.receipt()),
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
        let (profile, store, key, pair, _) = recovery_fixture_with_release(false);
        (profile, store, key, pair)
    }
    fn recovery_fixture_with_release(
        large: bool,
    ) -> (PathBuf, Store, PathBuf, PathBuf, VerifiedRelease) {
        let (profile, store, release) = super::super::super::tests::prepared_fixture();
        drop(store);
        let controller =
            crate::installations::InstallationController::new(profile.clone(), None).unwrap();
        controller.context().unwrap();
        drop(controller);
        if large {
            fs::write(profile.join("witness.bin"), vec![b'W'; 65 * 1024 * 1024]).unwrap();
        } else {
            fs::write(profile.join("witness.bin"), b"original host witness").unwrap();
        }
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
        (profile, store, key, pair, release)
    }
    fn bound_recovering_fixture(large: bool) -> (PathBuf, Store, PathBuf, PathBuf, PathBuf) {
        let (profile, mut store, key, pair, release) = recovery_fixture_with_release(large);
        let catalog = pair.join("source-bound.bin");
        let proof = store
            .verify_checkpoint_pair(
                &pair.join("pair-binding.bin"),
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &[4; 32],
            )
            .unwrap();
        // Synthetic storage adapter only; does not attest external Engine data.
        store
            .bind_observed_checkpoint(&proof, &catalog, &[4; 32])
            .unwrap();
        store
            .begin_update(super::super::super::tests::observations(), &release, 21)
            .unwrap();
        let operation = store.intent().unwrap().update.plan().operation_id.clone();
        store
            .update_failed(&operation, store.current.generation, 22)
            .unwrap();
        let mut policy = store.current.policy.clone();
        policy.public_keys = vec![
            ed25519_dalek::SigningKey::from_bytes(&[32; 32])
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ];
        policy.minimum_sequence = store.receipt().minimum_sequence + 1;
        store
            .replace_policy(policy, store.current.policy_generation, 23)
            .unwrap();
        (profile, store, key, pair, catalog)
    }
    #[test]
    fn rollback_missing_host_publishes_original_bytes_with_newer_revoked_authority() {
        let (profile, store, key, pair, catalog) = bound_recovering_fixture(false);
        let quarantine = profile.with_extension("original-retained");
        let registry = fs::read(profile.join("installation-selection.json")).unwrap();
        let before = store.checkpoint_records().unwrap();
        let head = store.current_sha256.clone();
        let current = serde_json::to_vec(&store.current).unwrap();
        let inputs: Vec<_> = [
            &key,
            &catalog,
            &pair.join("host.bin"),
            &pair.join("trust.bin"),
        ]
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect();
        fs::rename(&profile, &quarantine).unwrap();
        assert!(
            store
                .restore_bound_missing_host(
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                    &key,
                    &catalog,
                    true
                )
                .is_err()
        );
        assert!(!profile.exists());
        let receipt = store
            .restore_rollback_missing_host(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &key,
                &catalog,
                true,
            )
            .unwrap();
        assert_eq!(
            fs::read(profile.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        assert_eq!(
            fs::read(quarantine.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        assert_eq!(
            fs::read(profile.join("installation-selection.json")).unwrap(),
            registry
        );
        assert!(
            receipt.host_profile_restored
                && receipt.current_trust_preserved
                && receipt.pair_binding_verified
        );
        assert!(
            !receipt.trust_authority_restored
                && !receipt.runtime_data_restored
                && !receipt.runtime_started
        );
        assert_eq!(
            receipt
                .historical_checkpoint
                .as_ref()
                .unwrap()
                .checkpoint
                .generation,
            2
        );
        assert_eq!(receipt.retained_trust.generation, 5);
        assert!(
            receipt
                .recovery_workspace
                .join("latest-trust.bin")
                .is_file()
        );
        assert!(receipt.recovery_workspace.join("completed.json").is_file());
        assert_eq!(before, store.checkpoint_records().unwrap());
        assert_eq!(head, store.current_sha256);
        assert_eq!(current, serde_json::to_vec(&store.current).unwrap());
        assert!(!store.current.revoked_keys.is_empty());
        assert_eq!(store.receipt().minimum_sequence, 3);
        assert_eq!(
            inputs,
            [
                &key,
                &catalog,
                &pair.join("host.bin"),
                &pair.join("trust.bin")
            ]
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>()
        );
        drop(store);
        let reopened = Store::open(&profile, "default").unwrap();
        assert_eq!(before, reopened.checkpoint_records().unwrap());
        assert_eq!(
            reopened.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        drop(reopened);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
    #[test]
    fn rollback_missing_host_refusals_never_publish_or_change_inputs() {
        let (profile, store, key, pair, catalog) = bound_recovering_fixture(false);
        let run = |binding: &Path, ack| {
            store.restore_rollback_missing_host(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &key,
                binding,
                ack,
            )
        };
        assert_eq!(
            run(&catalog, false).unwrap_err().code,
            "HOST_WRITER_ACK_REQUIRED"
        );
        assert_eq!(
            run(&catalog, true).unwrap_err().code,
            "HOST_RESTORE_TARGET_EXISTS"
        );
        let quarantine = profile.with_extension("original-retained");
        fs::rename(&profile, &quarantine).unwrap();
        let before = store.checkpoint_records().unwrap();
        let parent = profile.parent().unwrap();
        let names: BTreeSet<_> = fs::read_dir(parent)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            run(&pair.join("pair-binding.bin"), true).unwrap_err().code,
            "RECOVERY_PAIR_INVALID"
        );
        let bad = pair.join("bad.bin");
        let mut bytes = fs::read(&catalog).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        private_file(&bad, true).unwrap().write_all(&bytes).unwrap();
        assert_eq!(run(&bad, true).unwrap_err().code, "RECOVERY_PAIR_INVALID");
        assert!(!profile.exists());
        assert_eq!(before, store.checkpoint_records().unwrap());
        assert_eq!(
            names,
            fs::read_dir(parent)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect()
        );
        assert_eq!(
            fs::read(quarantine.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        drop(store);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    #[ignore = "requires built release CLI and 65MiB synthetic disk budget"]
    fn rollback_missing_host_cli65_preserves_revocation_floor_and_original_bytes() {
        use std::os::unix::fs::PermissionsExt;
        let cli = PathBuf::from(
            std::env::var("EXHIBITOS_ROLLBACK_TEST_CLI")
                .expect("explicit built release CLI required"),
        );
        assert!(cli.is_absolute() && cli.is_file());
        let (profile, store, key, pair, catalog) = bound_recovering_fixture(true);
        let before = store.checkpoint_records().unwrap();
        let head = store.current_sha256.clone();
        let input_paths = [
            &key,
            &catalog,
            &pair.join("host.bin"),
            &pair.join("trust.bin"),
        ];
        let input_hashes: Vec<_> = input_paths
            .iter()
            .map(|p| hash(&fs::read(p).unwrap()))
            .collect();
        let registry = fs::read(profile.join("installation-selection.json")).unwrap();
        let quarantine = profile.with_extension("original-retained");
        fs::rename(&profile, &quarantine).unwrap();
        drop(store);
        let output = std::process::Command::new(&cli)
            .args(["restore-rollback-missing-host", "--profile"])
            .arg(&profile)
            .args(["--installation", "default", "--host-archive"])
            .arg(pair.join("host.bin"))
            .arg("--trust-archive")
            .arg(pair.join("trust.bin"))
            .arg("--key")
            .arg(&key)
            .arg("--pair-binding")
            .arg(&catalog)
            .args(["--absent-original-profile", "--apps-closed"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty());
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["retainedTrust"]["generation"], 5);
        assert_eq!(
            receipt["historicalCheckpoint"]["checkpoint"]["generation"],
            2
        );
        assert_eq!(receipt["currentTrustPreserved"], true);
        for field in [
            "trustAuthorityRestored",
            "runtimeDataRestored",
            "runtimeStarted",
        ] {
            assert_eq!(receipt[field], false);
        }
        let witness = fs::read(profile.join("witness.bin")).unwrap();
        assert_eq!(witness.len(), 65 * 1024 * 1024);
        assert_eq!(witness, fs::read(quarantine.join("witness.bin")).unwrap());
        assert_eq!(
            fs::metadata(profile.join("witness.bin"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let reopened = Store::open(&profile, "default").unwrap();
        assert_eq!(reopened.checkpoint_records().unwrap(), before);
        assert_eq!(reopened.current_sha256, head);
        assert_eq!(reopened.receipt().minimum_sequence, 3);
        assert!(!reopened.current.revoked_keys.is_empty());
        drop(reopened);
        assert_eq!(
            fs::read(profile.join("installation-selection.json")).unwrap(),
            registry
        );
        assert_eq!(
            input_hashes,
            input_paths
                .iter()
                .map(|p| hash(&fs::read(p).unwrap()))
                .collect::<Vec<_>>()
        );
        assert_eq!(receipt["historicalCheckpoint"]["retainedGeneration"], 5);
        let stage = PathBuf::from(receipt["recoveryWorkspace"].as_str().unwrap());
        assert!(stage.join("completed.json").is_file());
        assert!(!stage.join("host/payload.pending").exists());
        assert!(!stage.join("host/profile").exists());
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
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
    fn published_host_process_crash_worker() {
        let Ok(profile) = std::env::var("EXHIBITOS_SYNTHETIC_PUBLISHED_HOST_PROFILE") else {
            return;
        };
        let profile = PathBuf::from(profile);
        let parent = profile.parent().unwrap();
        assert!(
            parent
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("exhibitos-release-trust-")
        );
        assert!(!profile.exists());
        let store = Store::open(&profile, "default").unwrap();
        let pair = parent.join("recovery-pair");
        let receipt = store
            .restore_bound_missing_host(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &parent.join("recovery-key"),
                &pair.join("pair-binding.bin"),
                true,
            )
            .unwrap();
        assert!(
            receipt.host_profile_restored
                && receipt.current_trust_preserved
                && receipt.pair_binding_verified
                && !receipt.runtime_data_restored
                && !receipt.runtime_started
                && !receipt.trust_authority_restored
        );
        let pending = parent.join("published-host-ready.pending");
        let mut ready = private_file(&pending, true).unwrap();
        ready
            .write_all(&serde_json::to_vec(&receipt).unwrap())
            .unwrap();
        ready.sync_all().unwrap();
        publish(&pending, &parent.join("published-host-ready.json")).unwrap();
        // Real SIGKILL from parent while the Store still owns kernel fences.
        // This uses production publication, not an injected product crash hook.
        loop {
            std::thread::park();
        }
    }
    #[test]
    fn published_host_sigkill_preserves_exact_tree_and_cold_authority_and_refuses_overwrite() {
        use std::os::unix::{fs::PermissionsExt, process::ExitStatusExt};
        let (profile, store, key, pair) = recovery_fixture();
        let head = store.current_sha256.clone();
        let current = serde_json::to_vec(&store.current).unwrap();
        let records = store.checkpoint_records().unwrap();
        let ids = (store.used_operations.clone(), store.used_instances.clone());
        drop(store);
        let parent = profile.parent().unwrap();
        let held = parent.join("original-held");
        fs::rename(&profile, &held).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "signed_release::trust::trust_checkpoint::paired::tests::published_host_process_crash_worker", "--nocapture"])
            .env("EXHIBITOS_SYNTHETIC_PUBLISHED_HOST_PROFILE", &profile)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().unwrap();
        let ready = parent.join("published-host-ready.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // The marker is created only after the production method has synced
        // publication and returned its authenticated completion receipt.
        let marker_ready = ready.exists();
        let kill_result = child.kill();
        let status = child.wait().unwrap();
        assert!(
            marker_ready,
            "publication worker did not complete: {status}"
        );
        kill_result.unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(ready).unwrap()).unwrap();
        let workspace = PathBuf::from(receipt["recoveryWorkspace"].as_str().unwrap());
        assert!(workspace.join("completed.json").is_file());
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(workspace.join("host/manifest.json")).unwrap())
                .unwrap();
        for entry in manifest["items"].as_array().unwrap() {
            let relative = entry["path"].as_str().unwrap();
            let original = held.join(relative);
            let restored = profile.join(relative);
            assert_eq!(
                fs::symlink_metadata(&restored)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                entry["mode"].as_u64().unwrap() as u32
            );
            if entry["kind"] == "file" {
                let original_bytes = fs::read(original).unwrap();
                assert_eq!(fs::read(restored).unwrap(), original_bytes);
                assert_eq!(hash(&original_bytes), entry["sha256"].as_str().unwrap());
            } else {
                assert!(restored.is_dir());
            }
        }
        let cold = Store::open(&profile, "default").unwrap();
        assert_eq!(cold.current_sha256, head);
        assert_eq!(serde_json::to_vec(&cold.current).unwrap(), current);
        assert_eq!(cold.checkpoint_records().unwrap(), records);
        assert_eq!(
            (cold.used_operations.clone(), cold.used_instances.clone()),
            ids
        );
        assert_eq!(
            cold.restore_bound_missing_host(
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &key,
                &pair.join("pair-binding.bin"),
                true
            )
            .unwrap_err()
            .code,
            "HOST_RESTORE_TARGET_EXISTS"
        );
        assert_eq!(
            fs::read(profile.join("witness.bin")).unwrap(),
            b"original host witness"
        );
        drop(cold);
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
