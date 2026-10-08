// SPDX-License-Identifier: Apache-2.0
//! Per-record authenticated inactive extraction; never replaces live authority.
use super::*;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
const DOMAIN: &[u8] = b"ExhibitOS-trust-checkpoint-v1\0";
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    format: u8,
    scope: String,
    generation: u64,
    head: String,
}
fn destination(store: &Store, path: &Path) -> Result<PathBuf, Error> {
    let parent = path.parent().ok_or_else(invalid)?;
    if !path.is_absolute()
        || fs::canonicalize(parent).map_err(|_| invalid())? != parent
        || path.file_name().is_none()
        || path.starts_with(&store.profile)
        || path.starts_with(&store.root)
        || path.exists()
    {
        return Err(invalid());
    }
    installations::private_directory(parent).map_err(|_| invalid())?;
    Ok(parent.to_path_buf())
}
fn aad(header: &[u8], index: usize) -> Vec<u8> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(&(index as u64).to_be_bytes());
    bytes
}
impl Store {
    /// New authenticated archive, secret key supplied by an OS/private key boundary.
    pub fn archive_trust_checkpoint(
        &self,
        archive: &Path,
        key: &[u8; 32],
    ) -> Result<TrustCheckpointReceipt, Error> {
        let parent = destination(self, archive)?;
        let records = self.checkpoint_records()?;
        let header = serde_json::to_vec(&Header {
            format: 1,
            scope: self.scope.clone(),
            generation: self.current.generation,
            head: self.current_sha256.clone(),
        })
        .map_err(|_| invalid())?;
        let pending = parent.join(format!("pending-trust-{}.bin", uuid::Uuid::new_v4()));
        let mut file = private_file(&pending, true)?;
        file.write_all(DOMAIN)
            .and_then(|_| file.write_all(&(header.len() as u32).to_be_bytes()))
            .and_then(|_| file.write_all(&header))
            .map_err(|_| Error::TrustWriteUncertain)?;
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| invalid())?;
        for (index, (_, bytes)) in records.iter().enumerate() {
            let mut nonce = [0u8; 12];
            OsRng.fill_bytes(&mut nonce);
            let encrypted = cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: bytes,
                        aad: &aad(&header, index),
                    },
                )
                .map_err(|_| invalid())?;
            file.write_all(&(encrypted.len() as u32).to_be_bytes())
                .and_then(|_| file.write_all(&nonce))
                .and_then(|_| file.write_all(&encrypted))
                .map_err(|_| Error::TrustWriteUncertain)?;
        }
        file.sync_all().map_err(|_| Error::TrustWriteUncertain)?;
        drop(file);
        if self.checkpoint_records()? != records {
            return Err(invalid());
        }
        publish(&pending, archive)?;
        sync_dir(&parent)?;
        Ok(TrustCheckpointReceipt {
            generation: self.current.generation,
            head_sha256: self.current_sha256.clone(),
            records: records.len(),
            full_history_verified: true,
            live_trust_restored: false,
        })
    }
    /// Authenticate every frame and match exact current history before publication.
    /// Current Store remains necessary; this is not authority-loss recovery.
    pub fn extract_trust_checkpoint(
        &self,
        archive: &Path,
        target: &Path,
        key: &[u8; 32],
    ) -> Result<TrustCheckpointReceipt, Error> {
        let parent = destination(self, target)?;
        let records = self.checkpoint_records()?;
        let mut file = private_file(archive, false)?;
        let metadata = file.metadata().map_err(|_| invalid())?;
        if metadata.len() > MAX_RECORDS as u64 * (MAX_RECORD + 32) + 4096 {
            return Err(Error::TrustLimit);
        }
        let mut domain = vec![0u8; DOMAIN.len()];
        file.read_exact(&mut domain).map_err(|_| invalid())?;
        if domain != DOMAIN {
            return Err(invalid());
        }
        let mut length = [0u8; 4];
        file.read_exact(&mut length).map_err(|_| invalid())?;
        let size = u32::from_be_bytes(length) as usize;
        if size == 0 || size > 4096 {
            return Err(invalid());
        }
        let mut header = vec![0u8; size];
        file.read_exact(&mut header).map_err(|_| invalid())?;
        let parsed: Header = serde_json::from_slice(&header).map_err(|_| invalid())?;
        if parsed
            != (Header {
                format: 1,
                scope: self.scope.clone(),
                generation: self.current.generation,
                head: self.current_sha256.clone(),
            })
        {
            return Err(invalid());
        }
        let stage = parent.join(format!("pending-trust-extract-{}", uuid::Uuid::new_v4()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&stage).map_err(|_| invalid())?;
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| invalid())?;
        for (index, (name, expected)) in records.iter().enumerate() {
            file.read_exact(&mut length).map_err(|_| invalid())?;
            let size = u32::from_be_bytes(length) as usize;
            if size != expected.len() + 16 || size as u64 > MAX_RECORD + 16 {
                return Err(invalid());
            }
            let mut nonce = [0u8; 12];
            file.read_exact(&mut nonce).map_err(|_| invalid())?;
            let mut encrypted = vec![0u8; size];
            file.read_exact(&mut encrypted).map_err(|_| invalid())?;
            let mut plain = cipher
                .decrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: &encrypted,
                        aad: &aad(&header, index),
                    },
                )
                .map_err(|_| invalid())?;
            if plain != *expected {
                plain.fill(0);
                return Err(invalid());
            }
            let mut out = private_file(&stage.join(name), true)?;
            let written = out.write_all(&plain).and_then(|_| out.sync_all());
            plain.fill(0);
            written.map_err(|_| Error::TrustWriteUncertain)?;
        }
        let mut extra = [0u8; 1];
        if file.read(&mut extra).map_err(|_| invalid())? != 0 {
            return Err(invalid());
        }
        let after = file.metadata().map_err(|_| invalid())?;
        let current = fs::symlink_metadata(archive).map_err(|_| invalid())?;
        if !identity(&metadata, &after)
            || !identity(&after, &current)
            || metadata.len() != after.len()
            || current.is_symlink()
        {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let state = |m: &Metadata| {
                (
                    m.len(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec(),
                    m.mode(),
                    m.uid(),
                    m.nlink(),
                )
            };
            if state(&metadata) != state(&after) || state(&after) != state(&current) {
                return Err(invalid());
            }
        }
        sync_dir(&stage)?;
        let receipt = self.verify_trust_checkpoint(&stage)?;
        publish(&stage, target)?;
        sync_dir(&parent)?;
        self.verify_trust_checkpoint(target)?;
        Ok(receipt)
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip_and_wrong_key_tamper_truncate_extra_stale_refuse() {
        let (profile, mut store, _) = super::super::super::tests::prepared_fixture();
        let parent = profile.parent().unwrap();
        let archive = parent.join("encrypted.bin");
        let key = [7u8; 32];
        let old = store.current_sha256.clone();
        store.archive_trust_checkpoint(&archive, &key).unwrap();
        let target = parent.join("extracted");
        let r = store
            .extract_trust_checkpoint(&archive, &target, &key)
            .unwrap();
        assert!(!r.live_trust_restored);
        assert_eq!(store.current_sha256, old);
        assert!(store.archive_trust_checkpoint(&archive, &key).is_err());
        assert!(
            store
                .extract_trust_checkpoint(&archive, &target, &key)
                .is_err()
        );
        let bad = parent.join("wrong-key");
        assert!(
            store
                .extract_trust_checkpoint(&archive, &bad, &[8u8; 32])
                .is_err()
        );
        assert!(!bad.exists());
        let original = fs::read(&archive).unwrap();
        for kind in ["tamper", "truncate", "extra", "domain"] {
            let mut bytes = original.clone();
            match kind {
                "tamper" => {
                    let n = bytes.len();
                    bytes[n - 1] ^= 1;
                }
                "truncate" => {
                    bytes.pop();
                }
                "extra" => bytes.push(0),
                _ => bytes[0] ^= 1,
            }
            let corrupted = parent.join(format!("{kind}.bin"));
            private_file(&corrupted, true)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            let output = parent.join(kind);
            assert!(
                store
                    .extract_trust_checkpoint(&corrupted, &output, &key)
                    .is_err()
            );
            assert!(!output.exists());
        }
        let policy = store.current.policy.clone();
        store
            .replace_policy(policy, store.current.policy_generation, 30)
            .unwrap();
        let stale = parent.join("stale");
        assert!(
            store
                .extract_trust_checkpoint(&archive, &stale, &key)
                .is_err()
        );
        assert!(!stale.exists());
    }
}
