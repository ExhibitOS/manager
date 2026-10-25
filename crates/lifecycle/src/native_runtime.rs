// SPDX-License-Identifier: Apache-2.0
//! Owned PostgreSQL/Node lifecycle for a prepared native installation. No shell,
//! container engine, PATH lookup, PID adoption, or destructive recovery.
use super::*;
use super::{native_installation::Ports, native_payload::Payload};
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
            if unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
            {
                return Err(err("NATIVE_STOP_UNAVAILABLE"));
            }
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
    job_id: Option<String>,
    run: Option<PathBuf>,
    state: String,
    failure: Option<String>,
    exits: BTreeMap<String, i32>,
    readiness: Readiness,
    cancellation: Arc<AtomicBool>,
    local_authority: Option<LocalAuthority>,
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
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(err("NATIVE_JOURNAL_UNAVAILABLE")),
    }
}
impl Session {
    pub fn new(service: &LifecycleService, payload: Arc<Payload>, ports: Ports) -> Result<Self> {
        installations::private_directory(&service.root)?;
        let guard = service.lock()?;
        Self::new_with_guard(service, payload, ports, guard)
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
            job_id: None,
            run: None,
            state: "stopped".into(),
            failure: None,
            exits: BTreeMap::new(),
            readiness: Readiness::default(),
            cancellation: Arc::new(AtomicBool::new(false)),
            local_authority: None,
            maintenance_space: Vec::new(),
        })
    }
    /// Explicit private IPC only, never part of status/journal/log output.
    pub fn authoring_session(&mut self) -> Result<Value> {
        if self.status()?.state != "ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
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
        Ok(changed)
    }
    fn any_owned(&self) -> bool {
        self.postgres.is_some() || self.node.is_some() || self.auxiliary.is_some()
    }
    fn check_interrupted(&self) -> Result<()> {
        let pointer = self.service.root.join("native-current-run.json");
        if exists(&pointer)? {
            let current: Current = serde_json::from_slice(&installation_backup::source_bytes(
                &self.service.root,
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
                &self.service.root,
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
        let postmaster = self.service.root.join("native-database/postmaster.pid");
        if exists(&postmaster)? {
            let bytes = installation_backup::source_bytes(
                &self.service.root,
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
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        extra: &[(&str, String)],
        input: Option<&[u8]>,
        capture: bool,
        marker: bool,
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
        self.auxiliary = Some(self.spawn(&program, &args, &extra, input, true, false)?);
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
        let mut private_input = match &self.local_authority {
            Some(a) => Some(serde_json::to_vec(a).map_err(|_| err("STATE_INVALID"))?),
            None => None,
        };
        let spawned = self.spawn(
            &self.payload.node(),
            &[self.payload.script().to_string_lossy().into()],
            &environment,
            private_input.as_deref(),
            false,
            true,
        );
        if let Some(bytes) = private_input.as_mut() {
            bytes.fill(0);
        }
        self.node = Some(spawned?);
        self.record()?;
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
        if services && (exists(&self.service.root.join("native-restore-current.json"))?
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
        self.check_interrupted()?;
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
            } else {
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
                let record = self.record();
                self.guard = None;
                self.local_authority = None;
                record?;
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
                let recorded = self.record();
                self.guard = None;
                self.local_authority = None;
                recorded?;
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
