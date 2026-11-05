// SPDX-License-Identifier: Apache-2.0
//! Authenticated inventory of an already staged, non-runnable cold copy.
//! Staged data remains plaintext in a private directory; only the inventory is sealed.
//! Pending binding changes block normal startup until authenticated recovery.
use super::*;
use crate::installations::InstallationController;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload as CipherPayload, rand_core::RngCore},
};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::sync::Arc;

const OLD_MANIFEST: &str = "253de8963ec9f0e0a2884a27d8e756b453cf61899e38c31571d9993a7d6ecff4";
const NEW_MANIFEST: &str = "10a21f3b7a73a66011e077f7927f951f917e604720591acbcc2cdb47222f4032";
// Development-only transitions are admitted as directed, exact pairs. Keep
// the historical recovery path available alongside the reviewed Oak successor.
const TRANSITION_PAIRS: &[(&str, &str)] = &[
    (OLD_MANIFEST, NEW_MANIFEST),
    ("35b07cfc4c89abeaab518fd72c5be5f55569c03ae99906ff7220edf7d9712bf8",
     "9b9d6b1cba4b5722a8710471283e9f693f14883f5da950c4504ca7bbf00efeb7"),
];
fn admitted_pair(source: &str, target: &str) -> bool {
    TRANSITION_PAIRS.contains(&(source, target))
}

const DOMAIN: &[u8] = b"ExhibitOS-local-personal-physical-checkpoint-v1\0";
const ENVELOPE: &str = "physical-checkpoint.gcm";
const MAX_FILES: usize = 50_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
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
    trial: bool,
    drained_failure: Option<String>,
    restore: Option<RestorePlan>,
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

fn inventory(root: &Path, staged: bool) -> Result<BTreeMap<PathBuf, Entry>> {
    inventory_with_limit(root, staged, MAX_BYTES)
}

