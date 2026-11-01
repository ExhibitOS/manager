// SPDX-License-Identifier: Apache-2.0
//! Owned PostgreSQL/Node lifecycle for a prepared native installation. No shell,
//! container engine, PATH lookup, PID adoption, or destructive recovery.
use super::*;
use super::{native_installation::Ports, native_payload::Payload};
#[path = "native_runtime_oex_worker.rs"] mod oex_worker;
#[path = "native_runtime_oex_worker_v2.rs"] mod oex_worker_v2;
#[path = "native_runtime_oex_binding.rs"] mod oex_binding;
#[path = "native_runtime_oex_read_fd.rs"] mod oex_read_fd;
#[path = "native_runtime_oex_stage_root.rs"] mod oex_stage_root;
#[path = "native_runtime_oex_reservation.rs"] mod oex_reservation;
#[path = "native_runtime_oex_ack.rs"] mod oex_ack;
#[path = "native_runtime_oex_ack_v2.rs"] mod oex_ack_v2;
#[path = "native_runtime_oex_control_v2.rs"] mod oex_control_v2;
#[path = "native_runtime_oex_chunks.rs"] mod oex_chunks;
#[path = "native_runtime_oex_materializer_fds.rs"] mod oex_materializer_fds;
#[path = "native_runtime_oex_materializer_protocol.rs"] mod oex_materializer_protocol;
#[path = "native_runtime_oex_materializer.rs"] mod oex_materializer;
#[path = "native_runtime_owner_channel.rs"] mod owner_channel;
#[path = "native_runtime_owner_claim_frames.rs"] mod owner_claim_frames;
#[path = "native_runtime_owner_metadata_frames.rs"] mod owner_metadata_frames;
#[path = "native_runtime_owner_binding_encoding.rs"] mod owner_binding_encoding;
use std::{
    collections::BTreeMap,
    net::TcpListener,
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
fn cancelled() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAuthority {
    format: u32,
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
    owner_uid: u32,
    session_token: String,
    csrf_token: String,
}
impl Drop for LocalAuthority {
    fn drop(&mut self) {
        for value in [&mut self.session_token, &mut self.csrf_token] {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalSessionRenewal<'a> {
    format: u32,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    owner_uid: u32,
    previous_session_token: &'a str,
    session_token: &'a str,
    csrf_token: &'a str,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalSessionRenewalReceipt {
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
}
fn local_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(err("NATIVE_RANDOM_UNAVAILABLE"));
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(43);
    for block in bytes.chunks(3) {
        let a = block[0];
        let b = *block.get(1).unwrap_or(&0);
        let c = *block.get(2).unwrap_or(&0);
        output.push(alphabet[(a >> 2) as usize] as char);
        output.push(alphabet[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        if block.len() > 1 {
            output.push(alphabet[(((b & 15) << 2) | (c >> 6)) as usize] as char);
        }
        if block.len() > 2 {
            output.push(alphabet[(c & 63) as usize] as char);
        }
    }
    bytes.fill(0);
    Ok(output)
}

struct Environment(BTreeMap<String, String>);
impl Drop for Environment {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
struct Log {
    file: Mutex<(File, usize)>,
    truncated: AtomicBool,
}
impl Log {
    fn write(&self, bytes: &[u8]) {
        if let Ok(mut state) = self.file.lock() {
            let remaining = (1024 * 1024usize).saturating_sub(state.1);
            let count = bytes.len().min(remaining);
            if state.0.write_all(&bytes[..count]).is_err() || count != bytes.len() {
                self.truncated.store(true, Ordering::Relaxed);
            }
            state.1 += count;
        } else {
            self.truncated.store(true, Ordering::Relaxed);
        }
    }
}
struct Owned {
    child: Child,
    term_sent: bool,
    term_delivered: bool,
    capture: Option<Arc<Mutex<Vec<u8>>>>,
    stdout_thread: Option<thread::JoinHandle<()>>,
}
impl Owned {
    fn signal(&mut self) -> Result<()> {
        if !self.term_sent
            && self
                .child
                .try_wait()
                .map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))?
                .is_none()
        {
            let result = unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                return Err(err("NATIVE_STOP_UNAVAILABLE"));
            }
            self.term_delivered = result == 0;
            self.term_sent = true;
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        use std::os::unix::process::ExitStatusExt;
        let Some(exit) = self
            .child
            .try_wait()
            .map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))?
        else {
            return Ok(None);
        };
        if self.capture.is_some()
            && let Some(reader) = self.stdout_thread.take()
        {
            let _ = reader.join();
        }
        Ok(Some(exit.code().unwrap_or(-exit.signal().unwrap_or(1))))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub format: u32,
    pub job_id: Option<String>,
    pub state: String,
    pub ports: Ports,
    pub readiness: Readiness,
    pub error_code: Option<String>,
    pub failure_code: Option<String>,
    pub postgres_pid: Option<u32>,
    pub node_pid: Option<u32>,
    pub auxiliary_pid: Option<u32>,
    pub exits: BTreeMap<String, i32>,
    pub log_truncated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    format: u32,
    id: String,
}

pub struct Session {
    service: LifecycleService,
    observation_root: crate::maintenance_source::DirectoryPin,
    payload: Arc<Payload>,
    ports: Ports,
    environment: Environment,
    guard: Option<OperationGuard>,
    postgres: Option<Owned>,
    node: Option<Owned>,
    auxiliary: Option<Owned>,
    log: Option<Arc<Log>>,
    node_started: Arc<AtomicBool>,
    node_drain_requested: bool,
    job_id: Option<String>,
    run: Option<PathBuf>,
    state: String,
    failure: Option<String>,
    exits: BTreeMap<String, i32>,
    readiness: Readiness,
    cancellation: Arc<AtomicBool>,
    local_authority: Option<LocalAuthority>,
    owner_channel: Option<owner_channel::Connection>,
    fresh_oex_test: bool,
    normal_oex_selected: bool,
    fresh_oex_started: bool,
    oex_upload_selection_started: bool,
    oex_upload_selected: Option<(String, u64, String)>,
    maintenance_space: Vec<PathBuf>,
}
fn directory(root: &Path, name: &str) -> Result<PathBuf> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => installations::new_directory(&path)?,
        Ok(_) => installations::private_directory(&path)?,
        Err(_) => return Err(err("NATIVE_DIRECTORY_UNAVAILABLE")),
    }
    Ok(path)
}
fn alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return true;
    }
    (unsafe { libc::kill(pid as i32, 0) == 0 })
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
// A database-only maintenance session legitimately has no Node exit record.
// If a service ran, disappearance alone does not prove its workers drained.
fn clean_node_quiescence(service_expected: bool, drain_requested: bool, exits: &BTreeMap<String, i32>) -> bool {
    !service_expected || (drain_requested && exits.get("node") == Some(&0))
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(err("NATIVE_JOURNAL_UNAVAILABLE")),
    }
}
/// Read-only orphan guard. Callers must hold the root operation fence; it does
/// not inspect environment credentials, start a process, or confer authority.
pub(crate) fn check_interrupted_root(root: &Path) -> Result<()> {
    let pointer = root.join("native-current-run.json");
    if exists(&pointer)? {
        let current: Current = serde_json::from_slice(&installation_backup::source_bytes(
            root,
            "native-current-run.json",
            4096,
            true,
        )?)
        .map_err(|_| err("NATIVE_JOURNAL_INVALID"))?;
        if current.format != 1
            || Uuid::parse_str(&current.id)
                .ok()
                .is_none_or(|id| id.to_string() != current.id)
        {
            return Err(err("NATIVE_JOURNAL_INVALID"));
        }
        let relative = format!("native-runs/{}/state.json", current.id);
        let prior: Context = serde_json::from_slice(&installation_backup::source_bytes(
            root,
            &relative,
            8192,
            true,
        )?)
        .map_err(|_| err("NATIVE_JOURNAL_INVALID"))?;
        if prior.format != 1
            || prior.job_id.as_deref() != Some(&current.id)
            || [prior.postgres_pid, prior.node_pid, prior.auxiliary_pid]
                .into_iter()
                .flatten()
                .any(alive)
        {
            return Err(err("NATIVE_RECOVERY_REQUIRED"));
        }
    }
    let postmaster = root.join("native-database/postmaster.pid");
    if exists(&postmaster)? {
        let bytes = installation_backup::source_bytes(
            root,
            "native-database/postmaster.pid",
            8192,
            true,
        )?;
        let pid = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.lines().next())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| err("NATIVE_RECOVERY_REQUIRED"))?;
        if alive(pid) {
            return Err(err("NATIVE_RECOVERY_REQUIRED"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod interrupted_root_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    #[ignore = "requires the verified isolated native payload and shared heavy admission"]
    fn terminal_record_failure_keeps_fence_and_status_retries() {
        struct RestoreMode(PathBuf);
        impl Drop for RestoreMode { fn drop(&mut self) { let _ = fs::set_permissions(&self.0,fs::Permissions::from_mode(0o700)); } }
        let payload_path=PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PAYLOAD").expect("payload required"));
        let payload=Arc::new(Payload::open(&payload_path).unwrap());
        let base=std::env::temp_dir().join(format!("native-terminal-record-{}",Uuid::new_v4()));
        fs::create_dir(&base).unwrap();fs::set_permissions(&base,fs::Permissions::from_mode(0o700)).unwrap();
        let root=fs::canonicalize(base).unwrap();let service=LifecycleService::new(root.clone()).unwrap();
        service.prepare_native_personal(&payload,Ports::default()).unwrap();
        for via_status in [false,true] {
            let mut session=Session::new(&service,payload.clone(),Ports::default()).unwrap();
            let run=root.join(format!("terminal-test-{}",Uuid::new_v4()));fs::create_dir(&run).unwrap();fs::set_permissions(&run,fs::Permissions::from_mode(0o700)).unwrap();
            let _restore=RestoreMode(run.clone());
            session.run=Some(run.clone());session.job_id=Some(Uuid::new_v4().to_string());
            session.local_authority=Some(LocalAuthority{format:1,workspace_id:Uuid::new_v4().to_string(),tenant_id:Uuid::new_v4().to_string(),principal_id:Uuid::new_v4().to_string(),owner_uid:unsafe{libc::geteuid()},session_token:"synthetic-session".into(),csrf_token:"synthetic-csrf".into()});
            if via_status {session.state="stopping".into();}
            fs::set_permissions(&run,fs::Permissions::from_mode(0o500)).unwrap();
            let result=if via_status {session.status()} else {session.stop()};
            assert!(result.is_err());assert_eq!(session.state,"stopping");assert!(session.guard.is_some());assert!(session.local_authority.is_some());assert_eq!(service.lock().err().unwrap().code,"BUSY");
            fs::set_permissions(&run,fs::Permissions::from_mode(0o700)).unwrap();
            let terminal=session.status().unwrap();assert_eq!(terminal.state,"stopped");assert!(session.guard.is_none());assert!(session.local_authority.is_none());
            let persisted:Context=serde_json::from_slice(&fs::read(run.join("state.json")).unwrap()).unwrap();assert_eq!(persisted.state,"stopped");
            assert!(service.lock().is_ok());drop(session);
        }
        println!("isolated_record_failure_root={} payload_manifest_sha256={} both_paths=PASS",root.display(),payload.context().manifest_sha256);
    }
    #[test]
    fn node_drain_database_only_or_clean_exit_but_not_crash() {
        let mut exits = BTreeMap::new();
        assert!(clean_node_quiescence(false, false, &exits));
        assert!(!clean_node_quiescence(true, true, &exits));
        for (program, expected) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
            let status = Command::new(program).status().unwrap();
            exits.insert("node".into(), status.code().unwrap());
            assert_eq!(clean_node_quiescence(true, true, &exits), expected);
        }
        exits.insert("node".into(), -libc::SIGTERM);
        assert!(!clean_node_quiescence(true, true, &exits));
        exits.insert("node".into(), 0);
        assert!(!clean_node_quiescence(true, false, &exits));
    }

    #[test]
    fn node_drain_delivered_signal_without_graceful_exit_refused() {
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let mut owned = Owned { child, term_sent: false, term_delivered: false, capture: None, stdout_thread: None };
        if let Err(error) = owned.signal() {
            let _ = owned.child.kill(); let _ = owned.child.wait();
            panic!("owned signal failed: {}", error.code);
        }
        let mut code = None;
        for _ in 0..100 {
            code = owned.poll().unwrap();
            if code.is_some() { break; }
            thread::sleep(Duration::from_millis(10));
        }
        if code.is_none() { let _ = owned.child.kill(); let _ = owned.child.wait(); panic!("owned child did not exit"); }
        assert!(owned.term_delivered);
        let mut exits = BTreeMap::new();
        exits.insert("node".into(), code.unwrap());
        assert!(!clean_node_quiescence(true, owned.term_delivered, &exits));
    }

    #[test]
    #[ignore = "Explicit isolated stopped native profile and verified runtime under the heavy guard only"]
    fn node_drain_actual_owned_native_runtime() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_NATIVE_DRAIN_ROOT").expect("isolated test root required"));
        let payload_path = PathBuf::from(std::env::var_os("EXHIBITOS_NATIVE_DRAIN_PAYLOAD").expect("verified test runtime required"));
        assert!(!root.join("native-database/postmaster.pid").exists());
        let service = LifecycleService::bound_existing(root).unwrap();
        let payload = Arc::new(Payload::open(&payload_path).unwrap());
        let prepared = service.native_prepared().unwrap().unwrap();
        assert_eq!(prepared.manifest_sha256, payload.context().manifest_sha256);
        let mut session = Session::new(&service, payload, prepared.ports).unwrap();
        let started = session.start().unwrap();
        assert_eq!(started.state, "ready");
        let drained = session.quiesce_for_backup().unwrap();
        assert_eq!(drained.state, "database_ready");
        assert_eq!(drained.exits.get("node"), Some(&0));
        assert!(drained.node_pid.is_none() && drained.postgres_pid.is_some());
        let stopped = session.stop().unwrap();
        assert_eq!(stopped.state, "stopped");
        assert!(stopped.node_pid.is_none() && stopped.postgres_pid.is_none());
        assert_eq!(stopped.exits.get("postgres"), Some(&0));
    }

    #[test]
    fn offline_guard_refuses_live_and_malformed_postmaster_without_environment() {
        let root = std::env::temp_dir().join(format!("native-offline-orphan-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let db = root.join("native-database");
        fs::create_dir(&db).unwrap();
        fs::set_permissions(&db, fs::Permissions::from_mode(0o700)).unwrap();
        check_interrupted_root(&root).unwrap();
        write_private_new(&db, "postmaster.pid", format!("{}\n", std::process::id()).as_bytes(), 8192).unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_RECOVERY_REQUIRED");
        fs::write(db.join("postmaster.pid"), b"not-a-pid\n").unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_RECOVERY_REQUIRED");
        fs::remove_file(db.join("postmaster.pid")).unwrap();
        write_private_new(&root, "native-current-run.json", b"{}", 4096).unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_JOURNAL_INVALID");
        fs::remove_dir_all(root).unwrap();
    }
}

fn check_selected_empty_root(service: &LifecycleService, guard: &OperationGuard,
                             root_pin: &crate::maintenance_source::DirectoryPin) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    installations::private_directory(&service.root)?;
    root_pin.check_path(&service.root)?;
    let held = guard.metadata().map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
    let named = fs::symlink_metadata(service.root.join("operation.lock"))
        .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
    if !held.is_file() || named.is_symlink() || held.uid() != unsafe { libc::geteuid() }
        || held.mode() & 0o777 != 0o600 || held.nlink() != 1
        || (held.dev(), held.ino()) != (named.dev(), named.ino()) {
        return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
    }
    for entry in fs::read_dir(&service.root).map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))? {
        if entry.map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?.file_name()
            != std::ffi::OsStr::new("operation.lock") {
            return Err(err("NATIVE_OEX_FRESH_ROOT_REQUIRED"));
        }
    }
    root_pin.check_path(&service.root)
}

