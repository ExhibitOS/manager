// SPDX-License-Identifier: Apache-2.0
//! App-closed orchestration of the existing full signed-update preparation.
//! Jobs are native-derived siblings of the captured profile, never exclusions
//! from host comparison. Saved metadata is not a signature or execution permit.
use super::*;
use crate::signed_release::trust::{
    CheckpointInputs, MigrationRuntimeInputs, RecoveryRuntimeInputs, Store,
};
pub use crate::update_preparation_inputs::Inspection;
use crate::update_preparation_inputs::{self as inputs, Inputs};
use std::io::Seek;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
const LIMIT: usize = 16384;
const MAX_JOBS: usize = 128;
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    AwaitingAppClose,
    Running,
    Prepared,
    Failed,
    Interrupted,
    CancelledBeforeStart,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    ValidatingInputs,
    OriginalBackup,
    PreparingPlan,
    RestoringCandidate,
    StoppingCandidate,
    Checkpoint,
    FullQualification,
    Registered,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub installation_id: String,
    pub state: State,
    pub phase: Phase,
    pub code: Option<String>,
    pub reference_id: Option<String>,
    pub updated_at: u64,
    pub can_cancel: bool,
    pub can_retry: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    pub controller_blocked: bool,
    pub job: Option<Receipt>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub release_folder: PathBuf,
    pub key_file: PathBuf,
    pub preserve_data: bool,
    pub external_writers_quiesced: bool,
    pub close_app_and_prepare: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    id: String,
    installation_id: String,
    scope: String,
    registry_sha256: String,
    source_root: PathBuf,
    inputs: Inputs,
    created_at: u64,
}
fn invalid() -> LifecycleError {
    err("UPDATE_PREPARATION_INVALID")
}
fn persist<T: Serialize>(root: &Path, name: &str, v: &T) -> Result<()> {
    let bytes = serde_json::to_vec(v).map_err(|_| invalid())?;
    write_private_new(root, name, &bytes, LIMIT)?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("UPDATE_PREPARATION_UNCERTAIN"))
}
fn read<T: for<'a> Deserialize<'a>>(root: &Path, name: &str) -> Result<T> {
    serde_json::from_slice(&installation_backup::source_bytes(
        root,
        name,
        LIMIT as u64,
        true,
    )?)
    .map_err(|_| invalid())
}
/// Native profile identity only; there is no caller-selected work directory.
fn directory(profile: &Path, create: bool) -> Result<Option<PathBuf>> {
    if !profile.is_absolute()
        || profile
            .to_str()
            .is_none_or(|s| s.len() > 2048 || s.chars().any(char::is_control))
    {
        return Err(invalid());
    }
    let parent = profile.parent().ok_or_else(invalid)?;
    if !create
        && fs::symlink_metadata(parent).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    if fs::canonicalize(parent).ok().as_deref() != Some(parent) {
        return Err(invalid());
    }
    if profile.exists() && fs::canonicalize(profile).ok().as_deref() != Some(profile) {
        return Err(invalid());
    }
    let name = format!(
        ".exhibitos-update-preparation-{}",
        digest(profile.as_os_str().as_encoded_bytes())
    );
    let root = parent.join(name);
    match fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            installations::new_directory(&root)?;
        }
        Err(_) => return Err(invalid()),
        Ok(_) => installations::private_directory(&root)?,
    }
    if root.starts_with(profile) {
        return Err(invalid());
    }
    Ok(Some(root))
}
fn jobs(root: &Path) -> Result<Vec<String>> {
    let root = root.join("jobs");
    if !root.exists() {
        return Ok(Vec::new());
    }
    installations::private_directory(&root)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|_| invalid())?
        .take(MAX_JOBS + 1)
    {
        let entry = entry.map_err(|_| invalid())?;
        let name = entry.file_name().into_string().map_err(|_| invalid())?;
        if !installations::uuid(&name) {
            return Err(invalid());
        }
        installations::private_directory(&entry.path())?;
        out.push(name);
        if out.len() > MAX_JOBS {
            return Err(err("UPDATE_PREPARATION_QUOTA"));
        }
    }
    Ok(out)
}
fn workspace(profile: &Path, id: &str) -> Result<PathBuf> {
    if !installations::uuid(id) {
        return Err(invalid());
    }
    let root = directory(profile, false)?
        .ok_or_else(invalid)?
        .join("jobs")
        .join(id);
    installations::private_directory(&root)?;
    Ok(root)
}
fn request(root: &Path, id: &str) -> Result<Request> {
    let r: Request = read(root, "request.json")?;
    if r.id != id
        || !installations::uuid(&r.installation_id)
        || !hash_valid(&r.registry_sha256)
        || r.created_at == 0
        || r.created_at > 8_640_000_000_000_000
    {
        return Err(invalid());
    }
    Ok(r)
}
fn claim(root: &Path) -> Result<File> {
    let path = root.join("claim.lock");
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|_| invalid())?;
    let m = f.metadata().map_err(|_| invalid())?;
    let n = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o7777 != 0o600
        || (m.dev(), m.ino()) != (n.dev(), n.ino())
    {
        return Err(invalid());
    }
    f.try_lock_exclusive()
        .map_err(|_| err("UPDATE_PREPARATION_BUSY"))?;
    Ok(f)
}
fn valid_receipt(v: &Receipt, r: &Request) -> bool {
    let safe = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.as_bytes()[0].is_ascii_uppercase()
            && s.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    };
    if v.id != r.id
        || v.installation_id != r.installation_id
        || v.updated_at < r.created_at
        || v.updated_at > 8_640_000_000_000_000
        || v.can_retry
    {
        return false;
    }
    match v.state {
        State::Prepared => {
            v.phase == Phase::Registered
                && v.reference_id.as_deref().is_some_and(installations::uuid)
                && v.code.is_none()
                && !v.can_cancel
        }
        State::CancelledBeforeStart => {
            v.phase == Phase::ValidatingInputs
                && v.reference_id.is_none()
                && v.code.is_none()
                && !v.can_cancel
        }
        State::Failed | State::Interrupted => {
            v.reference_id.is_none() && v.code.as_deref().is_some_and(safe) && !v.can_cancel
        }
        State::AwaitingAppClose | State::Running => {
            v.reference_id.is_none()
                && v.code.is_none()
                && (!v.can_cancel || v.state == State::AwaitingAppClose)
        }
    }
}
fn owner_active(root: &Path) -> Result<bool> {
    match claim(root) {
        Ok(_) => Ok(false),
        Err(e) if e.code == "UPDATE_PREPARATION_BUSY" => Ok(true),
        Err(e) => Err(e),
    }
}
fn projection(root: &Path, r: &Request) -> Result<Receipt> {
    if root.join("outcome.json").exists() {
        let value: Receipt = read(root, "outcome.json")?;
        if !valid_receipt(&value, r)
            || matches!(value.state, State::Running | State::AwaitingAppClose)
        {
            return Err(invalid());
        }
        return Ok(value);
    }
    let mut last = Receipt {
        id: r.id.clone(),
        installation_id: r.installation_id.clone(),
        state: State::AwaitingAppClose,
        phase: Phase::ValidatingInputs,
        code: None,
        reference_id: None,
        updated_at: r.created_at,
        can_cancel: true,
        can_retry: false,
    };
    for (i, phase) in [
        Phase::ValidatingInputs,
        Phase::OriginalBackup,
        Phase::PreparingPlan,
        Phase::RestoringCandidate,
        Phase::StoppingCandidate,
        Phase::Checkpoint,
        Phase::FullQualification,
        Phase::Registered,
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("phase-{i}.json");
        if root.join(&name).exists() {
            let v: Receipt = read(root, &name)?;
            if !valid_receipt(&v, r)
                || v.phase != phase
                || v.state != State::Running
                || v.can_cancel
            {
                return Err(invalid());
            }
            last = v;
        }
    }
    let started = root.join("started.json").exists();
    if started {
        let marker: String = read(root, "started.json")?;
        if marker != r.id {
            return Err(invalid());
        }
        last.state = State::Running;
        last.can_cancel = false;
    }
    match claim(root) {
        Ok(_) => {
            if started {
                last.state = State::Interrupted;
                last.code = Some("UPDATE_PREPARATION_RECONCILE_REQUIRED".into());
            }
        }
        Err(e) if e.code == "UPDATE_PREPARATION_BUSY" => {
            last.state = State::Running;
            last.can_cancel = false;
        }
        Err(e) => return Err(e),
    }
    Ok(last)
}
pub fn startup(profile: &Path) -> Result<Startup> {
    let Some(root) = directory(profile, false)? else {
        return Ok(Startup {
            controller_blocked: false,
            job: None,
        });
    };
    let mut latest = None;
    let mut blocked = None;
    for id in jobs(&root)? {
        let work = root.join("jobs").join(&id);
        if !work.join("request.json").exists() {
            return Err(invalid());
        }
        let r = request(&work, &id)?;
        let value = projection(&work, &r)?;
        let active = matches!(
            value.state,
            State::AwaitingAppClose | State::Running | State::Interrupted
        ) || owner_active(&work)?;
        if active {
            if blocked.is_some() {
                return Err(err("UPDATE_PREPARATION_AMBIGUOUS"));
            }
            blocked = Some(value.clone());
        }
        if latest
            .as_ref()
            .is_none_or(|v: &Receipt| v.updated_at < value.updated_at)
        {
            latest = Some(value);
        }
    }
    Ok(Startup {
        controller_blocked: blocked.is_some(),
        job: blocked.or(latest),
    })
}
fn progress(root: &Path, r: &Request, phase: Phase) -> Result<()> {
    let i = match phase {
        Phase::ValidatingInputs => 0,
        Phase::OriginalBackup => 1,
        Phase::PreparingPlan => 2,
        Phase::RestoringCandidate => 3,
        Phase::StoppingCandidate => 4,
        Phase::Checkpoint => 5,
        Phase::FullQualification => 6,
        Phase::Registered => 7,
    };
    persist(
        root,
        &format!("phase-{i}.json"),
        &Receipt {
            id: r.id.clone(),
            installation_id: r.installation_id.clone(),
            state: State::Running,
            phase,
            code: None,
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    )
}
pub fn cancel(profile: &Path, id: &str) -> Result<Startup> {
    let root = workspace(profile, id)?;
    let _claim = claim(&root)?;
    let r = request(&root, id)?;
    if root.join("started.json").exists() || root.join("outcome.json").exists() {
        return Err(err("UPDATE_PREPARATION_CANCEL_UNAVAILABLE"));
    }
    persist(
        &root,
        "outcome.json",
        &Receipt {
            id: r.id,
            installation_id: r.installation_id,
            state: State::CancelledBeforeStart,
            phase: Phase::ValidatingInputs,
            code: None,
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    )?;
    drop(_claim);
    startup(profile)
}
pub fn launch_failed(profile: &Path, id: &str) -> Result<()> {
    let root = workspace(profile, id)?;
    let _claim = claim(&root)?;
    let r = request(&root, id)?;
    if root.join("started.json").exists() || root.join("outcome.json").exists() {
        return Err(invalid());
    }
    persist(
        &root,
        "outcome.json",
        &Receipt {
            id: r.id,
            installation_id: r.installation_id,
            state: State::Failed,
            phase: Phase::ValidatingInputs,
            code: Some("UPDATE_PREPARATION_LAUNCH_FAILED".into()),
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    )
}
fn child(parent: &Path, name: &str) -> Result<PathBuf> {
    installations::private_directory(parent)?;
    let path = parent.join(name);
    installations::new_directory(&path)?;
    Ok(path)
}
/// Read-only preliminary admission mirrors existing source_stopped::observe.
/// Actual stopped-container and full inventory checks still run later.
pub(crate) fn source_eligible(source: &LifecycleService) -> Result<()> {
    let _guard = source.lock()?;
    let manifest = source.manifest()?;
    let installed: BundleManifest = read_json(&checked_path(&source.root, "installed.json")?)?;
    if serde_json::to_value(&manifest).ok() != serde_json::to_value(&installed).ok() {
        return Err(err("UPDATE_SOURCE_CHANGED"));
    }
    if manifest.services.len() != 2
        || !manifest.services.iter().any(|s| s == "platform")
        || !manifest.services.iter().any(|s| s == "database")
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    if source.engine(&manifest, false)? != "docker" {
        return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
    }
    Ok(())
}
/// The existing backup producer requires a running database. Report that actual
/// prerequisite before closing the app; never start a service implicitly here.
fn source_readable(source:&LifecycleService)->Result<()> {
    let _guard=source.lock()?;
    let status=source.status()?;
    if status.state=="runtime_unavailable"{return Err(err("ENGINE_UNAVAILABLE"));}
    if !status.services.iter().any(|service|service.name=="database"&&service.state=="running"){
        return Err(err("UPDATE_PREPARATION_START_REQUIRED"));
    }
    Ok(())
}
pub(crate) fn inspect(
    profile: &Path,
    scope: &str,
    source: &Path,
    folder: &Path,
    key_file: &Path,
) -> Result<(Inputs, crate::signed_release::Release)> {
    source_readable(&LifecycleService::open_retry_diagnostics(source.into())?)?;
    let _key = inputs::key(key_file, profile, source)?;
    let root = directory(profile, true)?.ok_or_else(invalid)?;
    let inspections = root.join("inspections");
    if !inspections.exists() {
        installations::new_directory(&inspections)?;
    }
    installations::private_directory(&inspections)?;
    if fs::read_dir(&inspections)
        .map_err(|_| invalid())?
        .take(MAX_JOBS)
        .count()
        >= MAX_JOBS
    {
        return Err(err("UPDATE_PREPARATION_QUOTA"));
    }
    let scratch = child(&inspections, &Uuid::new_v4().to_string())?;
    // Read-only Store entry; no recovered intent, provisioned key or trust mutation.
    crate::signed_release::trust::inspect_preparation_inputs(
        profile, scope, folder, key_file, &scratch,
    )
}
pub(crate) fn enqueue(
    profile: &Path,
    installation: &str,
    scope: &str,
    registry: &[u8],
    source: &Path,
    input: Input,
) -> Result<Receipt> {
    if !input.preserve_data || !input.external_writers_quiesced || !input.close_app_and_prepare {
        return Err(err("UPDATE_PREPARATION_CONSENT_REQUIRED"));
    }
    if startup(profile)?.controller_blocked {
        return Err(err("UPDATE_PREPARATION_BUSY"));
    }
    let (inputs, _) = inspect(
        profile,
        scope,
        source,
        &input.release_folder,
        &input.key_file,
    )?;
    let root = directory(profile, true)?.ok_or_else(invalid)?;
    let jobs_dir = root.join("jobs");
    if !jobs_dir.exists() {
        installations::new_directory(&jobs_dir)?;
    }
    installations::private_directory(&jobs_dir)?;
    if jobs(&root)?.len() >= MAX_JOBS {
        return Err(err("UPDATE_PREPARATION_QUOTA"));
    }
    let id = Uuid::new_v4().to_string();
    let work = child(&root, &format!("pending-{id}"))?;
    let r = Request {
        id: id.clone(),
        installation_id: installation.into(),
        scope: scope.into(),
        registry_sha256: digest(registry),
        source_root: source.into(),
        inputs,
        created_at: now(),
    };
    persist(&work, "request.json", &r)?;
    write_private_new(&work, "claim.lock", b"", 0)?;
    fs::rename(&work, jobs_dir.join(&id)).map_err(|_| err("UPDATE_PREPARATION_UNCERTAIN"))?;
    File::open(&jobs_dir)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("UPDATE_PREPARATION_UNCERTAIN"))?;
    Ok(Receipt {
        id,
        installation_id: installation.into(),
        state: State::AwaitingAppClose,
        phase: Phase::ValidatingInputs,
        code: None,
        reference_id: None,
        updated_at: r.created_at,
        can_cancel: true,
        can_retry: false,
    })
}
/// Native resource adapter supplies only this app's fixed verified resources.
/// Never deserialize a runtime path/hash/image from a preparation request.
pub struct Runtime {
    python: PathBuf,
    python_pin: ExecutablePin,
    resources: PathBuf,
    image: String,
}
impl Runtime {
    pub fn from_native_resources(
        resources: &Path,
        python: &Path,
        python_sha256: &str,
    ) -> Result<Self> {
        if !hash_valid(python_sha256)
            || !python.is_absolute()
            || fs::canonicalize(python).ok().as_deref() != Some(python)
            || !fs::metadata(python).is_ok_and(|m| m.is_file())
        {
            return Err(err("UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"));
        }
        let image = runtime_prepare::maintenance_image(resources)?
            .ok_or_else(|| err("BACKUP_TOOL_UNAVAILABLE"))?;
        let python_pin = ExecutablePin::open(python, python_sha256)?;
        let value = Self {
            python: python.into(),
            python_pin,
            resources: resources.into(),
            image,
        };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        self.python_pin.check(&self.python)?;
        if runtime_prepare::maintenance_image(&self.resources)?.as_deref() != Some(&self.image) {
            return Err(err("UPDATE_PREPARATION_RUNTIME_CHANGED"));
        }
        Ok(())
    }
}
struct ExecutablePin {
    file: File,
    metadata: fs::Metadata,
}
fn executable_identity(
    m: &fs::Metadata,
) -> (u64, u64, u32, u32, u32, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.nlink(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
impl ExecutablePin {
    fn open(path: &Path, expected: &str) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| err("UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"))?;
        let metadata = file.metadata().map_err(|_| invalid())?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() == 0
            || metadata.len() > 150 * 1024 * 1024
            || metadata.mode() & 0o022 != 0
            || metadata.mode() & 0o111 == 0
            || ![0, unsafe { libc::geteuid() }].contains(&metadata.uid())
        {
            return Err(err("UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"));
        }
        let mut hash = Sha256::new();
        let mut count = 0;
        let mut bytes = [0; 65536];
        loop {
            let n = file.read(&mut bytes).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > metadata.len() {
                return Err(invalid());
            }
            hash.update(&bytes[..n]);
        }
        if count != metadata.len() || format!("{:x}", hash.finalize()) != expected {
            return Err(err("UPDATE_PREPARATION_RUNTIME_CHANGED"));
        }
        let value = Self { file, metadata };
        value.check(path)?;
        Ok(value)
    }
    fn check(&self, path: &Path) -> Result<()> {
        let current = fs::symlink_metadata(path).map_err(|_| invalid())?;
        let after = self.file.metadata().map_err(|_| invalid())?;
        if executable_identity(&self.metadata) != executable_identity(&after)
            || executable_identity(&self.metadata) != executable_identity(&current)
            || fs::canonicalize(path).ok().as_deref() != Some(path)
        {
            return Err(err("UPDATE_PREPARATION_RUNTIME_CHANGED"));
        }
        Ok(())
    }
}
struct KeyBytes([u8; 32]);
impl Drop for KeyBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
fn bound_source(profile: &Path, r: &Request, exact_registry: bool) -> Result<LifecycleService> {
    let (registry, raw) = installations::load(profile)?.ok_or_else(invalid)?;
    if exact_registry && digest(&raw) != r.registry_sha256
        || registry.active_id != r.installation_id
    {
        return Err(err("INSTALLATION_SELECTION_CHANGED"));
    }
    let entry = registry
        .installations
        .iter()
        .find(|e| e.id == r.installation_id)
        .ok_or_else(invalid)?;
    if installations::root(profile, entry) != r.source_root
        || !(r.scope == "default" || installations::uuid(&r.scope))
    {
        return Err(err("INSTALLATION_SELECTION_CHANGED"));
    }
    LifecycleService::open_retry_diagnostics(r.source_root.clone())
}
fn pin_inputs(r: &Request) -> Result<(inputs::Directory, Vec<u8>, inputs::InputFile)> {
    let directory = inputs::Directory::open(&r.inputs.folder)?;
    let mut envelope = directory.file(
        &r.inputs.envelope_name,
        crate::signed_release::MAX_ENVELOPE as u64,
        false,
    )?;
    let raw = envelope.read(crate::signed_release::MAX_ENVELOPE as u64)?;
    if digest(&raw) != r.inputs.envelope_sha256 {
        return Err(err("UPDATE_PREPARATION_INPUT_CHANGED"));
    }
    let mut artifact = directory.file(&r.inputs.artifact_name, 2 * 1024 * 1024 * 1024, false)?;
    if artifact.len() != r.inputs.artifact_bytes || artifact.pin()? != r.inputs.artifact_sha256 {
        return Err(err("UPDATE_ARTIFACT_MISMATCH"));
    }
    if let Some((name, pin)) = &r.inputs.catalog {
        let mut file = directory.file(name, 65536, false)?;
        if digest(&file.read(65536)?) != *pin {
            return Err(err("UPDATE_CATALOG_INVALID"));
        }
    }
    Ok((directory, raw, artifact))
}
fn private_artifact(root: &Path, name: &str, source: &mut inputs::InputFile) -> Result<PathBuf> {
    let path = root.join(name);
    let mut output = private_options().open(&path).map_err(|_| invalid())?;
    source.check()?;
    source.file.rewind().map_err(|_| invalid())?;
    let length = source.len();
    let copied = std::io::copy(
        &mut Read::by_ref(&mut source.file).take(length + 1),
        &mut output,
    )
    .map_err(|_| invalid())?;
    output.sync_all().map_err(|_| invalid())?;
    source.check()?;
    if copied != source.len() {
        return Err(invalid());
    }
    Ok(path)
}
// Reads only a candidate revision label from authenticated tar bytes. It is NOT
// provenance authority. The existing full OCI qualifier checks the digest graph,
// signed image and all source/licenses/version bindings again before any permit.
const REVISION: &str = r#"import sys,tarfile,json,re
values=set();count=0
with tarfile.open(fileobj=sys.stdin.buffer,mode='r|*') as archive:
 for member in archive:
  count+=1
  if count>100000: raise ValueError('limit')
  if member.isfile() and 0<member.size<=1048576:
   raw=archive.extractfile(member).read(1048577)
   try: value=json.loads(raw)
   except (ValueError,UnicodeError): continue
   if isinstance(value,dict):
    config=value.get('config')
    if isinstance(config,dict):
     labels=config.get('Labels')
     if isinstance(labels,dict):
      revision=labels.get('org.opencontainers.image.revision')
      if isinstance(revision,str) and re.fullmatch('[0-9a-f]{40}',revision): values.add(revision)
if len(values)!=1: raise ValueError('revision')
print(next(iter(values)))
"#;
fn revision(runtime: &Runtime, artifact: &mut inputs::InputFile) -> Result<String> {
    runtime.check()?;
    artifact.file.rewind().map_err(|_| invalid())?;
    let input = artifact.file.try_clone().map_err(|_| invalid())?;
    let args = vec!["-I".into(), "-B".into(), "-c".into(), REVISION.into()];
    let result = run_observed_input(
        runtime.python.to_str().ok_or_else(invalid)?,
        &args,
        None,
        300,
        Some(input),
        || {
            runtime.check()?;
            artifact.check()
        },
    )?;
    artifact.check()?;
    let value = std::str::from_utf8(&result).map_err(|_| invalid())?.trim();
    if value.len() != 40
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(err("UPDATE_OCI_INVALID"));
    }
    Ok(value.into())
}
/// Fixed native helper only. Claims once, preserves failures and never applies.
pub fn execute(
    profile: &Path,
    id: &str,
    runtime: &Runtime,
    check_resources: impl Fn() -> Result<()>,
) -> Result<Receipt> {
    let work = workspace(profile, id)?;
    let _claim = claim(&work)?;
    let r = request(&work, id)?;
    if work.join("outcome.json").exists() || work.join("started.json").exists() {
        return Err(err("UPDATE_PREPARATION_ALREADY_CLAIMED"));
    }
    persist(&work, "started.json", &r.id)?;
    let mut last = Phase::ValidatingInputs;
    let mut step = |phase| {
        check_resources()?;
        progress(&work, &r, phase)?;
        last = phase;
        Ok(())
    };
    let result = run(profile, &work, &r, runtime, &mut step, &check_resources);
    let receipt = match result {
        Ok(reference) => Receipt {
            id: r.id.clone(),
            installation_id: r.installation_id.clone(),
            state: State::Prepared,
            phase: Phase::Registered,
            code: None,
            reference_id: Some(reference),
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
        Err(e) => Receipt {
            id: r.id.clone(),
            installation_id: r.installation_id.clone(),
            state: State::Failed,
            phase: last,
            code: Some(e.code),
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    };
    persist(&work, "outcome.json", &receipt)?;
    Ok(receipt)
}
fn run(
    profile: &Path,
    work: &Path,
    r: &Request,
    runtime: &Runtime,
    step: &mut impl FnMut(Phase) -> Result<()>,
    check_resources: &impl Fn() -> Result<()>,
) -> Result<String> {
    step(Phase::ValidatingInputs)?;
    runtime.check()?;
    let source = bound_source(profile, r, true)?;
    source_eligible(&source)?;
    source_readable(&source)?;
    let profile_service = LifecycleService::open_retry_diagnostics(profile.into())?;
    {
        let _profile = profile_service.lock()?;
        profile_service.ensure_external_archive_reconciled()?;
        source.ensure_external_archive_reconciled()?;
    }
    let mut key = inputs::key(&r.inputs.key_file, profile, &r.source_root)?;
    let mut raw = key.read(32)?;
    let mut key_bytes = KeyBytes([0; 32]);
    key_bytes.0.copy_from_slice(&raw);
    raw.fill(0);
    let (directory, envelope, mut original_artifact) = pin_inputs(r)?;
    // Fresh independent signature and original authority recovery before mutation.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut store = loop {
        match Store::open_for_prepared_handoff(profile, &r.scope) {
            Ok(store) => break store,
            Err(e) if e.code() == "UPDATE_TRUST_BUSY" && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(err(e.code())),
        }
    };
    let (registry, _) = installations::load(profile)?.ok_or_else(invalid)?;
    store.verify_bound_selected(&registry)?;
    store
        .verify_authority_recovery()
        .map_err(|e| err(e.code()))?;
    let mut verified = store
        .verify_for_preparation(&envelope, now() / 1000)
        .map_err(|e| err(e.code()))?;
    original_artifact.file.rewind().map_err(|_| invalid())?;
    verified
        .verify_artifact(&mut original_artifact.file)
        .map_err(|e| err(e.code()))?;
    inputs::supported_release(verified.release())?;
    let source_commit = revision(runtime, &mut original_artifact)?;
    let staging = child(work, "staging")?;
    let artifact_path =
        private_artifact(&staging, &r.inputs.artifact_name, &mut original_artifact)?;
    let catalog = if let Some((name, pin)) = &r.inputs.catalog {
        let mut input = directory.file(name, 65536, false)?;
        let raw = input.read(65536)?;
        if digest(&raw) != *pin {
            return Err(err("UPDATE_CATALOG_INVALID"));
        }
        write_private_new(&staging, "target-catalog.json", &raw, 65536)?;
        Some((staging.join("target-catalog.json"), pin.clone()))
    } else {
        None
    };
    source.prepare_bundled_maintenance(&runtime.resources, &runtime.image)?;
    key.check()?;
    directory.check()?;
    original_artifact.check()?;
    bound_source(profile, r, true)?;
    step(Phase::OriginalBackup)?;
    let made = source.create_backup(&runtime.image, &r.inputs.key_file, true)?;
    let exports = child(work, "exports")?;
    let archive = source.export_backup(profile, &made.id, &exports)?;
    let authenticated = source.authenticate_update_plan_source(
        &runtime.image,
        &r.inputs.key_file,
        Path::new(&archive.path),
    )?;
    if authenticated.backup_id() != made.backup_id
        || authenticated.manifest_sha256() != made.authenticated_manifest_sha256
        || archive.backup_id != made.backup_id
    {
        return Err(err("UPDATE_PLAN_SOURCE_INVALID"));
    }
    key.check()?;
    original_artifact.check()?;
    directory.check()?;
    runtime.check()?;
    bound_source(profile, r, true)?;
    let (registry, _) = installations::load(profile)?.ok_or_else(invalid)?;
    store.verify_bound_selected(&registry)?;
    store
        .verify_authority_recovery()
        .map_err(|e| err(e.code()))?;
    verified = store
        .verify_for_preparation(&envelope, now() / 1000)
        .map_err(|e| err(e.code()))?;
    original_artifact.file.rewind().map_err(|_| invalid())?;
    verified
        .verify_artifact(&mut original_artifact.file)
        .map_err(|e| err(e.code()))?;
    inputs::supported_release(verified.release())?;
    let release = verified.release();
    if store.policy().source_schema_sha256 != authenticated.schema_sha256()
        || release.artifact.schema_sha256 != authenticated.schema_sha256() && catalog.is_none()
    {
        return Err(err("UPDATE_SOURCE_SCHEMA_MISMATCH"));
    }
    let required = archive
        .bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(r.inputs.artifact_bytes))
        .and_then(|n| n.checked_add(6 * 1024 * 1024 * 1024))
        .ok_or_else(invalid)?;
    if fs2::available_space(work).map_err(|_| err("STORAGE_UNAVAILABLE"))? < required {
        return Err(err("STORAGE_INSUFFICIENT"));
    }
    let plan = crate::update::Plan {
        operation_id: Uuid::new_v4().to_string(),
        source_instance: r.installation_id.clone(),
        target_instance: Uuid::new_v4().to_string(),
        source_image: authenticated.runtime_image_sha256().into(),
        target_image: release.artifact.runtime_image_sha256.clone(),
        source_schema: authenticated.schema_sha256().into(),
        target_schema: release.artifact.schema_sha256.clone(),
        backup_id: made.backup_id.clone(),
        backup_manifest: made.authenticated_manifest_sha256.clone(),
        source_inventory: authenticated.inventory_sha256().into(),
        required_free_bytes: required,
    };
    step(Phase::PreparingPlan)?;
    store
        .prepare_update(&envelope, &verified, plan.clone(), now() / 1000)
        .map_err(|e| err(e.code()))?;
    let target = store.register_planned_update_target(true)?;
    if target.source_instance != r.installation_id
        || target.target_instance != plan.target_instance
        || target.activated
    {
        return Err(invalid());
    }
    // Existing ordinary Stop remains admitted for the exact source under Prepared.
    let stopped = source.execute(Action::Stop)?;
    if stopped.state != JobState::Completed {
        return Err(err("UPDATE_SOURCE_NOT_STOPPED"));
    }
    let mut session = store.execution()?;
    session.verify_source_stopped(true)?;
    session.verify_backup(&runtime.image, &r.inputs.key_file, Path::new(&archive.path))?;
    step(Phase::RestoringCandidate)?;
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|_| err("PORT_IN_USE"))?;
    let number = port.local_addr().map_err(|_| invalid())?.port();
    drop(port);
    let restored = session.prepare_target_candidate(
        &runtime.image,
        &r.inputs.key_file,
        Path::new(&archive.path),
        number,
        true,
    )?;
    if restored.backup_id != plan.backup_id
        || restored.authenticated_manifest_sha256 != plan.backup_manifest
    {
        return Err(invalid());
    }
    step(Phase::StoppingCandidate)?;
    let (job, observed) = session.stop_target_candidate(true)?;
    if job.state != JobState::Completed || observed.source_instance != plan.target_instance {
        return Err(err("UPDATE_CANDIDATE_NOT_STOPPED"));
    }
    session.verify_source_deployment(true)?;
    key.check()?;
    directory.check()?;
    original_artifact.check()?;
    runtime.check()?;
    // Stop writes candidate history, so the final checkpoint MUST follow it.
    step(Phase::Checkpoint)?;
    let checkpoint = work.join("checkpoint");
    session.checkpoint_source_host_trust(
        &runtime.image,
        &r.inputs.key_file,
        &checkpoint,
        true,
        true,
    )?;
    let binding = checkpoint.join("pair-binding.bin");
    let host = checkpoint.join("host.bin");
    let trust = checkpoint.join("trust.bin");
    let recovery_exports = child(work, "recovery-exports")?;
    let artifact_staging = child(work, "artifact-staging")?;
    let inputs = RecoveryRuntimeInputs {
        checkpoint: CheckpointInputs {
            binding: &binding,
            host: &host,
            trust: &trust,
            key: &key_bytes.0,
        },
        export_parent: &recovery_exports,
        python: &runtime.python,
        source_commit: &source_commit,
        maintenance_image: &runtime.image,
        external_writers_quiesced: true,
    };
    step(Phase::FullQualification)?;
    let mut artifact = session.stage_prepared_artifact(&artifact_path, &artifact_staging)?;
    let qualification = work.join("qualification");
    let mut permit = match &catalog {
        Some((path, pin)) => session.prepare_owned_migrated_update(
            &mut artifact,
            &inputs,
            &MigrationRuntimeInputs {
                catalog: path,
                catalog_sha256: pin,
            },
            &qualification,
            &r.inputs.key_file,
        )?,
        None => session.prepare_owned_update(
            &mut artifact,
            &inputs,
            &qualification,
            &r.inputs.key_file,
        )?,
    };
    // A long qualification must not publish a reference after its bundled
    // interpreter tree changed. Reuse the retained native resource checker.
    check_resources()?;
    runtime.check()?;
    let value = permit.register_execution_reference()?;
    let id = value["referenceId"]
        .as_str()
        .filter(|s| installations::uuid(s))
        .ok_or_else(invalid)?
        .to_owned();
    if value["registered"] != true
        || value["executionStarted"] != false
        || value["operationId"] != plan.operation_id
        || value["installationId"] != r.installation_id
    {
        return Err(invalid());
    }
    drop(permit);
    drop(artifact);
    drop(session);
    drop(store);
    key.check()?;
    directory.check()?;
    original_artifact.check()?;
    runtime.check()?;
    step(Phase::Registered)?;
    Ok(id)
}
/// Metadata-only history for the exact native job. The all-job admission fence
/// still determines whether normal Manager construction is safe.
pub fn status(profile: &Path, id: Option<&str>) -> Result<Startup> {
    let mut value = startup(profile)?;
    if let Some(id) = id {
        if value.controller_blocked && value.job.as_ref().is_some_and(|job| job.id != id) {
            return Err(err("UPDATE_PREPARATION_BUSY"));
        }
        let root = workspace(profile, id)?;
        let r = request(&root, id)?;
        value.job = Some(projection(&root, &r)?);
    }
    Ok(value)
}
/// Explicit acknowledgement after the worker owner has actually gone away.
/// This never retries, deletes a candidate, claims a helper stopped, or creates
/// an apply permit. Existing Manager maintenance and intent recovery stay required.
pub fn reconcile(profile: &Path, id: &str) -> Result<Startup> {
    let work = workspace(profile, id)?;
    let _claim = claim(&work)?;
    let r = request(&work, id)?;
    if work.join("outcome.json").exists() || !work.join("started.json").exists() {
        return Err(err("UPDATE_PREPARATION_RECONCILE_UNAVAILABLE"));
    }
    let service = LifecycleService::open_retry_diagnostics(profile.into())?;
    let _profile = service.lock()?;
    let source = bound_source(profile, &r, false)?;
    let _source = source.lock()?;
    let (registry, _) = installations::load(profile)?.ok_or_else(invalid)?;
    if crate::signed_release::trust::resolve_selected_scope(profile, &registry)? != r.scope {
        return Err(err("INSTALLATION_SELECTION_CHANGED"));
    }
    let status = crate::signed_release::trust::inspect_update_status(profile, &r.scope)
        .map_err(|e| err(e.code()))?;
    if status.write_uncertain {
        return Err(err("UPDATE_TRUST_WRITE_UNCERTAIN"));
    }
    let mut last = Phase::ValidatingInputs;
    for (i, phase) in [
        Phase::ValidatingInputs,
        Phase::OriginalBackup,
        Phase::PreparingPlan,
        Phase::RestoringCandidate,
        Phase::StoppingCandidate,
        Phase::Checkpoint,
        Phase::FullQualification,
        Phase::Registered,
    ]
    .into_iter()
    .enumerate()
    {
        if work.join(format!("phase-{i}.json")).exists() {
            last = phase;
        }
    }
    persist(
        &work,
        "outcome.json",
        &Receipt {
            id: r.id,
            installation_id: r.installation_id,
            state: State::Failed,
            phase: last,
            code: Some("UPDATE_PREPARATION_RECOVERY_REQUIRED".into()),
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    )?;
    drop(_claim);
    startup(profile)
}
/// Fixed helper admission failed before any preparation action. Does not adopt
/// a claimed request or replace an outcome. Only safe closed error codes persist.
pub fn before_start_failed(profile: &Path, id: &str, code: &str) -> Result<()> {
    if code.is_empty()
        || code.len() > 128
        || !code.as_bytes()[0].is_ascii_uppercase()
        || !code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(invalid());
    }
    let root = workspace(profile, id)?;
    let _claim = claim(&root)?;
    let r = request(&root, id)?;
    if root.join("started.json").exists() || root.join("outcome.json").exists() {
        return Err(err("UPDATE_PREPARATION_ALREADY_CLAIMED"));
    }
    persist(
        &root,
        "outcome.json",
        &Receipt {
            id: r.id,
            installation_id: r.installation_id,
            state: State::Failed,
            phase: Phase::ValidatingInputs,
            code: Some(code.into()),
            reference_id: None,
            updated_at: now(),
            can_cancel: false,
            can_retry: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture() -> (PathBuf, PathBuf, String) {
        let parent = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-preparation-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let profile = child(&parent, "profile").unwrap();
        let root = directory(&profile, true).unwrap().unwrap();
        let jobs = child(&root, "jobs").unwrap();
        let id = Uuid::new_v4().to_string();
        let work = child(&jobs, &id).unwrap();
        let r = Request {
            id: id.clone(),
            installation_id: Uuid::new_v4().to_string(),
            scope: "default".into(),
            registry_sha256: "a".repeat(64),
            source_root: profile.join("local-runtime"),
            inputs: Inputs {
                folder: parent.join("release"),
                envelope_name: "release.json".into(),
                envelope_sha256: "b".repeat(64),
                artifact_name: "runtime.tar".into(),
                artifact_sha256: "c".repeat(64),
                artifact_bytes: 1,
                catalog: None,
                key_file: parent.join("separate.key"),
            },
            created_at: now(),
        };
        persist(&work, "request.json", &r).unwrap();
        write_private_new(&work, "claim.lock", b"", 0).unwrap();
        (profile, work, id)
    }
    #[test]
    fn pending_cancel_is_only_unclaimed_and_preserves_immutable_request() {
        let (profile, work, id) = fixture();
        let before = fs::read(work.join("request.json")).unwrap();
        let state = startup(&profile).unwrap();
        assert!(state.controller_blocked);
        assert!(state.job.unwrap().can_cancel);
        let after = cancel(&profile, &id).unwrap();
        assert!(!after.controller_blocked);
        assert!(matches!(
            after.job.unwrap().state,
            State::CancelledBeforeStart
        ));
        assert_eq!(fs::read(work.join("request.json")).unwrap(), before);
        assert!(cancel(&profile, &id).is_err());
    }
    #[test]
    fn live_owner_and_crash_remain_distinct_and_never_become_retryable() {
        let (profile, work, id) = fixture();
        let owner = claim(&work).unwrap();
        persist(&work, "started.json", &id).unwrap();
        let live = startup(&profile).unwrap();
        assert!(matches!(live.job.unwrap().state, State::Running));
        assert!(cancel(&profile, &id).is_err());
        drop(owner);
        let dead = startup(&profile).unwrap();
        assert!(dead.controller_blocked);
        let receipt = dead.job.unwrap();
        assert!(matches!(receipt.state, State::Interrupted));
        assert!(!receipt.can_retry);
        assert!(!receipt.can_cancel);
        assert!(!work.join("outcome.json").exists());
    }
    #[test]
    fn closed_request_rejects_extra_execution_claim_without_running_anything() {
        let (profile, work, _) = fixture();
        let mut value: Value = read(&work, "request.json").unwrap();
        value["authenticated"] = true.into();
        fs::write(
            work.join("request.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(startup(&profile).is_err());
        assert!(!work.join("started.json").exists());
        assert!(!work.join("outcome.json").exists());
    }
    #[test]
    fn first_boot_missing_parent_does_not_create_or_block_a_profile() {
        let parent = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-no-profile-{}", Uuid::new_v4()));
        let status = startup(&parent.join("profile")).unwrap();
        assert!(!status.controller_blocked);
        assert!(status.job.is_none());
        assert!(!parent.exists());
    }
    #[test]
    fn sibling_workspace_never_lives_inside_profile() {
        let (profile, work, _) = fixture();
        assert!(!work.starts_with(&profile));
        assert_eq!(work.parent().unwrap().file_name().unwrap(), "jobs");
        assert!(profile.read_dir().unwrap().next().is_none());
    }
}
