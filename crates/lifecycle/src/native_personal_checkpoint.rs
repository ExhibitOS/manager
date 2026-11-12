// SPDX-License-Identifier: Apache-2.0
//! Authenticated inventory of an already staged, non-runnable cold copy.
//! Staged data remains plaintext in a private directory; only the inventory is sealed.
//! Pending binding changes block normal startup until authenticated recovery.
use super::*;
use crate::installations::{InstallationController, native_maintenance::{RetainedProductImage, StartupCancellation045}};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload as CipherPayload, rand_core::RngCore},
};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::sync::Arc;

#[cfg(all(feature = "workspace-v5-owner-validation", target_os = "macos"))]
#[path = "native_personal_checkpoint_guard.rs"]
mod guard_diagnostic;

const OLD_MANIFEST: &str = "253de8963ec9f0e0a2884a27d8e756b453cf61899e38c31571d9993a7d6ecff4";
const NEW_MANIFEST: &str = "10a21f3b7a73a66011e077f7927f951f917e604720591acbcc2cdb47222f4032";
#[cfg(feature = "workspace-v5-owner-validation")]
use crate::native_payload::{
    CATALOG40_FRAME_TRANSITION, CATALOG40_TANGENT_TRANSITION, FurnishingTransition,
    saved_workspace_transition_supported, saved_workspace_transition_target,
    WORKSPACE_V5_TRANSITION, WORKSPACE_WALL_PREVIEW_TRANSITION,
};

// Development-only transitions are admitted as directed, exact pairs. Keep
// the historical recovery path available alongside the reviewed Oak successor.
const TRANSITION_PAIRS: &[(&str, &str)] = &[
    (OLD_MANIFEST, NEW_MANIFEST),
    #[cfg(feature = "workspace-v5-owner-validation")]
    WORKSPACE_V5_TRANSITION,
    #[cfg(feature = "workspace-v5-owner-validation")]
    WORKSPACE_WALL_PREVIEW_TRANSITION,
    #[cfg(feature = "workspace-v5-owner-validation")]
    CATALOG40_FRAME_TRANSITION,
    #[cfg(feature = "workspace-v5-owner-validation")]
    CATALOG40_TANGENT_TRANSITION,
    ("35b07cfc4c89abeaab518fd72c5be5f55569c03ae99906ff7220edf7d9712bf8",
     "9b9d6b1cba4b5722a8710471283e9f693f14883f5da950c4504ca7bbf00efeb7"),
    ("9b9d6b1cba4b5722a8710471283e9f693f14883f5da950c4504ca7bbf00efeb7",
     "7ff6e64700cbe7324b634d2cb11e0acc87288ce27ab90ae18e675ae5fa899eb2"),
];
fn admitted_pair(source: &str, target: &str) -> bool {
    #[cfg(feature = "workspace-v5-owner-validation")]
    if saved_workspace_transition_supported((source, target)) { return true; }
    TRANSITION_PAIRS.contains(&(source, target))
}

fn check_cold_transition_source(root: &Path, pair: (&str, &str)) -> Result<()> {
    crate::installations::gpu_model_job_root_empty(root)?;
    #[cfg(feature = "workspace-v5-owner-validation")]
    if let Some(transition @ FurnishingTransition::Recover45 { .. }) = FurnishingTransition::from_manifests(pair) {
        return crate::native_runtime::catalog45_recovery_source_generation(root, transition.startup_recovery()).map(|_| ());
    }
    #[cfg(not(feature = "workspace-v5-owner-validation"))]
    let _ = pair;
    crate::native_runtime::check_cold_checkpoint_root(root)
}

const DOMAIN: &[u8] = b"ExhibitOS-local-personal-physical-checkpoint-v1\0";
const ENVELOPE: &str = "physical-checkpoint.gcm";
const MAX_FILES: usize = 50_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[cfg(feature = "workspace-v5-owner-validation")]
const CATALOG45_STARTUP_RECOVERY_CHECKPOINT_MAX_BYTES: u64 = MAX_BYTES + 256 * 1024 * 1024;
#[cfg(feature = "workspace-v5-owner-validation")]
const CATALOG45_NATIVE_PUBLICATION_CHECKPOINT_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(feature = "workspace-v5-owner-validation")]
// Current saved demo data exceeds 5 GiB; inventory and copies remain streamed.
// Only already admitted catalog45 handoffs use this bound and space checks remain.
const CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const MAX_MANIFEST: usize = 16 * 1024 * 1024;
const DATA_DIRECTORIES: &[&str] = &["native-database", "blobs", "config"];
const MEMBERS: &[&str] = &["native-database", "blobs", "config", "runtime.env", RECORD];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    bytes: u64,
    mode: u32,
    sha256: Option<String>,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestorePlan {
    retained: String,
    retained_identity: (u64, u64),
    directories: BTreeMap<String, DirectorySwap>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirectorySwap {
    before: (u64, u64),
    trial: (u64, u64),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    source: PathBuf,
    staged: PathBuf,
    source_identity: (u64, u64),
    staged_identity: (u64, u64),
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
    owner_uid: u32,
    old_manifest: String,
    new_manifest: String,
    files: BTreeMap<PathBuf, Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    saved_document_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retained_product_image: Option<RetainedProductImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    startup_cancellation_045: Option<StartupCancellation045>,
    #[cfg(feature = "workspace-v5-owner-validation")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "catalog40")]
    furnishing: Option<crate::native_runtime::FurnishingTransitionTrial>,
    trial: bool,
    drained_failure: Option<String>,
    restore: Option<RestorePlan>,
}

fn saved_document_digest(document: &super::installations::native_document::DocumentState) -> Result<String> {
    Ok(digest(&serde_json::to_vec(document).map_err(|_| invalid())?))
}

#[cfg(feature = "workspace-v5-owner-validation")]
fn gpu_empty_document(
    pair: (&str, &str), document: &super::installations::native_document::DocumentState,
    owner: (&str, &str, &str),
) -> bool {
    (FurnishingTransition::from_manifests(pair).is_some_and(|step| matches!(step, FurnishingTransition::Upgrade49 { .. }))
        || crate::native_payload::gpu_model_wrapping_transition(pair))
        && document.format == 1 && document.revision == 0
        && document.draft.is_none() && document.etag.is_none()
        && document.saved_edit_version.is_none() && document.pending.is_none()
        && [owner.0, owner.1, owner.2].into_iter().all(installations::uuid)
        && (document.workspace_id.as_str(), document.tenant_id.as_str(), document.principal_id.as_str()) == owner
}

#[cfg(feature = "workspace-v5-owner-validation")]
fn check_saved_transition_document(
    pair: (&str, &str), document: &super::installations::native_document::DocumentState,
    owner: (&str, &str, &str),
) -> Result<()> {
    // Library-only 049 still seals the exact scoped Native document digest.
    // It needs no fabricated exhibition head; every other pair keeps its guards.
    if gpu_empty_document(pair, document, owner) { return Ok(()); }
    if FurnishingTransition::from_manifests(pair).is_some_and(|step| matches!(step, FurnishingTransition::Upgrade49 { .. } | FurnishingTransition::Upgrade50 { .. }))
        && (document.pending.is_some() || document.saved_edit_version.is_none()
            || document.draft.as_ref().and_then(|draft| draft["editVersion"].as_u64()) != document.saved_edit_version)
    { return Err(err("NATIVE_PHYSICAL_CHECKPOINT_SAVED_DOCUMENT_REQUIRED")); }
    if document.draft.is_none() || document.etag.is_none() {
        return Err(err("NATIVE_PHYSICAL_CHECKPOINT_SAVED_DOCUMENT_REQUIRED"));
    }
    Ok(())
}

#[cfg(feature = "workspace-v5-owner-validation")]
fn catalog45_activation_saved_document(
    pair: (&str, &str), retained_recovery: bool, historical_startup: bool,
    expected: Option<&str>, document: &super::installations::native_document::DocumentState,
) -> Result<bool> {
    let preservation = crate::native_payload::catalog45_startup_recovery(pair)
        .is_some_and(crate::native_payload::Catalog45StartupRecovery::preserves_pre_ready);
    if pair.0 != pair.1 && !retained_recovery && !historical_startup && !preservation
        && !crate::native_payload::catalog45_clean_exit_handoff(pair)
        && !crate::native_payload::catalog45_restored_backed_handoff(pair)
        && !crate::native_payload::catalog45_adoption_handoff(pair)
        && !FurnishingTransition::from_manifests(pair).is_some_and(|step| matches!(step, FurnishingTransition::Upgrade49 { .. } | FurnishingTransition::Upgrade50 { .. }))
    {
        return Ok(false);
    }
    if expected != Some(saved_document_digest(document)?.as_str()) {
        return Err(err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED"));
    }
    Ok(true)
}

fn check_transition_host(
    pair: (&str, &str),
    saved_document_sha256: Option<&str>,
    saved_workspace_host: bool,
) -> Result<()> {
    #[cfg(feature = "workspace-v5-owner-validation")]
    let is_saved_workspace_pair = saved_workspace_transition_supported(pair);
    #[cfg(not(feature = "workspace-v5-owner-validation"))]
    let is_saved_workspace_pair = false;
    if !admitted_pair(pair.0, pair.1)
        || is_saved_workspace_pair != saved_workspace_host
        || is_saved_workspace_pair != saved_document_sha256.is_some()
        || saved_document_sha256.is_some_and(|sha| !hash_valid(sha))
    {
        return Err(invalid());
    }
    Ok(())
}

impl Checkpoint {
    fn inventory_limit(&self) -> u64 {
        checkpoint_inventory_limit((&self.old_manifest, &self.new_manifest), self.startup_cancellation_045.as_ref())
    }

    fn check_host(&self, saved_workspace_host: bool) -> Result<()> {
        let pair = (self.old_manifest.as_str(), self.new_manifest.as_str());
        #[cfg(feature = "workspace-v5-owner-validation")]
        if FurnishingTransition::from_manifests(pair).is_some() != self.furnishing.is_some() {
            return Err(invalid());
        }
        if !retained_transition_proof_matches(pair, self.retained_product_image.as_ref())
            || !startup_cancellation_proof_matches(pair, self.retained_product_image.as_ref(), self.startup_cancellation_045.as_ref())
        {
            return Err(invalid());
        }
        check_transition_host(pair, self.saved_document_sha256.as_deref(), saved_workspace_host)
    }

    fn check_retained(&self, retained: Option<&RetainedProductImage>) -> Result<()> {
        if self.retained_product_image.as_ref() != retained {
            return Err(invalid());
        }
        Ok(())
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    fn verify_retained_product_image_files(&self, proof: &RetainedProductImage, receipt: &Value) -> Result<()> {
        let root = Path::new("blobs").join(&self.tenant_id);
        let quarantine = root.join("quarantine").join(format!("import-{}", proof.job_id));
        let full_hash = receipt["full"]["sha256"].as_str().filter(|value| hash_valid(value)).ok_or_else(invalid)?;
        let files = [
            (&receipt["original"], quarantine.join(&proof.source_sha256)),
            (&receipt["full"], root.join("approved").join(full_hash)),
            (&receipt["coarse"], quarantine.join("product-coarse")),
        ];
        for (descriptor, path) in files {
            if descriptor.is_null() {
                if path.ends_with("product-coarse") { continue; }
                return Err(invalid());
            }
            let entry = self.files.get(&path).ok_or_else(invalid)?;
            if entry.sha256.as_deref() != descriptor["sha256"].as_str()
                || entry.sha256.is_none() || descriptor["bytes"].as_u64() != Some(entry.bytes)
            {
                return Err(err("NATIVE_RETAINED_PRODUCT_IMAGE_CHANGED"));
            }
        }
        Ok(())
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    fn furnishing_transition(&self) -> Option<FurnishingTransition> {
        FurnishingTransition::from_manifests((&self.old_manifest, &self.new_manifest))
    }
}

fn retained_transition_proof_matches(pair: (&str, &str), proof: Option<&RetainedProductImage>) -> bool {
    #[cfg(feature = "workspace-v5-owner-validation")]
    return match proof {
        Some(proof) if proof.recovery.is_some() => crate::native_payload::retained_product_image_recovery_transition(pair),
        Some(_) => crate::native_payload::retained_product_image_transition(pair),
        None => !crate::native_payload::retained_product_image_recovery_transition(pair),
    };
    #[cfg(not(feature = "workspace-v5-owner-validation"))]
    { let _ = pair; proof.is_none() }
}

fn startup_cancellation_proof_matches(
    pair: (&str, &str), retained: Option<&RetainedProductImage>, proof: Option<&StartupCancellation045>,
) -> bool {
    #[cfg(feature = "workspace-v5-owner-validation")]
    if let Some(startup) = crate::native_payload::catalog45_startup_recovery(pair) {
        return retained.is_none() && proof.is_some() == startup.requires_hayes_proof();
    }
    #[cfg(not(feature = "workspace-v5-owner-validation"))]
    let _ = (pair, retained);
    proof.is_none()
}

fn checkpoint_inventory_limit(pair: (&str, &str), proof: Option<&StartupCancellation045>) -> u64 {
    #[cfg(feature = "workspace-v5-owner-validation")]
    if crate::native_payload::catalog45_adoption_handoff(pair)
        || FurnishingTransition::from_manifests(pair).is_some_and(|transition| matches!(transition, FurnishingTransition::Upgrade49 { .. } | FurnishingTransition::Upgrade50 { .. })) {
        return CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES;
    }
    #[cfg(feature = "workspace-v5-owner-validation")]
    if crate::native_payload::catalog45_native_publication_transition(pair).is_some()
        || crate::native_payload::catalog45_clean_exit_handoff(pair)
        || crate::native_payload::catalog45_restored_backed_handoff(pair)
    {
        return CATALOG45_NATIVE_PUBLICATION_CHECKPOINT_MAX_BYTES;
    }
    #[cfg(feature = "workspace-v5-owner-validation")]
    if proof.is_some() && crate::native_payload::catalog45_startup_recovery(pair)
        == Some(crate::native_payload::Catalog45StartupRecovery::Unqualified)
    {
        return CATALOG45_STARTUP_RECOVERY_CHECKPOINT_MAX_BYTES;
    }
    #[cfg(feature = "workspace-v5-owner-validation")]
    if crate::native_payload::catalog45_startup_recovery(pair)
        .is_some_and(crate::native_payload::Catalog45StartupRecovery::preserves_pre_ready)
    {
        return CATALOG45_NATIVE_PUBLICATION_CHECKPOINT_MAX_BYTES;
    }
    #[cfg(not(feature = "workspace-v5-owner-validation"))]
    let _ = (pair, proof);
    MAX_BYTES
}

#[cfg(feature = "workspace-v5-owner-validation")]
fn check_retained_recovery_generation(proof: &RetainedProductImage, generation: &str) -> Result<()> {
    let recovery = proof.recovery.as_ref().ok_or_else(invalid)?;
    if recovery.failed_generation != generation
        || !installations::uuid(&recovery.hayes_job_id)
        || !hash_valid(&recovery.hayes_completion_frame_sha256)
    {
        return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
    }
    Ok(())
}

fn invalid() -> LifecycleError {
    err("NATIVE_PHYSICAL_CHECKPOINT_INVALID")
}

fn staged_name(name: &str) -> &str {
    match name {
        RECORD => "native-personal.before",
        ENVIRONMENT => "runtime-env.before",
        _ => name,
    }
}

fn check_staged_names(staging: &Path, sealed: bool) -> Result<()> {
    let actual: std::collections::BTreeSet<_> = fs::read_dir(staging)
        .map_err(|_| invalid())?
        .map(|item| item.map(|entry| entry.file_name()).map_err(|_| invalid()))
        .collect::<Result<_>>()?;
    let mut expected: std::collections::BTreeSet<_> = MEMBERS
        .iter()
        .map(|name| std::ffi::OsString::from(staged_name(name)))
        .collect();
    if sealed {
        expected.insert(ENVELOPE.into());
    }
    if actual != expected {
        return Err(invalid());
    }
    Ok(())
}

fn directory_identity(path: &Path) -> Result<(u64, u64)> {
    installations::private_directory(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid())?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(test)]
fn inventory(root: &Path, staged: bool) -> Result<BTreeMap<PathBuf, Entry>> {
    inventory_with_limit(root, staged, MAX_BYTES)
}

fn inventory_with_limit(
    root: &Path,
    staged: bool,
    max_bytes: u64,
) -> Result<BTreeMap<PathBuf, Entry>> {
    inventory_with_sync(root, staged, max_bytes, File::sync_all)
}

fn inventory_with_sync(
    root: &Path,
    staged: bool,
    max_bytes: u64,
    sync: impl Fn(&File) -> std::io::Result<()>,
) -> Result<BTreeMap<PathBuf, Entry>> {
    inventory_members_with_sync(root, staged, max_bytes, MEMBERS, sync)
}

fn inventory_members_with_sync(
    root: &Path,
    staged: bool,
    max_bytes: u64,
    members: &[&str],
    sync: impl Fn(&File) -> std::io::Result<()>,
) -> Result<BTreeMap<PathBuf, Entry>> {
    let root_pin = crate::maintenance_source::DirectoryPin::open(root)?;
    let mut files = BTreeMap::new();
    let mut todo: Vec<_> = members.iter().map(|name| PathBuf::from(name)).collect();
    let mut total = 0u64;
    let mut visited = 0;
    let mut directories = Vec::new();
    while let Some(relative) = todo.pop() {
        visited += 1;
        if visited > MAX_FILES
            || relative.as_os_str().len() > 1024
            || relative.components().count() > 32
        {
            return Err(invalid());
        }
        let path = if staged {
            match relative.to_str() {
                Some(name) if MEMBERS.contains(&name) => root.join(staged_name(name)),
                _ => root.join(&relative),
            }
        } else {
            root.join(&relative)
        };
        let named = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if named.uid() != unsafe { libc::geteuid() } || named.mode() & 0o077 != 0 {
            return Err(invalid());
        }
        if named.is_dir() {
            let pin = crate::maintenance_source::DirectoryPin::open(&path)?;
            for child in fs::read_dir(&path).map_err(|_| invalid())? {
                todo.push(relative.join(child.map_err(|_| invalid())?.file_name()));
                if todo.len() > MAX_FILES {
                    return Err(invalid());
                }
            }
            pin.check()?;
            files.insert(
                relative,
                Entry {
                    bytes: 0,
                    mode: named.mode() & 0o777,
                    sha256: None,
                },
            );
            directories.push(pin);
            continue;
        }
        if !named.is_file() || named.nlink() != 1 {
            return Err(invalid());
        }
        total = total
            .checked_add(named.len())
            .filter(|v| *v <= max_bytes)
            .ok_or_else(invalid)?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|_| invalid())?;
        let held = file.metadata().map_err(|_| invalid())?;
        let identity = |m: &fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
                m.mode(),
            )
        };
        if !held.is_file() || held.nlink() != 1 || identity(&held) != identity(&named) {
            return Err(invalid());
        }
        let mut digest = Sha256::new();
        let mut chunk = [0u8; 64 * 1024];
        let mut bytes = 0;
        loop {
            let n = file.read(&mut chunk).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > held.len() {
                return Err(invalid());
            }
            digest.update(&chunk[..n]);
        }
        chunk.fill(0);
        if staged {
            sync(&file).map_err(|_| invalid())?;
        }
        if bytes != held.len()
            || identity(&held) != identity(&file.metadata().map_err(|_| invalid())?)
            || identity(&held) != identity(&fs::symlink_metadata(&path).map_err(|_| invalid())?)
        {
            return Err(invalid());
        }
        files.insert(
            relative,
            Entry {
                bytes,
                mode: held.mode() & 0o777,
                sha256: Some(format!("{:x}", digest.finalize())),
            },
        );
    }
    // The walk visits each parent before its children. Flush the completed
    // files first, then the directory entries from the leaves to the root.
    for pin in directories.iter().rev() {
        if staged {
            sync(&pin.try_clone_file()?).map_err(|_| invalid())?;
        }
        pin.check()?;
    }
    if staged {
        sync(&root_pin.try_clone_file()?).map_err(|_| invalid())?;
    }
    root_pin.check()?;
    Ok(files)
}

fn seal(checkpoint: &Checkpoint, key: &[u8; 32]) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(checkpoint).map_err(|_| invalid())?;
    if bytes.len() > MAX_MANIFEST {
        return Err(invalid());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| invalid())?;
    let mut nonce = [0; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            CipherPayload {
                msg: &bytes,
                aad: DOMAIN,
            },
        )
        .map_err(|_| invalid())?;
    let mut output = Vec::with_capacity(nonce.len() + ciphertext.len());
    output.extend(nonce);
    output.extend(ciphertext);
    Ok(output)
}

