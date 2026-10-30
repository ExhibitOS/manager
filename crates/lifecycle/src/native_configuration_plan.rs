// SPDX-License-Identifier: Apache-2.0
//! In-memory native configuration rebinding only. No installer or executable
//! restore plan is produced; secret bytes never enter a serializable receipt.
use super::*;
use crate::hosted_runtime_file::Pin;
use crate::maintenance_source::DirectoryPin;
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit, OsRng, Payload as CipherPayload, rand_core::RngCore}};

struct ConfigurationFile {
    _profile: bool,
    _name: String,
    _bytes: Secret,
}
pub(crate) struct ReboundConfigurationPlan {
    profile_root: PathBuf,
    source_root: PathBuf,
    target_root: PathBuf,
    files: Vec<ConfigurationFile>,
    pins: Vec<Pin>,
    target_pins: Vec<Pin>,
    directories: Vec<DirectoryPin>,
    candidate_configuration: crate::installations::native_backup::CandidateConfiguration,
    absent: Vec<(PathBuf, String)>,
}
const BEFOREIMAGE_DOMAIN: &[u8] = b"ExhibitOS-native-publication-beforeimages-v1\0";
const BEFOREIMAGE_LIMIT: usize = 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationCommitment {
    profile: bool,
    name: String,
    before_sha256: Option<String>,
    before_bytes: Option<u64>,
    after_sha256: String,
    after_bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeimageEntry<'a> {
    profile: bool,
    name: &'a str,
    bytes: Option<&'a [u8]>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeimageArchive<'a> {
    format: u8,
    operation_id: &'a str,
    attempt_id: &'a str,
    entries: Vec<BeforeimageEntry<'a>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationManifest {
    format: u8,
    operation_id: String,
    attempt_id: String,
    files: Vec<ConfigurationCommitment>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenBeforeimageEntry {
    profile: bool,
    name: String,
    bytes: Option<Vec<u8>>,
}
impl Drop for OpenBeforeimageEntry {
    fn drop(&mut self) { if let Some(bytes) = &mut self.bytes { bytes.fill(0); } }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenBeforeimageArchive {
    format: u8,
    operation_id: String,
    attempt_id: String,
    entries: Vec<OpenBeforeimageEntry>,
}
/// Authenticated bytes alone are not authority to alter a candidate. The
/// caller must independently verify the retained trust binding and fencing.
pub(crate) struct AuthenticatedBeforeimages {
    manifest: ConfigurationManifest,
    archive: OpenBeforeimageArchive,
    path: PathBuf,
    directory: DirectoryPin,
    manifest_pin: Pin,
    archive_pin: Pin,
}
/// Pins the exact published configuration and its absence/inventory boundaries.
/// This is an in-process observation, never an activation or execution permit.
pub(crate) struct PublishedConfigurationSnapshot {
    before: AuthenticatedBeforeimages,
    profile: PathBuf,
    target: PathBuf,
    owner: &'static str,
    pins: Vec<Pin>,
}
impl PublishedConfigurationSnapshot {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.before.check_closed_destinations(&self.target, self.owner)?;
        self.before.check_inventory(&self.profile, &self.target)?;
        self.before.check_no_repair_temps(&self.profile, &self.target)?;
        for (index, file) in self.before.manifest.files.iter().enumerate() {
            self.pins[index].check()?;
            let root = if file.profile { &self.profile } else { &self.target };
            let bytes = Secret(installation_backup::source_bytes(root, &file.name, 2_200_000, true)?);
            if bytes.0.len() as u64 != file.after_bytes || digest(&bytes.0) != file.after_sha256
                || self.before.after_temporary(index)?.is_some() { return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            self.pins[index].check()?;
        }
        self.before.recheck()
    }
}
impl AuthenticatedBeforeimages {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.directory.check()?; self.manifest_pin.check()?; self.archive_pin.check()
    }
    /// Derive every permissible destination from the selected target, never
    /// from a caller-supplied path. Journal beforeimages must be absent.
    pub(crate) fn check_closed_destinations(&self, target: &Path, owner: &str) -> Result<()> {
        if owner != "native-personal.json" && owner != "native-installation.json" { return Err(bad()); }
        let expected = BTreeSet::from(["runtime.env", owner, "config/freeze-signing-key.json"]);
        let mut roots = BTreeSet::new();
        let journals = ["document.json", "upload.json", "oex-import.json", "publication.json", "static-export.json"]
            .into_iter().map(|logical| journal(target, logical)).collect::<Result<BTreeSet<_>>>()?;
        for file in &self.manifest.files {
            if file.profile {
                if !journals.contains(&file.name) || file.before_sha256.is_some() || file.before_bytes.is_some() { return Err(bad()); }
            } else {
                roots.insert(file.name.as_str());
            }
        }
        if roots != expected { return Err(bad()); }
        self.recheck()
    }
    pub(crate) fn owner(&self) -> Result<&'static str> {
        let mut owners = self.manifest.files.iter().filter(|f| !f.profile && (f.name == "native-personal.json" || f.name == "native-installation.json"));
        let owner = owners.next().ok_or_else(bad)?;
        if owners.next().is_some() { return Err(bad()); }
        match owner.name.as_str() { "native-personal.json" => Ok("native-personal.json"), "native-installation.json" => Ok("native-installation.json"), _ => Err(bad()) }
    }
    pub(crate) fn capture_published(mut self, profile: &Path, target: &Path, owner: &'static str) -> Result<PublishedConfigurationSnapshot> {
        self.check_closed_destinations(target, owner)?;
        self.check_inventory(profile, target)?;
        self.check_no_repair_temps(profile, target)?;
        let mut pins = Vec::with_capacity(self.manifest.files.len());
        for (index, file) in self.manifest.files.iter().enumerate() {
            let root = if file.profile { profile } else { target };
            let private = Secret(installation_backup::source_bytes(root, &file.name, 2_200_000, true)?);
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&root.join(&file.name), 2_200_000)?;
            let bytes = Secret(bytes);
            if private.0 != bytes.0 || bytes.0.len() as u64 != file.after_bytes || digest(&bytes.0) != file.after_sha256
                || self.after_temporary(index)?.is_some() { return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            pins.push(pin);
        }
        // Rechecks need only commitments and pins; do not retain decrypted
        // beforeimage secrets in a long-lived observation capability.
        self.archive.entries.clear();
        let snapshot = PublishedConfigurationSnapshot { before: self, profile: profile.to_owned(), target: target.to_owned(), owner, pins };
        snapshot.recheck()?; Ok(snapshot)
    }
    /// Classify every destination before the first mutation. Unexpected bytes,
    /// links and unsafe file ownership fail closed; authenticated AFTER is not
    /// confused with an arbitrary file having the same name.
    fn is_before(&self, profile: &Path, target: &Path, index: usize) -> Result<bool> {
        let file = &self.manifest.files[index];
        let root = if file.profile { profile } else { target };
        let current = if exists(root, &file.name)? {
            let bytes = Secret(installation_backup::source_bytes(root, &file.name, 2_200_000, true)?);
            Some((digest(&bytes.0), bytes.0.len() as u64))
        } else { None };
        let before = file.before_sha256.as_ref().zip(file.before_bytes).map(|(h,n)| (h.clone(),n));
        if current == before { return Ok(true); }
        if current == Some((file.after_sha256.clone(), file.after_bytes)) { return Ok(false); }
        Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT"))
    }
    fn check_inventory(&self, profile: &Path, target: &Path) -> Result<()> {
        for logical in ["document.json", "upload.json", "oex-import.json", "publication.json", "static-export.json"] {
            let name = journal(target, logical)?;
            if !self.manifest.files.iter().any(|f| f.profile && f.name == name) && exists(profile, &name)? {
                return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT"));
            }
        }
        let key_index = self.manifest.files.iter().position(|f| !f.profile && f.name == "config/freeze-signing-key.json").ok_or_else(bad)?;
        let temp = format!(".native-publication-repair-{}-{key_index}", self.manifest.attempt_id);
        let config = target.join("config"); private_directory(&config)?;
        for entry in fs::read_dir(&config).map_err(|_| bad())? {
            let name = entry.map_err(|_| bad())?.file_name().into_string().map_err(|_| bad())?;
            if name != "freeze-signing-key.json" && name != temp { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        }
        Ok(())
    }
    fn check_no_repair_temps(&self, profile: &Path, target: &Path) -> Result<()> {
        for (index, file) in self.manifest.files.iter().enumerate() {
            let root = if file.profile { profile } else { target };
            let destination = root.join(&file.name);
            let parent = destination.parent().ok_or_else(bad)?;
            if exists(parent, &format!(".native-publication-repair-{}-{index}", self.manifest.attempt_id))? {
                return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
            }
        }
        Ok(())
    }
    fn after_temporary(&self, index: usize) -> Result<Option<Pin>> {
        let name = format!("after-{index}.tmp");
        if !exists(&self.path, &name)? { return Ok(None); }
        let file = &self.manifest.files[index];
        let private = Secret(installation_backup::source_bytes(&self.path, &name, 2_200_000, true)?);
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&self.path.join(&name), 2_200_000)?;
        let bytes = Secret(bytes);
        if private.0 != bytes.0 || digest(&bytes.0) != file.after_sha256 || bytes.0.len() as u64 != file.after_bytes { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        Ok(Some(pin))
    }
    pub(crate) fn publish_afterimages(&self, plan: &ReboundConfigurationPlan, profile: &Path, target: &Path,
                                     owner: &str, mut validate: impl FnMut() -> Result<()>) -> Result<()> {
        self.check_closed_destinations(target, owner)?;
        if plan.profile_root != profile || plan.target_root != target || plan.files.len() != self.manifest.files.len() { return Err(bad()); }
        for (file, commitment) in plan.files.iter().zip(&self.manifest.files) {
            if file._profile != commitment.profile || file._name != commitment.name || digest(&file._bytes.0) != commitment.after_sha256
                || file._bytes.0.len() as u64 != commitment.after_bytes { return Err(bad()); }
        }
        validate()?; self.check_inventory(profile, target)?;
        for index in 0..plan.files.len() {
            if !self.is_before(profile, target, index)? || self.after_temporary(index)?.is_some() { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        }
        for (index, file) in plan.files.iter().enumerate() {
            validate()?; self.recheck()?; self.check_inventory(profile, target)?;
            for other in 0..plan.files.len() {
                let before = self.is_before(profile, target, other)?;
                let c = &self.manifest.files[other];
                let same = c.before_sha256.as_deref() == Some(c.after_sha256.as_str()) && c.before_bytes == Some(c.after_bytes);
                if !same && before != (other >= index) { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            }
            let c = &self.manifest.files[index];
            if c.before_sha256.as_deref() == Some(c.after_sha256.as_str()) && c.before_bytes == Some(c.after_bytes) { continue; }
            let root = if file._profile { profile } else { target };
            let destination = root.join(&file._name);
            let parent = destination.parent().ok_or_else(bad)?;
            private_directory(parent)?;
            let parent_pin = DirectoryPin::open(parent)?;
            let previous = if exists(root, &file._name)? { Some(crate::hosted_runtime_file::bounded(&destination, 2_200_000)?.1) } else { None };
            let temporary = format!("after-{index}.tmp");
            write_private_new(&self.path, &temporary, &file._bytes.0, 2_200_000)?;
            File::open(&self.path).and_then(|f| f.sync_all()).map_err(|_| bad())?;
            let temp_pin = self.after_temporary(index)?.ok_or_else(bad)?;
            validate()?; self.recheck()?; parent_pin.check()?; temp_pin.check()?;
            if !self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            if let Some(pin) = previous {
                pin.check()?; fs::rename(self.path.join(&temporary), &destination).map_err(|_| bad())?;
            } else {
                use std::os::unix::ffi::OsStrExt;
                let from = std::ffi::CString::new(self.path.join(&temporary).as_os_str().as_bytes()).map_err(|_| bad())?;
                let to = std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|_| bad())?;
                if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            }
            File::open(parent).and_then(|f| f.sync_all()).map_err(|_| bad())?;
            File::open(&self.path).and_then(|f| f.sync_all()).map_err(|_| bad())?;
            if self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        }
        self.sync_destination_parents(profile, target, &mut validate, |path| File::open(path).and_then(|f| f.sync_all()).map_err(|_| bad()))?;
        validate()?; self.recheck()?; self.check_inventory(profile, target)?;
        for index in 0..plan.files.len() {
            let c = &self.manifest.files[index];
            let same = c.before_sha256.as_deref() == Some(c.after_sha256.as_str()) && c.before_bytes == Some(c.after_bytes);
            if (!same && self.is_before(profile, target, index)?) || self.after_temporary(index)?.is_some() { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        }
        Ok(())
    }
    fn sync_destination_parents(&self, profile: &Path, target: &Path,
                                mut validate: impl FnMut() -> Result<()>,
                                mut sync: impl FnMut(&Path) -> Result<()>) -> Result<()> {
        for file in &self.manifest.files {
            let root = if file.profile { profile } else { target };
            let destination = root.join(&file.name);
            let parent = destination.parent().ok_or_else(bad)?;
            private_directory(parent)?;
            let parent_pin = DirectoryPin::open(parent)?;
            validate()?; self.recheck()?; parent_pin.check()?;
            sync(parent)?;
            parent_pin.check()?;
        }
        Ok(())
    }
    /// Internal primitive only: caller supplies and retains all trust/profile/
    /// raw-target fences, and must commit the terminal binding afterwards.
    pub(crate) fn restore_beforeimages(&self, profile: &Path, target: &Path, owner: &str,
                            mut validate: impl FnMut() -> Result<()>) -> Result<()> {
        self.check_closed_destinations(target, owner)?;
        validate()?; self.check_inventory(profile, target)?;
        for (index, file) in self.manifest.files.iter().enumerate() {
            self.after_temporary(index)?;
            let before = self.is_before(profile, target, index)?;
            let root = if file.profile { profile } else { target };
            let parent = root.join(&file.name).parent().ok_or_else(bad)?.to_path_buf();
            let temporary = format!(".native-publication-repair-{}-{index}", self.manifest.attempt_id);
            if exists(&parent, &temporary)? {
                if before || self.archive.entries[index].bytes.is_none() { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
                let bytes = Secret(installation_backup::source_bytes(&parent, &temporary, 2_200_000, true)?);
                if self.archive.entries[index].bytes.as_ref() != Some(&bytes.0) { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            }
        }
        for (index, (file, entry)) in self.manifest.files.iter().zip(&self.archive.entries).enumerate() {
            validate()?; self.recheck()?;
            if self.is_before(profile, target, index)? { continue; }
            let root = if file.profile { profile } else { target };
            let destination = root.join(&file.name);
            let parent = destination.parent().ok_or_else(bad)?;
            private_directory(parent)?;
            let directory_pin = DirectoryPin::open(parent)?;
            let (_, pin) = crate::hosted_runtime_file::bounded(&destination, 2_200_000)?;
            if let Some(before) = &entry.bytes {
                let temporary = format!(".native-publication-repair-{}-{index}", self.manifest.attempt_id);
                if !exists(parent, &temporary)? { write_private_new(parent, &temporary, before, 2_200_000)?; }
                let private = Secret(installation_backup::source_bytes(parent, &temporary, 2_200_000, true)?);
                let (temp_bytes, temp_pin) = crate::hosted_runtime_file::bounded(&parent.join(&temporary), 2_200_000)?;
                let temp_bytes = Secret(temp_bytes);
                if private.0 != *before || temp_bytes.0 != *before { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
                validate()?; self.recheck()?; directory_pin.check()?; pin.check()?; temp_pin.check()?;
                if self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
                fs::rename(parent.join(&temporary), &destination).map_err(|_| bad())?;
            } else {
                validate()?; self.recheck()?; directory_pin.check()?; pin.check()?;
                if self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
                fs::remove_file(&destination).map_err(|_| bad())?;
            }
            File::open(parent).and_then(|f| f.sync_all()).map_err(|_| bad())?;
            if !self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
        }
        // A previous interrupted repair may already have renamed or removed a
        // file without syncing its parent. Sync every destination parent even
        // when this attempt observed the desired BEFORE bytes and made no write.
        self.sync_destination_parents(profile, target, &mut validate, |path| File::open(path).and_then(|f| f.sync_all()).map_err(|_| bad()))?;
        validate()?; self.recheck()?; self.check_inventory(profile, target)?;
        for index in 0..self.manifest.files.len() {
            if !self.is_before(profile, target, index)? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            let file = &self.manifest.files[index]; let root = if file.profile { profile } else { target };
            let parent = root.join(&file.name).parent().ok_or_else(bad)?.to_path_buf();
            if exists(&parent, &format!(".native-publication-repair-{}-{index}", self.manifest.attempt_id))? { return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT")); }
            if let Some(pin) = self.after_temporary(index)? {
                validate()?; self.recheck()?; pin.check()?;
                fs::remove_file(self.path.join(format!("after-{index}.tmp"))).map_err(|_| bad())?;
                File::open(&self.path).and_then(|f| f.sync_all()).map_err(|_| bad())?;
            }
        }
        Ok(())
    }
}
pub(crate) fn open_beforeimages(directory: &Path, operation: &str, attempt: &str, key: &[u8],
                     manifest_hash: &str, archive_hash: &str) -> Result<AuthenticatedBeforeimages> {
    if !uuid(operation) || !uuid(attempt) || operation == attempt || key.len() != 32
        || !hash_valid(manifest_hash) || !hash_valid(archive_hash) { return Err(bad()); }
    private_directory(directory)?;
    let directory_pin = DirectoryPin::open(directory)?;
    let read = |name: &str, limit: u64| -> Result<(Secret, Pin)> {
        let private = Secret(installation_backup::source_bytes(directory, name, limit, true)?);
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&directory.join(name), limit)?;
        let bytes = Secret(bytes);
        if bytes.0 != private.0 { return Err(bad()); }
        Ok((bytes, pin))
    };
    let (manifest_bytes, manifest_pin) = read("configuration.json", 65536)?;
    let (ciphertext, archive_pin) = read("beforeimages.bin", (BEFOREIMAGE_LIMIT + 128) as u64)?;
    if digest(&manifest_bytes.0) != manifest_hash || digest(&ciphertext.0) != archive_hash { return Err(bad()); }
    let manifest: ConfigurationManifest = serde_json::from_slice(&manifest_bytes.0).map_err(|_| bad())?;
    if manifest.format != 1 || manifest.operation_id != operation || manifest.attempt_id != attempt
        || !(3..=7).contains(&manifest.files.len()) { return Err(bad()); }
    let raw = &ciphertext.0;
    if !raw.starts_with(BEFOREIMAGE_DOMAIN) || raw.len() < BEFOREIMAGE_DOMAIN.len() + 12 + 16 { return Err(bad()); }
    let nonce_end = BEFOREIMAGE_DOMAIN.len() + 12;
    let mut aad = BEFOREIMAGE_DOMAIN.to_vec(); aad.extend_from_slice(manifest_hash.as_bytes());
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad())?;
    let plain = Secret(cipher.decrypt(Nonce::from_slice(&raw[BEFOREIMAGE_DOMAIN.len()..nonce_end]), CipherPayload { msg: &raw[nonce_end..], aad: &aad }).map_err(|_| bad())?);
    if plain.0.len() > BEFOREIMAGE_LIMIT { return Err(bad()); }
    let archive: OpenBeforeimageArchive = serde_json::from_slice(&plain.0).map_err(|_| bad())?;
    if archive.format != 1 || archive.operation_id != operation || archive.attempt_id != attempt
        || archive.entries.len() != manifest.files.len() { return Err(bad()); }
    let mut names = BTreeSet::new();
    for (commitment, entry) in manifest.files.iter().zip(&archive.entries) {
        if commitment.profile != entry.profile || commitment.name != entry.name
            || !names.insert((entry.profile, entry.name.as_str())) || entry.name.is_empty() || entry.name.len() > 240
            || !Path::new(&entry.name).components().all(|c| matches!(c, std::path::Component::Normal(_)))
            || !hash_valid(&commitment.after_sha256) || commitment.after_bytes == 0 || commitment.after_bytes > 2_200_000
            || commitment.before_sha256.as_ref().is_some_and(|s| !hash_valid(s))
            || commitment.before_sha256.as_deref() != entry.bytes.as_ref().map(|b| digest(b)).as_deref()
            || commitment.before_bytes != entry.bytes.as_ref().map(|b| b.len() as u64)
            || entry.bytes.as_ref().is_some_and(|b| b.len() > 65536) { return Err(bad()); }
    }
    let result = AuthenticatedBeforeimages { manifest, archive, path: directory.to_owned(), directory: directory_pin, manifest_pin, archive_pin };
    result.recheck()?; Ok(result)
}
/// Held staged identities are evidence for a later trust commit, not authority
/// to publish target files or reconstruct an operation after restart.
pub(crate) struct StagedNativeConfiguration {
    pub(crate) manifest_sha256: String,
    pub(crate) beforeimage_archive_sha256: String,
    directory: DirectoryPin,
    manifest: Pin,
    archive: Pin,
}
impl StagedNativeConfiguration {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.directory.check()?;
        self.manifest.check()?;
        self.archive.check()
    }
}
impl ReboundConfigurationPlan {
    #[cfg(test)]
    pub(crate) fn simulate_partial_for_test(&self, count: usize) -> Result<()> {
        self.recheck()?;
        if count == 0 || count >= self.files.len() { return Err(bad()); }
        for file in self.files.iter().take(count) {
            let root = if file._profile { &self.profile_root } else { &self.target_root };
            let destination = root.join(&file._name);
            let parent = destination.parent().ok_or_else(bad)?;
            let temporary = format!(".synthetic-interruption-{}", Uuid::new_v4());
            write_private_new(parent, &temporary, &file._bytes.0, 2_200_000)?;
            fs::rename(parent.join(&temporary), &destination).map_err(|_| bad())?;
            File::open(parent).and_then(|f| f.sync_all()).map_err(|_| bad())?;
        }
        Ok(())
    }
    pub(crate) fn file_count(&self) -> usize {
        self.files.len()
    }
    /// Compare private proposals without exposing bytes or making receipts an
    /// authority. Both sets of source and candidate pins remain independently
    /// live and must still pass their rechecks.
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.profile_root == other.profile_root && self.source_root == other.source_root && self.target_root == other.target_root
            && self.files.len() == other.files.len()
            && self.files.iter().zip(&other.files).all(|(a, b)| {
                a._profile == b._profile && a._name == b._name && a._bytes.0 == b._bytes.0
            })
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        self.recheck_source()?;
        for pin in &self.target_pins { pin.check()?; }
        self.candidate_configuration.check()?;
        for (root, name) in &self.absent {
            if exists(root, name)? {
                return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
            }
        }
        Ok(())
    }
    pub(crate) fn recheck_source(&self) -> Result<()> {
        for directory in &self.directories {
            directory.check()?;
        }
        for pin in &self.pins {
            pin.check()?;
        }
        Ok(())
    }

    /// Stage only under an already fenced inactive candidate. Caller retains
    /// both Session fences and the profile fence, and revalidates authority
    /// before and after this operation. No destination file is changed.
    pub(crate) fn stage_beforeimages(&self, profile: &Path, target: &Path, directory: &Path,
                                     operation: &str, attempt: &str, key: &[u8]) -> Result<StagedNativeConfiguration> {
        if profile != self.profile_root || target != self.target_root || !uuid(operation) || !uuid(attempt) || operation == attempt || key.len() != 32
            || directory != target.join(format!("native-target-preparation-{operation}"))
            || self.files.len() < 3 || self.files.len() > 7 { return Err(bad()); }
        private_directory(directory)?;
        if fs::canonicalize(directory).ok().as_deref() != Some(directory) { return Err(bad()); }
        self.recheck()?;
        let mut commitments = Vec::with_capacity(self.files.len());
        let mut previous = Vec::with_capacity(self.files.len());
        let mut names = BTreeSet::new();
        for file in &self.files {
            if !names.insert((file._profile, file._name.clone())) { return Err(bad()); }
            let root = if file._profile { profile } else { target };
            let before = if exists(root, &file._name)? {
                Some(Secret(installation_backup::source_bytes(root, &file._name, 65536, true)?))
            } else { None };
            if file._profile && before.is_some() { return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            commitments.push(ConfigurationCommitment {
                profile: file._profile, name: file._name.clone(),
                before_sha256: before.as_ref().map(|b| digest(&b.0)),
                before_bytes: before.as_ref().map(|b| b.0.len() as u64),
                after_sha256: digest(&file._bytes.0), after_bytes: file._bytes.0.len() as u64,
            });
            previous.push(before);
        }
        self.recheck()?;
        let entries: Vec<_> = self.files.iter().zip(&previous).map(|(f, b)| BeforeimageEntry {
            profile: f._profile, name: &f._name, bytes: b.as_ref().map(|v| v.0.as_slice()),
        }).collect();
        let manifest = serde_json::to_vec(&serde_json::json!({"format":1,"operationId":operation,"attemptId":attempt,"files":commitments})).map_err(|_| bad())?;
        if manifest.len() > 65536 { return Err(bad()); }
        let manifest_sha256 = digest(&manifest);
        let mut aad = BEFOREIMAGE_DOMAIN.to_vec(); aad.extend_from_slice(manifest_sha256.as_bytes());
        let mut plain = Secret(serde_json::to_vec(&BeforeimageArchive { format: 1, operation_id: operation, attempt_id: attempt, entries }).map_err(|_| bad())?);
        if plain.0.len() > BEFOREIMAGE_LIMIT { return Err(bad()); }
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad())?;
        let mut nonce = [0u8; 12]; OsRng.fill_bytes(&mut nonce);
        let encrypted = cipher.encrypt(Nonce::from_slice(&nonce), CipherPayload { msg: &plain.0, aad: &aad }).map_err(|_| bad())?;
        plain.0.fill(0);
        let mut archive = Vec::with_capacity(BEFOREIMAGE_DOMAIN.len() + nonce.len() + encrypted.len());
        archive.extend_from_slice(BEFOREIMAGE_DOMAIN); archive.extend_from_slice(&nonce); archive.extend_from_slice(&encrypted);
        let staged = directory.join(format!("publication-{attempt}"));
        new_directory(&staged)?;
        write_private_new(&staged, "configuration.json", &manifest, 65536)?;
        write_private_new(&staged, "beforeimages.bin", &archive, BEFOREIMAGE_LIMIT + 128)?;
        File::open(&staged).and_then(|f| f.sync_all()).map_err(|_| bad())?;
        File::open(directory).and_then(|f| f.sync_all()).map_err(|_| bad())?;
        let (manifest_check, manifest_pin) = crate::hosted_runtime_file::bounded(&staged.join("configuration.json"), 65536)?;
        let (archive_check, archive_pin) = crate::hosted_runtime_file::bounded(&staged.join("beforeimages.bin"), (BEFOREIMAGE_LIMIT + 128) as u64)?;
        if manifest_check != manifest || archive_check != archive { return Err(bad()); }
        let staged = StagedNativeConfiguration { manifest_sha256, beforeimage_archive_sha256: digest(&archive), directory: DirectoryPin::open(&staged)?, manifest: manifest_pin, archive: archive_pin };
        self.recheck()?; staged.recheck()?; Ok(staged)
    }
}
fn pinned_secret(root: &Path, name: &str, limit: u64, pins: &mut Vec<Pin>) -> Result<Secret> {
    let private = Secret(installation_backup::source_bytes(root, name, limit, true)?);
    let (bytes, pin) = crate::hosted_runtime_file::bounded(&root.join(name), limit)?;
    let bytes = Secret(bytes);
    if private.0 != bytes.0 {
        return Err(bad());
    }
    pin.check()?;
    pins.push(pin);
    Ok(bytes)
}
fn authenticated_file(
    manifest: &Value,
    role: &str,
    name: &str,
    root: &Path,
    logical: &str,
    limit: u64,
    pins: &mut Vec<Pin>,
) -> Result<Secret> {
    let entries = manifest["files"].as_array().ok_or_else(bad)?;
    let matches: Vec<_> = entries
        .iter()
        .filter(|v| v["role"] == role && v["name"] == name)
        .collect();
    if matches.len() != 1 {
        return Err(bad());
    }
    let bytes = pinned_secret(root, logical, limit, pins)?;
    if matches[0]["bytes"].as_u64() != Some(bytes.0.len() as u64)
        || matches[0]["sha256"].as_str() != Some(digest(&bytes.0).as_str())
    {
        return Err(bad());
    }
    Ok(bytes)
}
/// Called only by the authenticated same-catalog target adapter, under its
/// profile lock and both native session fences. This function reads/pins the
/// verified archive and generated candidate configuration; it writes nothing.
pub(crate) fn plan_rebound_configuration(
    verified: &Path,
    authenticated_manifest: &[u8],
    source_root: &Path,
    source_payload: &Payload,
    target: &mut Session,
    profile: &Path,
) -> Result<ReboundConfigurationPlan> {
    let root = target.target_observation_root().to_owned();
    let source = LifecycleService::bound_existing(source_root.to_owned())?;
    let candidate = LifecycleService::bound_existing(root.clone())?;
    let old = source.native_prepared()?.ok_or_else(bad)?;
    let new = candidate.native_prepared()?.ok_or_else(bad)?;
    let target_payload = target.maintenance_payload();
    if old.kind != new.kind
        || old.manifest_sha256 != source_payload.context().manifest_sha256
        || new.manifest_sha256 != target_payload.context().manifest_sha256
        || old.manifest_sha256 == new.manifest_sha256
    {
        return Err(bad());
    }
    let owner = owner_name(&old.kind)?;
    let configuration = verified.join("configuration");
    let mut directories = vec![
        DirectoryPin::open(verified)?,
        DirectoryPin::open(&configuration)?,
        DirectoryPin::open(source_root)?,
        DirectoryPin::open(&root)?,
        DirectoryPin::open(profile)?,
    ];
    let mut pins = Vec::new();
    let candidate_configuration =
        crate::installations::native_backup::CandidateConfiguration::capture(&root.join("config"))?;
    let mut absent = Vec::new();
    let manifest_bytes = pinned_secret(verified, "manifest.json", 16 * 1024 * 1024, &mut pins)?;
    if manifest_bytes.0 != authenticated_manifest {
        return Err(bad());
    }
    let manifest: Value = serde_json::from_slice(authenticated_manifest).map_err(|_| bad())?;
    let expected: BTreeSet<String> = [
        "native-source.json",
        "native-runtime.env",
        owner,
        "freeze-signing-key.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    names(&configuration, &expected)?;
    let config_names: Vec<_> = manifest["files"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .filter(|v| v["role"] == "configuration")
        .map(|v| v["name"].as_str().ok_or_else(bad))
        .collect::<Result<_>>()?;
    if config_names.len() != expected.len()
        || config_names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
            != expected
    {
        return Err(bad());
    }
    let map_bytes = authenticated_file(
        &manifest,
        "configuration",
        "native-source.json",
        &configuration,
        "native-source.json",
        65536,
        &mut pins,
    )?;
    let map: SourceMap = serde_json::from_slice(&map_bytes.0).map_err(|_| bad())?;
    if map.format != 1
        || map.source_root != source_root
        || map.owner_kind != old.kind
        || map.manifest_sha256 != old.manifest_sha256
        || map.journal_names.len() > 4
    {
        return Err(bad());
    }
    // Keep the ordinary restore's closed empty-destination rule even when a
    // particular journal was absent from the authenticated source archive.
    for logical in [
        "document.json",
        "upload.json",
        "oex-import.json",
        "publication.json",
        "static-export.json",
    ] {
        let name = journal(&root, logical)?;
        if exists(profile, &name)? {
            return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
        }
        absent.push((profile.to_owned(), name));
    }
    for (logical, name) in &map.journal_names {
        if journal(source_root, logical)? != *name {
            return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
        }
    }
    let original_env = authenticated_file(
        &manifest,
        "configuration",
        "native-runtime.env",
        &configuration,
        "native-runtime.env",
        8192,
        &mut pins,
    )?;
    let original_owner = authenticated_file(
        &manifest,
        "configuration",
        owner,
        &configuration,
        owner,
        4096,
        &mut pins,
    )?;
    let signing_key = authenticated_file(
        &manifest,
        "configuration",
        "freeze-signing-key.json",
        &configuration,
        "freeze-signing-key.json",
        65536,
        &mut pins,
    )?;
    // Bind parsed identities to the exact authenticated source bytes, and pin
    // BOTH candidate files while its validated DB credentials/ports are read.
    if original_env.0 != pinned_secret(source_root, "runtime.env", 8192, &mut pins)?.0
        || original_owner.0 != pinned_secret(source_root, owner, 4096, &mut pins)?.0
    {
        return Err(bad());
    }
    let mut target_pins = Vec::new();
    let _target_env = pinned_secret(&root, "runtime.env", 8192, &mut target_pins)?;
    let _target_owner = pinned_secret(&root, owner, 4096, &mut target_pins)?;
    let original_fields = Fields(source.native_start_environment(source_payload, old.ports)?);
    let target_fields = Fields(candidate.native_start_environment(&target_payload, new.ports)?);
    target.maintenance_owner(&old.kind, &original_fields.0)?;
    let environment = rebound_environment(&original_env.0, &target_fields.0)?;
    let mut proposed = Fields(original_fields.0.clone());
    for key in [
        "POSTGRES_PASSWORD",
        "DATABASE_URL",
        "EXHIBITOS_PORT",
        "EXHIBITOS_API_PORT",
        "NATIVE_RUNTIME_MANIFEST_SHA256",
    ] {
        proposed.0.insert(
            key.into(),
            target_fields.0.get(key).ok_or_else(bad)?.clone(),
        );
    }
    rebound_identity(&old.kind, &original_fields, &proposed)?;
    let mut record: Value = serde_json::from_slice(&original_owner.0).map_err(|_| bad())?;
    record["ports"] = serde_json::to_value(new.ports).map_err(|_| bad())?;
    record["manifestSha256"] = Value::String(new.manifest_sha256);
    record["environmentSha256"] = Value::String(digest(&environment.0));
    let mut files = vec![
        ConfigurationFile {
            _profile: false,
            _name: "runtime.env".into(),
            _bytes: environment,
        },
        ConfigurationFile {
            _profile: false,
            _name: owner.into(),
            _bytes: Secret(serde_json::to_vec(&record).map_err(|_| bad())?),
        },
        ConfigurationFile {
            _profile: false,
            _name: "config/freeze-signing-key.json".into(),
            _bytes: signing_key,
        },
    ];
    let journal_root = verified.join("deployment/native-authoring");
    let deployment_names: Vec<_> = manifest["files"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .filter(|v| v["role"] == "deployment")
        .map(|v| {
            v["name"]
                .as_str()
                .and_then(|s| s.strip_prefix("native-authoring/"))
                .ok_or_else(bad)
        })
        .collect::<Result<_>>()?;
    if deployment_names.len() != map.journal_names.len()
        || deployment_names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
            != map.journal_names.keys().cloned().collect()
    {
        return Err(bad());
    }
    if !map.journal_names.is_empty() {
        directories.push(DirectoryPin::open(&journal_root)?);
        names(&journal_root, &map.journal_names.keys().cloned().collect())?;
    }
    for logical in map.journal_names.keys() {
        let bytes = authenticated_file(
            &manifest,
            "deployment",
            &format!("native-authoring/{logical}"),
            &journal_root,
            logical,
            2_200_000,
            &mut pins,
        )?;
        let rebound = Secret(rebound_journal(logical, &bytes.0, &original_fields)?);
        files.push(ConfigurationFile {
            _profile: true,
            _name: journal(&root, logical)?,
            _bytes: rebound,
        });
    }
    let plan = ReboundConfigurationPlan {
        profile_root: profile.to_owned(),
        source_root: source_root.to_owned(),
        target_root: root,
        files,
        pins, target_pins,
        directories,
        candidate_configuration,
        absent,
    };
    plan.recheck()?;
    Ok(plan)
}

#[cfg(test)]
mod begin_stage_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use aes_gcm::aead::Aead;

    #[test]
    fn staged_beforeimages_are_authenticated_and_destinations_unchanged() {
        let root = std::env::temp_dir().join(format!("native-publication-stage-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let profile = root.join("profile"); let target = root.join("target");
        new_directory(&profile).unwrap(); new_directory(&target).unwrap(); new_directory(&target.join("config")).unwrap();
        write_private_new(&target, "runtime.env", b"old-env", 65536).unwrap();
        write_private_new(&target, "native-personal.json", b"old-owner", 65536).unwrap();
        let operation = Uuid::new_v4().to_string(); let attempt = Uuid::new_v4().to_string();
        let parent = target.join(format!("native-target-preparation-{operation}")); new_directory(&parent).unwrap();
        let plan = ReboundConfigurationPlan {
            profile_root: profile.clone(), source_root: root.join("synthetic-source"), target_root: target.clone(),
            files: vec![
                ConfigurationFile { _profile: false, _name: "runtime.env".into(), _bytes: Secret(b"new-env".to_vec()) },
                ConfigurationFile { _profile: false, _name: "native-personal.json".into(), _bytes: Secret(b"new-owner".to_vec()) },
                ConfigurationFile { _profile: false, _name: "config/freeze-signing-key.json".into(), _bytes: Secret(b"source-key".to_vec()) },
            ], pins: vec![], target_pins: vec![], directories: vec![DirectoryPin::open(&profile).unwrap(), DirectoryPin::open(&target).unwrap()],
            candidate_configuration: crate::installations::native_backup::CandidateConfiguration::capture(&target.join("config")).unwrap(), absent: vec![],
        };
        let key = [7u8; 32];
        let staged = plan.stage_beforeimages(&profile, &target, &parent, &operation, &attempt, &key).unwrap();
        staged.recheck().unwrap();
        assert_eq!(fs::read(target.join("runtime.env")).unwrap(), b"old-env");
        assert_eq!(fs::read(target.join("native-personal.json")).unwrap(), b"old-owner");
        assert!(!target.join("config/freeze-signing-key.json").exists());
        let directory = parent.join(format!("publication-{attempt}"));
        let opened = open_beforeimages(&directory, &operation, &attempt, &key, &staged.manifest_sha256, &staged.beforeimage_archive_sha256).unwrap();
        assert_eq!(opened.manifest.files.len(), 3);
        opened.check_closed_destinations(&target, "native-personal.json").unwrap();
        assert!(opened.check_closed_destinations(&target, "native-installation.json").is_err());
        assert_eq!(opened.archive.entries[0].bytes.as_deref(), Some(b"old-env".as_slice()));
        assert!(opened.archive.entries[2].bytes.is_none());
        let mut synced = Vec::new();
        opened.sync_destination_parents(&profile, &target, || Ok(()), |p| { synced.push(p.to_owned()); Ok(()) }).unwrap();
        assert_eq!(synced, vec![target.clone(), target.clone(), target.join("config")]);
        assert!(opened.sync_destination_parents(&profile, &target, || Ok(()), |_| Err(bad())).is_err());
        assert!(open_beforeimages(&directory, &operation, &attempt, &[8u8; 32], &staged.manifest_sha256, &staged.beforeimage_archive_sha256).is_err());
        assert!(open_beforeimages(&directory, &operation, &Uuid::new_v4().to_string(), &key, &staged.manifest_sha256, &staged.beforeimage_archive_sha256).is_err());
        assert!(open_beforeimages(&directory, &operation, &attempt, &key, &"0".repeat(64), &staged.beforeimage_archive_sha256).is_err());
        let raw = fs::read(directory.join("beforeimages.bin")).unwrap();
        let mut aad = BEFOREIMAGE_DOMAIN.to_vec(); aad.extend_from_slice(staged.manifest_sha256.as_bytes());
        let plaintext = Aes256Gcm::new_from_slice(&key).unwrap().decrypt(Nonce::from_slice(&raw[BEFOREIMAGE_DOMAIN.len()..BEFOREIMAGE_DOMAIN.len()+12]), CipherPayload { msg: &raw[BEFOREIMAGE_DOMAIN.len()+12..], aad: &aad }).unwrap();
        let value: Value = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(value["operationId"], operation); assert_eq!(value["attemptId"], attempt);
        assert_eq!(value["entries"][0]["bytes"], serde_json::json!(b"old-env".to_vec()));
        assert!(value["entries"][2]["bytes"].is_null());
        aad.push(0);
        assert!(Aes256Gcm::new_from_slice(&key).unwrap().decrypt(Nonce::from_slice(&raw[BEFOREIMAGE_DOMAIN.len()..BEFOREIMAGE_DOMAIN.len()+12]), CipherPayload { msg: &raw[BEFOREIMAGE_DOMAIN.len()+12..], aad: &aad }).is_err());
        opened.publish_afterimages(&plan, &profile, &target, "native-personal.json", || Ok(())).unwrap();
        assert_eq!(fs::read(target.join("runtime.env")).unwrap(), b"new-env");
        assert_eq!(fs::read(target.join("native-personal.json")).unwrap(), b"new-owner");
        assert_eq!(fs::read(target.join("config/freeze-signing-key.json")).unwrap(), b"source-key");
        let fresh = open_beforeimages(&directory, &operation, &attempt, &key, &staged.manifest_sha256, &staged.beforeimage_archive_sha256).unwrap();
        let snapshot = fresh.capture_published(&profile, &target, "native-personal.json").unwrap();
        snapshot.recheck().unwrap();
        fs::write(target.join("runtime.env"), b"tampered").unwrap();
        assert!(snapshot.recheck().is_err());
        fs::write(target.join("runtime.env"), b"new-env").unwrap();
        assert!(snapshot.recheck().is_err(), "same bytes cannot revive a changed held identity");
        drop(snapshot);
        opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).unwrap();
        write_private_new(&directory, "after-0.tmp", b"unrecognized", 65536).unwrap();
        assert!(opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).is_err());
        fs::remove_file(directory.join("after-0.tmp")).unwrap();
        write_private_new(&directory, "after-0.tmp", b"new-env", 65536).unwrap();
        opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).unwrap();
        assert!(!directory.join("after-0.tmp").exists());
        fs::write(target.join("runtime.env"), b"new-env").unwrap();
        fs::write(target.join("native-personal.json"), b"unexpected").unwrap();
        write_private_new(&target.join("config"), "freeze-signing-key.json", b"source-key", 65536).unwrap();
        write_private_new(&profile, &journal(&target, "upload.json").unwrap(), b"unexpected journal", 65536).unwrap();
        assert!(opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).is_err());
        fs::remove_file(profile.join(journal(&target, "upload.json").unwrap())).unwrap();
        write_private_new(&target.join("config"), "unexpected", b"unexpected", 65536).unwrap();
        assert!(opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).is_err());
        fs::remove_file(target.join("config/unexpected")).unwrap();
        assert!(opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).is_err());
        assert_eq!(fs::read(target.join("runtime.env")).unwrap(), b"new-env");
        fs::write(target.join("native-personal.json"), b"new-owner").unwrap();
        write_private_new(&target, &format!(".native-publication-repair-{attempt}-0"), b"old-env", 65536).unwrap();
        opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).unwrap();
        assert_eq!(fs::read(target.join("runtime.env")).unwrap(), b"old-env");
        assert_eq!(fs::read(target.join("native-personal.json")).unwrap(), b"old-owner");
        assert!(!target.join("config/freeze-signing-key.json").exists());
        opened.restore_beforeimages(&profile, &target, "native-personal.json", || Ok(())).unwrap();
        fs::write(directory.join("configuration.json"), b"changed").unwrap();
        assert!(staged.recheck().is_err());
        assert!(opened.recheck().is_err());
        assert!(plan.stage_beforeimages(&profile, &target, &parent, &operation, &attempt, &key).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
