// SPDX-License-Identifier: Apache-2.0
//! Authenticated identity of one host/trust checkpoint; not lost-authority recovery.
use super::*;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
const DOMAIN: &[u8] = b"ExhibitOS-recovery-pair-v1\0";
const LIMIT: usize = 16 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceBinding {
    operation_id: String,
    source_instance: String,
    target_instance: String,
    backup_id: String,
    backup_manifest: String,
    source_inventory: String,
    source_schema: String,
}
impl SourceBinding {
    pub(crate) fn from_plan(p: &crate::update::Plan) -> Self {
        Self {
            operation_id: p.operation_id.clone(),
            source_instance: p.source_instance.clone(),
            target_instance: p.target_instance.clone(),
            backup_id: p.backup_id.clone(),
            backup_manifest: p.backup_manifest.clone(),
            source_inventory: p.source_inventory.clone(),
            source_schema: p.source_schema.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveIdentity {
    bytes: u64,
    sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairBinding {
    format: u8,
    scope: String,
    generation: u64,
    head_sha256: String,
    host: ArchiveIdentity,
    trust: ArchiveIdentity,
    pub(crate) host_manifest_sha256: String,
    source: Option<SourceBinding>,
}
/// Opaque current pair identity. Cannot be deserialized from a saved receipt.
/// Recheck the ciphertexts and current Store before using it in an executor.
#[derive(Debug)]
pub struct VerifiedCheckpointPair {
    binding: PairBinding,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointPairReceipt {
    pub generation: u64,
    pub head_sha256: String,
    pub host_manifest_sha256: String,
    pub host_archive_sha256: String,
    pub host_archive_bytes: u64,
    pub trust_archive_sha256: String,
    pub trust_archive_bytes: u64,
    pub source_plan_bound: bool,
}
impl VerifiedCheckpointPair {
    /// Authentication/identity only: not extraction, authority recovery or preflight.
    pub fn receipt(&self) -> CheckpointPairReceipt {
        let b = &self.binding;
        CheckpointPairReceipt {
            generation: b.generation,
            head_sha256: b.head_sha256.clone(),
            host_manifest_sha256: b.host_manifest_sha256.clone(),
            host_archive_sha256: b.host.sha256.clone(),
            host_archive_bytes: b.host.bytes,
            trust_archive_sha256: b.trust.sha256.clone(),
            trust_archive_bytes: b.trust.bytes,
            source_plan_bound: b.source.is_some(),
        }
    }
}
/// Historical Prepared baseline authenticated against the retained complete chain.
/// This cannot authorize trust activation, host/data restoration or update execution.
#[derive(Debug)]
pub struct VerifiedRollbackCheckpointPair {
    binding: PairBinding,
    retained_generation: u64,
    retained_head_sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackCheckpointReceipt {
    pub checkpoint: CheckpointPairReceipt,
    pub retained_generation: u64,
    pub retained_head_sha256: String,
    pub prepared_history_prefix_verified: bool,
    pub live_trust_restored: bool,
}
impl VerifiedRollbackCheckpointPair {
    pub fn receipt(&self) -> RollbackCheckpointReceipt {
        RollbackCheckpointReceipt {
            checkpoint: VerifiedCheckpointPair {
                binding: self.binding.clone(),
            }
            .receipt(),
            retained_generation: self.retained_generation,
            retained_head_sha256: self.retained_head_sha256.clone(),
            prepared_history_prefix_verified: true,
            live_trust_restored: false,
        }
    }
}
/// Read-only current authority for checkpoint verification. No mutable Store,
/// activation or journal transition is exposed; normal Store opens still recover crashes.
pub struct CheckpointVerifier {
    store: Store,
}
impl CheckpointVerifier {
    pub fn open(profile: &Path, installation: &str) -> Result<Self, Error> {
        Ok(Self {
            store: Store::open_mode(profile, installation, false)?,
        })
    }
    /// Read-only historical baseline qualification; does not recover an in-flight journal.
    pub fn verify_rollback(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<VerifiedRollbackCheckpointPair> {
        self.store
            .verify_rollback_checkpoint_pair(catalog, host, trust, key)
    }
    pub fn recheck_rollback(
        &self,
        proof: &VerifiedRollbackCheckpointPair,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<()> {
        self.store
            .recheck_rollback_checkpoint_pair(proof, catalog, host, trust, key)
    }
    pub fn authority_receipt(&self) -> TrustReceipt {
        self.store.receipt()
    }
    pub fn verify(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<VerifiedCheckpointPair> {
        self.store.verify_checkpoint_pair(catalog, host, trust, key)
    }
    pub fn recheck(
        &self,
        proof: &VerifiedCheckpointPair,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<()> {
        self.store
            .recheck_checkpoint_pair(proof, catalog, host, trust, key)
    }
}
fn failure() -> crate::LifecycleError {
    crate::err("RECOVERY_PAIR_INVALID")
}
#[cfg(unix)]
fn archive(path: &Path) -> crate::Result<ArchiveIdentity> {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() || fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err(failure());
    }
    let mut file = private_file(path, false).map_err(|_| failure())?;
    let before = file.metadata().map_err(|_| failure())?;
    // Covers the existing64GiB host payload plus manifest/framing; not a smaller
    // replacement for host coverage. Stream only bounded chunks, no archive copy.
    if before.len() == 0 || before.len() > 66 * 1024 * 1024 * 1024 {
        return Err(failure());
    }
    let mut h = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|_| failure())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > before.len() {
            return Err(failure());
        }
        h.update(&buf[..n]);
    }
    let after = file.metadata().map_err(|_| failure())?;
    let current = fs::symlink_metadata(path).map_err(|_| failure())?;
    let state = |m: &Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.uid(),
            m.mode(),
            m.nlink(),
        )
    };
    if count != before.len()
        || current.is_symlink()
        || state(&before) != state(&after)
        || state(&after) != state(&current)
    {
        return Err(failure());
    }
    Ok(ArchiveIdentity {
        bytes: count,
        sha256: format!("{:x}", h.finalize()),
    })
}
#[cfg(not(unix))]
fn archive(_path: &Path) -> crate::Result<ArchiveIdentity> {
    Err(crate::err("BACKUP_PLATFORM_UNVERIFIED"))
}
impl Store {
    /// Produces an opaque proof under this Store's retained current authority.
    /// No filesystem output, extraction or journal transition is performed.
    pub fn verify_checkpoint_pair(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<VerifiedCheckpointPair> {
        let binding = self.verify_recovery_pair(catalog, host, trust, key)?;
        self.check_root().map_err(|_| failure())?;
        Ok(VerifiedCheckpointPair { binding })
    }
    /// Saved diagnostic receipts cannot supply this proof. Rechecks current
    /// authority/provenance and every ciphertext byte, not just recorded hashes.
    pub fn recheck_checkpoint_pair(
        &self,
        proof: &VerifiedCheckpointPair,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<()> {
        if self
            .verify_checkpoint_pair(catalog, host, trust, key)?
            .binding
            != proof.binding
        {
            return Err(failure());
        }
        Ok(())
    }
    /// Refresh only the small trust archive/catalog after full read-only host verification.
    /// `previous` is [catalog, host archive, trust archive]. The host remains externally
    /// retained; no source binding is inherited. Only unchanged Prepared history is accepted.
    #[cfg(unix)]
    pub fn refresh_checkpoint_trust(
        &self,
        previous: [&Path; 3],
        key_file: &Path,
        destination: &Path,
        writers_stopped: bool,
    ) -> crate::Result<CheckpointPairReceipt> {
        use crate::update::Stage;
        let [previous_catalog, host, previous_trust] = previous;
        self.check_root().map_err(|_| failure())?;
        if !writers_stopped {
            return Err(crate::err("HOST_WRITER_ACK_REQUIRED"));
        }
        let parent = destination.parent().ok_or_else(failure)?;
        if !destination.is_absolute()
            || destination.exists()
            || fs::canonicalize(parent).ok().as_deref() != Some(parent)
            || destination.starts_with(&self.profile)
            || destination.starts_with(&self.root)
            || !key_file.is_absolute()
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file)
            || key_file.starts_with(&self.profile)
            || key_file.starts_with(&self.root)
        {
            return Err(failure());
        }
        installations::private_directory(parent)?;
        let mut key = read_record(key_file).map_err(|_| failure())?;
        let result = (|| {
            let key_array: &[u8; 32] = key.as_slice().try_into().map_err(|_| failure())?;
            let previous = read_binding(previous_catalog, key_array)?;
            let records = self.checkpoint_records().map_err(|_| failure())?;
            let index = usize::try_from(previous.generation.checked_sub(1).ok_or_else(failure)?)
                .map_err(|_| failure())?;
            let (_, old_bytes) = records.get(index).ok_or_else(failure)?;
            let old: Record = serde_json::from_slice(old_bytes).map_err(|_| failure())?;
            let intent = self.intent().ok_or_else(failure)?;
            if previous.format != 1
                || previous.scope != self.scope
                || previous.head_sha256 != hash(old_bytes)
                || previous.host != archive(host)?
                || previous.trust != archive(previous_trust)?
                || !crate::hash_valid(&previous.host_manifest_sha256)
                || old.intent.as_ref().is_none_or(|i| {
                    i.update.stage() != Stage::Prepared || i.update.plan() != intent.update.plan()
                })
                || records.iter().skip(index).any(|(_, bytes)| {
                    serde_json::from_slice::<Record>(bytes)
                        .ok()
                        .is_none_or(|r| {
                            r.intent.as_ref().is_none_or(|i| {
                                i.update.stage() != Stage::Prepared
                                    || i.update.plan() != intent.update.plan()
                            })
                        })
                })
            {
                return Err(failure());
            }
            let session = profile_backup::anchored_session(
                &self.profile,
                self._anchor.try_clone().map_err(|_| failure())?,
                true,
            )?;
            let _legacy_locks = profile_backup::current_host_locks(&self.profile, &session)?;
            let observe = || {
                profile_backup::verify_host_current_borrowed(
                    &self.profile,
                    host,
                    key_array,
                    &session,
                    &previous.host_manifest_sha256,
                )
            };
            observe()?;
            let stage = parent.join(format!("pending-trust-refresh-{}", uuid::Uuid::new_v4()));
            installations::new_directory(&stage)?;
            self.archive_trust_checkpoint(&stage.join("trust.bin"), key_array)
                .map_err(|_| failure())?;
            observe()?;
            if read_record(key_file).map_err(|_| failure())? != key
                || self.checkpoint_records().map_err(|_| failure())? != records
                || read_binding(previous_catalog, key_array)? != previous
                || archive(host)? != previous.host
                || archive(previous_trust)? != previous.trust
            {
                return Err(failure());
            }
            let binding = PairBinding {
                format: 1,
                scope: self.scope.clone(),
                generation: self.current.generation,
                head_sha256: self.current_sha256.clone(),
                host: previous.host.clone(),
                trust: archive(&stage.join("trust.bin"))?,
                host_manifest_sha256: previous.host_manifest_sha256.clone(),
                source: None,
            };
            write_binding(&binding, &stage.join("pair-binding.bin"), key_array)?;
            observe()?;
            if read_record(key_file).map_err(|_| failure())? != key
                || self.checkpoint_records().map_err(|_| failure())? != records
                || read_binding(previous_catalog, key_array)? != previous
                || archive(host)? != previous.host
                || archive(previous_trust)? != previous.trust
            {
                return Err(failure());
            }
            self.check_root().map_err(|_| failure())?;
            session.check_exclusive(&self.profile)?;
            sync_dir(&stage).map_err(|_| failure())?;
            publish(&stage, destination).map_err(|_| failure())?;
            sync_dir(parent).map_err(|_| failure())?;
            self.verify_checkpoint_pair(
                &destination.join("pair-binding.bin"),
                host,
                &destination.join("trust.bin"),
                key_array,
            )
            .map(|proof| proof.receipt())
        })();
        key.fill(0);
        result
    }
    /// Authenticate an older Prepared checkpoint for the SAME currently recovering
    /// plan. Its head must be an exact prefix of the independently retained chain.
    /// Never substitute this proof for an exact-current checkpoint or preflight.
    pub fn verify_rollback_checkpoint_pair(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<VerifiedRollbackCheckpointPair> {
        use crate::update::Stage;
        self.check_root().map_err(|_| failure())?;
        let b = read_binding(catalog, key)?;
        let current = self.intent().ok_or_else(failure)?;
        if b.format != 1
            || b.scope != self.scope
            || b.generation == 0
            || b.generation >= self.current.generation
            || !crate::hash_valid(&b.host_manifest_sha256)
            || b.source.as_ref() != Some(&SourceBinding::from_plan(current.update.plan()))
            || !matches!(
                current.update.stage(),
                Stage::RecoveryRequired | Stage::Restoring | Stage::AwaitingRollbackHealth
            )
            || b.host != archive(host)?
            || b.trust != archive(trust)?
        {
            return Err(failure());
        }
        // checkpoint_records validates every transition, floor, revocation and
        // reserved operation/instance through the current head, not just a file name.
        let records = self.checkpoint_records().map_err(|_| failure())?;
        let index = usize::try_from(b.generation - 1).map_err(|_| failure())?;
        let (_, bytes) = records.get(index).ok_or_else(failure)?;
        let prepared: Record = serde_json::from_slice(bytes).map_err(|_| failure())?;
        let old = prepared.intent.as_ref().ok_or_else(failure)?;
        if hash(bytes) != b.head_sha256
            || old.update.stage() != Stage::Prepared
            || old.update.plan() != current.update.plan()
        {
            return Err(failure());
        }
        // No discarded intent, different plan or new update episode may intervene.
        for (_, bytes) in records.iter().skip(index + 1) {
            let record: Record = serde_json::from_slice(bytes).map_err(|_| failure())?;
            if record
                .intent
                .as_ref()
                .is_none_or(|i| i.update.plan() != old.update.plan())
            {
                return Err(failure());
            }
        }
        self.check_root().map_err(|_| failure())?;
        Ok(VerifiedRollbackCheckpointPair {
            binding: b,
            retained_generation: self.current.generation,
            retained_head_sha256: self.current_sha256.clone(),
        })
    }
    /// Any intervening retained-authority change invalidates an earlier opaque proof.
    pub fn recheck_rollback_checkpoint_pair(
        &self,
        proof: &VerifiedRollbackCheckpointPair,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<()> {
        let fresh = self.verify_rollback_checkpoint_pair(catalog, host, trust, key)?;
        if fresh.binding != proof.binding
            || fresh.retained_generation != proof.retained_generation
            || fresh.retained_head_sha256 != proof.retained_head_sha256
        {
            return Err(failure());
        }
        Ok(())
    }
    pub(crate) fn seal_recovery_pair(
        &self,
        stage: &Path,
        key: &[u8; 32],
        host: &profile_backup::HostReceipt,
        source: Option<SourceBinding>,
    ) -> crate::Result<()> {
        self.check_root().map_err(|_| failure())?;
        installations::private_directory(stage)?;
        if fs::canonicalize(stage).ok().as_deref() != Some(stage)
            || !crate::hash_valid(&host.manifest_sha256)
        {
            return Err(failure());
        }
        let binding = PairBinding {
            format: 1,
            scope: self.scope.clone(),
            generation: self.current.generation,
            head_sha256: self.current_sha256.clone(),
            host: archive(&stage.join("host.bin"))?,
            trust: archive(&stage.join("trust.bin"))?,
            host_manifest_sha256: host.manifest_sha256.clone(),
            source,
        };
        write_binding(&binding, &stage.join("pair-binding.bin"), key)?;
        self.check_root().map_err(|_| failure())?;
        Ok(())
    }
    /// Internal finalizer only. The caller has just performed current host and
    /// complete source/candidate observations plus all common final guards.
    /// Saved JSON receipts cannot invoke this private publication boundary.
    pub(crate) fn bind_observed_checkpoint(
        &self,
        proof: &VerifiedCheckpointPair,
        catalog: &Path,
        key: &[u8; 32],
    ) -> crate::Result<VerifiedCheckpointPair> {
        self.check_root().map_err(|_| failure())?;
        if proof.binding.scope != self.scope
            || proof.binding.generation != self.current.generation
            || proof.binding.head_sha256 != self.current_sha256
        {
            return Err(failure());
        }
        let mut binding = proof.binding.clone();
        binding.source = Some(SourceBinding::from_plan(
            self.intent().ok_or_else(failure)?.update.plan(),
        ));
        write_binding(&binding, catalog, key)?;
        self.check_root().map_err(|_| failure())?;
        Ok(VerifiedCheckpointPair { binding })
    }
    pub(crate) fn verify_recovery_pair(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<PairBinding> {
        self.check_root().map_err(|_| failure())?;
        let b = read_binding(catalog, key)?;
        if b.format != 1
            || b.scope != self.scope
            || b.generation != self.current.generation
            || b.head_sha256 != self.current_sha256
            || !crate::hash_valid(&b.host_manifest_sha256)
            || b.host != archive(host)?
            || b.trust != archive(trust)?
            || b.source.as_ref().is_some_and(|s| {
                self.intent()
                    .is_none_or(|i| *s != SourceBinding::from_plan(i.update.plan()))
            })
        {
            return Err(failure());
        }
        Ok(b)
    }
}

fn read_binding(catalog: &Path, key: &[u8; 32]) -> crate::Result<PairBinding> {
    if !catalog.is_absolute() || fs::canonicalize(catalog).ok().as_deref() != Some(catalog) {
        return Err(failure());
    }
    let bytes = read_record(catalog).map_err(|_| failure())?;
    if bytes.len() < DOMAIN.len() + 12 + 16
        || bytes.len() > DOMAIN.len() + 12 + LIMIT + 16
        || !bytes.starts_with(DOMAIN)
    {
        return Err(failure());
    }
    let nonce = &bytes[DOMAIN.len()..DOMAIN.len() + 12];
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| failure())?;
    let mut plain = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &bytes[DOMAIN.len() + 12..],
                aad: DOMAIN,
            },
        )
        .map_err(|_| failure())?;
    let decoded = serde_json::from_slice::<PairBinding>(&plain);
    plain.fill(0);
    let b = decoded.map_err(|_| failure())?;
    Ok(b)
}

fn write_binding(binding: &PairBinding, catalog: &Path, key: &[u8; 32]) -> crate::Result<()> {
    let parent = catalog.parent().ok_or_else(failure)?;
    if !catalog.is_absolute() || fs::canonicalize(parent).ok().as_deref() != Some(parent) {
        return Err(failure());
    }
    installations::private_directory(parent)?;
    let mut plain = serde_json::to_vec(binding).map_err(|_| failure())?;
    if plain.len() > LIMIT {
        plain.fill(0);
        return Err(failure());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| failure())?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let sealed = cipher.encrypt(
        Nonce::from_slice(&nonce),
        Payload {
            msg: &plain,
            aad: DOMAIN,
        },
    );
    plain.fill(0);
    let sealed = sealed.map_err(|_| failure())?;
    let mut file = private_file(catalog, true).map_err(|_| failure())?;
    file.write_all(DOMAIN)
        .and_then(|_| file.write_all(&nonce))
        .and_then(|_| file.write_all(&sealed))
        .and_then(|_| file.sync_all())
        .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
    sync_dir(parent).map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Store, VerifiedRelease, PathBuf) {
        let (profile, store, release) = super::super::super::tests::prepared_fixture();
        drop(store);
        let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
        controller.context().unwrap();
        drop(controller);
        let store = Store::open(&profile, "default").unwrap();
        let parent = profile.parent().unwrap();
        let key = parent.join("pair-key");
        private_file(&key, true)
            .unwrap()
            .write_all(&[4; 32])
            .unwrap();
        let pair = parent.join("pair");
        store.checkpoint_host_trust(&key, &pair, true).unwrap();
        let proof = store
            .verify_checkpoint_pair(
                &pair.join("pair-binding.bin"),
                &pair.join("host.bin"),
                &pair.join("trust.bin"),
                &[4; 32],
            )
            .unwrap();
        // Internal producer only: synthetic provenance does not attest real Engine data.
        store
            .bind_observed_checkpoint(&proof, &pair.join("bound.bin"), &[4; 32])
            .unwrap();
        (profile, store, release, pair)
    }
    #[test]
    fn refreshed_trust_reuses_exact_host_and_drops_source_observation() {
        let (profile, mut store, _, pair) = fixture();
        let parent = profile.parent().unwrap();
        let original = archive(&pair.join("host.bin")).unwrap();
        let policy = store.current.policy.clone();
        store
            .replace_policy(policy, store.current.policy_generation, 23)
            .unwrap();
        let records = store.checkpoint_records().unwrap();
        let output = parent.join("refreshed");
        let receipt = store
            .refresh_checkpoint_trust(
                [
                    &pair.join("bound.bin"),
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                ],
                &parent.join("pair-key"),
                &output,
                true,
            )
            .unwrap();
        assert_eq!(receipt.generation, store.current.generation);
        assert_eq!(receipt.host_archive_sha256, original.sha256);
        assert!(!receipt.source_plan_bound);
        assert!(!output.join("host.bin").exists());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        assert_eq!(store.checkpoint_records().unwrap(), records);
        assert_eq!(archive(&pair.join("host.bin")).unwrap(), original);
        assert!(
            store
                .refresh_checkpoint_trust(
                    [
                        &pair.join("bound.bin"),
                        &pair.join("host.bin"),
                        &pair.join("trust.bin")
                    ],
                    &parent.join("pair-key"),
                    &output,
                    true
                )
                .is_err()
        );
    }
    #[test]
    fn refreshed_trust_refuses_changed_host_and_missing_ack_without_publication() {
        let (profile, store, _, pair) = fixture();
        let parent = profile.parent().unwrap();
        let output = parent.join("refused");
        let run = |ack| {
            store.refresh_checkpoint_trust(
                [
                    &pair.join("bound.bin"),
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                ],
                &parent.join("pair-key"),
                &output,
                ack,
            )
        };
        assert_eq!(run(false).unwrap_err().code, "HOST_WRITER_ACK_REQUIRED");
        let witness = profile.join("new-witness");
        private_file(&witness, true)
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert!(run(true).is_err());
        assert!(!output.exists());
        assert_eq!(fs::read(witness).unwrap(), b"changed");
    }
    #[test]
    fn refreshed_trust_refuses_applying_history_and_preserves_archives() {
        let (profile, mut store, release, pair) = fixture();
        let before = archive(&pair.join("host.bin")).unwrap();
        store
            .begin_update(super::super::super::tests::observations(), &release, 21)
            .unwrap();
        let output = profile.parent().unwrap().join("applying-refused");
        assert!(
            store
                .refresh_checkpoint_trust(
                    [
                        &pair.join("bound.bin"),
                        &pair.join("host.bin"),
                        &pair.join("trust.bin")
                    ],
                    &profile.parent().unwrap().join("pair-key"),
                    &output,
                    true
                )
                .is_err()
        );
        assert!(!output.exists());
        assert_eq!(archive(&pair.join("host.bin")).unwrap(), before);
    }
    #[test]
    fn refreshed_trust_refuses_cipher_tamper_wrong_key_and_catalog() {
        let (profile, store, _, pair) = fixture();
        let parent = profile.parent().unwrap();
        let output = parent.join("tamper-refused");
        let key = parent.join("pair-key");
        let original_key = fs::read(&key).unwrap();
        fs::write(&key, [9; 32]).unwrap();
        let run = || {
            store.refresh_checkpoint_trust(
                [
                    &pair.join("bound.bin"),
                    &pair.join("host.bin"),
                    &pair.join("trust.bin"),
                ],
                &key,
                &output,
                true,
            )
        };
        assert!(run().is_err());
        fs::write(&key, original_key).unwrap();
        let catalog = pair.join("bound.bin");
        let old = fs::read(&catalog).unwrap();
        let mut changed = old.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(&catalog, changed).unwrap();
        assert!(run().is_err());
        fs::write(&catalog, old).unwrap();
        let host = pair.join("host.bin");
        let mut changed = fs::read(&host).unwrap();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(&host, changed).unwrap();
        assert!(run().is_err());
        assert!(!output.exists());
    }
    #[test]
    fn refreshed_trust_refuses_active_legacy_operation_lock() {
        let (profile, store, _, pair) = fixture();
        let guard = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(profile.join("operation.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&guard).unwrap();
        let output = profile.parent().unwrap().join("busy-refused");
        assert!(
            store
                .refresh_checkpoint_trust(
                    [
                        &pair.join("bound.bin"),
                        &pair.join("host.bin"),
                        &pair.join("trust.bin")
                    ],
                    &profile.parent().unwrap().join("pair-key"),
                    &output,
                    true
                )
                .is_err()
        );
        assert!(!output.exists());
        drop(guard);
    }
    fn recover(store: &mut Store, release: &VerifiedRelease) {
        store
            .begin_update(super::super::super::tests::observations(), release, 21)
            .unwrap();
        let operation = store.intent().unwrap().update.plan().operation_id.clone();
        store
            .update_failed(&operation, store.current.generation, 22)
            .unwrap();
    }
    #[test]
    fn historical_prepared_pair_requires_recovery_and_preserves_latest_authority() {
        let (profile, mut store, release, pair) = fixture();
        let catalog = pair.join("bound.bin");
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        let original = (
            fs::read(&catalog).unwrap(),
            fs::read(&host).unwrap(),
            fs::read(&trust).unwrap(),
        );
        assert!(
            store
                .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
                .is_err()
        );
        store
            .begin_update(super::super::super::tests::observations(), &release, 21)
            .unwrap();
        assert!(
            store
                .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
                .is_err()
        );
        let operation = store.intent().unwrap().update.plan().operation_id.clone();
        store
            .update_failed(&operation, store.current.generation, 22)
            .unwrap();
        let records = store.checkpoint_records().unwrap();
        let current = store.receipt();
        let operations = store.used_operations.clone();
        let instances = store.used_instances.clone();
        let proof = store
            .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
            .unwrap();
        assert_eq!(proof.receipt().checkpoint.generation, 2);
        assert_eq!(proof.receipt().retained_generation, 4);
        assert!(!proof.receipt().live_trust_restored);
        store
            .recheck_rollback_checkpoint_pair(&proof, &catalog, &host, &trust, &[4; 32])
            .unwrap();
        assert!(
            store
                .verify_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
                .is_err()
        );
        assert_eq!(records, store.checkpoint_records().unwrap());
        assert_eq!(current.generation, store.receipt().generation);
        assert_eq!(operations, store.used_operations);
        assert_eq!(instances, store.used_instances);
        assert_eq!(
            original,
            (
                fs::read(&catalog).unwrap(),
                fs::read(&host).unwrap(),
                fs::read(&trust).unwrap()
            )
        );
        let before_head = store.current_sha256.clone();
        drop(store);
        let reader = CheckpointVerifier::open(&profile, "default").unwrap();
        assert_eq!(reader.authority_receipt().generation, 4);
        assert_eq!(
            reader
                .verify_rollback(&catalog, &host, &trust, &[4; 32])
                .unwrap()
                .receipt()
                .retained_head_sha256,
            before_head
        );
        reader
            .recheck_rollback(&proof, &catalog, &host, &trust, &[4; 32])
            .unwrap();
        drop(reader);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
    #[test]
    fn historical_pair_refuses_unbound_wrong_plan_head_scope_key_and_ciphertext() {
        let (profile, mut store, release, pair) = fixture();
        let catalog = pair.join("bound.bin");
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        recover(&mut store, &release);
        let binding = read_binding(&catalog, &[4; 32]).unwrap();
        assert!(
            store
                .verify_rollback_checkpoint_pair(
                    &pair.join("pair-binding.bin"),
                    &host,
                    &trust,
                    &[4; 32]
                )
                .is_err()
        );
        assert!(
            store
                .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[5; 32])
                .is_err()
        );
        for n in 0..6 {
            let mut wrong = binding.clone();
            match n {
                0 => wrong.head_sha256 = "0".repeat(64),
                1 => wrong.scope = "unrelated".into(),
                2 => wrong.generation = store.current.generation,
                3 => wrong.source.as_mut().unwrap().backup_id = "unrelated".into(),
                4 => wrong.source.as_mut().unwrap().target_instance = "unrelated".into(),
                _ => wrong.generation = 1,
            }
            let path = pair.join(format!("wrong-{n}.bin"));
            write_binding(&wrong, &path, &[4; 32]).unwrap();
            assert!(
                store
                    .verify_rollback_checkpoint_pair(&path, &host, &trust, &[4; 32])
                    .is_err()
            );
        }
        let before = store.checkpoint_records().unwrap();
        let changed = pair.join("changed-host.bin");
        private_file(&changed, true)
            .unwrap()
            .write_all(b"different bytes")
            .unwrap();
        assert!(
            store
                .verify_rollback_checkpoint_pair(&catalog, &changed, &trust, &[4; 32])
                .is_err()
        );
        assert_eq!(before, store.checkpoint_records().unwrap());
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
    #[test]
    fn historical_proof_is_stale_after_newer_retained_policy_and_never_rewinds_it() {
        let (profile, mut store, release, pair) = fixture();
        let catalog = pair.join("bound.bin");
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        recover(&mut store, &release);
        let proof = store
            .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
            .unwrap();
        let previous_policy = store.current.policy.clone();
        let mut rotated = previous_policy.clone();
        rotated.public_keys = vec![
            ed25519_dalek::SigningKey::from_bytes(&[32; 32])
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ];
        rotated.minimum_sequence = store.receipt().minimum_sequence + 1;
        store
            .replace_policy(rotated.clone(), store.current.policy_generation, 23)
            .unwrap();
        let revoked = store.current.revoked_keys.clone();
        assert!(!revoked.is_empty());
        let records = store.checkpoint_records().unwrap();
        assert!(
            store
                .recheck_rollback_checkpoint_pair(&proof, &catalog, &host, &trust, &[4; 32])
                .is_err()
        );
        let fresh = store
            .verify_rollback_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
            .unwrap();
        assert_eq!(fresh.receipt().retained_generation, 5);
        assert_eq!(
            serde_json::to_value(&store.current.policy).unwrap(),
            serde_json::to_value(&rotated).unwrap()
        );
        assert_eq!(store.current.revoked_keys, revoked);
        assert!(
            store
                .replace_policy(previous_policy, store.current.policy_generation, 24)
                .is_err()
        );
        assert_eq!(records, store.checkpoint_records().unwrap());
        assert!(
            store
                .verify_checkpoint_pair(&catalog, &host, &trust, &[4; 32])
                .is_err()
        );
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
}