fn inventory_with_limit(
    root: &Path,
    staged: bool,
    max_bytes: u64,
) -> Result<BTreeMap<PathBuf, Entry>> {
    let root_pin = crate::maintenance_source::DirectoryPin::open(root)?;
    let mut files = BTreeMap::new();
    let mut todo: Vec<_> = MEMBERS.iter().map(|name| PathBuf::from(name)).collect();
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
            file.sync_all().map_err(|_| invalid())?;
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
    for pin in directories.iter().rev() {
        if staged {
            pin.try_clone_file()?.sync_all().map_err(|_| invalid())?;
        }
        pin.check()?;
    }
    if staged {
        root_pin
            .try_clone_file()?
            .sync_all()
            .map_err(|_| invalid())?;
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
    let mut live = inventory_with_limit(root, false, MAX_BYTES + pending.len() as u64)?;
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
    for (relative, entry) in &checkpoint.files {
        let name = relative.to_str().ok_or_else(invalid)?;
        let archived = Path::new(staged_name(name));
        let destination = copy.join(archived);
        if entry.sha256.is_none() {
            installations::new_directory(&destination)?;
            fs::set_permissions(&destination, fs::Permissions::from_mode(entry.mode))
                .map_err(|_| invalid())?;
            continue;
        }
        let source = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(checkpoint.staged.join(archived))
            .map_err(|_| invalid())?;
        let meta = source.metadata().map_err(|_| invalid())?;
        if !meta.is_file() || meta.nlink() != 1 || meta.len() != entry.bytes {
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
    if inventory(&copy, true)? != checkpoint.files {
        return Err(invalid());
    }
    sync_root(root)?;
    Ok(copy)
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
            || inventory(staging, true)? != checkpoint.files
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
    ) -> Result<()> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        if !admitted_pair(&source.context().manifest_sha256, &target.context().manifest_sha256) {
            return Err(invalid());
        }
        source.recheck()?;
        target.recheck()?;
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
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
            || inventory(&self.root, false)? != checkpoint.files
        {
            return Err(invalid());
        }
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
    ) -> Result<()> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        let mut pending = read_pending(&self.root)?;
        let mut envelope = pending
            .strip_prefix(PENDING_PREFIX)
            .ok_or_else(invalid)?
            .to_vec();
        let mut checkpoint = self.transition_checkpoint(&envelope, key)?;
        if checkpoint.staged != staging {
            return Err(invalid());
        }
        crate::native_runtime::check_cold_transition_root(
            &self.root,
            checkpoint.drained_failure.as_deref(),
        )?;
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
        replace_private(&self.root, ENVIRONMENT, &binding.old_env.0, 4096)?;
        if inventory(&self.root, false)? != checkpoint.files {
            return Err(invalid());
        }
        Ok(())
    }

    fn trial_local_personal_runtime(
        &self,
        target: Arc<Payload>,
        staging: &Path,
        key: &[u8; 32],
    ) -> Result<(
        crate::native_runtime::Session,
        Checkpoint,
        Bindings,
        Vec<u8>,
    )> {
        let guard = self.lock()?;
        self.check_prepare_guard(&guard)?;
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
        let target_context = target.context();
        if !TRANSITION_PAIRS.iter().any(|(_, target)| *target == target_context.manifest_sha256)
            || target_context.official_signed
            || target_context.redistribution_qualified
        {
            return Err(invalid());
        }
        target.recheck()?;
        let pending = read_pending(&self.root)?;
        let envelope = pending.strip_prefix(PENDING_PREFIX).ok_or_else(invalid)?;
        let mut checkpoint = self.transition_checkpoint(envelope, key)?;
        if checkpoint.new_manifest != target_context.manifest_sha256
            || checkpoint.trial || checkpoint.staged != staging {
            return Err(invalid());
        }
        verify_unchanged(&self.root, &checkpoint, &pending)?;
        let binding = bindings(&checkpoint)?;
        verify_beforeimage(&checkpoint, ENVIRONMENT, &binding.old_env.0)?;
        verify_beforeimage(&checkpoint, RECORD, &binding.old_record)?;
        let proposed: Personal =
            serde_json::from_slice(&binding.new_record).map_err(|_| invalid())?;
        let fields = personal_environment_fields(&proposed, &binding.new_env.0)?;
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
        Ok((session, checkpoint, binding, envelope))
    }

    fn activate_local_personal_runtime_transition(
        &self,
        target: Arc<Payload>,
        staging: &Path,
        key: &[u8; 32],
    ) -> Result<()> {
        let (mut session, mut checkpoint, binding, envelope) =
            self.trial_local_personal_runtime(target, staging, key)?;
        let qualified = (|| {
            if session.start()?.state != "ready" {
                return Err(invalid());
            }
            let authoring = session.authoring_session()?;
            if authoring["tenantId"].as_str() != Some(checkpoint.tenant_id.as_str())
                || authoring["principalId"].as_str() != Some(checkpoint.principal_id.as_str())
            {
                return Err(invalid());
            }
            Ok(())
        })();
        let stop = session.stop()?;
        self.check_prepare_guard(session.drained_transition_guard()?)?;
        if let Err(error) = qualified {
            if stop.state == "failed" {
                checkpoint.drained_failure = stop.job_id;
                crate::native_runtime::check_cold_transition_root(
                    &self.root,
                    checkpoint.drained_failure.as_deref(),
                )?;
                let drained = seal(&checkpoint, key)?;
                publish_pending(&self.root, &drained)?;
            }
            return Err(error);
        }
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
        let pending = read_pending(&self.root)?;
        if pending.strip_prefix(PENDING_PREFIX) != Some(envelope.as_slice()) {
            return Err(invalid());
        }
        self.transition_checkpoint(&envelope, key)?;
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
        source_payload.recheck()?;
        target_payload.recheck()?;
        installations::private_directory(staging)?;
        let staging_pin = crate::maintenance_source::DirectoryPin::open(staging)?;
        // Staged beforeimages have backup names, so there is no runnable
        // personal configuration at the staged root.
        check_staged_names(staging, false)?;
        crate::native_runtime::check_cold_checkpoint_root(&self.root)?;
        if present(staging, "native-database/postmaster.pid")? {
            return Err(invalid());
        }
        let record = read(self)?;
        let mut fields = self.personal_environment(source_payload, record.ports)?;
        for value in fields.values_mut() {
            std::mem::take(value).into_bytes().fill(0);
        }
        let files = inventory(&self.root, false)?;
        if files != inventory(staging, true)? {
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
            trial: false,
            drained_failure: None,
            restore: None,
        };
        let envelope = seal(&checkpoint, key)?;
        staging_pin.check()?;
        self.check_prepare_guard(&guard)?;
        if checkpoint.files != inventory(&self.root, false)?
            || checkpoint.files != inventory(staging, true)?
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
    fn with_transition_key(
        &self,
        token: &str,
        key_path: &Path,
        staging: &Path,
        task: impl FnOnce(&LifecycleService, &[u8; 32]) -> Result<()>,
    ) -> Result<()> {
        if key_path.starts_with(staging) {
            return Err(invalid());
        }
        self.with_personal_transition(token, |service, profile| {
            if key_path.starts_with(&service.root) {
                return Err(invalid());
            }
            let key = Bytes(crate::profile_backup::external_key(profile, key_path)?);
            task(service, key.0.as_slice().try_into().map_err(|_| invalid())?)
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
        self.with_transition_key(token, key_path, staging, |service, key| {
            if !present(staging, ENVELOPE)? {
                service.authenticate_local_personal_checkpoint(source, target, staging, key)?;
            }
            service.stage_local_personal_runtime_transition(source, target, staging, key)
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
        self.with_transition_key(token, key_path, staging, |service, key| {
            service.authenticate_local_personal_checkpoint(source, target, staging, key)
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
        self.with_transition_key(token, key_path, staging, |service, key| {
            service.stage_local_personal_runtime_transition(source, target, staging, key)
        })
    }

    pub(crate) fn activate_local_personal_runtime_transition(
        &self,
        token: &str,
        target: Arc<Payload>,
        staging: &Path,
        key_path: &Path,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, |service, key| {
            service.activate_local_personal_runtime_transition(target, staging, key)
        })
    }

    pub(crate) fn recover_local_personal_runtime_transition(
        &self,
        token: &str,
        staging: &Path,
        key_path: &Path,
    ) -> Result<()> {
        self.with_transition_key(token, key_path, staging, |service, key| {
            service.recover_local_personal_runtime_transition(staging, key)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .trial_local_personal_runtime(target, &staging, key.0.as_slice().try_into().unwrap())
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
                    .recover_local_personal_runtime_transition(token, &staged, &wrong_key_path)
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
                .recover_local_personal_runtime_transition(token, &staged, &key_path)
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
