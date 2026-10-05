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
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveIdentity {
    bytes: u64,
    sha256: String,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
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
        let mut plain = serde_json::to_vec(&binding).map_err(|_| failure())?;
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
        let mut file =
            private_file(&stage.join("pair-binding.bin"), true).map_err(|_| failure())?;
        file.write_all(DOMAIN)
            .and_then(|_| file.write_all(&nonce))
            .and_then(|_| file.write_all(&sealed))
            .and_then(|_| file.sync_all())
            .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
        sync_dir(stage).map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
        self.check_root().map_err(|_| failure())?;
        Ok(())
    }
    pub(crate) fn verify_recovery_pair(
        &self,
        catalog: &Path,
        host: &Path,
        trust: &Path,
        key: &[u8; 32],
    ) -> crate::Result<PairBinding> {
        self.check_root().map_err(|_| failure())?;
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