fn open(envelope: &[u8], key: &[u8; 32]) -> Result<Checkpoint> {
    if !(28..=MAX_MANIFEST + 28).contains(&envelope.len()) {
        return Err(invalid());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| invalid())?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&envelope[..12]),
            CipherPayload {
                msg: &envelope[12..],
                aad: DOMAIN,
            },
        )
        .map_err(|_| invalid())?;
    serde_json::from_slice(&plaintext).map_err(|_| invalid())
}

const ENVIRONMENT: &str = "runtime.env";
const PENDING_PREFIX: &[u8] = b"ExhibitOS-personal-transition-pending-v1\0";
const ENVELOPE_LIMIT: usize = MAX_MANIFEST + 28;
const PENDING_LIMIT: usize = PENDING_PREFIX.len() + ENVELOPE_LIMIT;

struct Bindings {
    old_env: Bytes,
    old_record: Vec<u8>,
    new_env: Bytes,
    new_record: Vec<u8>,
}

fn bindings(checkpoint: &Checkpoint) -> Result<Bindings> {
    if !admitted_pair(&checkpoint.old_manifest, &checkpoint.new_manifest) {
        return Err(invalid());
    }
    let stage = &checkpoint.staged;
    let old_env = Bytes(installation_backup::source_bytes(
        stage,
        staged_name(ENVIRONMENT),
        4096,
        true,
    )?);
    let old_record = installation_backup::source_bytes(stage, staged_name(RECORD), 4096, true)?;
    let mut record: Personal = serde_json::from_slice(&old_record).map_err(|_| invalid())?;
    if record.manifest_sha256 != checkpoint.old_manifest
        || record.workspace_id != checkpoint.workspace_id
        || record.tenant_id != checkpoint.tenant_id
        || record.principal_id != checkpoint.principal_id
        || record.owner_uid != checkpoint.owner_uid
    {
        return Err(invalid());
    }
    let prefix = b"NATIVE_RUNTIME_MANIFEST_SHA256=";
    let mut new_env = Bytes(Vec::with_capacity(old_env.0.len()));
    let mut replaced = false;
    for line in old_env.0.split_inclusive(|byte| *byte == b'\n') {
        if let Some(value) = line.strip_prefix(prefix) {
            if replaced || value.strip_suffix(b"\n").unwrap_or(value) != checkpoint.old_manifest.as_bytes() {
                return Err(invalid());
            }
            replaced = true;
            new_env.0.extend_from_slice(prefix);
            new_env.0.extend_from_slice(checkpoint.new_manifest.as_bytes());
            if line.ends_with(b"\n") {
                new_env.0.push(b'\n');
            }
        } else {
            new_env.0.extend_from_slice(line);
        }
    }
    if !replaced {
        return Err(invalid());
    }
    record.manifest_sha256 = checkpoint.new_manifest.clone();
    record.environment_sha256 = digest(&new_env.0);
    let new_record = serde_json::to_vec(&record).map_err(|_| invalid())?;
    Ok(Bindings {
        old_env,
        old_record,
        new_env,
        new_record,
    })
}

fn sync_root(root: &Path) -> Result<()> {
    File::open(root)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid())
}

fn replace_private(root: &Path, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
    let temporary = format!(".native-personal-transition-{}.tmp", Uuid::new_v4());
    write_private_new(root, &temporary, bytes, limit)?;
    fs::rename(root.join(temporary), root.join(name)).map_err(|_| invalid())?;
    sync_root(root)
}

fn publish_pending(root: &Path, envelope: &[u8]) -> Result<()> {
    let mut journal = Vec::with_capacity(PENDING_PREFIX.len() + envelope.len());
    journal.extend_from_slice(PENDING_PREFIX);
    journal.extend_from_slice(envelope);
    // The journal is also the startup fence. No version can use this file
    // as an authenticated personal environment after the atomic rename.
    replace_private(root, ENVIRONMENT, &journal, PENDING_LIMIT)
}

fn verify_beforeimage(checkpoint: &Checkpoint, name: &str, bytes: &[u8]) -> Result<()> {
    let entry = checkpoint.files.get(Path::new(name)).ok_or_else(invalid)?;
    if entry.mode != 0o600
        || entry.bytes != bytes.len() as u64
        || entry.sha256.as_deref() != Some(&digest(bytes))
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) struct TrialBinding {
    fields: BTreeMap<String, String>,
    pending_sha256: String,
}

impl TrialBinding {
    pub(crate) fn into_parts(self) -> (BTreeMap<String, String>, String) {
        (self.fields, self.pending_sha256)
    }
}

fn read_pending(root: &Path) -> Result<Vec<u8>> {
    installation_backup::source_bytes(root, ENVIRONMENT, PENDING_LIMIT as u64, true)
}

pub(crate) fn check_trial_fence(root: &Path, expected: &str) -> Result<()> {
    let pending = read_pending(root)?;
    if !pending.starts_with(PENDING_PREFIX) || digest(&pending) != expected {
        return Err(invalid());
    }
    Ok(())
}

