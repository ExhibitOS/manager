// SPDX-License-Identifier: Apache-2.0
//! Exact complete history under the existing fences; never activates/rewinds trust.
use super::*;
#[path = "trust_checkpoint_encrypted.rs"]
mod encrypted;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustCheckpointReceipt {
    pub generation: u64,
    pub head_sha256: String,
    pub records: usize,
    pub full_history_verified: bool,
    pub live_trust_restored: bool,
}
impl Store {
    fn checkpoint_records(&self) -> Result<Vec<(String, Vec<u8>)>, Error> {
        self.check_root()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| invalid())? {
            let entry = entry.map_err(|_| invalid())?;
            let name = entry.file_name().into_string().map_err(|_| invalid())?;
            if name != "trust.lock" {
                names.push(name);
            }
            if names.len() > MAX_RECORDS {
                return Err(Error::TrustLimit);
            }
        }
        names.sort();
        if names.len() as u64 != self.current.generation {
            return Err(invalid());
        }
        let mut previous = None;
        let mut operations = BTreeSet::new();
        let mut instances = BTreeSet::new();
        let mut digest = "0".repeat(64);
        let mut records = Vec::new();
        for (index, name) in names.iter().enumerate() {
            if *name != format!("{:020}.json", index + 1) {
                return Err(invalid());
            }
            let bytes = read_record(&self.root.join(name))?;
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            valid_record(&record, &self.scope)?;
            if record.generation != index as u64 + 1 || record.previous_sha256 != digest {
                return Err(invalid());
            }
            if let Some(old) = &previous {
                transition(old, &record)?;
            } else if record.policy_generation != 1
                || !record.revoked_keys.is_empty()
                || record.acceptance.is_some()
                || record.intent.is_some()
                || record.update_event.is_some()
            {
                return Err(invalid());
            }
            validate_new_ids(previous.as_ref(), &record, &operations, &instances)?;
            remember_ids(previous.as_ref(), &record, &mut operations, &mut instances);
            digest = hash(&bytes);
            previous = Some(record);
            records.push((name.clone(), bytes));
        }
        if digest != self.current_sha256
            || operations != self.used_operations
            || instances != self.used_instances
        {
            return Err(invalid());
        }
        self.check_root()?;
        Ok(records)
    }
    /// Copy all validated committed records into a new private inactive directory.
    /// Pending writes refuse; exact byte verification requires this same current Store.
    pub fn checkpoint_trust(&self, destination: &Path) -> Result<TrustCheckpointReceipt, Error> {
        let parent = destination.parent().ok_or_else(invalid)?;
        if !destination.is_absolute()
            || fs::canonicalize(parent).map_err(|_| invalid())? != parent
            || destination.starts_with(&self.profile)
            || destination.starts_with(&self.root)
            || destination.file_name().is_none()
        {
            return Err(invalid());
        }
        installations::private_directory(parent).map_err(|_| invalid())?;
        let before = self.checkpoint_records()?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(destination).map_err(|_| invalid())?;
        installations::private_directory(destination).map_err(|_| invalid())?;
        for (name, bytes) in &before {
            let mut file = private_file(&destination.join(name), true)?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::TrustWriteUncertain)?;
        }
        sync_dir(destination)?;
        sync_dir(parent)?;
        if self.checkpoint_records()? != before {
            return Err(invalid());
        }
        self.verify_trust_checkpoint(destination)
    }
    /// Exact current bytes, no caller success flags, historical-head fallback or activation.
    pub fn verify_trust_checkpoint(
        &self,
        destination: &Path,
    ) -> Result<TrustCheckpointReceipt, Error> {
        installations::private_directory(destination).map_err(|_| invalid())?;
        if fs::canonicalize(destination).map_err(|_| invalid())? != destination
            || destination.starts_with(&self.profile)
            || destination.starts_with(&self.root)
        {
            return Err(invalid());
        }
        let before = self.checkpoint_records()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(destination).map_err(|_| invalid())? {
            let entry = entry.map_err(|_| invalid())?;
            names.push(entry.file_name().into_string().map_err(|_| invalid())?);
            if names.len() > MAX_RECORDS {
                return Err(Error::TrustLimit);
            }
        }
        names.sort();
        if names != before.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>() {
            return Err(invalid());
        }
        for (name, bytes) in &before {
            if read_record(&destination.join(name))? != *bytes {
                return Err(invalid());
            }
        }
        if self.checkpoint_records()? != before {
            return Err(invalid());
        }
        Ok(TrustCheckpointReceipt {
            generation: self.current.generation,
            head_sha256: self.current_sha256.clone(),
            records: before.len(),
            full_history_verified: true,
            live_trust_restored: false,
        })
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn exact_history_and_corruption_scope_and_stale_refusals() {
        let (profile, _key, policy, _release) = super::super::tests::fixture();
        let mut store = Store::provision(&profile, "default", policy, 10).unwrap();
        let destination = profile.parent().unwrap().join("checkpoint");
        let original = store.current_sha256.clone();
        let receipt = store.checkpoint_trust(&destination).unwrap();
        assert_eq!(receipt.records, 1);
        assert!(!receipt.live_trust_restored);
        assert_eq!(store.current_sha256, original);
        assert!(store.checkpoint_trust(&destination).is_err());
        assert!(store.checkpoint_trust(&profile.join("unsafe")).is_err());
        let file = destination.join("00000000000000000001.json");
        let bytes = fs::read(&file).unwrap();
        fs::write(&file, b"{}").unwrap();
        assert!(store.verify_trust_checkpoint(&destination).is_err());
        fs::write(&file, &bytes).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.verify_trust_checkpoint(&destination).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let alias = destination.with_extension("alias");
        symlink(&destination, &alias).unwrap();
        assert!(store.verify_trust_checkpoint(&alias).is_err());
        let pending = store
            .root
            .join(format!("pending-{}.json", uuid::Uuid::new_v4()));
        let mut f = private_file(&pending, true).unwrap();
        f.write_all(b"{}").unwrap();
        drop(f);
        assert!(store.verify_trust_checkpoint(&destination).is_err());
        fs::remove_file(pending).unwrap();
        let next = store.current.policy.clone();
        store.replace_policy(next, 1, 11).unwrap();
        assert!(store.verify_trust_checkpoint(&destination).is_err());
        let latest = destination.with_extension("latest");
        assert_eq!(store.checkpoint_trust(&latest).unwrap().records, 2);
        fs::remove_file(latest.join("00000000000000000001.json")).unwrap();
        assert!(store.verify_trust_checkpoint(&latest).is_err());
    }
    #[test]
    fn prepared_intent_and_reserved_ids_are_copied_without_transition() {
        let (profile, store, _release) = super::super::tests::prepared_fixture();
        let destination = profile.parent().unwrap().join("prepared-checkpoint");
        let old = serde_json::to_vec(&store.current).unwrap();
        let receipt = store.checkpoint_trust(&destination).unwrap();
        assert_eq!(receipt.generation, store.current.generation);
        assert!(!store.used_operations.is_empty());
        assert!(!store.used_instances.is_empty());
        let head =
            read_record(&destination.join(format!("{:020}.json", receipt.generation))).unwrap();
        assert_eq!(hash(&head), store.current_sha256);
        assert_eq!(serde_json::to_vec(&store.current).unwrap(), old);
        let extra = destination.join("extra.json");
        private_file(&extra, true)
            .unwrap()
            .write_all(b"{}")
            .unwrap();
        assert!(store.verify_trust_checkpoint(&destination).is_err());
    }
}
