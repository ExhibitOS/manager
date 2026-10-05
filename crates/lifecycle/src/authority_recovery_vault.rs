// SPDX-License-Identifier: Apache-2.0
//! Independent retained authority namespace. This does not resist a trusted OS
//! owner rolling back/deleting BOTH namespaces or losing the entire storage device.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: u8,
    scope: String,
    vault: PathBuf,
    enrolled_generation: u64,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completion {
    format: u8,
    scope: String,
    generation: u64,
    sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityRecoveryReceipt {
    pub generation: u64,
    pub records: usize,
    pub head_sha256: String,
    pub live_authority_restored: bool,
    pub host_restored: bool,
    pub services_restored: bool,
    pub update_executed: bool,
}
/// Explicit authority journal repair; never an execution or data activation permit.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityReconciliationReceipt {
    pub generation: u64,
    pub records: usize,
    pub head_sha256: String,
    pub primary_record_published: bool,
    pub completion_marker_published: bool,
    pub enrollment_completed: bool,
    pub in_flight_intent_requires_restart_recovery: bool,
    pub host_restored: bool,
    pub services_restored: bool,
    pub update_executed: bool,
}
struct PrimaryFence {
    root: PathBuf,
    identity: Metadata,
    lock: File,
    operations: (File, Vec<File>),
}
impl Drop for PrimaryFence {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
        let _ = FileExt::unlock(&self.operations.0);
        for lock in &self.operations.1 {
            let _ = FileExt::unlock(lock);
        }
    }
}
impl PrimaryFence {
    fn check(&self) -> Result<(), Error> {
        if !identity(&self.identity, &private_dir(&self.root)?)
            || !identity(
                &self.lock.metadata().map_err(|_| invalid())?,
                &private_file(&self.root.join("trust.lock"), false)?
                    .metadata()
                    .map_err(|_| invalid())?,
            )
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(super) struct Vault {
    locator: PathBuf,
    locator_identity: Metadata,
    root: PathBuf,
    root_identity: Metadata,
    records_identity: Metadata,
    completed_identity: Metadata,
    binding: Binding,
    binding_sha256: String,
    _locator_lock: File,
    _vault_lock: File,
}
impl Drop for Vault {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._vault_lock);
        let _ = FileExt::unlock(&self._locator_lock);
    }
}
pub(super) fn locator(root: &Path, scope: &str) -> Result<PathBuf, Error> {
    Ok(root
        .parent()
        .ok_or_else(invalid)?
        .join(format!(".exhibitos-release-recovery-{scope}")))
}
pub(super) fn absent(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(invalid()),
    }
}
fn new_dir(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|_| invalid())?;
    sync_dir(path.parent().ok_or_else(invalid)?)
}
fn private_dir(path: &Path) -> Result<Metadata, Error> {
    installations::private_directory(path).map_err(|_| invalid())?;
    if fs::canonicalize(path).map_err(|_| invalid())? != path {
        return Err(invalid());
    }
    fs::symlink_metadata(path).map_err(|_| invalid())
}
fn exact_names(root: &Path) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| invalid())? {
        let name = entry
            .map_err(|_| invalid())?
            .file_name()
            .into_string()
            .map_err(|_| invalid())?;
        names.push(name);
        if names.len() > MAX_RECORDS + 3 {
            return Err(Error::TrustLimit);
        }
    }
    names.sort();
    Ok(names)
}
fn numbered(name: &str) -> bool {
    name.len() == 25
        && name.ends_with(".json")
        && name.as_bytes()[..20].iter().all(u8::is_ascii_digit)
}
fn history(root: &Path, scope: &str) -> Result<Vec<(String, Vec<u8>, Record)>, Error> {
    private_dir(root)?;
    let names: Vec<_> = exact_names(root)?
        .into_iter()
        .filter(|n| n != "trust.lock")
        .collect();
    if names.is_empty() || names.len() > MAX_RECORDS {
        return Err(invalid());
    }
    let mut result = Vec::new();
    let mut previous = None;
    let mut sha = "0".repeat(64);
    let mut operations = BTreeSet::new();
    let mut instances = BTreeSet::new();
    for (index, name) in names.into_iter().enumerate() {
        if name != format!("{:020}.json", index + 1) {
            return Err(invalid());
        }
        let bytes = read_record(&root.join(&name))?;
        let r: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        valid_record(&r, scope)?;
        if r.generation != index as u64 + 1 || r.previous_sha256 != sha {
            return Err(invalid());
        }
        if let Some(old) = &previous {
            transition(old, &r)?;
        } else if r.policy_generation != 1
            || !r.revoked_keys.is_empty()
            || r.acceptance.is_some()
            || r.intent.is_some()
            || r.update_event.is_some()
        {
            return Err(invalid());
        }
        validate_new_ids(previous.as_ref(), &r, &operations, &instances)?;
        remember_ids(previous.as_ref(), &r, &mut operations, &mut instances);
        sha = hash(&bytes);
        previous = Some(r.clone());
        result.push((name, bytes, r));
    }
    Ok(result)
}
fn publish_bytes(root: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_RECORD {
        return Err(Error::TrustLimit);
    }
    let pending = root.join(format!("pending-{}.json", uuid::Uuid::new_v4()));
    let mut file = private_file(&pending, true)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::TrustWriteUncertain)?;
    drop(file);
    publish(&pending, &root.join(name))?;
    sync_dir(root)
}
impl Vault {
    fn open(root: &Path, scope: &str, expected: Option<&str>) -> Result<Self, Error> {
        let loc = locator(root, scope)?;
        let li = private_dir(&loc)?;
        let ll = lock(&loc)?;
        if exact_names(&loc)? != vec!["binding.json", "trust.lock"] {
            return Err(invalid());
        }
        let bytes = read_record(&loc.join("binding.json"))?;
        let digest = hash(&bytes);
        if expected.is_some_and(|e| e != digest) {
            return Err(invalid());
        }
        let binding: Binding = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if binding.format != 1
            || binding.scope != scope
            || binding.enrolled_generation < 2
            || !binding.vault.is_absolute()
            || binding.vault.starts_with(root)
            || binding.vault.starts_with(&loc)
        {
            return Err(invalid());
        }
        let ri = private_dir(&binding.vault)?;
        let vl = lock(&binding.vault)?;
        let value = Self {
            locator: loc,
            locator_identity: li,
            root: binding.vault.clone(),
            records_identity: private_dir(&binding.vault.join("records"))?,
            completed_identity: private_dir(&binding.vault.join("completed"))?,
            root_identity: ri,
            binding,
            binding_sha256: digest,
            _locator_lock: ll,
            _vault_lock: vl,
        };
        value.check_identity()?;
        Ok(value)
    }
    pub(super) fn open_for(store: &Store) -> Result<Option<Self>, Error> {
        let loc = locator(&store.root, &store.scope)?;
        match &store.current.recovery_binding {
            Some(expected) => Ok(Some(Self::open(&store.root, &store.scope, Some(expected))?)),
            None => {
                if !absent(&loc)? {
                    return Err(Error::TrustWriteUncertain);
                }
                Ok(None)
            }
        }
    }
    fn check_identity(&self) -> Result<(), Error> {
        if !identity(&self.locator_identity, &private_dir(&self.locator)?)
            || !identity(&self.root_identity, &private_dir(&self.root)?)
            || exact_names(&self.locator)? != vec!["binding.json", "trust.lock"]
            || hash(&read_record(&self.locator.join("binding.json"))?) != self.binding_sha256
            || exact_names(&self.root)? != vec!["completed", "records", "trust.lock"]
        {
            return Err(invalid());
        }
        if !identity(
            &self.records_identity,
            &private_dir(&self.root.join("records"))?,
        ) || !identity(
            &self.completed_identity,
            &private_dir(&self.root.join("completed"))?,
        ) || !identity(
            &self._locator_lock.metadata().map_err(|_| invalid())?,
            &private_file(&self.locator.join("trust.lock"), false)?
                .metadata()
                .map_err(|_| invalid())?,
        ) || !identity(
            &self._vault_lock.metadata().map_err(|_| invalid())?,
            &private_file(&self.root.join("trust.lock"), false)?
                .metadata()
                .map_err(|_| invalid())?,
        ) {
            return Err(invalid());
        }
        if exact_names(&self.root.join("records"))?
            .iter()
            .any(|n| !numbered(n))
            || exact_names(&self.root.join("completed"))?
                .iter()
                .any(|n| !numbered(n))
        {
            return Err(Error::TrustWriteUncertain);
        }
        Ok(())
    }
    // After enrollment every committed generation has exactly one marker. A single
    // unmarked final record is a write-ahead candidate, never accepted authority.
    fn completed_prefix(&self, records: &[(String, Vec<u8>, Record)]) -> Result<u64, Error> {
        self.check_identity()?;
        let head = records.last().ok_or_else(invalid)?.2.generation;
        if head + 1 < self.binding.enrolled_generation {
            return Err(invalid());
        }
        let mut last = self.binding.enrolled_generation - 1;
        for name in exact_names(&self.root.join("completed"))? {
            let generation = last.checked_add(1).ok_or_else(invalid)?;
            if generation > head || name != format!("{generation:020}.json") {
                return Err(invalid());
            }
            let c: Completion =
                serde_json::from_slice(&read_record(&self.root.join("completed").join(&name))?)
                    .map_err(|_| invalid())?;
            if c != self.completion(
                &records[generation as usize - 1].2,
                &hash(&records[generation as usize - 1].1),
            ) {
                return Err(invalid());
            }
            last = generation;
        }
        if last > head || head - last > 1 {
            return Err(Error::TrustWriteUncertain);
        }
        self.check_identity()?;
        Ok(last)
    }
    fn latest(&self) -> Result<Vec<(String, Vec<u8>, Record)>, Error> {
        self.check_identity()?;
        let records = history(&self.root.join("records"), &self.binding.scope)?;
        let r = &records.last().ok_or_else(invalid)?.2;
        if r.generation < self.binding.enrolled_generation
            || r.recovery_binding.as_deref() != Some(&self.binding_sha256)
            || self.completed_prefix(&records)? != r.generation
        {
            return Err(Error::TrustWriteUncertain);
        }
        self.check_identity()?;
        Ok(records)
    }
    fn completion(&self, r: &Record, h: &str) -> Completion {
        Completion {
            format: 1,
            scope: self.binding.scope.clone(),
            generation: r.generation,
            sha256: h.into(),
        }
    }
    pub(super) fn check_enrollment(&self, primary: &Path, head: &str) -> Result<(), Error> {
        self.check_identity()?;
        let mirrored = history(&self.root.join("records"), &self.binding.scope)?;
        let source = history(primary, &self.binding.scope)?;
        if !exact_names(&self.root.join("completed"))?.is_empty()
            || mirrored.len() as u64 + 1 != self.binding.enrolled_generation
            || mirrored
                .iter()
                .map(|(n, b, _)| (n, b))
                .ne(source.iter().map(|(n, b, _)| (n, b)))
            || source
                .last()
                .is_none_or(|(_, b, r)| hash(b) != head || r.recovery_binding.is_some())
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn check(&self, primary: &Path, head: &str) -> Result<(), Error> {
        let mirrored = self.latest()?;
        let source = history(primary, &self.binding.scope)?;
        if mirrored
            .iter()
            .map(|(n, b, _)| (n, b))
            .ne(source.iter().map(|(n, b, _)| (n, b)))
            || source.last().is_none_or(|(_, b, _)| hash(b) != head)
        {
            return Err(invalid());
        }
        self.check_identity()
    }
    pub(super) fn prepare(&self, next: &Record) -> Result<(), Error> {
        self.check_identity()?;
        write(&self.root.join("records"), next)?;
        sync_dir(&self.root)
    }
    pub(super) fn finish(&self, primary: &Path, next: &Record, h: &str) -> Result<(), Error> {
        self.check_identity()?;
        if read_record(&primary.join(format!("{:020}.json", next.generation)))?
            != read_record(
                &self
                    .root
                    .join("records")
                    .join(format!("{:020}.json", next.generation)),
            )?
        {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(&self.completion(next, h)).map_err(|_| invalid())?;
        publish_bytes(
            &self.root.join("completed"),
            &format!("{:020}.json", next.generation),
            &bytes,
        )?;
        self.check(primary, h)
    }
}
/// Fresh inactive restoration of the entire current independent authority chain.
/// This proof is internal; it cannot be manufactured from a receipt or head hash.
pub(super) struct InactiveAuthorityProof {
    root: PathBuf,
    identity: Metadata,
    generation: u64,
    head_sha256: String,
}
impl Store {
    pub(super) fn require_authority_recovery(&self) -> Result<(), Error> {
        self.check_root()?;
        self.recovery
            .as_ref()
            .ok_or_else(invalid)?
            .check(&self.root, &self.current_sha256)
    }
    pub(super) fn restore_inactive_authority(
        &self,
        destination: &Path,
    ) -> Result<InactiveAuthorityProof, Error> {
        self.require_authority_recovery()?;
        let vault = self.recovery.as_ref().ok_or_else(invalid)?;
        let parent = destination.parent().ok_or_else(invalid)?;
        private_dir(parent)?;
        if !destination.is_absolute()
            || !absent(destination)?
            || [&self.profile, &self.root, &vault.root, &vault.locator]
                .into_iter()
                .any(|p| destination.starts_with(p) || p.starts_with(destination))
        {
            return Err(invalid());
        }
        let records = vault.latest()?;
        let bytes = records.iter().try_fold(0u64, |total, (_, b, _)| {
            total.checked_add(b.len() as u64).ok_or_else(invalid)
        })?;
        if fs2::available_space(parent).map_err(|_| invalid())?
            < bytes
                .checked_add(6 * 1024 * 1024 * 1024)
                .ok_or_else(invalid)?
        {
            return Err(Error::TrustLimit);
        }
        new_dir(destination)?;
        for (name, bytes, _) in &records {
            publish_bytes(destination, name, bytes)?;
        }
        let proof = InactiveAuthorityProof {
            root: destination.to_owned(),
            identity: private_dir(destination)?,
            generation: self.current.generation,
            head_sha256: self.current_sha256.clone(),
        };
        self.recheck_inactive_authority(&proof)?;
        Ok(proof)
    }
    pub(super) fn recheck_inactive_authority(
        &self,
        proof: &InactiveAuthorityProof,
    ) -> Result<(), Error> {
        self.require_authority_recovery()?;
        if proof.generation != self.current.generation
            || proof.head_sha256 != self.current_sha256
            || !identity(&proof.identity, &private_dir(&proof.root)?)
        {
            return Err(invalid());
        }
        let records = self.recovery.as_ref().ok_or_else(invalid)?.latest()?;
        let restored = history(&proof.root, &self.scope)?;
        if records
            .iter()
            .map(|(n, b, _)| (n, b))
            .ne(restored.iter().map(|(n, b, _)| (n, b)))
        {
            return Err(invalid());
        }
        self.require_authority_recovery()
    }
}
impl Store {
    /// Trusted administrator enrollment while the original complete Store exists.
    /// Every later commit writes an independent candidate BEFORE primary mutation,
    /// then completes it only after primary publication/sync. No host/archive copy.
    pub fn enroll_authority_recovery(
        &mut self,
        vault: &Path,
        now: u64,
    ) -> Result<AuthorityRecoveryReceipt, Error> {
        self.check_root()?;
        if self.current.recovery_binding.is_some() || self.recovery.is_some() {
            return Err(Error::TrustExists);
        }
        if now < self.current.observed_at {
            return Err(Error::TrustClockRollback);
        }
        let loc = locator(&self.root, &self.scope)?;
        if !vault.is_absolute()
            || vault.starts_with(&self.profile)
            || vault.starts_with(&self.root)
            || vault.starts_with(&loc)
            || self.root.starts_with(vault)
            || loc.starts_with(vault)
            || !absent(vault)?
            || !absent(&loc)?
        {
            return Err(invalid());
        }
        private_dir(vault.parent().ok_or_else(invalid)?)?;
        let records = history(&self.root, &self.scope)?;
        if records
            .last()
            .is_none_or(|(_, b, _)| hash(b) != self.current_sha256)
        {
            return Err(invalid());
        }
        new_dir(vault)?;
        new_dir(&vault.join("records"))?;
        new_dir(&vault.join("completed"))?;
        for (name, bytes, _) in &records {
            publish_bytes(&vault.join("records"), name, bytes)?;
        }
        let binding = Binding {
            format: 1,
            scope: self.scope.clone(),
            vault: vault.into(),
            enrolled_generation: self.current.generation.checked_add(1).ok_or_else(invalid)?,
        };
        new_dir(&loc)?;
        let encoded = serde_json::to_vec(&binding).map_err(|_| invalid())?;
        publish_bytes(&loc, "binding.json", &encoded)?;
        let bound = Vault::open(&self.root, &self.scope, Some(&hash(&encoded)))?;
        let mut next = self.current.clone();
        next.recovery_binding = Some(bound.binding_sha256.clone());
        next.update_event = None;
        self.recovery = Some(bound);
        self.commit(next, now)?;
        Ok(AuthorityRecoveryReceipt {
            generation: self.current.generation,
            records: self.current.generation as usize,
            head_sha256: self.current_sha256.clone(),
            live_authority_restored: false,
            host_restored: false,
            services_restored: false,
            update_executed: false,
        })
    }
    /// Explicitly finish at most one retained write-ahead journal candidate, or
    /// resume a fully copied/bound enrollment. Both original and vault must exist.
    /// No caller-selected head, rollback, record rewrite, runtime replay or reset.
    pub fn reconcile_authority(
        profile: &Path,
        installation: &str,
        apps_closed: bool,
        now: u64,
    ) -> Result<AuthorityReconciliationReceipt, Error> {
        if !apps_closed {
            return Err(invalid());
        }
        let (root, id, profile, anchor) = scope(profile, installation)?;
        let session =
            profile_backup::anchored_session(&profile, anchor, true).map_err(|_| invalid())?;
        let operations =
            profile_backup::current_host_locks(&profile, &session).map_err(|_| invalid())?;
        let fence = PrimaryFence {
            identity: private_dir(&root)?,
            lock: lock(&root)?,
            root: root.clone(),
            operations,
        };
        let vault = Vault::open(&root, &id, None)?;
        if vault.root.starts_with(&profile)
            || root.starts_with(&vault.root)
            || vault.locator.starts_with(&vault.root)
        {
            return Err(invalid());
        }
        let mirrored = history(&vault.root.join("records"), &id)?;
        let primary = history(&root, &id)?;
        let completed = vault.completed_prefix(&mirrored)?;
        let (_, bytes, current) = mirrored.last().ok_or_else(invalid)?;
        let head = current.generation;
        let equal_prefix = |a: &[(String, Vec<u8>, Record)], b: &[(String, Vec<u8>, Record)]| {
            a.iter()
                .map(|(n, b, _)| (n, b))
                .eq(b.iter().map(|(n, b, _)| (n, b)))
        };
        let mut primary_published = false;
        let mut marker_published = false;
        let mut enrollment_completed = false;
        let (final_generation, final_hash, final_in_flight);
        fence.check()?;
        session.check_exclusive(&profile).map_err(|_| invalid())?;
        if current.recovery_binding.is_none() {
            // Locator publication interrupted registration before its candidate.
            // Copy must be complete and exactly equal; no arbitrary history selection.
            if head + 1 != vault.binding.enrolled_generation
                || !equal_prefix(&mirrored, &primary)
                || !exact_names(&vault.root.join("completed"))?.is_empty()
                || head >= MAX_RECORDS as u64
            {
                return Err(Error::TrustWriteUncertain);
            }
            if now < current.observed_at {
                return Err(Error::TrustClockRollback);
            }
            vault.check_enrollment(&root, &hash(bytes))?;
            let mut next = current.clone();
            next.generation += 1;
            next.previous_sha256 = hash(bytes);
            next.observed_at = now;
            next.recovery_binding = Some(vault.binding_sha256.clone());
            next.update_event = None;
            valid_record(&next, &id)?;
            transition(current, &next)?;
            vault.prepare(&next)?;
            fence.check()?;
            final_hash = write(&root, &next)?;
            primary_published = true;
            fence.check()?;
            vault.finish(&root, &next, &final_hash)?;
            marker_published = true;
            enrollment_completed = true;
            final_generation = next.generation;
            final_in_flight = next
                .intent
                .as_ref()
                .is_some_and(|i| in_flight(i.update.stage()));
        } else {
            if head < vault.binding.enrolled_generation
                || current.recovery_binding.as_deref() != Some(&vault.binding_sha256)
                || primary.len() > mirrored.len()
                || mirrored.len() - primary.len() > 1
                || !equal_prefix(&primary, &mirrored[..primary.len()])
            {
                return Err(invalid());
            }
            if completed == head {
                // A stale primary cannot use reconciliation to bypass current vault checks.
                if primary.len() != mirrored.len() {
                    return Err(Error::TrustRollback);
                }
                vault.check(&root, &hash(bytes))?;
            } else {
                if completed + 1 != head {
                    return Err(Error::TrustWriteUncertain);
                }
                // Validate both unchanged snapshots again immediately before publication.
                if !equal_prefix(&primary, &history(&root, &id)?)
                    || !equal_prefix(&mirrored, &history(&vault.root.join("records"), &id)?)
                {
                    return Err(invalid());
                }
                vault.check_identity()?;
                fence.check()?;
                if primary.len() != mirrored.len() {
                    publish_bytes(&root, &format!("{head:020}.json"), bytes)?;
                    primary_published = true;
                }
                fence.check()?;
                vault.finish(&root, current, &hash(bytes))?;
                marker_published = true;
                enrollment_completed = head == vault.binding.enrolled_generation;
            }
            final_generation = head;
            final_hash = hash(bytes);
            final_in_flight = current
                .intent
                .as_ref()
                .is_some_and(|i| in_flight(i.update.stage()));
        }
        fence.check()?;
        session.check_exclusive(&profile).map_err(|_| invalid())?;
        vault.check(&root, &final_hash)?;
        Ok(AuthorityReconciliationReceipt {
            generation: final_generation,
            records: final_generation as usize,
            head_sha256: final_hash,
            primary_record_published: primary_published,
            completion_marker_published: marker_published,
            enrollment_completed,
            in_flight_intent_requires_restart_recovery: final_in_flight,
            host_restored: false,
            services_restored: false,
            update_executed: false,
        })
    }
    /// Recover only the absent original namespace from its previously enrolled,
    /// complete independent current vault. No caller vault/head/generation selector.
    /// Existing/partial roots, incomplete commits and unbound historical archives refuse.
    pub fn restore_missing_authority(
        profile: &Path,
        installation: &str,
        apps_closed: bool,
    ) -> Result<AuthorityRecoveryReceipt, Error> {
        if !apps_closed {
            return Err(invalid());
        }
        let (root, id, profile, anchor) = scope(profile, installation)?;
        match fs::symlink_metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Error::TrustExists),
        }
        let session =
            profile_backup::anchored_session(&profile, anchor, true).map_err(|_| invalid())?;
        let locks =
            profile_backup::current_host_locks(&profile, &session).map_err(|_| invalid())?;
        let vault = Vault::open(&root, &id, None)?;
        let records = vault.latest()?;
        let (_, bytes, current) = records.last().ok_or_else(invalid)?;
        let parent = root.parent().ok_or_else(invalid)?;
        let stage = parent.join(format!("pending-authority-{}", uuid::Uuid::new_v4()));
        new_dir(&stage)?;
        for (name, bytes, _) in &records {
            publish_bytes(&stage, name, bytes)?;
        }
        vault.check(&stage, &hash(bytes))?;
        session.check_exclusive(&profile).map_err(|_| invalid())?;
        publish(&stage, &root)?;
        sync_dir(parent)?;
        vault.check(&root, &hash(bytes))?;
        session.check_exclusive(&profile).map_err(|_| invalid())?;
        drop(locks);
        Ok(AuthorityRecoveryReceipt {
            generation: current.generation,
            records: records.len(),
            head_sha256: hash(bytes),
            live_authority_restored: true,
            host_restored: false,
            services_restored: false,
            update_executed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn setup() -> (PathBuf, Store, PathBuf) {
        let (p, s, _) = super::super::tests::prepared_fixture();
        drop(s);
        let c = crate::installations::InstallationController::new(p.clone(), None).unwrap();
        drop(c);
        let mut s = Store::open(&p, "default").unwrap();
        let vault = p.parent().unwrap().join("independent-vault");
        let before = serde_json::to_vec(&s.current.intent).unwrap();
        s.enroll_authority_recovery(&vault, 21).unwrap();
        assert_eq!(serde_json::to_vec(&s.current.intent).unwrap(), before);
        (p, s, vault)
    }
    fn bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|x| x.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .map(|p| {
                (
                    p.file_name().unwrap().to_str().unwrap().into(),
                    fs::read(p).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }
    fn retire(p: &Path) {
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn owned_preflight_authority_restore_uses_exact_current_chain_and_refuses_stale_proof() {
        let (p, mut s, vault) = setup();
        let destination = p.parent().unwrap().join("inactive-authority");
        let original = bytes(&s.root);
        let intent = serde_json::to_vec(&s.current.intent).unwrap();
        let proof = s.restore_inactive_authority(&destination).unwrap();
        assert_eq!(bytes(&destination), original);
        assert_eq!(bytes(&s.root), original);
        assert_eq!(bytes(&vault.join("records")), original);
        assert_eq!(serde_json::to_vec(&s.current.intent).unwrap(), intent);
        s.recheck_inactive_authority(&proof).unwrap();
        assert!(s.restore_inactive_authority(&destination).is_err());
        assert_eq!(bytes(&destination), original);
        s.discard_prepared("update-1", 3, 22).unwrap();
        let latest = bytes(&s.root);
        assert!(s.recheck_inactive_authority(&proof).is_err());
        assert_eq!(bytes(&destination), original);
        assert_eq!(bytes(&s.root), latest);
        assert_eq!(bytes(&vault.join("records")), latest);
        drop(s);
        retire(&p);
    }
    #[test]
    fn owned_preflight_authority_restore_refuses_alias_tamper_permissions_and_shared_inputs() {
        for kind in 0..4 {
            let (p, s, vault) = setup();
            let destination = p.parent().unwrap().join("inactive-authority");
            let original = bytes(&s.root);
            let proof = s.restore_inactive_authority(&destination).unwrap();
            let record = destination.join("00000000000000000003.json");
            match kind {
                0 => fs::write(&record, b"{}").unwrap(),
                1 => fs::hard_link(&record, p.parent().unwrap().join("alias.json")).unwrap(),
                2 => fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap(),
                _ => {
                    let retained = p.parent().unwrap().join("retained-inactive");
                    fs::rename(&destination, &retained).unwrap();
                    symlink(&retained, &destination).unwrap();
                }
            }
            let changed = bytes(&destination);
            assert!(s.recheck_inactive_authority(&proof).is_err());
            assert_eq!(bytes(&destination), changed);
            assert_eq!(bytes(&s.root), original);
            assert_eq!(bytes(&vault.join("records")), original);
            assert!(
                s.restore_inactive_authority(&s.profile.join("forbidden"))
                    .is_err()
            );
            assert!(!s.profile.join("forbidden").exists());
            drop(s);
            retire(&p);
        }
    }
    #[test]
    fn authority_vault_recovers_exact_latest_floors_revocations_and_reserved_ids() {
        let (p, mut s, vault) = setup();
        s.discard_prepared("update-1", 3, 22).unwrap();
        let mut policy = s.policy().clone();
        policy.minimum_sequence += 1;
        policy.public_keys = vec![
            ed25519_dalek::SigningKey::from_bytes(&[32; 32])
                .verifying_key()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ];
        s.replace_policy(policy, s.receipt().policy_generation, 23)
            .unwrap();
        let root = s.root.clone();
        let expected = bytes(&root);
        let head = s.current_sha256.clone();
        let revoked = s.current.revoked_keys.clone();
        let ids = (s.used_operations.clone(), s.used_instances.clone());
        let floor = s.receipt().minimum_sequence;
        assert_eq!(bytes(&vault.join("records")), expected);
        drop(s);
        let retained = p.parent().unwrap().join("retained-original-authority");
        fs::rename(&root, &retained).unwrap();
        assert!(matches!(
            Store::open(&p, "default"),
            Err(Error::TrustMissing)
        ));
        let receipt = Store::restore_missing_authority(&p, "default", true).unwrap();
        assert!(
            receipt.live_authority_restored
                && !receipt.host_restored
                && !receipt.services_restored
                && !receipt.update_executed
        );
        assert_eq!(bytes(&root), expected);
        assert_eq!(bytes(&retained), expected);
        let restored = Store::open(&p, "default").unwrap();
        assert_eq!(restored.current_sha256, head);
        assert_eq!(restored.current.revoked_keys, revoked);
        assert_eq!(restored.receipt().minimum_sequence, floor);
        assert_eq!(
            (
                restored.used_operations.clone(),
                restored.used_instances.clone()
            ),
            ids
        );
        drop(restored);
        retire(&p);
    }
    #[test]
    fn authority_vault_incomplete_dual_commit_refuses_open_and_missing_root_recovery() {
        for primary_written in [false, true] {
            let (p, s, vault) = setup();
            let root = s.root.clone();
            let mut next = s.current.clone();
            next.generation += 1;
            next.previous_sha256 = s.current_sha256.clone();
            next.observed_at = 22;
            next.update_event = None;
            next.policy_generation += 1;
            next.acceptance = None;
            s.recovery.as_ref().unwrap().prepare(&next).unwrap();
            if primary_written {
                write(&root, &next).unwrap();
            }
            drop(s);
            let expected = bytes(&vault.join("records"));
            assert!(Store::open(&p, "default").is_err());
            fs::rename(&root, p.parent().unwrap().join("retained-original")).unwrap();
            assert!(Store::restore_missing_authority(&p, "default", true).is_err());
            assert!(!root.exists());
            assert_eq!(bytes(&vault.join("records")), expected);
            retire(&p);
        }
    }
    #[test]
    fn authority_vault_stale_primary_cannot_open_or_bootstrap_and_existing_root_is_preserved() {
        let (p, mut s, _) = setup();
        let root = s.root.clone();
        let policy = s.policy().clone();
        s.discard_prepared("update-1", 3, 22).unwrap();
        let newest = root.join(format!("{:020}.json", s.current.generation));
        drop(s);
        let before = bytes(&root);
        assert!(matches!(
            Store::restore_missing_authority(&p, "default", true),
            Err(Error::TrustExists)
        ));
        assert_eq!(bytes(&root), before);
        fs::rename(&newest, p.parent().unwrap().join("retained-latest.json")).unwrap();
        assert!(Store::open(&p, "default").is_err());
        fs::rename(&root, p.parent().unwrap().join("stale-original")).unwrap();
        assert!(matches!(
            Store::provision(&p, "default", policy, 30),
            Err(Error::TrustExists)
        ));
        assert!(!root.exists());
        retire(&p);
    }
    #[test]
    fn authority_vault_unknown_hardlinked_or_tampered_records_refuse_without_overwrite() {
        for kind in 0..4 {
            let (p, s, vault) = setup();
            let root = s.root.clone();
            drop(s);
            let record = vault.join("records/00000000000000000003.json");
            match kind {
                0 => {
                    private_file(&vault.join("records/unknown"), true).unwrap();
                }
                1 => {
                    fs::hard_link(&record, vault.join("record-alias")).unwrap();
                }
                2 => {
                    fs::write(&record, b"{}").unwrap();
                }
                _ => {
                    private_file(&vault.join("records/trust.lock"), true).unwrap();
                }
            }
            fs::rename(&root, p.parent().unwrap().join("retained-original")).unwrap();
            let before = bytes(&vault.join("records"));
            assert!(Store::restore_missing_authority(&p, "default", true).is_err());
            assert!(!root.exists());
            assert_eq!(bytes(&vault.join("records")), before);
            retire(&p);
        }
    }
    #[test]
    fn authority_vault_binding_permission_alias_and_absent_acknowledgement_refuse() {
        let (p, s, vault) = setup();
        let root = s.root.clone();
        let loc = s.recovery.as_ref().unwrap().locator.clone();
        drop(s);
        fs::rename(&root, p.parent().unwrap().join("retained-original")).unwrap();
        assert!(Store::restore_missing_authority(&p, "default", false).is_err());
        assert!(!root.exists());
        fs::set_permissions(loc.join("binding.json"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::restore_missing_authority(&p, "default", true).is_err());
        fs::set_permissions(loc.join("binding.json"), fs::Permissions::from_mode(0o600)).unwrap();
        let saved = p.parent().unwrap().join("retained-vault");
        fs::rename(&vault, &saved).unwrap();
        symlink(&saved, &vault).unwrap();
        assert!(Store::restore_missing_authority(&p, "default", true).is_err());
        assert!(!root.exists());
        retire(&p);
    }
    #[test]
    fn authority_vault_registration_cannot_drop_or_change_binding_or_mutate_policy() {
        let (p, s, _) = setup();
        let mut next = s.current.clone();
        next.generation += 1;
        next.observed_at += 1;
        next.recovery_binding = None;
        assert!(transition(&s.current, &next).is_err());
        next.recovery_binding = Some("f".repeat(64));
        assert!(transition(&s.current, &next).is_err());
        drop(s);
        retire(&p);
        let (p, mut s, _) = super::super::tests::prepared_fixture();
        let current = s.current.clone();
        let mut next = current.clone();
        next.generation += 1;
        next.observed_at += 1;
        next.recovery_binding = Some("f".repeat(64));
        next.policy.minimum_sequence += 1;
        assert!(transition(&current, &next).is_err());
        let before = s.current_sha256.clone();
        assert!(
            s.enroll_authority_recovery(&p.join("unsafe-vault"), 21)
                .is_err()
        );
        assert_eq!(s.current_sha256, before);
        drop(s);
        retire(&p);
    }
    #[test]
    fn authority_vault_child_or_lock_replacement_refuses_before_primary_mutation() {
        for lock_replaced in [false, true] {
            let (p, mut s, vault) = setup();
            let before = bytes(&s.root);
            let head = s.current_sha256.clone();
            if lock_replaced {
                let lock = vault.join("trust.lock");
                fs::rename(&lock, vault.join("retained-lock")).unwrap();
                private_file(&lock, true).unwrap();
            } else {
                let child = vault.join("records");
                fs::rename(&child, vault.join("retained-records")).unwrap();
                new_dir(&child).unwrap();
                for (name, bytes) in bytes(&vault.join("retained-records")) {
                    publish_bytes(&child, &name, &bytes).unwrap();
                }
            }
            assert!(s.discard_prepared("update-1", 3, 22).is_err());
            assert_eq!(s.current_sha256, head);
            assert_eq!(bytes(&s.root), before);
            drop(s);
            retire(&p);
        }
    }
    #[test]
    fn authority_vault_interrupted_enrollment_cannot_continue_unenrolled_writes() {
        let (p, mut s, _) = super::super::tests::prepared_fixture();
        let root = s.root.clone();
        let loc = locator(&root, &s.scope).unwrap();
        let before = bytes(&root);
        new_dir(&loc).unwrap();
        assert!(matches!(
            s.discard_prepared("update-1", 2, 21),
            Err(Error::TrustWriteUncertain)
        ));
        assert_eq!(bytes(&root), before);
        drop(s);
        assert!(Store::open(&p, "default").is_err());
        retire(&p);
    }
    fn candidate(s: &Store) -> Record {
        let mut next = s.current.clone();
        next.generation += 1;
        next.previous_sha256 = s.current_sha256.clone();
        next.observed_at = 22;
        next.update_event = None;
        next.policy_generation += 1;
        next.acceptance = None;
        next
    }
    #[test]
    fn authority_reconcile_crash_worker() {
        let Ok(profile) = std::env::var("EXHIBITOS_SYNTHETIC_AUTHORITY_CRASH_PROFILE") else {
            return;
        };
        let profile = PathBuf::from(profile);
        assert!(
            profile
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("exhibitos-release-trust-")
        );
        let s = Store::open(&profile, "default").unwrap();
        let next = candidate(&s);
        s.recovery.as_ref().unwrap().prepare(&next).unwrap();
        if std::env::var("EXHIBITOS_SYNTHETIC_PRIMARY_WRITTEN").unwrap() == "true" {
            write(&s.root, &next).unwrap();
        }
        // Qualified test-only process exit: no destructors, real durable writes
        // and kernel lock release. No product environment flag or unsafe endpoint.
        unsafe {
            libc::_exit(77);
        }
    }
    #[test]
    fn authority_reconcile_abrupt_exit_preserves_candidate_and_ids_without_replay() {
        for primary_written in [false, true] {
            let (p, s, vault_path) = setup();
            let root = s.root.clone();
            let previous = bytes(&root);
            let ids = (s.used_operations.clone(), s.used_instances.clone());
            let expected = serde_json::to_vec(&candidate(&s)).unwrap();
            drop(s);
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "signed_release::trust::authority_recovery_vault::tests::authority_reconcile_crash_worker", "--nocapture"])
                .env("EXHIBITOS_SYNTHETIC_AUTHORITY_CRASH_PROFILE", &p)
                .env("EXHIBITOS_SYNTHETIC_PRIMARY_WRITTEN", primary_written.to_string())
                .output().unwrap();
            assert_eq!(
                child.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&child.stderr)
            );
            assert!(Store::open(&p, "default").is_err());
            let mirrored_before = bytes(&vault_path.join("records"));
            let repaired = Store::reconcile_authority(&p, "default", true, 23).unwrap();
            assert_eq!(repaired.generation, 4);
            assert_eq!(repaired.primary_record_published, !primary_written);
            assert!(repaired.completion_marker_published);
            assert!(
                !repaired.host_restored && !repaired.services_restored && !repaired.update_executed
            );
            assert_eq!(
                fs::read(root.join("00000000000000000004.json")).unwrap(),
                expected
            );
            assert_eq!(bytes(&vault_path.join("records")), mirrored_before);
            for (name, content) in &previous {
                assert_eq!(fs::read(root.join(name)).unwrap(), *content);
            }
            let reopened = Store::open(&p, "default").unwrap();
            assert_eq!(
                (
                    reopened.used_operations.clone(),
                    reopened.used_instances.clone()
                ),
                ids
            );
            assert_eq!(reopened.current_sha256, hash(&expected));
            drop(reopened);
            let repeated = Store::reconcile_authority(&p, "default", true, 24).unwrap();
            assert!(!repeated.primary_record_published && !repeated.completion_marker_published);
            assert_eq!(bytes(&root), mirrored_before);
            retire(&p);
        }
    }
    #[test]
    fn authority_reconcile_resumes_exact_enrollment_without_changing_intent_or_policy() {
        for stage in ["bound", "candidate", "primary"] {
            let (p, s, vault_path) = setup();
            let root = s.root.clone();
            let old_intent = serde_json::to_vec(&s.current.intent).unwrap();
            let old_policy = serde_json::to_vec(&s.current.policy).unwrap();
            let enrollment = read_record(&root.join("00000000000000000003.json")).unwrap();
            drop(s);
            let held = p.parent().unwrap().join("interrupted-enrollment-retained");
            new_dir(&held).unwrap();
            fs::rename(
                vault_path.join("completed/00000000000000000003.json"),
                held.join("completion.json"),
            )
            .unwrap();
            if stage != "primary" {
                fs::rename(
                    root.join("00000000000000000003.json"),
                    held.join("primary.json"),
                )
                .unwrap();
            }
            if stage == "bound" {
                fs::rename(
                    vault_path.join("records/00000000000000000003.json"),
                    held.join("vault.json"),
                )
                .unwrap();
            }
            assert!(Store::open(&p, "default").is_err());
            let receipt = Store::reconcile_authority(&p, "default", true, 23).unwrap();
            assert!(receipt.enrollment_completed && receipt.completion_marker_published);
            assert_eq!(receipt.primary_record_published, stage != "primary");
            let opened = Store::open(&p, "default").unwrap();
            assert_eq!(
                serde_json::to_vec(&opened.current.intent).unwrap(),
                old_intent
            );
            assert_eq!(
                serde_json::to_vec(&opened.current.policy).unwrap(),
                old_policy
            );
            if stage != "bound" {
                assert_eq!(
                    read_record(&root.join("00000000000000000003.json")).unwrap(),
                    enrollment
                );
            }
            assert_eq!(bytes(&root), bytes(&vault_path.join("records")));
            drop(opened);
            retire(&p);
        }
    }
    #[test]
    fn authority_reconcile_refuses_stale_primary_absence_unknown_pending_and_missing_markers() {
        for mutation in ["stale", "absent", "partial", "marker_gap", "multiple"] {
            let (p, mut s, vault_path) = setup();
            let root = s.root.clone();
            let mut policy = s.policy().clone();
            policy.minimum_sequence += 1;
            s.replace_policy(policy, s.current.policy_generation, 22)
                .unwrap();
            drop(s);
            let held = p.parent().unwrap().join("retained-input");
            new_dir(&held).unwrap();
            match mutation {
                "stale" => fs::rename(
                    root.join("00000000000000000004.json"),
                    held.join("stale.json"),
                )
                .unwrap(),
                "absent" => fs::rename(&root, held.join("root")).unwrap(),
                "partial" => {
                    let pending = vault_path
                        .join("records")
                        .join(format!("pending-{}.json", uuid::Uuid::new_v4()));
                    let mut f = private_file(&pending, true).unwrap();
                    f.write_all(b"{incomplete").unwrap();
                    f.sync_all().unwrap();
                }
                "marker_gap" => fs::rename(
                    vault_path.join("completed/00000000000000000003.json"),
                    held.join("marker.json"),
                )
                .unwrap(),
                "multiple" => {
                    fs::rename(
                        vault_path.join("completed/00000000000000000003.json"),
                        held.join("marker3.json"),
                    )
                    .unwrap();
                    fs::rename(
                        vault_path.join("completed/00000000000000000004.json"),
                        held.join("marker4.json"),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            let retained = bytes(&vault_path.join("records"));
            let completed = bytes(&vault_path.join("completed"));
            let source = if root.exists() {
                Some(bytes(&root))
            } else {
                None
            };
            assert!(Store::reconcile_authority(&p, "default", true, 23).is_err());
            assert_eq!(bytes(&vault_path.join("records")), retained);
            assert_eq!(bytes(&vault_path.join("completed")), completed);
            assert_eq!(
                if root.exists() {
                    Some(bytes(&root))
                } else {
                    None
                },
                source
            );
            retire(&p);
        }
    }
    #[test]
    fn authority_reconcile_refuses_live_owner_and_missing_ack_without_mutation() {
        let (p, s, vault_path) = setup();
        let root = s.root.clone();
        let before = bytes(&root);
        let mirrored = bytes(&vault_path.join("records"));
        assert_eq!(
            Store::reconcile_authority(&p, "default", false, 23).unwrap_err(),
            Error::TrustInvalid
        );
        assert_eq!(
            Store::reconcile_authority(&p, "default", true, 23).unwrap_err(),
            Error::TrustBusy
        );
        assert_eq!(bytes(&root), before);
        assert_eq!(bytes(&vault_path.join("records")), mirrored);
        drop(s);
        retire(&p);
    }
    #[test]
    fn authority_reconcile_inflight_reopens_interrupted_without_runtime_reexecution() {
        let (p, s, _) = setup();
        let root = s.root.clone();
        // Synthetic state-machine evidence; does not qualify the real owned executor.
        let event = UpdateEvent::Begin(Box::new(crate::update::Preflight {
            plan: s.intent().unwrap().update.plan().clone(),
            signature_verified: true,
            artifact_verified: true,
            compatibility_verified: true,
            backup_restore_verified: true,
            current_source_matches_backup: true,
            available_free_bytes: u64::MAX,
            image_only_rollback_verified: false,
        }));
        let mut next = s.current.clone();
        next.generation += 1;
        next.previous_sha256 = s.current_sha256.clone();
        next.observed_at = 22;
        next.intent = evolve(s.intent().unwrap(), &event).unwrap();
        next.update_event = Some(event);
        valid_record(&next, &s.scope).unwrap();
        transition(&s.current, &next).unwrap();
        s.recovery.as_ref().unwrap().prepare(&next).unwrap();
        let pending = serde_json::to_vec(&next).unwrap();
        drop(s);
        let receipt = Store::reconcile_authority(&p, "default", true, 23).unwrap();
        assert!(receipt.in_flight_intent_requires_restart_recovery);
        assert!(!receipt.update_executed);
        assert_eq!(
            read_record(&root.join("00000000000000000004.json")).unwrap(),
            pending
        );
        let opened = Store::open(&p, "default").unwrap();
        assert_eq!(opened.receipt().generation, 5);
        assert_eq!(
            opened.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        assert!(opened.used_operations.contains("update-1"));
        assert!(opened.used_instances.contains("target-1"));
        drop(opened);
        let repeated = Store::reconcile_authority(&p, "default", true, 24).unwrap();
        assert_eq!(repeated.generation, 5);
        assert!(
            !repeated.primary_record_published
                && !repeated.in_flight_intent_requires_restart_recovery
        );
        retire(&p);
    }
}