fn verify_unchanged(root: &Path, checkpoint: &Checkpoint, pending: &[u8]) -> Result<()> {
    let mut live = inventory_with_limit(root, false, checkpoint.inventory_limit() + pending.len() as u64)?;
    let entry = live.get_mut(Path::new(ENVIRONMENT)).ok_or_else(invalid)?;
    if entry.mode != 0o600
        || entry.bytes != pending.len() as u64
        || entry.sha256.as_deref() != Some(&digest(pending))
    {
        return Err(invalid());
    }
    *entry = checkpoint
        .files
        .get(Path::new(ENVIRONMENT))
        .ok_or_else(invalid)?
        .clone();
    if live != checkpoint.files {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(feature = "workspace-v5-owner-validation")]
fn verify_selected_staging_delta(
    before: &BTreeMap<PathBuf, Entry>, live: &BTreeMap<PathBuf, Entry>,
    prefix: &Path, bytes: usize,
) -> Result<()> {
    const CHUNK: usize = 16 * 1024 * 1024;
    let mismatch = || err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
    let keys: BTreeMap<_, _> = (0..bytes.div_ceil(CHUNK)).map(|i| {
        (prefix.join(format!("chunk-{i}")), (bytes - i * CHUNK).min(CHUNK) as u64)
    }).collect();
    for (path, bytes) in &keys {
        if live.contains_key(path) || before.get(path).is_some_and(|entry|
            entry.sha256.is_none() || entry.bytes != *bytes)
        {
            return Err(mismatch());
        }
    }
    let protected = |path: &Path| path.starts_with("blobs") || path.starts_with("config");
    if live.iter().filter(|(path, _)| protected(path))
        .ne(before.iter().filter(|(path, _)| protected(path) && !keys.contains_key(*path)))
    {
        return Err(mismatch());
    }
    Ok(())
}

fn keep_receipt(root: &Path, kind: &str, envelope: &[u8]) -> Result<()> {
    let name = format!("native-personal-transition-{kind}-{}.gcm", digest(envelope));
    if !present(root, &name)? {
        replace_private(root, &name, envelope, ENVELOPE_LIMIT)?;
    }
    if installation_backup::source_bytes(root, &name, ENVELOPE_LIMIT as u64, true)? != envelope {
        return Err(invalid());
    }
    Ok(())
}

fn recovery_copy(root: &Path, checkpoint: &Checkpoint) -> Result<PathBuf> {
    let copy = root.join(format!(
        "native-personal-transition-retained-{}",
        Uuid::new_v4()
    ));
    installations::new_directory(&copy)?;
    copy_beforeimages(&checkpoint.staged, true, &copy, &checkpoint.files, checkpoint.inventory_limit())?;
    sync_root(root)?;
    Ok(copy)
}

fn copy_beforeimages(
    source_root: &Path,
    source_staged: bool,
    destination: &Path,
    files: &BTreeMap<PathBuf, Entry>,
    max_bytes: u64,
) -> Result<()> {
    copy_entries(source_root, source_staged, destination, files)?;
    // Verify and flush the final-mode files before a seal or recovery plan is published.
    if inventory_with_limit(destination, true, max_bytes)? != *files {
        return Err(invalid());
    }
    Ok(())
}

fn copy_entries(
    source_root: &Path,
    source_staged: bool,
    destination: &Path,
    files: &BTreeMap<PathBuf, Entry>,
) -> Result<()> {
    for (relative, entry) in files {
        let name = relative.to_str().ok_or_else(invalid)?;
        let archived = Path::new(staged_name(name));
        let destination = destination.join(archived);
        if entry.sha256.is_none() {
            installations::new_directory(&destination)?;
            fs::set_permissions(&destination, fs::Permissions::from_mode(entry.mode))
                .map_err(|_| invalid())?;
            continue;
        }
        let source = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(source_root.join(if source_staged { archived } else { relative }))
            .map_err(|_| invalid())?;
        let meta = source.metadata().map_err(|_| invalid())?;
        if !meta.is_file() || meta.nlink() != 1 || meta.len() != entry.bytes
            || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o777 != entry.mode {
            return Err(invalid());
        }
        let mut output = private_options().open(destination).map_err(|_| invalid())?;
        let count =
            std::io::copy(&mut source.take(entry.bytes + 1), &mut output).map_err(|_| invalid())?;
        if count != entry.bytes {
            return Err(invalid());
        }
        output
            .set_permissions(fs::Permissions::from_mode(entry.mode))
            .map_err(|_| invalid())?;
    }
    Ok(())
}

fn prepare_physical_recovery(
    root: &Path,
    checkpoint: &mut Checkpoint,
    key: &[u8; 32],
) -> Result<Vec<u8>> {
    if checkpoint.restore.is_some() {
        return Err(invalid());
    }
    let retained = recovery_copy(root, checkpoint)?;
    let mut directories = BTreeMap::new();
    for name in DATA_DIRECTORIES {
        directories.insert(
            (*name).into(),
            DirectorySwap {
                before: directory_identity(&retained.join(name))?,
                trial: directory_identity(&root.join(name))?,
            },
        );
    }
    checkpoint.restore = Some(RestorePlan {
        retained: retained
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?
            .into(),
        retained_identity: directory_identity(&retained)?,
        directories,
    });
    let envelope = seal(checkpoint, key)?;
    publish_pending(root, &envelope)?;
    Ok(envelope)
}

fn exchange_data_directories(root: &Path, plan: &RestorePlan, members: &[&str]) -> Result<()> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
    if !plan
        .retained
        .strip_prefix("native-personal-transition-retained-")
        .is_some_and(installations::uuid)
        || plan.directories.len() != DATA_DIRECTORIES.len()
    {
        return Err(invalid());
    }
    let retained = root.join(&plan.retained);
    let root_pin = crate::maintenance_source::DirectoryPin::open(root)?;
    let retained_pin = crate::maintenance_source::DirectoryPin::open(&retained)?;
    let live = root_pin.try_clone_file()?;
    let copy = retained_pin.try_clone_file()?;
    for name in members {
        root_pin.check()?;
        retained_pin.check()?;
        if directory_identity(&retained)? != plan.retained_identity {
            return Err(invalid());
        }
        let swap = plan.directories.get(*name).ok_or_else(invalid)?;
        let at_live = directory_identity(&root.join(name))?;
        let at_retained = directory_identity(&retained.join(name))?;
        if at_live == swap.before && at_retained == swap.trial {
            continue;
        }
        if at_live != swap.trial || at_retained != swap.before {
            return Err(invalid());
        }
        let name = std::ffi::CString::new(Path::new(name).as_os_str().as_bytes())
            .map_err(|_| invalid())?;
        if unsafe {
            libc::renameatx_np(
                live.as_raw_fd(),
                name.as_ptr(),
                copy.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_SWAP,
            )
        } != 0
        {
            return Err(invalid());
        }
        live.sync_all()
            .and_then(|_| copy.sync_all())
            .map_err(|_| invalid())?;
    }
    live.sync_all()
        .and_then(|_| copy.sync_all())
        .map_err(|_| invalid())?;
    root_pin.check()?;
    retained_pin.check()?;
    Ok(())
}

impl LifecycleService {
    #[cfg(feature = "workspace-v5-owner-validation")]
    fn selected_045_original_store(
        &self, profile: &Path, checkpoint: &Checkpoint,
        lineage: &[crate::installations::native_maintenance::RecoveryRollback045],
        staging_prefix: &Path,
    ) -> Result<(Option<PathBuf>, BTreeMap<usize, u64>)> {
        if lineage.len() > 8 {
            return Err(invalid());
        }
        let mut original = None;
        let mut predecessor = None;
        let mut absent_chunks = BTreeMap::new();
        for item in lineage {
            if !hash_valid(&item.receipt_sha256) {
                return Err(invalid());
            }
            let name = format!("native-personal-transition-recovery-{}.gcm", item.receipt_sha256);
            let envelope = installation_backup::source_bytes(&self.root, &name, ENVELOPE_LIMIT as u64, true)?;
            if digest(&envelope) != item.receipt_sha256 {
                return Err(invalid());
            }
            let key = Bytes(crate::profile_backup::external_key(profile, &item.key_file)?);
            let prior = self.transition_checkpoint(&envelope, key.0.as_slice().try_into().map_err(|_| invalid())?)?;
            prior.check_host(true)?;
            if !prior.trial || prior.source_identity != checkpoint.source_identity
                || prior.workspace_id != checkpoint.workspace_id || prior.tenant_id != checkpoint.tenant_id
                || prior.principal_id != checkpoint.principal_id || prior.owner_uid != checkpoint.owner_uid
            {
                return Err(invalid());
            }
            let restore = prior.restore.as_ref().ok_or_else(invalid)?;
            if !restore.retained.strip_prefix("native-personal-transition-retained-").is_some_and(installations::uuid) {
                return Err(invalid());
            }
            let retained = self.root.join(&restore.retained);
            let store = retained.join("blobs");
            let swap = restore.directories.get("blobs").ok_or_else(invalid)?;
            if directory_identity(&retained)? != restore.retained_identity
                || directory_identity(&store)? != swap.trial
                || predecessor.is_some_and(|expected| expected != swap.trial)
            {
                return Err(invalid());
            }
            if original.is_none() {
                let stored = inventory_members_with_sync(&retained, false, MAX_BYTES, &["blobs"], File::sync_all)?;
                if stored.iter().ne(prior.files.iter().filter(|(path, _)| path.starts_with("blobs"))) {
                    return Err(invalid());
                }
                for (path, entry) in stored {
                    match checkpoint.files.get(&path) {
                        Some(current) if current == &entry => (),
                        None if entry.sha256.is_some() => {
                            let suffix = path.strip_prefix(staging_prefix).ok()
                                .and_then(Path::to_str).ok_or_else(invalid)?;
                            let index = suffix.strip_prefix("chunk-").and_then(|index| index.parse::<usize>().ok())
                                .filter(|index| *index < 32 && suffix == format!("chunk-{index}")).ok_or_else(invalid)?;
                            absent_chunks.insert(index, entry.bytes);
                        }
                        _ => return Err(invalid()),
                    }
                }
                original = Some(store);
            }
            predecessor = Some(swap.before);
        }
        if let Some(expected) = predecessor {
            if directory_identity(&self.root.join("blobs"))? != expected {
                return Err(invalid());
            }
        }
        Ok((original, absent_chunks))
    }

    pub(crate) fn check_selected_checkpoint_source(
        &self,
        source: &Payload,
        guard: &OperationGuard,
    ) -> Result<()> {
        self.check_prepare_guard(guard)?;
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
        let context = source.context();
        if !TRANSITION_PAIRS.iter().any(|(source, _)| *source == context.manifest_sha256)
            || context.official_signed
            || context.redistribution_qualified
        {
            return Err(invalid());
        }
        source.recheck()?;
        let record = read(self)?;
        let mut fields = self.personal_environment(source, record.ports)?;
        for value in fields.values_mut() {
            std::mem::take(value).into_bytes().fill(0);
        }
        Ok(())
    }

    fn transition_checkpoint(&self, envelope: &[u8], key: &[u8; 32]) -> Result<Checkpoint> {
        let checkpoint = open(envelope, key)?;
        let staging = &checkpoint.staged;
        if checkpoint.source != self.root
            || !admitted_pair(&checkpoint.old_manifest, &checkpoint.new_manifest)
            || checkpoint.owner_uid != unsafe { libc::geteuid() }
            || !staging.is_absolute()
            || staging.starts_with(&self.root)
            || self.root.starts_with(staging)
        {
            return Err(invalid());
        }
        check_staged_names(staging, true)?;
        if directory_identity(&self.root)? != checkpoint.source_identity
            || directory_identity(staging)? != checkpoint.staged_identity
            || inventory_with_limit(staging, true, checkpoint.inventory_limit())? != checkpoint.files
        {
            return Err(invalid());
        }
        Ok(checkpoint)
    }