#[cfg(test)]
mod selected_empty_root_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn empty_root_check_refuses_existing_data_without_changing_it() {
        let root = std::env::temp_dir().join(format!("exhibitos-selected-empty-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let pin = crate::maintenance_source::DirectoryPin::open(&root).unwrap();
        let guard = service.lock().unwrap();
        assert!(check_selected_empty_root(&service, &guard, &pin).is_ok());
        let marker = root.join("existing-data");
        fs::write(&marker, b"preserve").unwrap();
        assert!(check_selected_empty_root(&service, &guard, &pin).is_err());
        assert_eq!(fs::read(marker).unwrap(), b"preserve");
    }
}

impl Session {
    pub fn new(service: &LifecycleService, payload: Arc<Payload>, ports: Ports) -> Result<Self> {
        installations::private_directory(&service.root)?;
        let guard = service.lock()?;
        Self::new_with_guard(service, payload, ports, guard)
    }
    /// The opaque proof is minted by the atomic creation/selection
    /// transaction after owner admission.
    /// An empty path or a reselected existing workspace cannot mint it.
    #[allow(dead_code)]
    pub(crate) fn prepare_selected_empty(proof: installations::NewSelectedRoot, payload: Arc<Payload>, ports: Ports) -> Result<Self> {
        let (service, guard, root_pin) = proof.into_parts();
        if !payload.normal_oex_selected_input()? { return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        check_selected_empty_root(&service, &guard, &root_pin)?;
        service.prepare_native_personal_locked(&payload, ports, &guard)?;
        root_pin.check_path(&service.root)?;
        let mut session = Self::new_with_guard(&service, payload, ports, guard)?;
        session.check_native_observation_fence()?;
        session.normal_oex_selected = true;
        Ok(session)
    }
    pub(crate) fn new_with_guard(
        service: &LifecycleService,
        payload: Arc<Payload>,
        ports: Ports,
        guard: OperationGuard,
    ) -> Result<Self> {
        installations::private_directory(&service.root)?;
        signed_release::trust::lifecycle_writer::admit(&service.root, &Action::Start)?;
        let environment = Environment(service.native_start_environment(&payload, ports)?);
        Ok(Self {
            service: LifecycleService::bound_existing(service.root.clone())?,
            observation_root: crate::maintenance_source::DirectoryPin::open(&service.root)?,
            payload,
            ports,
            environment,
            guard: Some(guard),
            postgres: None,
            node: None,
            auxiliary: None,
            log: None,
            node_started: Arc::new(AtomicBool::new(false)),
            node_drain_requested: false,
            job_id: None,
            run: None,
            state: "stopped".into(),
            failure: None,
            exits: BTreeMap::new(),
            readiness: Readiness::default(),
            cancellation: Arc::new(AtomicBool::new(false)),
            local_authority: None,
            owner_channel: None,
            fresh_oex_test: false,
            normal_oex_selected: false,
            fresh_oex_started: false,
            oex_upload_selection_started: false,
            oex_upload_selected: None,
            maintenance_space: Vec::new(),
        })
    }
    /// Explicit private IPC only, never part of status/journal/log output.
    pub fn authoring_session(&mut self) -> Result<Value> {
        if self.oex_upload_selection_started { return Err(err("NATIVE_OEX_SESSION_BOUND")); }
        if self.status()?.state != "ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        if self.auxiliary.is_some() { return Err(err("NATIVE_OPERATION_ACTIVE")); }
        self.observation_root.check()?;
        let identity = self.service.personal_identity()?;
        let old = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if identity != (old.workspace_id.clone(), old.tenant_id.clone(), old.principal_id.clone())
            || old.owner_uid != unsafe { libc::geteuid() }
        { return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH")); }
        let script = self.payload.authoring_session_script()?;
        let database = self.environment.0.get("DATABASE_URL").ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?.clone();
        let next = LocalAuthority {
            format: 1, workspace_id: old.workspace_id.clone(), tenant_id: old.tenant_id.clone(),
            principal_id: old.principal_id.clone(), owner_uid: old.owner_uid,
            session_token: local_token()?, csrf_token: local_token()?,
        };
        let input = LocalSessionRenewal {
            format: 1, workspace_id: &old.workspace_id, tenant_id: &old.tenant_id, principal_id: &old.principal_id,
            owner_uid: old.owner_uid, previous_session_token: &old.session_token,
            session_token: &next.session_token, csrf_token: &next.csrf_token,
        };
        let mut bytes = serde_json::to_vec(&input).map_err(|_| err("STATE_INVALID"))?;
        if bytes.len() > 4096 { bytes.fill(0); return Err(err("NATIVE_AUTHORING_RENEWAL_INVALID")); }
        // Once issuance starts, an ambiguous child/commit result must not emit
        // either credential or retry by reviving the old (possibly revoked) row.
        // The server owns revocation/expiry/current membership policy.
        let _old = self.local_authority.take();
        let result = self.auxiliary_timed(self.payload.node(), vec![script.to_string_lossy().into()],
            vec![("DATABASE_URL", database), ("EXHIBITOS_RUNTIME_MODE", "native".into()), ("EXHIBITOS_AUTHORING_MODE", "personal".into()),
                 ("TENANT_ID", next.tenant_id.clone())], Some(&bytes), Duration::from_secs(15));
        bytes.fill(0);
        let (code, receipt) = result.map_err(|_| err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"))?;
        self.observation_root.check()?;
        if self.service.personal_identity()? != identity { return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH")); }
        self.payload.recheck()?;
        let receipt: LocalSessionRenewalReceipt = serde_json::from_slice(&receipt)
            .map_err(|_| err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"))?;
        if code != 0 || receipt.workspace_id != next.workspace_id || receipt.tenant_id != next.tenant_id || receipt.principal_id != next.principal_id {
            return Err(err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"));
        }
        self.local_authority = Some(next);
        let a = self
            .local_authority
            .as_ref()
            .ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        Ok(
            serde_json::json!({"tenantId":a.tenant_id,"principalId":a.principal_id,"origin":format!("http://127.0.0.1:{}",self.ports.web),"token":a.session_token}),
        )
    }
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancellation.clone()
    }
    pub fn belongs_to(&self, service: &LifecycleService) -> bool {
        self.service.root == service.root
    }
    fn cancelled(&self) -> bool {
        cancelled() || self.cancellation.load(Ordering::SeqCst)
    }
    fn snapshot(&self) -> Context {
        let readiness = if self.state == "ready" {
            self.readiness.clone()
        } else {
            Readiness::default()
        };
        Context {
            format: 1,
            job_id: self.job_id.clone(),
            state: self.state.clone(),
            ports: self.ports,
            readiness,
            error_code: if self.state == "stopping" {
                Some("NATIVE_STOP_PENDING".into())
            } else {
                self.failure.clone()
            },
            failure_code: self.failure.clone(),
            postgres_pid: self.postgres.as_ref().map(|p| p.child.id()),
            node_pid: self.node.as_ref().map(|p| p.child.id()),
            auxiliary_pid: self.auxiliary.as_ref().map(|p| p.child.id()),
            exits: self.exits.clone(),
            log_truncated: self
                .log
                .as_ref()
                .is_some_and(|l| l.truncated.load(Ordering::Relaxed)),
        }
    }
    fn record(&self) -> Result<()> {
        if let Some(run) = &self.run {
            write_json(run, "state.json", &self.snapshot())?;
            File::open(run)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<bool> {
        let mut changed = false;
        for (name, slot) in [
            ("auxiliary", &mut self.auxiliary),
            ("node", &mut self.node),
            ("postgres", &mut self.postgres),
        ] {
            if let Some(child) = slot
                && let Some(code) = child.poll()?
            {
                self.exits.insert(name.into(), code);
                *slot = None;
                changed = true;
            }
        }
        if self.node.is_none() { self.owner_channel = None; }
        Ok(changed)
    }
    fn any_owned(&self) -> bool {
        self.postgres.is_some() || self.node.is_some() || self.auxiliary.is_some()
    }
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        extra: &[(&str, String)],
        input: Option<&[u8]>,
        capture: bool,
        marker: bool,
        owner_setup: Option<&owner_channel::Setup>,
    ) -> Result<Owned> {
        let log = self
            .log
            .as_ref()
            .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?
            .clone();
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(
                self.run
                    .as_ref()
                    .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?,
            )
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            command.env(key, value);
        }
        if let Some(setup) = owner_setup { setup.configure(&mut command)?; }
        let mut child = command
            .spawn()
            .map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?;
        let mut stdin = child.stdin.take();
        let capture = if capture {
            Some(Arc::new(Mutex::new(Vec::new())))
        } else {
            None
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| err("NATIVE_PROCESS_START_FAILED"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| err("NATIVE_PROCESS_START_FAILED"))?;
        let captured = capture.clone();
        let stdout_log = log.clone();
        let started = self.node_started.clone();
        let stdout_thread = thread::spawn(move || {
            let mut stream = stdout;
            let mut block = [0u8; 4096];
            let mut line = Vec::new();
            while let Ok(count) = stream.read(&mut block) {
                if count == 0 {
                    break;
                }
                stdout_log.write(&block[..count]);
                if let Some(captured) = &captured
                    && let Ok(mut bytes) = captured.lock()
                {
                    let room = 16384usize.saturating_sub(bytes.len());
                    bytes.extend_from_slice(&block[..count.min(room)]);
                }
                if marker {
                    for byte in &block[..count] {
                        if *byte == b'\n' {
                            if line == b"ExhibitOS local runtime started." {
                                started.store(true, Ordering::SeqCst);
                            }
                            line.clear();
                        } else if line.len() < 1024 {
                            line.push(*byte);
                        }
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut stream = stderr;
            let mut block = [0u8; 4096];
            while let Ok(count) = stream.read(&mut block) {
                if count == 0 {
                    break;
                }
                log.write(&block[..count]);
            }
        });
        if let (Some(stdin), Some(input)) = (&mut stdin, input) {
            let _ = stdin.write_all(input);
        }
        drop(stdin);
        Ok(Owned {
            child,
            term_sent: false,
            term_delivered: false,
            capture,
            stdout_thread: Some(stdout_thread),
        })
    }
    fn auxiliary(
        &mut self,
        program: PathBuf,
        args: Vec<String>,
        extra: Vec<(&str, String)>,
        input: Option<&[u8]>,
    ) -> Result<(i32, Vec<u8>)> {
        self.auxiliary_timed(program, args, extra, input, Duration::from_secs(60))
    }
    fn auxiliary_timed(
        &mut self,
        program: PathBuf,
        args: Vec<String>,
        extra: Vec<(&str, String)>,
        input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<(i32, Vec<u8>)> {
        self.check_maintenance_space()?;
        self.auxiliary = Some(self.spawn(&program, &args, &extra, input, true, false, None)?);
        self.record()?;
        let deadline = Instant::now() + timeout;
        loop {
            self.check_maintenance_space()?;
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            let child = self
                .auxiliary
                .as_mut()
                .ok_or_else(|| err("NATIVE_PROCESS_UNAVAILABLE"))?;
            if let Some(code) = child.poll()? {
                let bytes = child
                    .capture
                    .as_ref()
                    .and_then(|p| p.lock().ok().map(|b| b.clone()))
                    .unwrap_or_default();
                self.exits.insert("auxiliary".into(), code);
                self.auxiliary = None;
                self.record()?;
                return Ok((code, bytes));
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_PROCESS_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn check_maintenance_space(&self) -> Result<()> {
        for path in &self.maintenance_space {
            // Restore expansion is not bounded by compressed dump size. Keep
            // 1GiB of cancellation/shutdown headroom above the existing floor,
            // checking the actual source/candidate/output filesystems per poll.
            // This is operational monitoring, not a quota or atomic reservation.
            if fs2::available_space(path).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < 7 * 1024 * 1024 * 1024
            {
                return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
            }
        }
        Ok(())
    }
    fn pg_tool(&self, name: &str) -> Result<PathBuf> {
        Ok(self
            .payload
            .postgres()
            .parent()
            .ok_or_else(|| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?
            .join(name))
    }
    fn query(&mut self, sql: &str) -> Result<String> {
        self.query_database("postgres", sql)
    }
    fn query_database(&mut self, database: &str, sql: &str) -> Result<String> {
        let password = self
            .environment
            .0
            .get("POSTGRES_PASSWORD")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        let (code, bytes) = self.auxiliary(
            self.pg_tool("psql")?,
            vec![
                "-h".into(),
                "127.0.0.1".into(),
                "-p".into(),
                self.ports.database.to_string(),
                "-U".into(),
                "exhibitos".into(),
                "-d".into(),
                database.into(),
                "--no-password".into(),
                "--no-psqlrc".into(),
                "--tuples-only".into(),
                "--no-align".into(),
                "--command".into(),
                sql.into(),
            ],
            vec![("PGPASSWORD", password)],
            None,
        )?;
        if code != 0 {
            return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
        }
        String::from_utf8(bytes)
            .map(|s| s.trim().into())
            .map_err(|_| err("NATIVE_DATABASE_UNAVAILABLE"))
    }
    fn start_inner(&mut self, services: bool) -> Result<()> {
        let mut reservations = Vec::new();
        for port in [self.ports.web, self.ports.api, self.ports.database] {
            reservations.push(
                TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
                    .map_err(|_| err("NATIVE_PORT_IN_USE"))?,
            );
        }
        if fs2::available_space(&self.service.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < 3 * 1024 * 1024 * 1024
        {
            return Err(err("STORAGE_LOW"));
        }
        let database = self.service.root.join("native-database");
        if !exists(&database)? {
            self.state = "initializing_database".into();
            self.record()?;
            let mut input = self
                .environment
                .0
                .get("POSTGRES_PASSWORD")
                .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                .as_bytes()
                .to_vec();
            input.push(b'\n');
            let result = self.auxiliary(
                self.payload.initdb(),
                vec![
                    "-D".into(),
                    database.to_string_lossy().into(),
                    "-U".into(),
                    "exhibitos".into(),
                    "--auth-host=scram-sha-256".into(),
                    "--auth-local=scram-sha-256".into(),
                    "--pwfile=/dev/fd/0".into(),
                    "--no-locale".into(),
                    "--encoding=UTF8".into(),
                    "--no-instructions".into(),
                ],
                vec![],
                Some(&input),
            );
            input.fill(0);
            if result?.0 != 0 {
                return Err(err("NATIVE_DATABASE_INITIALIZATION_FAILED"));
            }
        }
        installations::private_directory(&database)?;
        let version = installation_backup::source_bytes(
            &self.service.root,
            "native-database/PG_VERSION",
            32,
            true,
        )?;
        if std::str::from_utf8(&version).ok().map(str::trim) != Some("18") {
            return Err(err("NATIVE_DATABASE_VERSION_UNSUPPORTED"));
        }
        directory(&self.service.root, "blobs")?;
        directory(&self.service.root, "config")?;
        drop(reservations);
        self.state = "starting_database".into();
        self.record()?;
        let nonce = self
            .job_id
            .clone()
            .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
        self.postgres = Some(self.spawn(
            &self.payload.postgres(),
            &[
                "-D".into(),
                database.to_string_lossy().into(),
                "-h".into(),
                "127.0.0.1".into(),
                "-p".into(),
                self.ports.database.to_string(),
                "-k".into(),
                "".into(),
                "-c".into(),
                "jit=off".into(),
                "-c".into(),
                format!("exhibitos.instance={nonce}"),
            ],
            &[],
            None,
            false,
            false,
            None,
        )?);
        self.record()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            self.poll()?;
            if self.postgres.is_none() {
                return Err(err("NATIVE_DATABASE_EXITED"));
            }
            let (code, _) = self.auxiliary(
                self.pg_tool("pg_isready")?,
                vec![
                    "-h".into(),
                    "127.0.0.1".into(),
                    "-p".into(),
                    self.ports.database.to_string(),
                    "-U".into(),
                    "exhibitos".into(),
                ],
                vec![],
                None,
            )?;
            if code == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_DATABASE_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(100));
        }
        if self.query("SELECT current_setting('exhibitos.instance',true);")? != nonce {
            return Err(err("NATIVE_DATABASE_OWNERSHIP_INVALID"));
        }
        match self
            .query("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname='exhibitos');")?
            .as_str()
        {
            "t" => {}
            "f" => {
                let password = self
                    .environment
                    .0
                    .get("POSTGRES_PASSWORD")
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                    .clone();
                if self
                    .auxiliary(
                        self.pg_tool("createdb")?,
                        vec![
                            "-h".into(),
                            "127.0.0.1".into(),
                            "-p".into(),
                            self.ports.database.to_string(),
                            "-U".into(),
                            "exhibitos".into(),
                            "--no-password".into(),
                            "exhibitos".into(),
                        ],
                        vec![("PGPASSWORD", password)],
                        None,
                    )?
                    .0
                    != 0
                {
                    return Err(err("NATIVE_DATABASE_CREATION_FAILED"));
                }
            }
            _ => return Err(err("NATIVE_DATABASE_UNAVAILABLE")),
        }
        // Restore must reach an empty database before any application migration
        // or owner bootstrap runs. Keep the same Session and operation fence.
        if !services {
            self.state = "database_ready".into();
            self.record()?;
            return Ok(());
        }
        self.state = "starting_services".into();
        self.record()?;
        let mut environment = vec![
            ("NODE_ENV", "production".into()),
            (
                "BLOB_ROOT",
                self.service.root.join("blobs").to_string_lossy().into(),
            ),
            (
                "CONFIG_ROOT",
                self.service.root.join("config").to_string_lossy().into(),
            ),
        ];
        let personal = self
            .environment
            .0
            .get("EXHIBITOS_AUTHORING_MODE")
            .is_some_and(|v| v == "personal");
        let mut keys = vec![
            "EXHIBITOS_RUNTIME_MODE",
            "EXHIBITOS_PORT",
            "EXHIBITOS_API_PORT",
            "DATABASE_URL",
            "TENANT_ID",
        ];
        if personal {
            keys.push("EXHIBITOS_AUTHORING_MODE");
        } else {
            keys.extend(["ADMIN_SUBJECT", "ADMIN_PASSWORD"]);
        }
        for key in keys {
            environment.push((
                key,
                self.environment
                    .0
                    .get(key)
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                    .clone(),
            ));
        }
        self.local_authority = if personal {
            let value = |key: &str| {
                self.environment
                    .0
                    .get(key)
                    .cloned()
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))
            };
            Some(LocalAuthority {
                format: 1,
                workspace_id: value("WORKSPACE_ID")?,
                tenant_id: value("TENANT_ID")?,
                principal_id: value("PRINCIPAL_ID")?,
                owner_uid: value("OWNER_UID")?
                    .parse()
                    .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?,
                session_token: local_token()?,
                csrf_token: local_token()?,
            })
        } else {
            None
        };
        let mut owner_setup = if self.normal_oex_selected {
            if self.local_authority.is_none() || !self.payload.normal_oex_selected_input()? {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Some(owner_channel::Setup::new()?)
        } else { None };
        let generation = self.job_id.as_deref().ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
        let mut private_input = match (&self.local_authority, &owner_setup) {
            (Some(a), Some(setup)) => Some(setup.normal_bootstrap(a, generation)?),
            (Some(a), None) => Some(serde_json::to_vec(a).map_err(|_| err("STATE_INVALID"))?),
            (None, _) => None,
        };
        let spawned = self.spawn(
            &self.payload.node(),
            &[self.payload.script().to_string_lossy().into()],
            &environment,
            private_input.as_deref(),
            false,
            true,
            owner_setup.as_ref(),
        );
        if let Some(bytes) = private_input.as_mut() {
            bytes.fill(0);
        }
        self.node = Some(spawned?);
        if let Some(setup) = owner_setup.as_mut() { setup.close_child(); }
        self.record()?;
        if let Some(setup) = owner_setup {
            let authority = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let generation = self.job_id.as_deref().ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
            self.owner_channel = Some(setup.handshake(authority, generation, &self.cancellation)?);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            self.poll()?;
            if self.node.is_none() || self.postgres.is_none() {
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            if self.node_started.load(Ordering::SeqCst) {
                let ready = readiness_target(
                    &format!("http://127.0.0.1:{}/api/v1/readiness", self.ports.web),
                    &[self.ports.web],
                    "1.0.0-draft.1",
                    "1",
                    "0.1.0",
                );
                if ready.ready {
                    self.readiness = ready;
                    self.state = "ready".into();
                    if self.normal_oex_selected { self.await_oex_owner_ready()?; }
                    // The selected owner is not ready for use until its private
                    // operation-ready frame has also been verified.  Do not
                    // publish a ready journal entry while that check is pending.
                    self.record()?;
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_READINESS_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn start(&mut self) -> Result<Context> {
        self.cancellation.store(false, Ordering::SeqCst);
        self.start_cancellable()
    }
    /// The host resets cancellation before publishing its handle, so a stop
    /// arriving between publication and startup cannot be overwritten.
    pub(crate) fn start_cancellable(&mut self) -> Result<Context> {
        self.start_owned(true)
    }
    /// Maintenance preparation only: owned PG18 and database, without Node,
    /// API/workers, schema migrations, authentication bootstrap or readiness.
    pub fn start_database_only(&mut self) -> Result<Context> {
        self.cancellation.store(false, Ordering::SeqCst);
        self.start_database_only_cancellable()
    }
    pub(crate) fn start_database_only_cancellable(&mut self) -> Result<Context> {
        self.start_owned(false)
    }
    fn start_owned(&mut self, services: bool) -> Result<Context> {
        if self.normal_oex_selected {
            if self.fresh_oex_started || !services || self.guard.is_none()
                || exists(&self.service.root.join("native-database"))?
                || exists(&self.service.root.join("blobs"))?
                || exists(&self.service.root.join("config"))?
                || !self.payload.normal_oex_selected_input()? {
                return Err(err("NATIVE_OEX_FRESH_ROOT_REQUIRED"));
            }
            self.check_native_observation_fence()?;
            self.fresh_oex_started = true;
        }
        if services && (exists(&self.service.root.join("native-restore-current.json"))?
            || exists(&self.service.root.join("native-target-preparation-current.json"))?
            || exists(&self.service.root.join("native-legacy-migration-current.json"))?) {
            return Err(err("NATIVE_RESTORE_ACTIVATION_REQUIRED"));
        }
        if self.any_owned() {
            return Err(err("BUSY"));
        }
        if self.cancelled() {
            return Err(err("NATIVE_CANCELLED"));
        }
        self.readiness = Readiness::default();
        if self.guard.is_none() {
            self.guard = Some(self.service.lock()?);
        }
        signed_release::trust::lifecycle_writer::admit(&self.service.root, &Action::Start)?;
        self.environment = Environment(
            self.service
                .native_start_environment(&self.payload, self.ports)?,
        );
        // Check before publishing a new current pointer, including orphaned jobs.
        check_interrupted_root(&self.service.root)?;
        let parent = directory(&self.service.root, "native-runs")?;
        let id = Uuid::new_v4().to_string();
        let run = directory(&parent, &id)?;
        let file = private_options()
            .open(run.join("process.log"))
            .map_err(|_| err("NATIVE_LOG_UNAVAILABLE"))?;
        self.log = Some(Arc::new(Log {
            file: Mutex::new((file, 0)),
            truncated: AtomicBool::new(false),
        }));
        self.job_id = Some(id.clone());
        self.run = Some(run);
        self.failure = None;
        self.exits.clear();
        self.node_started.store(false, Ordering::SeqCst);
        self.node_drain_requested = false;
        self.state = "starting".into();
        self.record()?;
        write_json(
            &self.service.root,
            "native-current-run.json",
            &Current { format: 1, id },
        )?;
        File::open(&self.service.root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        // The current pointer now describes this owned attempt; it has no PIDs yet.
        let result = self.start_inner(services);
        if let Err(error) = result {
            self.failure = Some(error.code.clone());
            let _ = self.stop();
            return Err(error);
        }
        Ok(self.snapshot())
    }
    /// Stop and reap the owned API/worker process while retaining PostgreSQL
    /// and the root operation fence for the authenticated backup transaction.
    /// No successful quiescence is reported while an owned writer remains.
    pub fn quiesce_for_backup(&mut self) -> Result<Context> {
        self.poll()?;
        if !matches!(
            self.state.as_str(),
            "ready" | "unavailable" | "quiescing" | "database_ready"
        ) || self.postgres.is_none()
            || self.auxiliary.is_some()
            || self.guard.is_none()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        let service_expected = matches!(self.state.as_str(), "ready" | "unavailable")
            || self.node.is_some()
            || self.exits.contains_key("node");
        self.state = "quiescing".into();
        self.readiness = Readiness::default();
        self.local_authority = None;
        self.record()?;
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            self.poll()?;
            if self.postgres.is_none() {
                self.failure = Some("NATIVE_PROCESS_EXITED".into());
                let _ = self.stop();
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            if let Some(node) = self.node.as_mut() {
                node.signal()?;
                self.node_drain_requested |= node.term_delivered;
            } else {
                if !clean_node_quiescence(service_expected, self.node_drain_requested, &self.exits) {
                    return Err(err("NATIVE_QUIESCENCE_UNCONFIRMED"));
                }
                self.state = "database_ready".into();
                self.record()?;
                return Ok(self.snapshot());
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_QUIESCENCE_PENDING"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn backup_available(&self) -> Result<()> {
        if exists(&self.service.root.join("native-restore-current.json"))?
            || exists(&self.service.root.join("native-target-preparation-current.json"))?
            || exists(&self.service.root.join("native-legacy-migration-current.json"))? {
            return Err(err("NATIVE_RESTORE_PENDING"));
        }
        self.maintenance_available()
    }
    pub(crate) fn maintenance_available(&self) -> Result<()> {
        self.payload.backup_resources()?;
        if self.payload.backup_runtime_bytes()? > 32 * 1024 * 1024 {
            return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
        }
        Ok(())
    }
    pub(crate) fn backup_source_current(&self) -> Result<()> {
        let current = Environment(
            self.service
                .native_start_environment(&self.payload, self.ports)?,
        );
        if current.0 != self.environment.0 {
            return Err(err("NATIVE_CONFIGURATION_CHANGED"));
        }
        Ok(())
    }
    pub(crate) fn maintenance_system_identifier(&mut self) -> Result<String> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let value = self.query("SELECT system_identifier::text FROM pg_control_system()")?;
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
        }
        Ok(value)
    }
    pub(crate) fn maintenance_empty_target(&mut self) -> Result<bool> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let empty = self.query_database("exhibitos", "SELECT NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public') AND NOT EXISTS(SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace WHERE n.nspname='public')")? == "t";
        Ok(empty
            && fs::read_dir(self.service.root.join("blobs"))
                .map_err(|_| err("NATIVE_DIRECTORY_UNAVAILABLE"))?
                .next()
                .is_none())
    }
    pub(crate) fn maintenance_owner(
        &mut self,
        kind: &str,
        fields: &BTreeMap<String, String>,
    ) -> Result<()> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let get = |name: &str| {
            fields
                .get(name)
                .ok_or_else(|| err("NATIVE_RESTORE_OWNER_MISMATCH"))
        };
        let tenant = get("TENANT_ID")?;
        if !installations::uuid(tenant) {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        }
        let condition = if kind == "personal" {
            let workspace = get("WORKSPACE_ID")?;
            let principal = get("PRINCIPAL_ID")?;
            let uid = get("OWNER_UID")?
                .parse::<u32>()
                .map_err(|_| err("NATIVE_RESTORE_OWNER_MISMATCH"))?;
            if !installations::uuid(workspace)
                || !installations::uuid(principal)
                || uid == 0
                || uid != unsafe { libc::geteuid() }
            {
                return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
            }
            format!(
                "EXISTS(SELECT 1 FROM local_workspaces w JOIN memberships m ON m.tenant_id=w.tenant_id AND m.user_id=w.user_id JOIN users u ON u.id=w.user_id JOIN tenants t ON t.id=w.tenant_id WHERE w.id='{workspace}'::uuid AND w.tenant_id='{tenant}'::uuid AND w.user_id='{principal}'::uuid AND w.mac_uid={uid} AND m.role='admin' AND NOT u.disabled AND t.deleted_at IS NULL)"
            )
        } else if kind == "legacy_account" {
            let subject = get("ADMIN_SUBJECT")?;
            if subject.is_empty() || subject.len() > 256 || subject.chars().any(char::is_control) {
                return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
            }
            let subject = subject.replace('\'', "''");
            format!(
                "EXISTS(SELECT 1 FROM memberships m JOIN users u ON u.id=m.user_id JOIN tenants t ON t.id=m.tenant_id WHERE m.tenant_id='{tenant}'::uuid AND u.subject='{subject}' AND m.role='admin' AND NOT u.disabled AND t.deleted_at IS NULL)"
            )
        } else {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        };
        if self.query_database("exhibitos", &format!("SELECT {condition}"))? != "t" {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        }
        Ok(())
    }
    pub(crate) fn matches_native_source(&self, root: &Path, manifest_sha256: &str) -> bool {
        self.service.root == root && self.payload.context().manifest_sha256 == manifest_sha256
    }
    pub(crate) fn maintenance_payload(&self) -> Arc<Payload> {
        self.payload.clone()
    }
    /// Fixed rollback-only legacy schema inspector, never an arbitrary script.
    pub(crate) fn inspect_legacy_schema(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 0)
    }
    pub(crate) fn apply_legacy_schema(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 1)
    }
    pub(crate) fn check_legacy_activation(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 2)
    }
    fn legacy_schema_tool(&mut self, input: &[u8], kind: u8) -> Result<Value> {
        if self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
            || input.len() > 16384
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        self.payload.recheck()?;
        let (script, pins) = match kind {
            0 => crate::native_legacy_migration::inspection_resources(&self.payload)?,
            1 => crate::native_legacy_migration::schema_resources(&self.payload)?,
            2 => crate::native_legacy_migration::activation_resources(&self.payload)?,
            _ => return Err(err("NATIVE_LEGACY_OPERATION_INVALID")),
        };
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        self.maintenance_space = vec![self.service.root.clone()];
        let result = self.auxiliary_timed(
            self.payload.node(),
            vec![script.to_string_lossy().into_owned()],
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            Some(input),
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        for pin in pins {
            pin.check()?;
        }
        self.payload.recheck()?;
        match result {
            Ok((0, mut bytes)) => {
                let value = serde_json::from_slice(&bytes)
                    .map_err(|_| err("NATIVE_LEGACY_SCHEMA_RESULT_INVALID"));
                bytes.fill(0);
                value
            }
            Err(error) => Err(error),
            _ => Err(err("NATIVE_LEGACY_INSPECTION_FAILED")),
        }
    }
    /// Closed maintenance entry, callable only while this Session owns the PG
    /// and root fence and has reaped every application writer. Credentials are
    /// supplied through the cleared child's private environment, never argv.
    pub(crate) fn backup_tool(
        &mut self,
        args: Vec<String>,
        configuration: String,
        deployment: String,
        budget_bytes: u64,
        scratch: &Path,
    ) -> Result<Value> {
        if self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        let tools = self.payload.backup_resources()?;
        let resume_identifier = if args.first().is_some_and(|v| v == "reconcile") {
            Some(self.maintenance_system_identifier()?)
        } else {
            None
        };
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        let runtime = self
            .payload
            .script()
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?
            .join("apps/web/dist");
        self.maintenance_space = vec![self.service.root.clone()];
        if let Some(index) = args.iter().position(|v| v == "--destination") {
            let path = args
                .get(index + 1)
                .map(Path::new)
                .and_then(Path::parent)
                .ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?;
            if !self.maintenance_space.iter().any(|p| p == path) {
                self.maintenance_space.push(path.into());
            }
        }
        let mut command = vec![tools.script.to_string_lossy().into_owned()];
        command.extend(args);
        let result = self.auxiliary_timed(
            self.payload.node(),
            command,
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ("PG_DUMP_BIN", tools.pg_dump.to_string_lossy().into_owned()),
                (
                    "PG_RESTORE_BIN",
                    tools.pg_restore.to_string_lossy().into_owned(),
                ),
                (
                    "FREEZE_RUNTIME_ROOT",
                    runtime.to_string_lossy().into_owned(),
                ),
                ("BACKUP_CONFIGURATION_FILES", configuration),
                ("BACKUP_DEPLOYMENT_FILES", deployment),
                (
                    "EXHIBITOS_NATIVE_BACKUP_JOB_BUDGET_BYTES",
                    budget_bytes.to_string(),
                ),
                (
                    "BACKUP_MAX_DUMP_BYTES",
                    (32u64 * 1024 * 1024 * 1024).to_string(),
                ),
                ("TMPDIR", scratch.to_string_lossy().into_owned()),
                (
                    "EXHIBITOS_NATIVE_RESTORE_SYSTEM_IDENTIFIER",
                    resume_identifier.unwrap_or_default(),
                ),
            ],
            None,
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        match result {
            Ok((0, mut bytes)) => {
                let value = serde_json::from_slice(&bytes).map_err(|_| err("NATIVE_BACKUP_FAILED"));
                bytes.fill(0);
                value
            }
            other => {
                // Keep the caller's profile fence until children actually exit.
                while self.any_owned() {
                    self.stop()?;
                    if self.any_owned() {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                match other {
                    Err(error) => Err(error),
                    _ => Err(err("NATIVE_BACKUP_FAILED")),
                }
            }
        }
    }
    pub fn stop(&mut self) -> Result<Context> {
        self.state = "stopping".into();
        self.readiness = Readiness::default();
        let _ = self.record();
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            let changed = self.poll()?;
            if let Some(child) = self.auxiliary.as_mut() {
                child.signal()?;
            } else if let Some(child) = self.node.as_mut() {
                child.signal()?;
            } else if let Some(child) = self.postgres.as_mut() {
                child.signal()?;
            } else {
                self.state = if self.failure.is_some() {
                    "failed"
                } else {
                    "stopped"
                }
                .into();
                if let Err(error) = self.record() {
                    self.state = "stopping".into();
                    return Err(error);
                }
                self.guard = None;
                self.local_authority = None;
                return Ok(self.snapshot());
            }
            if changed {
                let _ = self.record();
            }
            if Instant::now() >= deadline {
                return Ok(self.snapshot());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn status(&mut self) -> Result<Context> {
        self.poll()?;
        if self.state == "stopping" {
            // Advance ordered shutdown without waiting a full stop deadline
            // inside an IPC status request. The ownership fence stays held.
            if let Some(child) = self.auxiliary.as_mut() {
                child.signal()?;
            } else if let Some(child) = self.node.as_mut() {
                child.signal()?;
            } else if let Some(child) = self.postgres.as_mut() {
                child.signal()?;
            } else {
                self.state = if self.failure.is_some() {
                    "failed"
                } else {
                    "stopped"
                }
                .into();
                if let Err(error) = self.record() {
                    self.state = "stopping".into();
                    return Err(error);
                }
                self.guard = None;
                self.local_authority = None;
            }
            return Ok(self.snapshot());
        }
        if (matches!(self.state.as_str(), "ready" | "unavailable")
            && (self.node.is_none() || self.postgres.is_none()))
            || (matches!(self.state.as_str(), "database_ready" | "quiescing")
                && self.postgres.is_none())
        {
            self.failure = Some("NATIVE_PROCESS_EXITED".into());
            return self.stop();
        }
        if matches!(self.state.as_str(), "ready" | "unavailable") {
            let previous = self.state.clone();
            self.readiness = readiness_target(
                &format!("http://127.0.0.1:{}/api/v1/readiness", self.ports.web),
                &[self.ports.web],
                "1.0.0-draft.1",
                "1",
                "0.1.0",
            );
            self.state = if self.readiness.ready {
                "ready"
            } else {
                "unavailable"
            }
            .into();
            if previous != self.state {
                self.record()?;
            }
        }
        Ok(self.snapshot())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Keep the ownership fence until children really stop. A slow stop is
        // never converted to success or a force-kill of an arbitrary PID.
        while self.any_owned() {
            let _ = self.stop();
            if self.any_owned() {
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

impl Session {
    /// Inactive one-shot selected-path readiness. Repeating it closes the
    /// channel. Success is correlation, not claim or reservation authority.
    #[allow(dead_code)]
    fn await_oex_owner_ready(&mut self) -> Result<()> {
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=self.job_id.clone().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.poll()?;
            self.check_native_observation_fence()?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.service.personal_identity()?;
            if self.state!="ready" || self.node.is_none() || self.postgres.is_none()
                || current.owner_uid!=unsafe {libc::geteuid()} || actual!=identity
                || self.job_id.as_deref()!=Some(generation.as_str())
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())
                    != (identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let result=channel.receive_ready(&cancel,&mut check,&identity.0,&identity.1,&identity.2,&generation);
        drop(check);
        match result {
            Ok(())=>{self.owner_channel=Some(channel);Ok(())},
            Err(error)=>{channel.invalidate();Err(error)},
        }
    }
    /// The host supplies only the currently selected, persisted intent. The
    /// Swift caller must retain and match the selected private snapshot too.
    pub(crate) fn select_oex_upload_from_intent(&mut self,state:&crate::installations::native_oex_import::Transfer)->Result<()> {
        crate::installations::native_oex_import::validate(state)?;
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (state.workspace_id.as_str(),state.tenant_id.as_str(),state.principal_id.as_str())!=(auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())
            || state.job_id.is_some() {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let intent=state.intent.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if intent.profile.as_deref()!=Some("bounded-file-v1")||intent.oex_format.as_deref()!=Some("1.0.0-draft.9"){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        self.select_oex_upload(&intent.request_id,intent.bytes,&intent.sha256)
    }
    /// Inactive until the host binds this to the exact durably saved intent
    /// and retained private snapshot. It never selects a path or grants HTTP
    /// authorization by itself. Selection attempts and renewal are one-shot.
    #[allow(dead_code)]
    fn select_oex_upload(&mut self,request:&str,bytes:u64,sha256:&str)->Result<()> {
        if !(self.fresh_oex_test || self.normal_oex_selected)||self.oex_upload_selection_started||self.state!="ready"||self.node.is_none()||self.postgres.is_none(){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        self.oex_upload_selection_started=true;
        let auth=match self.local_authority.as_ref(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_PERSONAL_SESSION_REQUIRED"));
        }};
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=match self.job_id.clone(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }};
        let frame=owner_claim_frames::UploadSelection{version:1,kind:"upload-selection".into(),correlation_id:Uuid::new_v4().to_string(),runtime_generation:generation.clone(),workspace_id:identity.0.clone(),tenant_id:identity.1.clone(),principal_id:identity.2.clone(),request_id:request.into(),archive_bytes:bytes,archive_sha256:sha256.into()};
        let encoded=match frame.encode(){Ok(value)=>value,Err(error)=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.poll()?;self.check_native_observation_fence()?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            if self.cancelled()||self.state!="ready"||self.node.is_none()||self.postgres.is_none()||self.job_id.as_deref()!=Some(generation.as_str())
                || current.owner_uid!=unsafe{libc::geteuid()}||self.service.personal_identity()?!=identity
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())!=(identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}Ok(())
        };
        let response=channel.exchange(&encoded,&cancel,&mut check).and_then(|reply|frame.check_selected(&reply)).and_then(|_|check());drop(check);
        match response {Ok(())=>{self.oex_upload_selected=Some((request.into(),bytes,sha256.into()));self.owner_channel=Some(channel);Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Inactive one-shot claim. The borrow keeps this exact Session alive; on
    /// error/abandonment the socket closes and the API must retain uncertainty.
    #[allow(dead_code)]
    fn begin_owned_oex_claim<'a>(&'a mut self,stage_root:&'a oex_stage_root::StageRoot,job:&str,request:&str,bytes:u64,sha256:&str) -> Result<OwnedOexClaim<'a>> {
        if !(self.fresh_oex_test || self.normal_oex_selected) || self.oex_upload_selected.as_ref().is_none_or(|v|v.0!=request||v.1!=bytes||v.2!=sha256){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=self.job_id.clone().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let open=owner_claim_frames::ClaimOpen{version:1,kind:"claim-open".into(),correlation_id:Uuid::new_v4().to_string(),
            runtime_generation:generation.clone(),workspace_id:identity.0.clone(),tenant_id:identity.1.clone(),job_id:job.to_string()};
        let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            stage_root.check_empty(self)?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.service.personal_identity()?;
            if self.state!="ready" || self.node.is_none() || self.postgres.is_none() || actual!=identity
                || current.owner_uid!=unsafe {libc::geteuid()} || self.job_id.as_deref()!=Some(generation.as_str())
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())
                    != (identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let result=channel.begin_claim(&open,&identity.2,request,bytes,sha256,stage_root.path(),stage_root.device(),0,&cancel,&mut check);
        drop(check);
        match result {
            Ok((challenge,ready,capacity))=>{self.owner_channel=Some(channel);Ok(OwnedOexClaim{session:self,stage_root,challenge,ready,capacity,stage_started:false,retained_packed:None,save_started:false,save_ready:None,settled:false})},
            Err(error)=>{channel.invalidate();Err(error)},
        }
    }
    /// One fresh-selected operation using the exact durable queued intent.
    /// The callback must persist the announced save request and complete the
    /// real same-session GET/one PUT/GET before returning its new ETag. It may
    /// not rearm on error. This routine holds the Session and claim through
    /// selected-flow settlement and completion; failure preserves uncertainty.
    #[allow(dead_code)]
    pub(crate) fn run_fresh_oex_once<F>(&mut self,state:&crate::installations::native_oex_import::Transfer,
        cancelled:&AtomicBool,mut save:F)->Result<()>
    where F:FnMut(&str,&str,&str,&str)->Result<String> {
        crate::installations::native_oex_import::validate(state)?;
        if !(self.fresh_oex_test || self.normal_oex_selected){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (state.workspace_id.as_str(),state.tenant_id.as_str(),state.principal_id.as_str())!=(auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str()) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let intent=state.intent.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if intent.profile.as_deref()!=Some("bounded-file-v1")||intent.oex_format.as_deref()!=Some("1.0.0-draft.9"){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let job=state.job_id.as_deref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        let result=(|| {
        let root=oex_stage_root::StageRoot::create(self)?;
        let mut claim=self.begin_owned_oex_claim(&root,job,&intent.request_id,intent.bytes,&intent.sha256)?;
        let input_root=claim.session.observation_root.try_clone_file()?;
        let chunks=oex_chunks::open_local(&input_root,&claim.ready.tenant_id,&claim.ready.job_id,claim.ready.archive_bytes)?;
        let stage=claim.stage_root.create_input(claim.session,&claim.ready.stage_id)?;
        let (device,inode)=stage.identity()?;let r=&claim.ready;
        let request=oex_materializer_protocol::Request{version:1,kind:"oex-chunks-v1".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),stage_device:device.to_string(),stage_inode:inode.to_string(),chunks:chunks.iter().map(|c|oex_materializer_protocol::Chunk{device:c.device.to_string(),inode:c.inode.to_string(),bytes:c.bytes}).collect(),reservation_digest:r.reservation_digest.clone(),source_digest:r.source_digest.clone(),worker_digest:claim.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
        let correlation=claim.challenge.correlation_id.clone();
        claim.materialize_pack_and_serve(stage,&chunks,&request,&correlation,cancelled)?;
        let ready=claim.wait_save_ready(cancelled)?;
        let etag=save(&ready.exhibition_id,&ready.etag,&ready.request_id,&ready.attempt_id)?;
        claim.confirm_save(&etag,cancelled)
        })();
        if result.is_err(){if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}}
        result
    }
    pub(crate) fn target_observation_root(&self) -> &Path {
        &self.service.root
    }
    pub(crate) fn check_native_observation_fence(&self) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        self.observation_root.check()?;
        let held = self
            .guard
            .as_ref()
            .ok_or_else(|| err("NATIVE_NOT_READY"))?
            .metadata()
            .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        let named = fs::symlink_metadata(self.service.root.join("operation.lock"))
            .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        if !held.is_file()
            || named.is_symlink()
            || held.dev() != named.dev()
            || held.ino() != named.ino()
            || held.nlink() != 1
            || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o777 != 0o600
        {
            return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        Ok(())
    }
    /// A fixed read-only worker, under the existing native session/root fence.
    /// It never runs application bootstrap, migrations, restore or a caller script.
    pub(crate) fn observe_target_inventory(
        &mut self,
        manifest: &[u8],
        manifest_sha256: &str,
        target_schema: &str,
    ) -> Result<Value> {
        if manifest.is_empty()
            || manifest.len() > 16 * 1024 * 1024
            || digest(manifest) != manifest_sha256
            || !hash_valid(target_schema)
            || self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        self.check_native_observation_fence()?;
        self.backup_source_current()?;
        let resources = self.payload.target_inventory_resources()?;
        let system = self.maintenance_system_identifier()?;
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        self.maintenance_space = vec![self.service.root.clone()];
        let result = self.auxiliary_timed(
            self.payload.node(),
            vec![
                "--max-old-space-size=256".into(),
                "--input-type=module".into(),
                "-e".into(),
                include_str!("native_target_inventory.mjs").into(),
                resources.platform_root.to_string_lossy().into_owned(),
                manifest_sha256.into(),
                target_schema.into(),
                system,
                String::from_utf8(resources.migrations.clone())
                    .map_err(|_| err("NATIVE_TARGET_INPUT_INVALID"))?,
                resources.migrations_sha256.clone(),
            ],
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            Some(manifest),
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        match result {
            Ok((0, mut bytes)) => {
                resources.recheck()?;
                self.payload.recheck()?;
                self.check_native_observation_fence()?;
                self.backup_source_current()?;
                if bytes.len() > 16384 {
                    bytes.fill(0);
                    return Err(err("NATIVE_TARGET_RESULT_INVALID"));
                }
                let value =
                    serde_json::from_slice(&bytes).map_err(|_| err("NATIVE_TARGET_RESULT_INVALID"));
                bytes.fill(0);
                value
            }
            other => {
                // A cancellation/timeout is not permission to release a running
                // maintenance child. Reuse the existing normal owned drain.
                while self.any_owned() {
                    self.stop()?;
                    if self.any_owned() {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                resources.recheck()?;
                self.payload.recheck()?;
                match other {
                    Err(e) => Err(e),
                    _ => Err(err("NATIVE_TARGET_OBSERVATION_FAILED")),
                }
            }
        }
    }
}

/// A borrowed Session plus a correlated live-bridge claim. Its selected-flow
/// settlement is not general publication or crash-recovery authority.
#[allow(dead_code)]
struct OwnedOexClaim<'a> {
    session:&'a mut Session,
    stage_root:&'a oex_stage_root::StageRoot,
    challenge:owner_claim_frames::ClaimChallenge,
    ready:owner_claim_frames::ClaimReady,
    capacity:u64,
    stage_started:bool,
    retained_packed:Option<RetainedOexPacked>,
    save_started:bool,
    save_ready:Option<owner_claim_frames::SaveFrame>,
    settled:bool,
}
// Kept with the borrowed Session/claim until settlement or conservative drop.
// These handles are not authority to release on their own.
#[allow(dead_code)]
struct RetainedOexPacked {
    _input:oex_read_fd::ObservedStage,
    _output:File,
    _prepared:oex_worker_v2::Prepared,
    _journal:crate::maintenance_source::DirectoryPin,
    _materializer:oex_materializer::ReapedMaterializer,
    _materializer_request:oex_materializer_protocol::Request,
}
#[allow(dead_code)]
impl OwnedOexClaim<'_> {
    /// One sequential attempt, under this same borrowed Session and claim.
    /// Retains descriptors after drain response; neither response nor return
    /// establishes Node decoder drain, DB commit, or reservation release.
    fn materialize_pack_and_serve(&mut self,stage:oex_read_fd::Stage,chunks:&[oex_chunks::Chunk],
        request:&oex_materializer_protocol::Request,correlation:&str,cancelled:&AtomicBool)->Result<()> {
        use std::os::unix::fs::MetadataExt;
        if self.stage_started||self.retained_packed.is_some()||correlation!=self.challenge.correlation_id {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let (input,materializer)=self.materialize(stage,chunks,request,cancelled)?;
        let r=&self.ready;
        let result=(|| {
            self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;
            let output=self.stage_root.create_output(self.session,&r.attempt_id)?;
            let m=output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            let output_id=(m.dev(),m.ino());let input_id=(input.device,input.inode);
            self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;
            let journal_path=self.session.run.as_ref().ok_or_else(||err("NATIVE_JOURNAL_INVALID"))?.clone();
            let journal=crate::maintenance_source::DirectoryPin::open(&journal_path)?;
            let journal_file=journal.try_clone_file()?;
            let control=oex_control_v2::Control{version:2,kind:"oex-packed-v2".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),capacity:self.capacity,input_device:input.device.to_string(),input_inode:input.inode.to_string(),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),reservation_digest:r.reservation_digest.clone(),source_digest:r.source_digest.clone(),worker_digest:self.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
            let payload=self.session.payload.clone();
            let mut check=|| {
                self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;journal.check()?;
                if self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
                self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()
            };
            let prepared=oex_worker_v2::run_direct(&payload,&journal_file,&input,&output,&control,cancelled,&mut check)?;
            drop(check);
            let cancel=self.session.cancellation.clone();
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {
                self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;journal.check()?;
                if self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())
            };
            let served=channel.serve_packed(&control,&prepared,&output,&r.attempt_id,correlation,&cancel,&mut check);
            drop(check);
            if let Err(error)=served{channel.invalidate();return Err(error);}
            self.session.owner_channel=Some(channel);
            Ok(RetainedOexPacked{_input:input,_output:output,_prepared:prepared,_journal:journal,
                _materializer:materializer,_materializer_request:request.clone()})
        })();
        match result {Ok(retained)=>{self.retained_packed=Some(retained);Ok(())},Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)
        }}
    }
    /// Receive one post-import save offer without releasing this claim/output.
    /// This is not itself permission to mutate a draft or infer COMMIT.
    fn wait_save_ready(&mut self,cancelled:&AtomicBool)->Result<owner_claim_frames::SaveFrame> {
        use std::os::unix::fs::MetadataExt;
        if self.save_started||self.retained_packed.is_none(){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.save_started=true;let retained=self.retained_packed.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let input=(retained._input.device,retained._input.inode);let m=match retained._output.metadata(){Ok(value)=>value,Err(_)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }};let output=(m.dev(),m.ino());
        let stamp=retained._prepared.verified.output_stamp;let r=&self.ready;let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.stage_root.check_pair(self.session,&r.stage_id,input,&r.attempt_id,output)?;retained._journal.check()?;
            let m=retained._output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            if (m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())!=stamp
                || self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}Ok(())
        };
        let result=channel.receive_save_ready(&r.attempt_id,&cancel,&mut check);drop(check);
        match result {Ok(ready)=>{self.save_ready=Some(ready.clone());self.session.owner_channel=Some(channel);Ok(ready)},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Caller invokes only after the real same-session GET/one PUT/GET has
    /// returned and the exact pending request/draft is durably retained.
    fn confirm_save(&mut self,etag:&str,cancelled:&AtomicBool)->Result<()> {
        use std::os::unix::fs::MetadataExt;
        let ready=self.save_ready.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let retained=match self.retained_packed.as_ref(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }};
        let input=(retained._input.device,retained._input.inode);let m=match retained._output.metadata(){Ok(value)=>value,Err(_)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }};let output=(m.dev(),m.ino());
        let stamp=retained._prepared.verified.output_stamp;let r=&self.ready;let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let trace_log=self.session.log.clone();
        let mut trace=|message: &'static [u8]| { if let Some(log)=&trace_log { log.write(message); } };
        let materializer_request=&retained._materializer_request;
        let materializer_bytes=materializer_request.encode()?;
        let packed:oex_control_v2::Control=serde_json::from_slice(&retained._prepared.control).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let started=self.stage_started;
        let mut check=|| {
            if !started || !retained._materializer.matches(materializer_request,&materializer_bytes)
                || packed.encode()?!=retained._prepared.control || packed.attempt_id!=r.attempt_id
                || packed.runtime_generation!=r.runtime_generation || packed.reservation_id!=r.reservation_id
                || packed.reservation_generation!=r.reservation_generation || packed.reservation_digest!=r.reservation_digest
                || packed.stage_id!=r.stage_id || packed.archive_sha256!=r.archive_sha256 {
                return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
            }
            self.stage_root.check_pair(self.session,&r.stage_id,input,&r.attempt_id,output)?;retained._journal.check()?;
            let m=retained._output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            if (m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())!=stamp
                || self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}Ok(())
        };
        let result=channel.confirm_save(&ready,&self.challenge,etag,&cancel,&mut check,&mut trace);drop(check);
        match result {Ok(())=>{self.session.owner_channel=Some(channel);self.settled=true;Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Inactive connection of the same claimed Session to the directly owned
    /// materializer. The caller must create the stage in the independently
    /// pinned live provider reservation and open source chunks from the exact
    /// authenticated job root. A correlated result is still not a stage seal,
    /// DB commit, claim release, or proof of absent noncooperating writers.
    fn materialize(&mut self,stage:oex_read_fd::Stage,chunks:&[oex_chunks::Chunk],
        request:&oex_materializer_protocol::Request,cancelled:&AtomicBool)->Result<(oex_read_fd::ObservedStage,oex_materializer::ReapedMaterializer)> {
        let r=&self.ready;
        if self.stage_started || !stage.matches_stage(&r.stage_id)
            || request.tenant_id!=r.tenant_id || request.resource_id!=r.workspace_id || request.job_id!=r.job_id
            || request.user_id!=r.user_id || request.request_id!=r.request_id || request.lease_id!=r.lease_id
            || request.attempt_id!=r.attempt_id || request.runtime_generation!=r.runtime_generation
            || request.stage_id!=r.stage_id || request.reservation_id!=r.reservation_id
            || request.reservation_generation!=r.reservation_generation || request.archive_bytes!=r.archive_bytes
            || request.archive_sha256!=r.archive_sha256 || request.source_digest!=r.source_digest
            || request.reservation_digest!=r.reservation_digest || request.worker_digest!=self.session.payload.context().manifest_sha256 {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let (device,inode)=match stage.identity(){Ok(id)=>id,Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};
        if let Err(error)=self.stage_root.check_input(self.session,&r.stage_id,device,inode) {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }
        if request.stage_device!=device.to_string() || request.stage_inode!=inode.to_string() {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        self.stage_started=true;
        let stage_bound=owner_claim_frames::StageBound {version:1,kind:"stage-bound".into(),correlation_id:self.challenge.correlation_id.clone(),
            runtime_generation:r.runtime_generation.clone(),workspace_id:r.workspace_id.clone(),tenant_id:r.tenant_id.clone(),
            job_id:r.job_id.clone(),attempt_id:r.attempt_id.clone(),generation:r.generation.clone(),stage_id:r.stage_id.clone(),
            reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),
            stage_device:device.to_string(),stage_inode:inode.to_string()};
        let bytes=match stage_bound.encode(){Ok(bytes)=>bytes,Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
            let auth=self.session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.session.service.personal_identity()?;
            if self.session.cancelled() || cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
            if self.session.state!="ready" || self.session.node.is_none() || self.session.postgres.is_none()
                || self.session.job_id.as_deref()!=Some(r.runtime_generation.as_str()) || auth.owner_uid!=unsafe {libc::geteuid()}
                || actual!=(r.workspace_id.clone(),r.tenant_id.clone(),r.user_id.clone())
                || (auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())!=(r.workspace_id.as_str(),r.tenant_id.as_str(),r.user_id.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let response=channel.exchange(&bytes,&cancel,&mut check)
            .and_then(|reply|stage_bound.check_accepted(&bytes,&reply)).and_then(|_|check());
        drop(check);
        if let Err(error)=response {channel.invalidate();return Err(error);}
        self.session.owner_channel=Some(channel);
        let payload=self.session.payload.clone();
        let mut check=|| {
            self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
            self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()?;
            let auth=self.session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.session.service.personal_identity()?;
            if self.session.cancelled() || cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
            if self.session.state!="ready" || self.session.node.is_none() || self.session.postgres.is_none()
                || self.session.job_id.as_deref()!=Some(r.runtime_generation.as_str()) || auth.owner_uid!=unsafe {libc::geteuid()}
                || actual!=(r.workspace_id.clone(),r.tenant_id.clone(),r.user_id.clone())
                || (auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())!=(r.workspace_id.as_str(),r.tenant_id.as_str(),r.user_id.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let observed=(|| {
            let reaped=oex_materializer::run_direct(&payload,&stage,chunks,request,cancelled,&mut check)?;
            if !reaped.matches(request,&request.encode()?) {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            let observed=oex_read_fd::observe_stage(stage,r.archive_bytes,&r.archive_sha256,cancelled,&mut check)?;
            Ok((observed,reaped))
        })();
        drop(check);
        if observed.is_err() {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
        }
        observed
    }
}
impl Drop for OwnedOexClaim<'_> {
    fn drop(&mut self) {
        // Any path without both settlement and operation-complete remains
        // uncertain: close the channel and preserve the durable records.
        if !self.settled {if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}}
    }
}