    /// Stage the exact new binding, while keeping all native startup blocked.
    fn stage_local_personal_runtime_transition(
        &self,
        source: &Payload,
        target: &Payload,
        staging: &Path,
        key: &[u8; 32],
        saved_workspace_host: bool,
    ) -> Result<()> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        if !admitted_pair(&source.context().manifest_sha256, &target.context().manifest_sha256) {
            return Err(invalid());
        }
        source.recheck()?;
        target.recheck()?;
        check_cold_transition_source(&self.root, (
            &source.context().manifest_sha256, &target.context().manifest_sha256,
        ))?;
        let record = read(self)?;
        let mut fields = self.personal_environment(source, record.ports)?;
        for value in fields.values_mut() {
            std::mem::take(value).into_bytes().fill(0);
        }
        let envelope =
            installation_backup::source_bytes(staging, ENVELOPE, (MAX_MANIFEST + 28) as u64, true)?;
        let checkpoint = self.transition_checkpoint(&envelope, key)?;
        if checkpoint.old_manifest != source.context().manifest_sha256
            || checkpoint.new_manifest != target.context().manifest_sha256
            || checkpoint.trial
            || checkpoint.staged != staging
            || inventory_with_limit(&self.root, false, checkpoint.inventory_limit())? != checkpoint.files
        {
            return Err(invalid());
        }
        checkpoint.check_host(saved_workspace_host)?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        let _writer = checkpoint.furnishing.as_ref().zip(checkpoint.furnishing_transition())
            .map(|(trial, transition)| trial.hold_source(self, transition)).transpose()?;
        let binding = bindings(&checkpoint)?;
        verify_beforeimage(&checkpoint, ENVIRONMENT, &binding.old_env.0)?;
        verify_beforeimage(&checkpoint, RECORD, &binding.old_record)?;
        let proposed: Personal =
            serde_json::from_slice(&binding.new_record).map_err(|_| invalid())?;
        let mut fields = personal_environment_fields(&proposed, &binding.new_env.0)?;
        for value in fields.values_mut() {
            std::mem::take(value).into_bytes().fill(0);
        }
        publish_pending(&self.root, &envelope)
    }

    /// Recover the original environment only when the entire cold checkpoint
    /// still matches and the live environment holds its authenticated journal.
    fn recover_local_personal_runtime_transition(
        &self,
        staging: &Path,
        key: &[u8; 32],
        saved_workspace_host: bool,
        retained: Option<&RetainedProductImage>,
    ) -> Result<()> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        let mut pending = read_pending(&self.root)?;
        let mut envelope = pending
            .strip_prefix(PENDING_PREFIX)
            .ok_or_else(invalid)?
            .to_vec();
        let mut checkpoint = self.transition_checkpoint(&envelope, key)?;
        checkpoint.check_host(saved_workspace_host)?;
        checkpoint.check_retained(retained)?;
        if checkpoint.staged != staging {
            return Err(invalid());
        }
        #[cfg(feature = "workspace-v5-owner-validation")]
        let furnishing = checkpoint.furnishing.as_ref().zip(checkpoint.furnishing_transition());
        #[cfg(feature = "workspace-v5-owner-validation")]
        let writer = furnishing.map(|(trial, transition)| {
            if checkpoint.trial { trial.hold_recovery(self, transition) } else { trial.hold_source(self, transition) }
        }).transpose()?;
        let ordinary_cold_check = || crate::native_runtime::check_cold_transition_root(
            &self.root, checkpoint.drained_failure.as_deref(),
        );
        #[cfg(feature = "workspace-v5-owner-validation")]
        if let (Some((trial, transition)), Some(writer)) = (furnishing, &writer) {
            trial.check_recovery_root(&self.root, writer, transition, retained)?;
        } else {
            ordinary_cold_check()?;
        }
        #[cfg(not(feature = "workspace-v5-owner-validation"))]
        ordinary_cold_check()?;
        let binding = bindings(&checkpoint)?;
        verify_beforeimage(&checkpoint, ENVIRONMENT, &binding.old_env.0)?;
        verify_beforeimage(&checkpoint, RECORD, &binding.old_record)?;
        if checkpoint.trial {
            let record = installation_backup::source_bytes(&self.root, RECORD, 4096, true)?;
            if record != binding.old_record && record != binding.new_record {
                return Err(invalid());
            }
            if checkpoint.restore.is_none() {
                envelope = prepare_physical_recovery(&self.root, &mut checkpoint, key)?;
                pending = read_pending(&self.root)?;
                if pending.strip_prefix(PENDING_PREFIX) != Some(envelope.as_slice()) {
                    return Err(invalid());
                }
            }
            self.check_prepare_guard(&guard)?;
            check_trial_fence(&self.root, &digest(&pending))?;
            exchange_data_directories(
                &self.root,
                checkpoint.restore.as_ref().ok_or_else(invalid)?,
                DATA_DIRECTORIES,
            )?;
            if record != binding.old_record {
                replace_private(&self.root, RECORD, &binding.old_record, 4096)?;
            }
        }
        verify_unchanged(&self.root, &checkpoint, &pending)?;
        keep_receipt(&self.root, "recovery", &envelope)?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        if let (Some((trial, transition)), Some(writer)) = (
            checkpoint.furnishing.as_ref().zip(checkpoint.furnishing_transition()), &writer,
        ) {
            trial.recover_source(self, writer, transition)?;
        }
        replace_private(&self.root, ENVIRONMENT, &binding.old_env.0, 4096)?;
        if inventory_with_limit(&self.root, false, checkpoint.inventory_limit())? != checkpoint.files {
            return Err(invalid());
        }
        Ok(())
    }

    fn trial_local_personal_runtime(
        &self,
        target: Arc<Payload>,
        staging: &Path,
        key: &[u8; 32],
        saved_workspace_host: bool,
        retained: Option<&RetainedProductImage>,
        document: Option<&super::installations::native_document::DocumentState>,
    ) -> Result<(
        crate::native_runtime::Session,
        Checkpoint,
        Bindings,
        Vec<u8>,
    )> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        let target_context = target.context();
        #[cfg(feature = "workspace-v5-owner-validation")]
        let saved_target = saved_workspace_transition_target(&target_context.manifest_sha256);
        #[cfg(not(feature = "workspace-v5-owner-validation"))]
        let saved_target = false;
        if (!saved_target && !TRANSITION_PAIRS.iter().any(|(_, target)| *target == target_context.manifest_sha256))
            || target_context.official_signed || target_context.redistribution_qualified
        {
            return Err(invalid());
        }
        target.recheck()?;
        let pending = read_pending(&self.root)?;
        let envelope = pending.strip_prefix(PENDING_PREFIX).ok_or_else(invalid)?;
        let mut checkpoint = self.transition_checkpoint(envelope, key)?;
        checkpoint.check_host(saved_workspace_host)?;
        checkpoint.check_retained(retained)?;
        if checkpoint.new_manifest != target_context.manifest_sha256
            || checkpoint.trial || checkpoint.staged != staging {
            return Err(invalid());
        }
        #[cfg(feature = "workspace-v5-owner-validation")]
        {
            let pair = (checkpoint.old_manifest.as_str(), checkpoint.new_manifest.as_str());
            if (crate::native_payload::catalog45_clean_exit_handoff(pair)
                || crate::native_payload::catalog45_restored_backed_handoff(pair)
                || crate::native_payload::catalog45_adoption_handoff(pair)
                || FurnishingTransition::from_manifests(pair).is_some_and(|step| matches!(step, FurnishingTransition::Upgrade49 { .. } | FurnishingTransition::Upgrade50 { .. })))
                && !catalog45_activation_saved_document(
                    pair, false, false, checkpoint.saved_document_sha256.as_deref(), document.ok_or_else(invalid)?,
                )?
            { return Err(invalid()); }
        }
        #[cfg(not(feature = "workspace-v5-owner-validation"))]
        let _ = document;
        check_cold_transition_source(&self.root, (&checkpoint.old_manifest, &checkpoint.new_manifest))?;
        verify_unchanged(&self.root, &checkpoint, &pending)?;
        let binding = bindings(&checkpoint)?;
        verify_beforeimage(&checkpoint, ENVIRONMENT, &binding.old_env.0)?;
        verify_beforeimage(&checkpoint, RECORD, &binding.old_record)?;
        let proposed: Personal =
            serde_json::from_slice(&binding.new_record).map_err(|_| invalid())?;
        let fields = personal_environment_fields(&proposed, &binding.new_env.0)?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        let transition = checkpoint.furnishing_transition();
        #[cfg(feature = "workspace-v5-owner-validation")]
        let _writer = checkpoint.furnishing.as_mut().zip(transition).map(|(trial, transition)| {
            trial.renew_generation();
            trial.hold_source(self, transition)
        }).transpose()?;
        checkpoint.trial = true;
        let envelope = seal(&checkpoint, key)?;
        publish_pending(&self.root, &envelope)?;
        let fence = digest(&read_pending(&self.root)?);
        let session = crate::native_runtime::Session::new_transition_trial(
            self,
            target,
            proposed.ports,
            guard,
            TrialBinding {
                fields,
                pending_sha256: fence,
            },
        )?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        let session = if let Some((trial, transition)) = checkpoint.furnishing.clone().zip(transition) {
            session.with_furnishing_transition_trial(trial, transition)
        } else { session };
        Ok((session, checkpoint, binding, envelope))
    }

    fn activate_local_personal_runtime_transition(
        &self,
        target: Arc<Payload>,
        staging: &Path,
        key: &[u8; 32],
        saved_workspace_host: bool,
        document: &super::installations::native_document::DocumentState,
        retained: Option<&RetainedProductImage>,
        #[cfg(feature = "workspace-v5-owner-validation")]
        selected: Option<(crate::native_runtime::oex_retained_cleanup::Selected045, PathBuf)>,
    ) -> Result<()> {
        let (mut session, mut checkpoint, binding, envelope) =
            self.trial_local_personal_runtime(target, staging, key, saved_workspace_host, retained, Some(document))?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        let selected_staging = selected.map(|(selection, prefix)| {
            session.arm_selected_recovery_045(selection)?;
            Ok::<_, LifecycleError>(prefix)
        }).transpose()?;
        #[cfg(feature = "workspace-v5-owner-validation")]
        if let Some(proof) = retained {
            session.arm_retained_product_image(proof.clone())?;
        }
        #[cfg(feature = "workspace-v5-owner-validation")]
        if let Some(proof) = &checkpoint.startup_cancellation_045 {
            session.arm_startup_cancellation_045(proof.clone())?;
        }
        let qualified = (|| {
            if session.start()?.state != "ready" {
                return Err(invalid());
            }
            if let Some(expected) = &checkpoint.saved_document_sha256
                && saved_document_digest(document)? != *expected
            {
                return Err(err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED"));
            }
            let authoring = session.authoring_session()?;
            if authoring["tenantId"].as_str() != Some(checkpoint.tenant_id.as_str())
                || authoring["principalId"].as_str() != Some(checkpoint.principal_id.as_str())
            {
                return Err(invalid());
            }
            #[cfg(feature = "workspace-v5-owner-validation")]
            let empty_gpu_document = gpu_empty_document(
                (&checkpoint.old_manifest, &checkpoint.new_manifest), document,
                (&checkpoint.workspace_id, &checkpoint.tenant_id, &checkpoint.principal_id),
            );
            #[cfg(not(feature = "workspace-v5-owner-validation"))]
            let empty_gpu_document = false;
            if checkpoint.saved_document_sha256.is_some() && !empty_gpu_document {
                session.verify_transition_studio(&authoring, document)?;
            }
            #[cfg(feature = "workspace-v5-owner-validation")]
            if let Some(proof) = retained {
                let receipt = session.verify_retained_product_image(&authoring)?;
                checkpoint.verify_retained_product_image_files(proof, &receipt)?;
            }
            #[cfg(feature = "workspace-v5-owner-validation")]
            session.verify_startup_cancellation_045()?;
            #[cfg(feature = "workspace-v5-owner-validation")]
            session.verify_clean_exit_hayes_after_start()?;
            Ok(())
        })();
        let stop = session.stop()?;
        self.check_prepare_guard(session.drained_transition_guard()?)?;
        if let Err(error) = qualified {
            if stop.state == "failed" {
                checkpoint.drained_failure = stop.job_id;
                session.check_drained_transition_failure_root(checkpoint.drained_failure.as_deref())?;
                let drained = seal(&checkpoint, key)?;
                publish_pending(&self.root, &drained)?;
            }
            return Err(error);
        }
        if let Some(proof) = retained {
            proof.check_replacement_attempt(document)?;
        }
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
        let pending = read_pending(&self.root)?;
        if pending.strip_prefix(PENDING_PREFIX) != Some(envelope.as_slice()) {
            return Err(invalid());
        }
        self.transition_checkpoint(&envelope, key)?;
        if checkpoint.saved_document_sha256.is_some() {
            let unchanged = |path: &Path| path.starts_with("blobs") || path.starts_with("config");
            let live = inventory_with_limit(&self.root, false, checkpoint.inventory_limit() + pending.len() as u64)?;
            #[cfg(feature = "workspace-v5-owner-validation")]
            if let Some(prefix) = selected_staging {
                let bytes = session.selected_recovery_045_staging_bytes()?.ok_or_else(invalid)?;
                verify_selected_staging_delta(&checkpoint.files, &live, &prefix, bytes)?;
            } else if live.iter().filter(|(path, _)| unchanged(path))
                .ne(checkpoint.files.iter().filter(|(path, _)| unchanged(path))) {
                return Err(err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED"));
            }
            #[cfg(not(feature = "workspace-v5-owner-validation"))]
            if live.iter().filter(|(path, _)| unchanged(path))
                .ne(checkpoint.files.iter().filter(|(path, _)| unchanged(path))) {
                return Err(err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED"));
            }
        }
        keep_receipt(&self.root, "qualified", &envelope)?;
        replace_private(&self.root, RECORD, &binding.new_record, 4096)?;
        replace_private(&self.root, ENVIRONMENT, &binding.new_env.0, 4096)?;
        Ok(())
    }

    /// Seal an external private staging inventory under the same operation
    /// guard used by native starts. Staging must not be a Manager workspace.
    /// The key is supplied in memory by the private caller, never persisted here.
    fn authenticate_local_personal_checkpoint(
        &self,
        source_payload: &Payload,
        target_payload: &Payload,
        staging: &Path,
        key: &[u8; 32],
        saved_document_sha256: Option<String>,
        retained_product_image: Option<&RetainedProductImage>,
        startup_cancellation_045: Option<&StartupCancellation045>,
    ) -> Result<()> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        let source = source_payload.context();
        let target = target_payload.context();
        if !admitted_pair(&source.manifest_sha256, &target.manifest_sha256)
            || source.official_signed
            || source.redistribution_qualified
            || target.official_signed
            || target.redistribution_qualified
            || !staging.is_absolute()
            || staging.starts_with(&self.root)
            || self.root.starts_with(staging)
        {
            return Err(invalid());
        }
        #[cfg(feature = "workspace-v5-owner-validation")]
        if saved_workspace_transition_supported((&source.manifest_sha256, &target.manifest_sha256))
            != saved_document_sha256.is_some()
        {
            return Err(invalid());
        }
        source_payload.recheck()?;
        target_payload.recheck()?;
        installations::private_directory(staging)?;
        let staging_pin = crate::maintenance_source::DirectoryPin::open(staging)?;
        check_cold_transition_source(&self.root, (&source.manifest_sha256, &target.manifest_sha256))?;
        let record = read(self)?;
        let mut fields = self.personal_environment(source_payload, record.ports)?;
        for value in fields.values_mut() {
            std::mem::take(value).into_bytes().fill(0);
        }
        #[cfg(feature = "workspace-v5-owner-validation")]
        let transition = FurnishingTransition::from_manifests((&source.manifest_sha256, &target.manifest_sha256));
        #[cfg(feature = "workspace-v5-owner-validation")]
        let furnishing = if let Some(transition) = transition {
            let source_catalog = transition.source_catalog();
            let target_catalog = transition.target_catalog();
            if source_payload.workspace_furnishing_catalog()? != Some(source_catalog)
                || target_payload.workspace_furnishing_catalog()? != Some(target_catalog)
            {
                return Err(invalid());
            }
            let target_expected_source = match transition {
                FurnishingTransition::Upgrade39(_) => target_payload.catalog39_upgrade_source_ledger()?,
                FurnishingTransition::Upgrade40(_) => target_payload.catalog40_upgrade_source_ledger()?,
                FurnishingTransition::Upgrade44 { .. } => target_payload.catalog44_upgrade_source_ledger()?,
                FurnishingTransition::Upgrade50 { .. } => target_payload.catalog50_upgrade_source_ledger()?,
                FurnishingTransition::Upgrade49 { .. } => target_payload.catalog49_upgrade_source_ledger()?,
                _ => target_payload.workspace_furnishing_ledger(target_catalog)?,
            };
            if source_payload.workspace_furnishing_ledger(source_catalog)? != target_expected_source {
                return Err(invalid());
            }
            Some(crate::native_runtime::FurnishingTransitionTrial::capture(self, transition)?)
        } else { None };
        #[cfg(feature = "workspace-v5-owner-validation")]
        let _writer = furnishing.as_ref().zip(transition)
            .map(|(trial, transition)| trial.hold_source(self, transition)).transpose()?;
        let max_bytes = checkpoint_inventory_limit((&source.manifest_sha256, &target.manifest_sha256), startup_cancellation_045);
        let files = inventory_with_limit(&self.root, false, max_bytes)?;
        if saved_document_sha256.is_some() {
            let device = directory_identity(&self.root)?.0;
            if fs::read_dir(staging).map_err(|_| invalid())?.next().is_some()
                || device != directory_identity(staging)?.0
            {
                return Err(invalid());
            }
            for name in DATA_DIRECTORIES {
                if directory_identity(&self.root.join(name))?.0 != device {
                    return Err(invalid());
                }
            }
            let bytes: u64 = files.values().map(|entry| entry.bytes).sum();
            if fs2::available_space(staging).map_err(|_| invalid())?
                < 2 * bytes + MAX_MANIFEST as u64 + 256 * 1024 * 1024
            {
                return Err(err("NATIVE_PHYSICAL_CHECKPOINT_SPACE"));
            }
            copy_beforeimages(&self.root, false, staging, &files, max_bytes)?;
        }
        // Backup names keep the staged directory from becoming a runnable profile.
        check_staged_names(staging, false)?;
        if files != inventory_with_limit(staging, true, max_bytes)? {
            return Err(invalid());
        }
        let checkpoint = Checkpoint {
            source: self.root.clone(),
            staged: staging.to_owned(),
            source_identity: directory_identity(&self.root)?,
            staged_identity: directory_identity(staging)?,
            workspace_id: record.workspace_id,
            tenant_id: record.tenant_id,
            principal_id: record.principal_id,
            owner_uid: record.owner_uid,
            old_manifest: source.manifest_sha256,
            new_manifest: target.manifest_sha256,
            files,
            saved_document_sha256,
            retained_product_image: retained_product_image.cloned(),
            startup_cancellation_045: startup_cancellation_045.cloned(),
            #[cfg(feature = "workspace-v5-owner-validation")]
            furnishing,
            trial: false,
            drained_failure: None,
            restore: None,
        };
        let envelope = seal(&checkpoint, key)?;
        staging_pin.check()?;
        self.check_prepare_guard(&guard)?;
        if checkpoint.files != inventory_with_limit(&self.root, false, max_bytes)?
            || checkpoint.files != inventory_with_limit(staging, true, max_bytes)?
        {
            return Err(invalid());
        }
        write_private_new(staging, ENVELOPE, &envelope, MAX_MANIFEST + 28)?;
        File::open(staging)
            .and_then(|f| f.sync_all())
            .map_err(|_| invalid())?;
        let sealed =
            installation_backup::source_bytes(staging, ENVELOPE, (MAX_MANIFEST + 28) as u64, true)?;
        if open(&sealed, key)? != checkpoint {
            return Err(invalid());
        }
        Ok(())
    }
}

impl InstallationController {
    fn with_transition_key<T>(
        &self,
        token: &str,
        key_path: &Path,
        staging: &Path,
        retained: Option<&RetainedProductImage>,
        task: impl FnOnce(&LifecycleService, &Path, &super::installations::native_document::DocumentState, &[u8; 32]) -> Result<T>,
    ) -> Result<T> {
        if key_path.starts_with(staging) {
            return Err(invalid());
        }
        self.with_personal_transition(token, retained, |service, profile, document| {
            if key_path.starts_with(&service.root) {
                return Err(invalid());
            }
            let key = Bytes(crate::profile_backup::external_key(profile, key_path)?);
            task(service, profile, document, key.0.as_slice().try_into().map_err(|_| invalid())?)
        })
    }

    pub(crate) fn prepare_local_personal_runtime_transition(
        &self,
        token: &str,
        source: &Payload,
        target: &Payload,
        staging: &Path,
        key_path: &Path,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, None, |service, _, _, key| {
            if !present(staging, ENVELOPE)? {
                service.authenticate_local_personal_checkpoint(source, target, staging, key, None, None, None)?;
            }
            service.stage_local_personal_runtime_transition(source, target, staging, key, false)
        })
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    pub(crate) fn prepare_saved_workspace_v5_transition(
        &self,
        token: &str,
        source: &Payload,
        target: &Payload,
        staging: &Path,
        key_path: &Path,
        retained: Option<&RetainedProductImage>,
        startup_cancellation_045: Option<&StartupCancellation045>,
    ) -> Result<()> {
        let source_manifest = source.context().manifest_sha256;
        let target_manifest = target.context().manifest_sha256;
        let pair = (source_manifest.as_str(), target_manifest.as_str());
        if !saved_workspace_transition_supported(pair) || !retained_transition_proof_matches(pair, retained)
            || !startup_cancellation_proof_matches(pair, retained, startup_cancellation_045)
        {
            return Err(invalid());
        }
        self.with_transition_key(token, key_path, staging, retained, |service, profile, document, key| {
            if !staging.is_absolute() || staging.starts_with(profile) || profile.starts_with(staging) {
                return Err(invalid());
            }
            let owner = service.personal_identity()?;
            check_saved_transition_document(pair, document, (&owner.0, &owner.1, &owner.2))?;
            if target.catalog45_recovery_validation() {
                let generation = crate::native_runtime::catalog45_recovery_source_generation(
                    &service.root, crate::native_payload::catalog45_startup_recovery(pair),
                )?;
                if let Some(proof) = startup_cancellation_045 {
                    self.verify_startup_cancellation_045_preservation(service, document, proof)?;
                    if proof.failed_generation != generation || !installations::uuid(&proof.hayes_job_id)
                        || !hash_valid(&proof.hayes_completion_frame_sha256)
                    {
                        return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
                    }
                } else if let Some(proof) = retained.filter(|proof| proof.recovery.is_some()) {
                    check_retained_recovery_generation(proof, &generation)?;
                } else if pair.0 != pair.1 && crate::native_payload::catalog45_startup_recovery(pair)
                    != Some(crate::native_payload::Catalog45StartupRecovery::PreReadyPreservation) {
                    self.check_catalog45_recovery_export_unlocked(service, &generation, document)?;
                }
            }
            let document_sha256 = saved_document_digest(document)?;
            if present(staging, ENVELOPE)? {
                let envelope = installation_backup::source_bytes(staging, ENVELOPE, ENVELOPE_LIMIT as u64, true)?;
                let checkpoint = service.transition_checkpoint(&envelope, key)?;
                checkpoint.check_retained(retained)?;
                if checkpoint.saved_document_sha256.as_deref() != Some(document_sha256.as_str())
                    || checkpoint.startup_cancellation_045.as_ref() != startup_cancellation_045
                {
                    return Err(invalid());
                }
            } else {
                service.authenticate_local_personal_checkpoint(
                    source, target, staging, key, Some(document_sha256), retained, startup_cancellation_045,
                )?;
            }
            service.stage_local_personal_runtime_transition(source, target, staging, key, true)
        })
    }

    #[allow(dead_code)] // Reserved for the private transition caller.
    pub(crate) fn authenticate_local_personal_checkpoint(
        &self,
        token: &str,
        source: &Payload,
        target: &Payload,
        staging: &Path,
        key_path: &Path,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, None, |service, _, _, key| {
            service.authenticate_local_personal_checkpoint(source, target, staging, key, None, None, None)
        })
    }

    #[allow(dead_code)] // Reserved for the private transition caller.
    pub(crate) fn stage_local_personal_runtime_transition(
        &self,
        token: &str,
        source: &Payload,
        target: &Payload,
        staging: &Path,
        key_path: &Path,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, None, |service, _, _, key| {
            service.stage_local_personal_runtime_transition(source, target, staging, key, false)
        })
    }

    pub(crate) fn activate_local_personal_runtime_transition(
        &self,
        token: &str,
        target: Arc<Payload>,
        staging: &Path,
        key_path: &Path,
        saved_workspace_host: bool,
        retained: Option<&RetainedProductImage>,
        startup_cancellation_045: Option<&StartupCancellation045>,
    ) -> Result<()> {
        #[cfg(not(feature = "workspace-v5-owner-validation"))]
        if startup_cancellation_045.is_some() { return Err(invalid()); }
        self.with_transition_key(token, key_path, staging, retained, |service, _, document, key| {
            #[cfg(feature = "workspace-v5-owner-validation")]
            if target.catalog45_recovery_validation() {
                let checkpoint = self.catalog45_activation_checkpoint(service, document, &target, staging, key)?;
                let startup = crate::native_payload::catalog45_startup_recovery((
                    checkpoint.old_manifest.as_str(), checkpoint.new_manifest.as_str(),
                ));
                if (checkpoint.old_manifest == checkpoint.new_manifest && startup.is_none())
                    || checkpoint.startup_cancellation_045.as_ref() != startup_cancellation_045
                {
                    return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
                }
                if let Some(proof) = startup_cancellation_045 {
                    self.verify_startup_cancellation_045_preservation(service, document, proof)?;
                }
            } else if startup_cancellation_045.is_some() {
                return Err(invalid());
            }
            service.activate_local_personal_runtime_transition(
                target, staging, key, saved_workspace_host, document, retained,
                #[cfg(feature = "workspace-v5-owner-validation")]
                None,
            )
        })
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    fn catalog45_activation_checkpoint(
        &self,
        service: &LifecycleService,
        document: &super::installations::native_document::DocumentState,
        target: &Payload,
        staging: &Path,
        key: &[u8; 32],
    ) -> Result<Checkpoint> {
        let pending = read_pending(&service.root)?;
        let checkpoint = service.transition_checkpoint(pending.strip_prefix(PENDING_PREFIX).ok_or_else(invalid)?, key)?;
        checkpoint.check_host(true)?;
        if checkpoint.trial || checkpoint.staged != staging
            || checkpoint.new_manifest != target.context().manifest_sha256
        {
            return Err(invalid());
        }
        let pair = (checkpoint.old_manifest.as_str(), checkpoint.new_manifest.as_str());
        let generation = crate::native_runtime::catalog45_recovery_source_generation(
            &service.root, crate::native_payload::catalog45_startup_recovery(pair),
        )?;
        if checkpoint.startup_cancellation_045.as_ref().is_some_and(|proof| proof.failed_generation != generation) {
            return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
        }
        let retained_recovery = checkpoint.retained_product_image.as_ref().filter(|proof| proof.recovery.is_some());
        if let Some(proof) = retained_recovery {
            check_retained_recovery_generation(proof, &generation)?;
        }
        if !catalog45_activation_saved_document(pair, retained_recovery.is_some(),
            checkpoint.startup_cancellation_045.is_some(), checkpoint.saved_document_sha256.as_deref(), document)? {
            self.check_catalog45_recovery_export_unlocked(service, &generation, document)?;
        }
        verify_unchanged(&service.root, &checkpoint, &pending)?;
        Ok(checkpoint)
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    pub(crate) fn activate_selected_recovery_045(
        &self, token: &str, target: Arc<Payload>, staging: &Path, key_path: &Path,
        selection: crate::installations::native_maintenance::SelectedRecovery045,
    ) -> Result<()> {
        use crate::native_runtime::{oex_recovery::SelectedRetirement, oex_retained_cleanup};
        if !target.catalog45_selected_recovery_validation() {
            return Err(invalid());
        }
        self.with_transition_key(token, key_path, staging, None, |service, profile, document, key| {
            let checkpoint = self.catalog45_activation_checkpoint(service, document, &target, staging, key)?;
            let oex = oex_retained_cleanup::Selection::new(&selection.oex_job_id, &selection.oex_import_runtime_generation)?;
            let prefix = Path::new("blobs").join(&checkpoint.tenant_id).join("oex-staging").join(&selection.oex_job_id);
            let settled_only = checkpoint.old_manifest == checkpoint.new_manifest;
            // Settlement durably observes absence. Require its directory to
            // exist and be empty so that observation cannot create any paths.
            if settled_only && (checkpoint.files.get(&prefix).is_none_or(|entry| entry.sha256.is_some())
                || checkpoint.files.keys().any(|path| path != &prefix && path.starts_with(&prefix)))
            {
                return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
            }
            let (original, absent_chunks) = service.selected_045_original_store(
                profile, &checkpoint, &selection.rollback_lineage, &prefix,
            )?;
            let retired = SelectedRetirement::open(
                &service.root, (&checkpoint.workspace_id, &checkpoint.tenant_id, &checkpoint.principal_id),
                &selection.oex_job_id, &selection.oex_import_runtime_generation, &selection.oex_retirement_sha256,
                original.as_deref(),
            )?;
            let selected = oex_retained_cleanup::Selected045::new(
                oex, &selection.hayes_job_id, &selection.hayes_completion_frame_sha256, retired, absent_chunks, settled_only,
            )?;
            service.activate_local_personal_runtime_transition(target, staging, key, true, document, None, Some((selected, prefix)))
        })
    }

    pub(crate) fn recover_local_personal_runtime_transition(
        &self,
        token: &str,
        staging: &Path,
        key_path: &Path,
        saved_workspace_host: bool,
        retained: Option<&RetainedProductImage>,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, retained, |service, _, _, key| {
            service.recover_local_personal_runtime_transition(staging, key, saved_workspace_host, retained)
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn gpu_library_empty_document() -> crate::installations::native_document::DocumentState {
        crate::installations::native_document::DocumentState {
            format: 1, revision: 0,
            workspace_id: "10000000-0000-4000-8000-000000000001".into(),
            tenant_id: "10000000-0000-4000-8000-000000000002".into(),
            principal_id: "10000000-0000-4000-8000-000000000003".into(),
            draft: None, etag: None, saved_edit_version: None, pending: None,
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn gpu_library_stage_accepts_only_exact_fresh_empty_document_and_owner() {
        let pair = (crate::native_payload::CATALOG45_DEFAULT_1_7_TARGET.2,
            "3ee986e7d6601240db6b16f311235bb3a3d43cd2fef00688a740a6c9e91cd377");
        let document = gpu_library_empty_document();
        let owner = (document.workspace_id.as_str(), document.tenant_id.as_str(), document.principal_id.as_str());
        assert!(gpu_empty_document(pair, &document, owner));
        check_saved_transition_document(pair, &document, owner).unwrap();
        let mut changes = Vec::new();
        let mut changed = document.clone(); changed.format = 2; changes.push(changed);
        let mut changed = document.clone(); changed.revision = 1; changes.push(changed);
        let mut changed = document.clone(); changed.draft = Some(serde_json::json!({"editVersion": 1})); changes.push(changed);
        let mut changed = document.clone(); changed.etag = Some("\"studio-r1-unknown\"".into()); changes.push(changed);
        let mut changed = document.clone(); changed.saved_edit_version = Some(0); changes.push(changed);
        let mut changed = document.clone(); changed.pending = Some(serde_json::from_value(serde_json::json!({
            "requestId": "10000000-0000-4000-8000-000000000004", "etag": null, "draft": {"editVersion": 1}
        })).unwrap()); changes.push(changed);
        for field in ["workspaceId", "tenantId", "principalId"] {
            let mut value = serde_json::to_value(&document).unwrap();
            value[field] = "10000000-0000-4000-8000-000000000005".into();
            changes.push(serde_json::from_value(value).unwrap());
        }
        for changed in changes {
            assert!(!gpu_empty_document(pair, &changed, owner));
            assert_eq!(check_saved_transition_document(pair, &changed, owner).unwrap_err().code,
                "NATIVE_PHYSICAL_CHECKPOINT_SAVED_DOCUMENT_REQUIRED");
        }
        assert!(!gpu_empty_document(pair, &document, ("invalid", owner.1, owner.2)));
        for other in [(pair.1, pair.0), (pair.0, pair.0), (OLD_MANIFEST, NEW_MANIFEST),
            (pair.0, "2c29a8ffe2a56395dd1cdc1b0acd93e4a1623e8aac8b0e10aca30369e60b4626")] {
            assert!(!gpu_empty_document(other, &document, owner));
            assert!(check_saved_transition_document(other, &document, owner).is_err());
        }
        // Saved-head behavior remains available for both 049 and 050.
        let mut saved = document.clone();
        saved.revision = 1; saved.draft = Some(serde_json::json!({"editVersion": 1}));
        saved.etag = Some(format!("\"studio-r1-{}\"", "a".repeat(64))); saved.saved_edit_version = Some(1);
        for target in [pair.1, "2c29a8ffe2a56395dd1cdc1b0acd93e4a1623e8aac8b0e10aca30369e60b4626"] {
            assert!(!gpu_empty_document((pair.0, target), &saved, owner));
            check_saved_transition_document((pair.0, target), &saved, owner).unwrap();
            let mut unsaved = saved.clone(); unsaved.saved_edit_version = Some(2);
            assert!(check_saved_transition_document((pair.0, target), &unsaved, owner).is_err());
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn gpu_wrapping_same_catalog_transition_keeps_empty_document_scope() {
        let old = crate::native_payload::GPU_MODEL_LEGACY_TARGET.2;
        let new = crate::native_payload::GPU_MODEL_WRAPPING_TARGET.2;
        let document = gpu_library_empty_document();
        let owner = (document.workspace_id.as_str(), document.tenant_id.as_str(), document.principal_id.as_str());
        assert!(gpu_empty_document((old, new), &document, owner));
        check_saved_transition_document((old, new), &document, owner).unwrap();
        for pair in [(new, old), (old, old), (new, new), ("unknown", new)] {
            assert!(!gpu_empty_document(pair, &document, owner));
            assert!(check_saved_transition_document(pair, &document, owner).is_err());
        }
        let mut changed = document.clone(); changed.revision = 1;
        assert!(!gpu_empty_document((old, new), &changed, owner));
        assert!(!gpu_empty_document((old, new), &document, (owner.1, owner.0, owner.2)));
        let digest = saved_document_digest(&document).unwrap();
        changed = document.clone(); changed.saved_edit_version = Some(1);
        assert!(!gpu_empty_document((old, new), &changed, owner));
        assert_ne!(digest, saved_document_digest(&changed).unwrap());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn gpu_library_activation_retains_authenticated_document_digest_and_host_fences() {
        let pair = (crate::native_payload::CATALOG45_DEFAULT_1_7_TARGET.2,
            "3ee986e7d6601240db6b16f311235bb3a3d43cd2fef00688a740a6c9e91cd377");
        let document = gpu_library_empty_document();
        let owner = (document.workspace_id.as_str(), document.tenant_id.as_str(), document.principal_id.as_str());
        let expected = saved_document_digest(&document).unwrap();
        check_transition_host(pair, Some(&expected), true).unwrap();
        assert!(check_transition_host(pair, None, true).is_err());
        assert!(check_transition_host(pair, Some(&expected), false).is_err());
        assert!(catalog45_activation_saved_document(pair, false, false, Some(&expected), &document).unwrap());
        assert!(gpu_empty_document(pair, &document, owner));
        for field in ["revision", "workspaceId", "tenantId", "principalId", "draft", "etag", "savedEditVersion", "pending"] {
            let mut value = serde_json::to_value(&document).unwrap();
            value[field] = match field {
                "revision" | "savedEditVersion" => serde_json::json!(1),
                "draft" => serde_json::json!({"editVersion": 1}),
                "pending" => serde_json::json!({"requestId": "10000000-0000-4000-8000-000000000004", "etag": null, "draft": {"editVersion": 1}}),
                _ => serde_json::json!("10000000-0000-4000-8000-000000000005"),
            };
            let changed = serde_json::from_value(value).unwrap();
            assert_eq!(catalog45_activation_saved_document(pair, false, false, Some(&expected), &changed).unwrap_err().code,
                "NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
            assert!(!gpu_empty_document(pair, &changed, owner));
        }
        assert!(catalog45_activation_saved_document(pair, false, false, None, &document).is_err());
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    #[test]
    fn catalog45_checkpoint_capacity_is_exactly_scoped() {
        #[cfg(all(debug_assertions, target_os = "macos"))]
        {
            // The current cold saved workspace is larger than the former 5 GiB
            // handoff bound. These admitted successors need the streamed 6 GiB
            // family limit; reverse and self pairs must not inherit it.
            let scene = crate::native_payload::CATALOG45_SCENE_AUTHORING_TARGET.2;
            for source in [crate::native_payload::CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2,
                           crate::native_payload::CATALOG45_PORTAL_HAYES_CARD_TARGET.2] {
                let limit = checkpoint_inventory_limit((source, scene), None);
                assert_eq!(limit, CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES);
                assert!(5_594_726_660 <= limit);
                assert_eq!(checkpoint_inventory_limit((scene, source), None), MAX_BYTES);
            }
            assert_eq!(checkpoint_inventory_limit((scene, scene), None), MAX_BYTES);
            let freshness = crate::native_payload::CATALOG45_THUMBNAIL_FRESHNESS_TARGET.2;
            assert_eq!(checkpoint_inventory_limit((scene, freshness), None), CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((freshness, scene), None), MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((freshness, freshness), None), MAX_BYTES);
            let complete = crate::native_payload::CATALOG45_COMPLETE_STATIC_TARGET.2;
            assert_eq!(checkpoint_inventory_limit((freshness, complete), None), CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((complete, freshness), None), MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((complete, complete), None), MAX_BYTES);
            let fixture = crate::native_payload::CATALOG45_FIXTURE_AUTHORING_TARGET.2;
            assert_eq!(checkpoint_inventory_limit((complete, fixture), None), CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((fixture, complete), None), MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((fixture, fixture), None), MAX_BYTES);
            let default_1_7 = crate::native_payload::CATALOG45_DEFAULT_1_7_TARGET.2;
            assert_eq!(checkpoint_inventory_limit((fixture, default_1_7), None), CATALOG45_ADOPTION_CHECKPOINT_MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((default_1_7, fixture), None), MAX_BYTES);
            assert_eq!(checkpoint_inventory_limit((default_1_7, default_1_7), None), MAX_BYTES);
        }
        let current = "f738f33c777de39629b6a0ab2dd3a477b45d078b738189e173d00d2cd7ab652a";
        let previous = "f15d2530a298c3e9507e4719fa3ba65f26aff8cb9b40d9f646f1043976d0a75d";
        let proof = StartupCancellation045 {
            failed_generation: Uuid::new_v4().to_string(),
            hayes_job_id: Uuid::new_v4().to_string(),
            hayes_completion_frame_sha256: "a".repeat(64),
            saved_etag: "\"studio-r20-fixture\"".into(),
            transfer_sha256: "b".repeat(64),
        };
        let default = checkpoint_inventory_limit((current, current), None);
        let extended = checkpoint_inventory_limit((current, current), Some(&proof));
        assert_eq!(default, 2_147_483_648);
        assert_eq!(extended, 2_415_919_104);
        let publication = "1209d5d95417d1e29f637d8ce327e78cf38ed1b32eabcb5ab6fd3e2a62a10203";
        let captions = "4fbd168027090f38776bd75990e27ed336db6c65d15c4af3f17bf428d140440b";
        let prepared_sign_audio = "cbf4214bbf755216e8173190ce98ee32fb3f3708bd9c6a25e394eb3ce30e0a0a";
        let pre_ready_preservation = "baf33be8865ee1c558874702ca8a772db90a9c59c3ab28759e7fc6ae8d608c86";
        let pre_ready_hayes_preservation = "5cb2431e561c21e1463f78faf938a5cfc3807c9dd690a545f3474769c1c7f4a9";
        let placed_exit_doors = "1245b842efb55525ebf0c307f984391431552ce19d6f59874dbcd8435b8e39d2";
        let caption_fixture = "31b12d9f6fac4019f39c2a9128ced09b7ee918c49ffb119beb544e5e40831ac0";
        let publication_limit = checkpoint_inventory_limit((current, publication), None);
        assert_eq!(publication_limit, 4_294_967_296);
        assert_eq!(checkpoint_inventory_limit((publication, captions), None), publication_limit);
        assert_eq!(checkpoint_inventory_limit((captions, prepared_sign_audio), None), publication_limit);
        let pre_ready = (prepared_sign_audio, pre_ready_preservation);
        assert_eq!(checkpoint_inventory_limit(pre_ready, None), publication_limit);
        assert!(startup_cancellation_proof_matches(pre_ready, None, None));
        assert!(!startup_cancellation_proof_matches(pre_ready, None, Some(&proof)));
        let pre_ready_hayes = (prepared_sign_audio, pre_ready_hayes_preservation);
        let clean_exit_handoff = (pre_ready_hayes_preservation, placed_exit_doors);
        assert_eq!(checkpoint_inventory_limit(clean_exit_handoff, None), publication_limit);
        #[cfg(all(debug_assertions, target_os = "macos"))]
        {
            let backed_bench = "f8d5933a2c2e361154dbcc9cb4a332144ca9217e5edb363fdd5056ee70aed702";
            assert_eq!(checkpoint_inventory_limit((placed_exit_doors, backed_bench), None), publication_limit);
            assert_eq!(checkpoint_inventory_limit((backed_bench, placed_exit_doors), None), default);
            assert_eq!(checkpoint_inventory_limit((backed_bench, backed_bench), None), default);
        }
        assert_eq!(checkpoint_inventory_limit((placed_exit_doors, pre_ready_hayes_preservation), None), default);
        assert!(startup_cancellation_proof_matches(clean_exit_handoff, None, None));
        assert!(!startup_cancellation_proof_matches(clean_exit_handoff, None, Some(&proof)));
        assert_eq!(checkpoint_inventory_limit(pre_ready_hayes, Some(&proof)), publication_limit);
        assert!(!startup_cancellation_proof_matches(pre_ready_hayes, None, None));
        assert!(startup_cancellation_proof_matches(pre_ready_hayes, None, Some(&proof)));
        let mut document = crate::installations::native_document::DocumentState {
            format: 1, revision: 1, workspace_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(), principal_id: Uuid::new_v4().to_string(),
            draft: None, etag: None, saved_edit_version: None, pending: None,
        };
        let expected = saved_document_digest(&document).unwrap();
        assert!(catalog45_activation_saved_document(pre_ready, false, false, Some(&expected), &document).unwrap());
        assert!(catalog45_activation_saved_document(pre_ready_hayes, false, true, Some(&expected), &document).unwrap());
        assert!(catalog45_activation_saved_document(clean_exit_handoff, false, false, Some(&expected), &document).unwrap());
        assert!(!catalog45_activation_saved_document((previous, current), false, false, Some(&expected), &document).unwrap());
        document.revision += 1;
        assert_eq!(catalog45_activation_saved_document(pre_ready, false, false, Some(&expected), &document).unwrap_err().code,
            "NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
        assert_eq!(catalog45_activation_saved_document(pre_ready_hayes, false, true, Some(&expected), &document).unwrap_err().code,
            "NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
        assert_eq!(catalog45_activation_saved_document(clean_exit_handoff, false, false, Some(&expected), &document).unwrap_err().code,
            "NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
        for pair in [(captions, publication), (captions, captions), (current, captions), (caption_fixture, captions),
                     (prepared_sign_audio, captions), (publication, prepared_sign_audio), (prepared_sign_audio, prepared_sign_audio),
                     (pre_ready_preservation, prepared_sign_audio), (pre_ready_preservation, pre_ready_preservation)] {
            assert_eq!(checkpoint_inventory_limit(pair, None), default);
        }
        assert_eq!(checkpoint_inventory_limit((publication, current), None), default);
        assert_eq!(checkpoint_inventory_limit((publication, publication), None), default);
        assert_eq!(checkpoint_inventory_limit((previous, publication), None), default);
        for pair in [(previous, current), (current, previous), (previous, previous), (OLD_MANIFEST, NEW_MANIFEST)] {
            assert_eq!(checkpoint_inventory_limit(pair, Some(&proof)), default);
        }

        // Oversize members are rejected from metadata before any file copy.
        let root = std::env::temp_dir().join(format!("checkpoint-capacity-{}", Uuid::new_v4()));
        installations::new_directory(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        for name in DATA_DIRECTORIES {
            installations::new_directory(&root.join(name)).unwrap();
        }
        for name in [RECORD, ENVIRONMENT] {
            private_options().open(root.join(name)).unwrap();
        }
        let file = private_options().open(root.join("blobs/oversize")).unwrap();
        file.set_len(default + 1).unwrap();
        assert_eq!(inventory_with_limit(&root, false, default).unwrap_err().code, invalid().code);
        file.set_len(extended + 1).unwrap();
        assert_eq!(inventory_with_limit(&root, false, extended).unwrap_err().code, invalid().code);
        file.set_len(publication_limit).unwrap();
        let files = inventory_with_limit(&root, false, publication_limit).unwrap();
        assert_eq!(files[Path::new("blobs/oversize")].bytes, publication_limit);
        file.set_len(publication_limit + 1).unwrap();
        assert_eq!(inventory_with_limit(&root, false, publication_limit).unwrap_err().code, invalid().code);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    #[test]
    fn selected_045_delta_only_removes_exact_staging_files_and_allows_repeat() {
        let prefix = Path::new("blobs/tenant/oex-staging/job");
        let entry = |bytes| Entry { bytes, mode: 0o600, sha256: Some("a".repeat(64)) };
        let stage = prefix.join("chunk-0");
        let extra = prefix.join("unselected");
        let config = PathBuf::from("config/settings");
        let before = BTreeMap::from([(stage.clone(), entry(7)), (extra.clone(), entry(2)), (config.clone(), entry(3))]);
        let mut after = before.clone();
        after.remove(&stage);
        assert!(verify_selected_staging_delta(&before, &after, prefix, 7).is_ok());
        assert!(verify_selected_staging_delta(&after, &after, prefix, 7).is_ok());
        assert!(verify_selected_staging_delta(&before, &before, prefix, 7).is_err());
        assert!(verify_selected_staging_delta(&before, &after, prefix, 8).is_err());
        after.remove(&extra);
        assert!(verify_selected_staging_delta(&before, &after, prefix, 7).is_err());
        after.insert(extra, entry(2));
        after.insert(config, entry(4));
        assert!(verify_selected_staging_delta(&before, &after, prefix, 7).is_err());
    }
    use std::{net::TcpListener, os::unix::fs::PermissionsExt, process::Command, sync::Arc};

    #[test]
    #[ignore = "Launched as an isolated crash child by the checkpoint test"]
    fn checkpoint_journal_crash_child() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_CHECKPOINT_TEST_ROOT").unwrap());
        let staging = PathBuf::from(std::env::var_os("EXHIBITOS_CHECKPOINT_TEST_STAGE").unwrap());
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let guard = service.lock().unwrap();
        service.check_prepare_guard(&guard).unwrap();
        let envelope = fs::read(staging.join(ENVELOPE)).unwrap();
        publish_pending(&root, &envelope).unwrap();
        std::process::exit(97);
    }

    #[test]
    #[ignore = "Launched as the isolated post-trial interruption child"]
    fn checkpoint_trial_exit_child() {
        let path = |name| PathBuf::from(std::env::var_os(name).unwrap());
        let root = path("EXHIBITOS_CHECKPOINT_TEST_ROOT");
        let staging = path("EXHIBITOS_CHECKPOINT_TEST_STAGE");
        let profile = path("EXHIBITOS_CHECKPOINT_TEST_PROFILE");
        let key_path = path("EXHIBITOS_CHECKPOINT_TEST_KEY");
        let key = Bytes(crate::profile_backup::external_key(&profile, &key_path).unwrap());
        let target = Arc::new(Payload::open(&path("EXHIBITOS_TEST_TARGET_PAYLOAD")).unwrap());
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let (mut session, checkpoint, binding, _) = service
            .trial_local_personal_runtime(target, &staging, key.0.as_slice().try_into().unwrap(), false, None, None)
            .unwrap();
        assert_eq!(session.start().unwrap().state, "ready");
        let authoring = session.authoring_session().unwrap();
        assert_eq!(
            authoring["principalId"].as_str(),
            Some(checkpoint.principal_id.as_str())
        );
        drop(authoring);
        write_private_new(
            &root.join("blobs"),
            "synthetic-trial-blob",
            b"preserve failed trial",
            4096,
        )
        .unwrap();
        assert_eq!(session.stop().unwrap().state, "stopped");
        service
            .check_prepare_guard(session.drained_transition_guard().unwrap())
            .unwrap();
        // Interrupt after the owner record replacement, with the journal
        // still fencing both ordinary native runtimes.
        replace_private(&root, RECORD, &binding.new_record, 4096).unwrap();
        std::process::exit(98);
    }

    #[test]
    #[ignore = "Launched as the isolated interrupted data exchange child"]
    fn checkpoint_recovery_exit_child() {
        let path = |name| PathBuf::from(std::env::var_os(name).unwrap());
        let root = path("EXHIBITOS_CHECKPOINT_TEST_ROOT");
        let staging = path("EXHIBITOS_CHECKPOINT_TEST_STAGE");
        let profile = path("EXHIBITOS_CHECKPOINT_TEST_PROFILE");
        let key_path = path("EXHIBITOS_CHECKPOINT_TEST_KEY");
        let key = Bytes(crate::profile_backup::external_key(&profile, &key_path).unwrap());
        let key = key.0.as_slice().try_into().unwrap();
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let guard = service.lock().unwrap();
        service.check_prepare_guard(&guard).unwrap();
        crate::native_runtime::check_cold_checkpoint_root(&root).unwrap();
        let pending = read_pending(&root).unwrap();
        let envelope = pending.strip_prefix(PENDING_PREFIX).unwrap();
        let mut checkpoint = service.transition_checkpoint(envelope, key).unwrap();
        assert!(checkpoint.trial);
        assert_eq!(checkpoint.staged, staging);
        prepare_physical_recovery(&root, &mut checkpoint, key).unwrap();
        exchange_data_directories(
            &root,
            checkpoint.restore.as_ref().unwrap(),
            &DATA_DIRECTORIES[..1],
        )
        .unwrap();
        std::process::exit(99);
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    #[test]
    fn catalog39_upgrade_cannot_target_the_earlier_tangent_package() {
        let upgrade = FurnishingTransition::test_upgrade();
        let old_target = crate::native_payload::CATALOG40_TANGENT_TARGET.2;
        assert!(!admitted_pair(upgrade.source().2, old_target));
        assert!(!admitted_pair(old_target, upgrade.source().2));
        assert!(!admitted_pair(upgrade.source().2, upgrade.target().2));
        let target = "bdfc8202d851f9f8983440b9475da3fc3ddc6ee1f0bf4cf2c0d4bf1995dde338";
        assert!(FurnishingTransition::from_manifests((upgrade.source().2, target))
            .is_some_and(FurnishingTransition::upgrades_catalog));
        assert!(!admitted_pair(target, upgrade.source().2));
        assert!(!admitted_pair(old_target, target));
    }

    #[cfg(feature = "workspace-v5-owner-validation")]
    #[test]
    fn transition_scope_requires_pair_saved_proof_and_dedicated_host() {
        let legacy = (OLD_MANIFEST, NEW_MANIFEST);
        let proof = "a".repeat(64);
        assert!(check_transition_host(legacy, None, false).is_ok());
        assert!(check_transition_host(legacy, None, true).is_err());
        for &saved in crate::native_payload::WORKSPACE_V5_SAVED_TRANSITIONS {
            assert!(check_transition_host(saved, Some(&proof), true).is_ok());
            for host in [false, true] {
                assert!(check_transition_host(legacy, Some(&proof), host).is_err());
                assert!(check_transition_host(saved, None, host).is_err());
            }
            assert!(check_transition_host(saved, Some(&proof), false).is_err());
            assert!(check_transition_host(saved, Some("malformed"), true).is_err());
        }
    }

    #[test]
    fn transition_staged_inventory_requires_durable_files_and_nested_directories() {
        let root = std::env::temp_dir().join(format!("transition-sync-{}", Uuid::new_v4()));
        installations::new_directory(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        for name in DATA_DIRECTORIES {
            installations::new_directory(&root.join(name)).unwrap();
        }
        let nested = root.join("blobs/nested");
        installations::new_directory(&nested).unwrap();
        write_private_new(&nested, "asset", b"proof", 128).unwrap();
        for name in [RECORD, ENVIRONMENT] {
            write_private_new(&root, staged_name(name), b"before", 128).unwrap();
        }
        let inode = |path: &Path| fs::metadata(path).unwrap().ino();
        let visited = std::cell::RefCell::new(Vec::new());
        inventory_with_sync(&root, true, MAX_BYTES, |file| {
            visited.borrow_mut().push(file.metadata()?.ino());
            file.sync_all()
        }).unwrap();
        let order = visited.into_inner();
        let position = |path: &Path| order.iter().position(|value| *value == inode(path)).unwrap();
        assert!(position(&nested.join("asset")) < position(&nested));
        assert!(position(&nested) < position(&root.join("blobs")));
        assert!(position(&root.join("blobs")) < position(&root));
        for failure in [inode(&nested.join("asset")), inode(&nested)] {
            assert!(inventory_with_sync(&root, true, MAX_BYTES, |file| {
                if file.metadata()?.ino() == failure {
                    return Err(std::io::Error::other("injected checkpoint flush failure"));
                }
                file.sync_all()
            }).is_err());
            assert!(!root.join(ENVELOPE).exists());
            assert!(!root.join(ENVIRONMENT).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transition_manifest_pairs_reject_cross_pair_reverse_and_unknown() {
        for &(source, target) in TRANSITION_PAIRS {
            assert!(admitted_pair(source, target));
            assert!(!admitted_pair(target, source));
            assert!(!admitted_pair(source, source));
            assert!(!admitted_pair(source, &"0".repeat(64)));
            for &(other_source, other_target) in TRANSITION_PAIRS {
                if source != other_source {
                    assert!(!admitted_pair(source, other_target));
                    assert!(!admitted_pair(other_source, target));
                }
            }
        }
    }

    fn owner_transition(
        host: &mut crate::native_host::Host,
        token: &str,
        transition: Value,
        acknowledged: bool,
    ) -> Result<()> {
        let request = serde_json::from_value(serde_json::json!({
            "version": 1,
            "id": 1,
            "command": "native_maintenance",
            "selection_token": token,
            "maintenance": {
                "action": "local_personal_transition",
                "transition": transition,
                "acknowledged": acknowledged,
            },
        }))
        .unwrap();
        host.handle(request)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        loop {
            let status = host.status(token)?;
            if !status.busy {
                return status
                    .error_code
                    .as_deref()
                    .map_or(Ok(()), |code| Err(err(code)));
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    #[ignore = "Requires exact isolated local payload pair and shared heavy admission"]
    fn cold_personal_checkpoint_authenticates_actual_stopped_database() {
        let payload = |name| {
            Arc::new(Payload::open(&PathBuf::from(std::env::var_os(name).unwrap())).unwrap())
        };
        let old = payload("EXHIBITOS_TEST_SOURCE_PAYLOAD");
        let new = payload("EXHIBITOS_TEST_TARGET_PAYLOAD");
        let parent = std::env::temp_dir().join(format!("personal-checkpoint-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let controller =
            Arc::new(InstallationController::new(parent.join("profile"), None).unwrap());
        let mut host = crate::native_host::Host::new(
            controller.clone(),
            PathBuf::from(std::env::var_os("EXHIBITOS_TEST_TARGET_PAYLOAD").unwrap()),
        );
        let selection = controller.context().unwrap();
        let token = &selection.selection_token;
        let root = PathBuf::from(&selection.installations[0].path);
        let staged = parent.join("checkpoint");
        fs::create_dir(&staged).unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)).unwrap();
        let listeners: Vec<_> = (0..3)
            .map(|_| TcpListener::bind(("127.0.0.1", 0)).unwrap())
            .collect();
        let port = |i: usize| listeners[i].local_addr().unwrap().port();
        let ports = Ports {
            web: port(0),
            api: port(1),
            database: port(2),
        };
        drop(listeners);
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        service.prepare_native_personal(&old, ports).unwrap();
        let key_path = parent.join("checkpoint.key");
        let wrong_key_path = parent.join("wrong-checkpoint.key");
        let key = {
            controller.create_backup_key(token, &key_path).unwrap();
            controller
                .create_backup_key(token, &wrong_key_path)
                .unwrap();
            Bytes(crate::profile_backup::external_key(&parent.join("profile"), &key_path).unwrap())
        };
        let mut session =
            crate::native_runtime::Session::new(&service, old.clone(), ports).unwrap();
        assert_eq!(session.start().unwrap().state, "ready");
        assert_eq!(
            controller
                .authenticate_local_personal_checkpoint(token, &old, &new, &staged, &key_path)
                .unwrap_err()
                .code,
            "BUSY"
        );
        let stop = session.stop().unwrap();
        assert_eq!(stop.state, "stopped");
        for name in ["postgres", "node", "auxiliary"] {
            assert_eq!(stop.exits.get(name), Some(&0));
        }
        drop(session);

        for name in ["blobs", "config"] {
            let path = root.join(name);
            if !path.exists() {
                fs::create_dir(&path).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        for name in MEMBERS {
            let target = staged.join(staged_name(name));
            assert!(
                Command::new("/bin/cp")
                    .arg("-cR")
                    .arg(root.join(name))
                    .arg(target)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let empty = staged.join("blobs/extra-empty");
        fs::create_dir(&empty).unwrap();
        fs::set_permissions(&empty, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            controller
                .authenticate_local_personal_checkpoint(token, &old, &new, &staged, &key_path)
                .is_err()
        );
        assert!(!staged.join(ENVELOPE).exists());
        fs::remove_dir(&empty).unwrap();
        controller
            .authenticate_local_personal_checkpoint(token, &old, &new, &staged, &key_path)
            .unwrap();
        let envelope = fs::read(staged.join(ENVELOPE)).unwrap();
        let checkpoint = open(&envelope, key.0.as_slice().try_into().unwrap()).unwrap();
        assert_eq!(checkpoint.files, inventory(&root, false).unwrap());
        assert_eq!(checkpoint.files, inventory(&staged, true).unwrap());
        assert!(!staged.join(RECORD).exists());
        assert!(!staged.join(ENVIRONMENT).exists());
        let mut corrupt = envelope.clone();
        corrupt[15] ^= 1;
        assert!(open(&corrupt, key.0.as_slice().try_into().unwrap()).is_err());
        assert!(open(&corrupt, &[0; 32]).is_err());
        assert!(
            controller
                .authenticate_local_personal_checkpoint(token, &old, &new, &staged, &key_path)
                .is_err()
        );
        let mut transfer = controller.native_oex_import(token).unwrap();
        transfer.intent = Some(crate::installations::native_oex_import::Intent {
            request_id: Uuid::new_v4().to_string(),
            sha256: digest(b"synthetic OEX"),
            bytes: 13,
            profile: None,
            oex_format: None,
        });
        let mut transfer = controller.save_native_oex_import(token, transfer).unwrap();
        assert_eq!(
            controller
                .stage_local_personal_runtime_transition(token, &old, &new, &staged, &key_path)
                .unwrap_err()
                .code,
            "NATIVE_OEX_IMPORT_PENDING"
        );
        assert_eq!(inventory(&root, false).unwrap(), checkpoint.files);
        transfer.intent = None;
        controller.save_native_oex_import(token, transfer).unwrap();
        for environment_changed in [false, true] {
            if environment_changed {
                controller
                    .stage_local_personal_runtime_transition(token, &old, &new, &staged, &key_path)
                    .unwrap();
            } else {
                let status = Command::new(std::env::current_exe().unwrap())
                    .args(["checkpoint_journal_crash_child", "--ignored"])
                    .env("EXHIBITOS_CHECKPOINT_TEST_ROOT", &root)
                    .env("EXHIBITOS_CHECKPOINT_TEST_STAGE", &staged)
                    .status()
                    .unwrap();
                assert_eq!(status.code(), Some(97));
            }
            for payload in [&old, &new] {
                assert!(service.personal_environment(payload, ports).is_err());
                assert!(
                    crate::native_runtime::Session::new(&service, payload.clone(), ports).is_err()
                );
            }
            assert!(
                controller
                    .recover_local_personal_runtime_transition(token, &staged, &wrong_key_path, false, None)
                    .is_err()
            );
            assert!(
                fs::read(root.join(ENVIRONMENT))
                    .unwrap()
                    .starts_with(PENDING_PREFIX)
            );
            if !environment_changed {
                let original_bytes = checkpoint
                    .files
                    .values()
                    .map(|entry| entry.bytes)
                    .sum::<u64>();
                let pending_bytes = fs::metadata(root.join(ENVIRONMENT)).unwrap().len();
                assert!(inventory_with_limit(&root, false, original_bytes).is_err());
                assert!(inventory_with_limit(&root, false, original_bytes + pending_bytes).is_ok());
            }
            controller
                .recover_local_personal_runtime_transition(token, &staged, &key_path, false, None)
                .unwrap();
            assert_eq!(inventory(&root, false).unwrap(), checkpoint.files);
            assert!(service.personal_environment(&old, ports).is_ok());
        }
        controller
            .stage_local_personal_runtime_transition(token, &old, &new, &staged, &key_path)
            .unwrap();
        let identity = service.personal_identity().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["checkpoint_trial_exit_child", "--ignored"])
            .env("EXHIBITOS_CHECKPOINT_TEST_ROOT", &root)
            .env("EXHIBITOS_CHECKPOINT_TEST_STAGE", &staged)
            .env("EXHIBITOS_CHECKPOINT_TEST_PROFILE", parent.join("profile"))
            .env("EXHIBITOS_CHECKPOINT_TEST_KEY", &key_path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(98));
        let changed = inventory(&root, false).unwrap();
        assert!(
            checkpoint
                .files
                .iter()
                .any(|(path, entry)| path.starts_with("native-database")
                    && changed.get(path) != Some(entry))
        );
        assert!(root.join("blobs/synthetic-trial-blob").exists());
        for payload in [&old, &new] {
            assert!(crate::native_runtime::Session::new(&service, payload.clone(), ports).is_err());
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["checkpoint_recovery_exit_child", "--ignored"])
            .env("EXHIBITOS_CHECKPOINT_TEST_ROOT", &root)
            .env("EXHIBITOS_CHECKPOINT_TEST_STAGE", &staged)
            .env("EXHIBITOS_CHECKPOINT_TEST_PROFILE", parent.join("profile"))
            .env("EXHIBITOS_CHECKPOINT_TEST_KEY", &key_path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(99));
        assert!(root.join("blobs/synthetic-trial-blob").exists());
        let recover = serde_json::json!({"phase":"recover", "staging":staged, "keyFile":key_path});
        assert_eq!(
            owner_transition(&mut host, token, recover.clone(), false)
                .unwrap_err()
                .code,
            "INSTALLATION_SELECTION_ACK_REQUIRED"
        );
        owner_transition(&mut host, token, recover, true).unwrap();
        assert_eq!(inventory(&root, false).unwrap(), checkpoint.files);
        let retained: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("native-personal-transition-retained-")
            })
            .collect();
        assert_eq!(retained.len(), 1);
        assert_eq!(
            fs::read(retained[0].path().join("blobs/synthetic-trial-blob")).unwrap(),
            b"preserve failed trial"
        );
        let retained_files = inventory(&retained[0].path(), true).unwrap();
        assert_eq!(
            changed
                .iter()
                .filter(|(path, _)| path.starts_with("native-database"))
                .collect::<Vec<_>>(),
            retained_files
                .iter()
                .filter(|(path, _)| path.starts_with("native-database"))
                .collect::<Vec<_>>()
        );
        owner_transition(&mut host, token, serde_json::json!({
            "phase":"stage", "staging":staged, "keyFile":key_path,
            "sourcePayload":PathBuf::from(std::env::var_os("EXHIBITOS_TEST_SOURCE_PAYLOAD").unwrap()),
        }), true).unwrap();
        owner_transition(
            &mut host,
            token,
            serde_json::json!({"phase":"activate", "staging":staged, "keyFile":key_path}),
            true,
        )
        .unwrap();
        assert_eq!(service.personal_identity().unwrap(), identity);
        assert!(crate::native_runtime::Session::new(&service, old, ports).is_err());
        let mut active = crate::native_runtime::Session::new(&service, new, ports).unwrap();
        assert_eq!(active.start().unwrap().state, "ready");
        let stop = active.stop().unwrap();
        assert_eq!(stop.state, "stopped");
        for name in ["postgres", "node", "auxiliary"] {
            assert_eq!(stop.exits.get(name), Some(&0));
        }
        println!(
            "verified cold synthetic checkpoint and exact target readiness: {} entries",
            checkpoint.files.len()
        );
    }
}
