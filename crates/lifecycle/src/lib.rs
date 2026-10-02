// SPDX-License-Identifier: Apache-2.0
//! Trusted-bundle desktop lifecycle. No shell, arbitrary compose paths or destructive volume removal.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Start,
    Stop,
    Restart,
    Retry,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Completed,
    Failed,
    Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub action: Action,
    pub state: JobState,
    pub attempt: u32,
    pub progress: u8,
    pub created_at: u64,
    pub updated_at: u64,
    pub error_code: Option<String>,
    pub guidance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbe {
    pub kind: String,
    pub installed: bool,
    pub available: bool,
    pub engine_version: Option<String>,
    pub compose_version: Option<String>,
    pub error_code: Option<String>,
    pub guidance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub name: String,
    pub state: String,
    pub health: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub ready: bool,
    pub version: Option<String>,
    pub protocol_version: Option<String>,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
    pub used_bytes: Option<u64>,
    pub free_bytes: u64,
    pub minimum_free_bytes: u64,
    pub quota_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub state: String,
    pub services: Vec<ServiceState>,
    pub readiness: Readiness,
    pub storage: StorageState,
    pub active_job: Option<Job>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub at: u64,
    pub job_id: Option<String>,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Archive {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Image {
    pub reference: String,
    pub archive: Option<Archive>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleManifest {
    pub schema_version: String,
    pub bundle_id: String,
    pub version: String,
    pub protocol_version: String,
    pub compose_sha256: String,
    #[serde(default)]
    pub preferred_engine: Option<String>,
    pub project_name: String,
    pub services: Vec<String>,
    pub images: Vec<Image>,
    pub ports: Vec<u16>,
    pub open_url: String,
    pub readiness_url: String,
    pub minimum_free_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleError {
    pub code: String,
    pub guidance: String,
}
impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for LifecycleError {}
type Result<T> = std::result::Result<T, LifecycleError>;
// Docker reports sha256-prefixed IDs; Podman reports the same digest without a prefix.
fn same_local_image_id(actual: &str, reference: &str) -> bool {
    let Some(expected) = reference.strip_prefix("sha256:") else {
        return false;
    };
    let actual = actual.strip_prefix("sha256:").unwrap_or(actual);
    hash_valid(expected) && hash_valid(actual) && actual == expected
}
fn err(code: &str) -> LifecycleError {
    let guidance = match code {
        "RUNTIME_MISSING" => "컨테이너 실행 도구를 설치한 후 다시 시도하세요.",
        "ENGINE_UNAVAILABLE" => "컨테이너 실행 도구를 시작한 후 다시 시도하세요.",
        "ENGINE_PERMISSION" => "실행 도구 접근 권한을 확인한 후 다시 시도하세요.",
        "PORT_IN_USE" => {
            "전시에 사용할 포트를 다른 앱이 사용하고 있습니다. 해당 앱을 종료한 후 다시 시도하세요."
        }
        "STORAGE_QUOTA" => "저장 공간이 부족합니다. 공간을 확보한 후 다시 시도하세요.",
        "BUSY" => "진행 중인 작업이 끝날 때까지 기다려 주세요.",
        "VERSION_MISMATCH" => "호환되는 전시 실행 패키지가 필요합니다.",
        "BUNDLE_INVALID" | "BUNDLE_CHANGED" => "검증된 설치 패키지를 다시 준비해 주세요.",
        "READINESS_TIMEOUT" => "서버 준비가 끝나지 않았습니다. 상태를 확인한 후 다시 시도하세요.",
        _ => "작업을 완료하지 못했습니다. 상태를 확인한 후 다시 시도하세요.",
    };
    LifecycleError {
        code: code.into(),
        guidance: guidance.into(),
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn same_registry_pin(actual: &str, expected: &str) -> bool {
    fn normalize(repo: &str) -> String {
        let repo = repo.strip_prefix("docker.io/").unwrap_or(repo);
        let repo = repo.strip_prefix("library/").unwrap_or(repo);
        if let Some((base, tag)) = repo.rsplit_once(':')
            && !tag.contains('/')
        {
            return base.to_string();
        }
        repo.to_string()
    }
    match (
        actual.split_once("@sha256:"),
        expected.split_once("@sha256:"),
    ) {
        (Some((a, h)), Some((b, k))) => hash_valid(h) && h == k && normalize(a) == normalize(b),
        _ => false,
    }
}
fn hash_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn safe_relative(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 240
        && !s.contains('\\')
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !s.starts_with('/')
}
fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}
fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    if !safe_relative(relative) {
        return Err(err("BUNDLE_INVALID"));
    }
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part);
        let m = fs::symlink_metadata(&path).map_err(|_| err("BUNDLE_INVALID"))?;
        if m.file_type().is_symlink() {
            return Err(err("BUNDLE_INVALID"));
        }
    }
    Ok(path)
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    let m = fs::symlink_metadata(path).map_err(|_| err("STATE_UNAVAILABLE"))?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 4 * 1024 * 1024 {
        return Err(err("STATE_INVALID"));
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| err("STATE_UNAVAILABLE"))?)
        .map_err(|_| err("STATE_INVALID"))
}
fn write_json<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value).map_err(|_| err("STATE_INVALID"))?;
    if payload.len() > 4 * 1024 * 1024 {
        return Err(err("JOB_HISTORY_FULL"));
    }
    let temp = root.join(format!(".{}.tmp", Uuid::new_v4()));
    let mut f = private_options()
        .open(&temp)
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    f.write_all(&payload)
        .and_then(|_| f.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    fs::rename(&temp, root.join(name)).map_err(|_| err("STATE_UNAVAILABLE"))?;
    Ok(())
}
fn engine_executable(kind: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute())
            .map(|p| {
                p.join(if cfg!(windows) {
                    format!("{kind}.exe")
                } else {
                    kind.to_string()
                })
            })
            .collect();
    #[cfg(target_os = "macos")]
    {
        for dir in [
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/Applications/Docker.app/Contents/Resources/bin",
        ] {
            candidates.push(Path::new(dir).join(kind));
        }
    }
    #[cfg(windows)]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let base = PathBuf::from(program_files);
            candidates.push(if kind == "docker" {
                base.join("Docker/Docker/resources/bin/docker.exe")
            } else {
                base.join("RedHat/Podman/podman.exe")
            });
        }
    }
    candidates
        .into_iter()
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))
}
// Both streams are drained, but output is capped. Raw engine errors never leave this module.
fn run(binary: &str, args: &[String], cwd: Option<&Path>, seconds: u64) -> Result<Vec<u8>> {
    let executable = if matches!(binary, "docker" | "podman") {
        engine_executable(binary).ok_or_else(|| err("RUNTIME_MISSING"))?
    } else {
        PathBuf::from(binary)
    };
    let mut command = Command::new(executable);
    if matches!(binary, "docker" | "podman") {
        let mut paths: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .filter(|p| p.is_absolute())
                .collect();
        for path in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"] {
            if !paths.iter().any(|p| p == Path::new(path)) {
                paths.push(path.into());
            }
        }
        if let Ok(value) = std::env::join_paths(paths) {
            command.env("PATH", value);
        }
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            err("RUNTIME_MISSING")
        } else {
            err("ENGINE_PERMISSION")
        }
    })?;
    let (tx, rx) = mpsc::channel();
    for (index, mut stream) in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
        Box::new(child.stderr.take().unwrap()) as Box<dyn Read + Send>,
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut data = Vec::new();
            let mut buf = [0; 8192];
            let mut overflow = false;
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if data.len() + n <= 4 * 1024 * 1024 {
                            data.extend_from_slice(&buf[..n]);
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            let _ = tx.send((index, data, overflow));
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err("ENGINE_TIMEOUT"));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(err("ENGINE_OPERATION_FAILED")),
        }
    };
    let a = rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| err("ENGINE_TIMEOUT"))?;
    let b = rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| err("ENGINE_TIMEOUT"))?;
    if a.2 || b.2 {
        return Err(err("ENGINE_OUTPUT_LIMIT"));
    }
    if !status.success() {
        let text = format!(
            "{} {}",
            String::from_utf8_lossy(&a.1),
            String::from_utf8_lossy(&b.1)
        )
        .to_lowercase();
        return Err(err(if text.contains("permission denied") {
            "ENGINE_PERMISSION"
        } else if text.contains("address already in use")
            || text.contains("port is already allocated")
        {
            "PORT_IN_USE"
        } else {
            "ENGINE_OPERATION_FAILED"
        }));
    }
    // Stream identity is preserved below by forwarding a tagged tuple (stdout tag true).
    Ok(if a.0 == 0 { a.1 } else { b.1 })
}
fn compose_args(m: &BundleManifest, tail: &[&str]) -> Vec<String> {
    let mut a = vec![
        "compose".into(),
        "--env-file".into(),
        "../runtime.env".into(),
        "--project-name".into(),
        m.project_name.clone(),
        "--file".into(),
        "compose.yaml".into(),
    ];
    a.extend(tail.iter().map(|x| x.to_string()));
    a
}
fn loopback_url(input: &str, ports: &[u16]) -> Result<(SocketAddr, String)> {
    let rest = input
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| err("BUNDLE_INVALID"))?;
    let (port, path) = rest
        .split_once('/')
        .map(|(p, x)| (p, format!("/{x}")))
        .unwrap_or((rest, "/".into()));
    let port = port.parse::<u16>().map_err(|_| err("BUNDLE_INVALID"))?;
    if !ports.contains(&port)
        || path.contains(['\r', '\n', '?', '#', '\\'])
        || path.contains("..")
        || path.len() > 240
    {
        return Err(err("BUNDLE_INVALID"));
    }
    Ok((SocketAddr::from(([127, 0, 0, 1], port)), path))
}
fn readiness(m: &BundleManifest) -> Readiness {
    let attempt = (|| -> Result<Readiness> {
        let (address, path) = loopback_url(&m.readiness_url, &m.ports)?;
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        write!(
            socket,
            "GET {path} HTTP/1.0\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        let mut bytes = Vec::new();
        socket
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        if bytes.len() > 65536 {
            return Err(err("READINESS_INVALID"));
        }
        let response = String::from_utf8(bytes).map_err(|_| err("READINESS_INVALID"))?;
        if !response.starts_with("HTTP/1.1 200 ") && !response.starts_with("HTTP/1.0 200 ") {
            return Err(err("READINESS_UNAVAILABLE"));
        }
        let (_, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| err("READINESS_INVALID"))?;
        let v: Value = serde_json::from_str(body).map_err(|_| err("READINESS_INVALID"))?;
        if v["schemaVersion"] != m.schema_version
            || v["protocolVersion"] != m.protocol_version
            || v["platformVersion"] != m.version
        {
            return Err(err("VERSION_MISMATCH"));
        }
        let service_ready = v["services"].as_array().is_some_and(|entries| {
            entries.len() == 5
                && ["platform", "api", "database", "web", "storage"]
                    .iter()
                    .all(|name| {
                        entries
                            .iter()
                            .filter(|entry| entry["name"] == *name && entry["status"] == "ready")
                            .count()
                            == 1
                    })
        });
        Ok(Readiness {
            ready: v["ready"] == true && service_ready,
            version: Some(m.version.clone()),
            protocol_version: Some(m.protocol_version.clone()),
            error_code: None,
        })
    })();
    attempt.unwrap_or_else(|e| Readiness {
        error_code: Some(e.code),
        ..Default::default()
    })
}

pub struct LifecycleService {
    root: PathBuf,
}
impl LifecycleService {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(err("STATE_INVALID"));
        }
        fs::create_dir_all(&root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        let m = fs::symlink_metadata(&root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(err("STATE_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        let service = Self { root };
        service.recover_jobs()?;
        Ok(service)
    }
    fn lock(&self) -> Result<File> {
        let path = self.root.join("operation.lock");
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(|_| err("STATE_INVALID"))?
                .file_type()
                .is_symlink()
        {
            return Err(err("STATE_INVALID"));
        }
        let file = match private_options().open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|_| err("STATE_UNAVAILABLE"))?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        file.try_lock_exclusive().map_err(|_| err("BUSY"))?;
        Ok(file)
    }
    fn manifest(&self) -> Result<BundleManifest> {
        let path = checked_path(&self.root, "bundle/manifest.json")?;
        let m: BundleManifest = read_json(&path)?;
        if m.schema_version != "1.0.0-draft.1"
            || m.protocol_version != "1"
            || !name(&m.bundle_id)
            || !name(&m.project_name)
            || !m.project_name.starts_with("exhibitos-")
            || m.preferred_engine
                .as_ref()
                .is_some_and(|v| !matches!(v.as_str(), "docker" | "podman"))
            || m.services.is_empty()
            || m.services.len() > 16
            || !m.services.iter().all(|s| name(s))
            || m.images.is_empty()
            || m.images.len() > 16
            || m.ports.is_empty()
            || m.ports.iter().any(|p| *p < 1024)
            || !hash_valid(&m.compose_sha256)
            || m.version.len() > 64
            || m.version.is_empty()
        {
            return Err(err("BUNDLE_INVALID"));
        }
        loopback_url(&m.open_url, &m.ports)?;
        loopback_url(&m.readiness_url, &m.ports)?;
        for i in &m.images {
            let (repository, d) = if let Some(d) = i.reference.strip_prefix("sha256:") {
                ("local", d)
            } else if let Some(pair) = i.reference.split_once("@sha256:") {
                pair
            } else {
                return Err(err("BUNDLE_INVALID"));
            };
            if !hash_valid(d)
                || repository.is_empty()
                || !repository
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/._-:".contains(&c))
            {
                return Err(err("BUNDLE_INVALID"));
            }
        }
        let compose = fs::read(checked_path(&self.root, "bundle/compose.yaml")?)
            .map_err(|_| err("BUNDLE_INVALID"))?;
        if compose.len() > 256 * 1024 || digest(&compose) != m.compose_sha256 {
            return Err(err("BUNDLE_CHANGED"));
        }
        Ok(m)
    }
    fn engine(&self, m: &BundleManifest, install: bool) -> Result<String> {
        let path = self.root.join("engine.json");
        if path.exists() {
            let kind: String = read_json(&path)?;
            if matches!(kind.as_str(), "docker" | "podman") {
                return Ok(kind);
            }
            return Err(err("STATE_INVALID"));
        }
        if !install {
            return Err(err("NOT_INSTALLED"));
        }
        let probes = self.detect()?;
        let preferred = m.preferred_engine.as_deref().unwrap_or("podman");
        let chosen = probes
            .iter()
            .find(|p| p.kind == preferred && p.available)
            .or_else(|| probes.iter().find(|p| p.available))
            .ok_or_else(|| {
                err(if probes.iter().any(|p| p.installed) {
                    "ENGINE_UNAVAILABLE"
                } else {
                    "RUNTIME_MISSING"
                })
            })?;
        write_json(&self.root, "engine.json", &chosen.kind)?;
        Ok(chosen.kind.clone())
    }
    fn runtime_env(&self, m: &BundleManifest) -> Result<()> {
        let path = self.root.join("runtime.env");
        if path.exists() {
            let info = fs::symlink_metadata(&path).map_err(|_| err("SECRET_FILE_INVALID"))?;
            if !info.is_file() || info.file_type().is_symlink() || info.len() > 8192 {
                return Err(err("SECRET_FILE_INVALID"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if info.permissions().mode() & 0o777 != 0o600 {
                    return Err(err("SECRET_FILE_INVALID"));
                }
            }
            return Ok(());
        }
        let secret = || {
            format!(
                "{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            )
        };
        let password = secret();
        let contents = format!(
            "EXHIBITOS_PORT={}\nPOSTGRES_PASSWORD={}\nDATABASE_URL=postgresql://exhibitos:{}@database:5432/exhibitos\nADMIN_SUBJECT=local-admin\nADMIN_PASSWORD={}\nTENANT_ID={}\n",
            m.ports[0],
            password,
            password,
            secret(),
            Uuid::new_v4()
        );
        let mut file = private_options()
            .open(path)
            .map_err(|_| err("SECRET_FILE_INVALID"))?;
        file.write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| err("SECRET_FILE_INVALID"))?;
        Ok(())
    }
    pub fn detect(&self) -> Result<Vec<EngineProbe>> {
        let mut probes = Vec::new();
        for kind in ["podman", "docker"] {
            let version = run(
                kind,
                &[
                    "version".into(),
                    "--format".into(),
                    if kind == "docker" {
                        "{{json .}}".into()
                    } else {
                        "json".into()
                    },
                ],
                None,
                10,
            );
            let probe = match version {
                Err(e) => EngineProbe {
                    kind: kind.into(),
                    installed: e.code != "RUNTIME_MISSING",
                    available: false,
                    engine_version: None,
                    compose_version: None,
                    error_code: Some(e.code),
                    guidance: Some(e.guidance),
                },
                Ok(data) => {
                    let v: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
                    let version = v["Client"]["Version"]
                        .as_str()
                        .or_else(|| v["client"]["version"].as_str())
                        .or_else(|| v["Version"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let engine_ready = run(
                        kind,
                        &[
                            "info".into(),
                            "--format".into(),
                            if kind == "docker" {
                                "{{json .}}".into()
                            } else {
                                "json".into()
                            },
                        ],
                        None,
                        10,
                    )
                    .is_ok();
                    let compose = run(
                        kind,
                        &["compose".into(), "version".into(), "--short".into()],
                        None,
                        10,
                    );
                    let cv = compose
                        .ok()
                        .and_then(|v| String::from_utf8(v).ok())
                        .map(|v| v.trim().trim_start_matches('v').to_string());
                    let supported = engine_ready
                        && version
                            .split('.')
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .is_some_and(|v| v >= if kind == "docker" { 24 } else { 5 })
                        && cv.as_ref().is_some_and(|v| {
                            v.split('.')
                                .next()
                                .and_then(|v| v.parse::<u32>().ok())
                                .is_some_and(|v| v >= 2)
                        });
                    EngineProbe {
                        kind: kind.into(),
                        installed: true,
                        available: supported,
                        engine_version: Some(version),
                        compose_version: cv,
                        error_code: if supported {
                            None
                        } else {
                            Some("VERSION_MISMATCH".into())
                        },
                        guidance: if supported {
                            None
                        } else {
                            Some(err("VERSION_MISMATCH").guidance)
                        },
                    }
                }
            };
            probes.push(probe);
        }
        Ok(probes)
    }
    fn recover_jobs(&self) -> Result<()> {
        let Ok(_guard) = self.lock() else {
            return Ok(());
        };
        let mut jobs = self.job_history()?;
        let mut changed = false;
        for job in &mut jobs {
            if job.state == JobState::Running {
                job.state = JobState::Interrupted;
                job.updated_at = now();
                job.error_code = Some("INTERRUPTED".into());
                job.guidance = Some(err("INTERRUPTED").guidance);
                changed = true;
            }
        }
        if changed {
            write_json(&self.root, "jobs.json", &jobs)?;
        }
        Ok(())
    }
    fn job_history(&self) -> Result<Vec<Job>> {
        let p = self.root.join("jobs.json");
        if p.exists() {
            read_json(&p)
        } else {
            Ok(Vec::new())
        }
    }
    fn event_history(&self) -> Result<Vec<LogEvent>> {
        let p = self.root.join("events.json");
        if p.exists() {
            read_json(&p)
        } else {
            Ok(Vec::new())
        }
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        let jobs = self.job_history()?;
        Ok(jobs
            .into_iter()
            .rev()
            .take(200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
    pub fn logs(&self) -> Result<Vec<LogEvent>> {
        let events = self.event_history()?;
        Ok(events
            .into_iter()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
    fn event(&self, job: &Job, code: &str, message: &str) -> Result<()> {
        let mut events = self.event_history()?;
        events.push(LogEvent {
            at: now(),
            job_id: Some(job.id.clone()),
            code: code.into(),
            message: message.into(),
        });
        write_json(&self.root, "events.json", &events)
    }
    pub fn install(&self) -> Result<Job> {
        self.execute(Action::Install)
    }
    pub fn execute(&self, action: Action) -> Result<Job> {
        let _lock = self.lock()?;
        let mut jobs = self.job_history()?;
        for j in &mut jobs {
            if j.state == JobState::Running {
                j.state = JobState::Interrupted;
                j.error_code = Some("INTERRUPTED".into());
                j.guidance = Some(err("INTERRUPTED").guidance);
                j.updated_at = now();
            }
        }
        if jobs.len() >= 10000 {
            return Err(err("JOB_HISTORY_FULL"));
        }
        let mut job = if action == Action::Retry {
            let Some(previous) = jobs
                .last()
                .filter(|j| matches!(j.state, JobState::Failed | JobState::Interrupted))
            else {
                return Err(err("NOTHING_TO_RETRY"));
            };
            if previous.attempt >= 5 {
                return Err(err("RETRY_LIMIT"));
            }
            Job {
                attempt: previous.attempt + 1,
                id: previous.id.clone(),
                action: previous.action.clone(),
                state: JobState::Running,
                progress: 0,
                created_at: previous.created_at,
                updated_at: now(),
                error_code: None,
                guidance: None,
            }
        } else {
            Job {
                id: Uuid::new_v4().to_string(),
                action,
                state: JobState::Running,
                attempt: 1,
                progress: 0,
                created_at: now(),
                updated_at: now(),
                error_code: None,
                guidance: None,
            }
        };
        let index = jobs.len();
        jobs.push(job.clone());
        write_json(&self.root, "jobs.json", &jobs)?;
        self.event(&job, "STARTED", "작업을 시작했습니다.")?;
        let result = self.operation(&job.action);
        job.updated_at = now();
        match result {
            Ok(()) => {
                job.state = JobState::Completed;
                job.progress = 100;
                self.event(&job, "COMPLETED", "작업이 완료되었습니다.")?;
            }
            Err(e) => {
                job.state = JobState::Failed;
                job.error_code = Some(e.code.clone());
                job.guidance = Some(e.guidance.clone());
                self.event(&job, &e.code, &e.guidance)?;
            }
        }
        jobs[index] = job.clone();
        write_json(&self.root, "jobs.json", &jobs)?;
        Ok(job)
    }
    fn operation(&self, action: &Action) -> Result<()> {
        let m = self.manifest()?;
        let engine = self.engine(&m, *action == Action::Install)?;
        let probe = self
            .detect()?
            .into_iter()
            .find(|p| p.kind == engine)
            .ok_or_else(|| err("RUNTIME_MISSING"))?;
        if !probe.available {
            return Err(err(probe
                .error_code
                .as_deref()
                .unwrap_or("ENGINE_UNAVAILABLE")));
        }
        if *action != Action::Stop
            && fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < m.minimum_free_bytes
        {
            return Err(err("STORAGE_QUOTA"));
        }
        let bundle = self.root.join("bundle");
        self.runtime_env(&m)?;
        self.validate_compose(&m, &engine)?;
        if *action == Action::Install {
            for image in &m.images {
                if let Some(a) = &image.archive {
                    let p = checked_path(&bundle, &a.path)?;
                    let metadata = fs::metadata(&p).map_err(|_| err("BUNDLE_INVALID"))?;
                    if !metadata.is_file()
                        || a.bytes == 0
                        || metadata.len() != a.bytes
                        || a.bytes > 8 * 1024 * 1024 * 1024
                        || !hash_valid(&a.sha256)
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                    let mut f = File::open(&p).map_err(|_| err("BUNDLE_INVALID"))?;
                    let mut h = Sha256::new();
                    let mut b = [0; 65536];
                    loop {
                        let n = f.read(&mut b).map_err(|_| err("BUNDLE_INVALID"))?;
                        if n == 0 {
                            break;
                        }
                        h.update(&b[..n]);
                    }
                    if format!("{:x}", h.finalize()) != a.sha256 {
                        return Err(err("BUNDLE_CHANGED"));
                    }
                    run(
                        &engine,
                        &[
                            "load".into(),
                            "--input".into(),
                            p.to_string_lossy().into_owned(),
                        ],
                        None,
                        180,
                    )?;
                }
                if image.archive.is_none() {
                    if image.reference.starts_with("sha256:") {
                        return Err(err("BUNDLE_INVALID"));
                    }
                    run(
                        &engine,
                        &["pull".into(), image.reference.clone()],
                        None,
                        180,
                    )?;
                }
                let inspected: Value = serde_json::from_slice(&run(
                    &engine,
                    &["image".into(), "inspect".into(), image.reference.clone()],
                    None,
                    15,
                )?)
                .map_err(|_| err("IMAGE_INTEGRITY"))?;
                if image.reference.starts_with("sha256:")
                    && !inspected[0]["Id"]
                        .as_str()
                        .is_some_and(|id| same_local_image_id(id, &image.reference))
                {
                    return Err(err("IMAGE_INTEGRITY"));
                }
                if image.reference.contains("@sha256:")
                    && !inspected[0]["RepoDigests"].as_array().is_some_and(|v| {
                        v.iter().any(|d| {
                            d.as_str()
                                .is_some_and(|value| same_registry_pin(value, &image.reference))
                        })
                    })
                {
                    return Err(err("IMAGE_INTEGRITY"));
                }
            }
            write_json(&self.root, "installed.json", &m)?;
            return Ok(());
        }
        let installed: BundleManifest = read_json(&self.root.join("installed.json"))?;
        if serde_json::to_value(&installed).ok() != serde_json::to_value(&m).ok() {
            return Err(err("BUNDLE_CHANGED"));
        }
        self.validate_ownership(&m, &engine)?;
        self.validate_volumes(&m, &engine)?;
        let tail = match action {
            Action::Start => vec!["up", "--detach", "--no-build", "--pull", "never"],
            Action::Stop => vec!["stop", "--timeout", "30"],
            Action::Restart => vec!["restart", "--timeout", "30"],
            _ => return Err(err("INVALID_ACTION")),
        };
        run(&engine, &compose_args(&m, &tail), Some(&bundle), 180)?;
        if *action != Action::Stop {
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline {
                let status = self.status()?;
                if status.readiness.error_code.as_deref() == Some("VERSION_MISMATCH") {
                    return Err(err("VERSION_MISMATCH"));
                }
                if status.state == "running" {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }
            return Err(err("READINESS_TIMEOUT"));
        }
        Ok(())
    }
    fn validate_compose(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let v: Value = serde_json::from_slice(&run(
            engine,
            &compose_args(m, &["config", "--format", "json"]),
            Some(&self.root.join("bundle")),
            30,
        )?)
        .map_err(|_| err("BUNDLE_INVALID"))?;
        let services = v["services"]
            .as_object()
            .ok_or_else(|| err("BUNDLE_INVALID"))?;
        if services.len() != m.services.len() {
            return Err(err("BUNDLE_INVALID"));
        }
        for (name, s) in services {
            if !m.services.contains(name)
                || !m.images.iter().any(|i| s["image"] == i.reference)
                || s.get("build").is_some()
                || s["privileged"] == true
                || s["network_mode"] == "host"
                || s.get("container_name").is_some()
            {
                return Err(err("BUNDLE_INVALID"));
            }
            if s["labels"]["com.exhibitos.bundle"] != m.bundle_id
                || s["labels"]["com.exhibitos.project"] != m.project_name
                || s["labels"]["com.exhibitos.schema"] != m.schema_version
            {
                return Err(err("BUNDLE_INVALID"));
            }
            if let Some(ports) = s["ports"].as_array() {
                for p in ports {
                    let published = p["published"]
                        .as_str()
                        .and_then(|p| p.parse::<u16>().ok())
                        .or_else(|| p["published"].as_u64().and_then(|n| u16::try_from(n).ok()));
                    if p["host_ip"] != "127.0.0.1"
                        || !published.is_some_and(|n| m.ports.contains(&n))
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                }
            }
            if let Some(volumes) = s["volumes"].as_array() {
                for volume in volumes {
                    let source = volume["source"]
                        .as_str()
                        .ok_or_else(|| err("BUNDLE_INVALID"))?;
                    if volume["type"] != "volume"
                        || v["volumes"][source]["external"] == true
                        || v["volumes"][source]["labels"]["com.exhibitos.bundle"] != m.bundle_id
                        || v["volumes"][source]["labels"]["com.exhibitos.project"] != m.project_name
                        || v["volumes"][source]["labels"]["com.exhibitos.schema"]
                            != m.schema_version
                        || !v["volumes"][source]["name"]
                            .as_str()
                            .is_some_and(|n| n.starts_with(&(m.project_name.clone() + "_")))
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                }
            }
        }
        Ok(())
    }
    fn validate_ownership(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let data = run(
            engine,
            &[
                "ps".into(),
                "--all".into(),
                "--filter".into(),
                format!("label=com.docker.compose.project={}", m.project_name),
                "--format".into(),
                "{{.ID}}".into(),
            ],
            None,
            15,
        )?;
        for id in String::from_utf8(data)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            .lines()
        {
            if id.is_empty() {
                continue;
            }
            if !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            let v: Value =
                serde_json::from_slice(&run(engine, &["inspect".into(), id.into()], None, 15)?)
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            if v[0]["Config"]["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                || v[0]["Config"]["Labels"]["com.exhibitos.project"] != m.project_name
                || v[0]["Config"]["Labels"]["com.exhibitos.schema"] != m.schema_version
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
        }
        Ok(())
    }
    fn validate_volumes(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let v: Value = serde_json::from_slice(&run(
            engine,
            &compose_args(m, &["config", "--format", "json"]),
            Some(&self.root.join("bundle")),
            30,
        )?)
        .map_err(|_| err("BUNDLE_INVALID"))?;
        if let Some(volumes) = v["volumes"].as_object() {
            for (_, spec) in volumes {
                let name = spec["name"].as_str().ok_or_else(|| err("BUNDLE_INVALID"))?;
                let existing = run(
                    engine,
                    &[
                        "volume".into(),
                        "ls".into(),
                        "--format".into(),
                        "{{.Name}}".into(),
                    ],
                    None,
                    15,
                )?;
                let text = String::from_utf8(existing).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                if text.lines().any(|n| n == name) {
                    let inspected: Value = serde_json::from_slice(&run(
                        engine,
                        &["volume".into(), "inspect".into(), name.into()],
                        None,
                        15,
                    )?)
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                    if inspected[0]["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                        || inspected[0]["Labels"]["com.exhibitos.project"] != m.project_name
                        || inspected[0]["Labels"]["com.exhibitos.schema"] != m.schema_version
                    {
                        return Err(err("OWNERSHIP_CONFLICT"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Status> {
        let jobs = self.jobs()?;
        let active_job = jobs
            .iter()
            .rev()
            .find(|j| j.state == JobState::Running)
            .cloned();
        let installed_path = self.root.join("installed.json");
        if !installed_path.exists() {
            return Ok(Status {
                installed: false,
                bundle_id: None,
                version: None,
                state: "not_installed".into(),
                services: Vec::new(),
                readiness: Readiness::default(),
                storage: StorageState {
                    used_bytes: None,
                    quota_bytes: 5 * 1024 * 1024 * 1024,
                    free_bytes: fs2::available_space(&self.root).unwrap_or(0),
                    minimum_free_bytes: 0,
                },
                active_job,
            });
        }
        let m = self.manifest()?;
        let engine = self.engine(&m, false)?;
        if let Err(error) = self.validate_ownership(&m, &engine)
            && matches!(
                error.code.as_str(),
                "OWNERSHIP_CONFLICT" | "ENGINE_OUTPUT_INVALID"
            )
        {
            return Err(error);
        }
        let r = readiness(&m);
        let output = run(
            &engine,
            &compose_args(&m, &["ps", "--all", "--format", "json"]),
            Some(&self.root.join("bundle")),
            15,
        );
        let mut services = Vec::new();
        let mut state = "stopped".to_string();
        if let Ok(data) = output {
            let text = String::from_utf8(data).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            let rows: Vec<Value> = if text.trim_start().starts_with('[') {
                serde_json::from_str(&text).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            } else {
                text.lines()
                    .filter(|x| !x.is_empty())
                    .map(serde_json::from_str)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            };
            for row in rows {
                if let Some(name) = row["Service"].as_str()
                    && m.services.iter().any(|s| s == name)
                {
                    services.push(ServiceState {
                        name: name.into(),
                        state: row["State"].as_str().unwrap_or("unknown").into(),
                        health: row["Health"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                    });
                }
            }
            if r.ready
                && services.len() == m.services.len()
                && services
                    .iter()
                    .all(|s| s.state == "running" && s.health.as_deref() != Some("unhealthy"))
            {
                state = "running".into();
            } else if services.iter().any(|s| s.state == "running") {
                state = "degraded".into();
            }
        } else {
            state = "runtime_unavailable".into();
        }
        Ok(Status {
            installed: true,
            bundle_id: Some(m.bundle_id),
            version: Some(m.version),
            state,
            services,
            readiness: r,
            storage: StorageState {
                used_bytes: None,
                quota_bytes: 5 * 1024 * 1024 * 1024,
                free_bytes: fs2::available_space(&self.root).unwrap_or(0),
                minimum_free_bytes: m.minimum_free_bytes,
            },
            active_job,
        })
    }
    pub fn open_url(&self) -> Result<String> {
        let m = self.manifest()?;
        if self.status()?.state != "running" {
            return Err(err("READINESS_UNAVAILABLE"));
        }
        loopback_url(&m.open_url, &m.ports)?;
        Ok(m.open_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_image_ids_preserve_exact_digest_across_engine_representation() {
        let digest = "a".repeat(64);
        let reference = format!("sha256:{digest}");
        assert!(same_local_image_id(&digest, &reference));
        assert!(same_local_image_id(&reference, &reference));
        assert!(!same_local_image_id(&"b".repeat(64), &reference));
        assert!(!same_local_image_id(&format!("md5:{digest}"), &reference));
        assert!(!same_local_image_id(&digest.to_uppercase(), &reference));
        assert!(!same_local_image_id(&digest, &digest));
    }
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("exhibitos-manager-core-{}", Uuid::new_v4()))
    }
    fn manifest() -> BundleManifest {
        BundleManifest {
            schema_version: "1.0.0-draft.1".into(),
            bundle_id: Uuid::new_v4().to_string(),
            version: "0.1.0".into(),
            protocol_version: "1".into(),
            compose_sha256: "a".repeat(64),
            preferred_engine: None,
            project_name: format!("exhibitos-{}", Uuid::new_v4()),
            services: vec!["platform".into(), "database".into()],
            images: vec![Image {
                reference: format!("sha256:{}", "a".repeat(64)),
                archive: None,
            }],
            ports: vec![13200],
            open_url: "http://127.0.0.1:13200".into(),
            readiness_url: "http://127.0.0.1:13200/api/v1/readiness".into(),
            minimum_free_bytes: 1,
        }
    }
    #[test]
    fn no_url_escape() {
        for url in [
            "https://example.org",
            "http://localhost:13200",
            "http://127.0.0.1:80",
            "http://127.0.0.1:13200/../../x",
            "http://127.0.0.1:13200/\r\nAuthorization: secret",
        ] {
            assert!(loopback_url(url, &[13200]).is_err());
        }
        assert!(loopback_url("http://127.0.0.1:13200/api/v1/readiness", &[13200]).is_ok());
    }
    #[test]
    fn private_secret_file_persists_without_rotation() {
        let s = LifecycleService::new(root()).unwrap();
        let m = manifest();
        s.runtime_env(&m).unwrap();
        let before = fs::read(s.root.join("runtime.env")).unwrap();
        s.runtime_env(&m).unwrap();
        assert_eq!(before, fs::read(s.root.join("runtime.env")).unwrap());
        let text = String::from_utf8(before).unwrap();
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("DATABASE_URL=postgresql://exhibitos:"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(s.root.join("runtime.env"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(s.logs().unwrap().is_empty());
    }
    #[test]
    fn interrupted_jobs_recover_and_retry_keeps_identity() {
        let p = root();
        let s = LifecycleService::new(p.clone()).unwrap();
        let job = Job {
            id: Uuid::new_v4().to_string(),
            action: Action::Install,
            state: JobState::Running,
            attempt: 1,
            progress: 30,
            created_at: now(),
            updated_at: now(),
            error_code: None,
            guidance: None,
        };
        write_json(&s.root, "jobs.json", &vec![job.clone()]).unwrap();
        let recovered = LifecycleService::new(p).unwrap();
        assert_eq!(recovered.jobs().unwrap()[0].state, JobState::Interrupted);
        let retry = recovered.execute(Action::Retry).unwrap();
        assert_eq!(retry.id, job.id);
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.state, JobState::Failed);
        assert!(retry.error_code.is_some());
        assert_eq!(recovered.jobs().unwrap().len(), 2);
    }
    #[test]
    fn active_cross_process_lock_is_never_interrupted() {
        let p = root();
        let s = LifecycleService::new(p.clone()).unwrap();
        let _lock = s.lock().unwrap();
        assert_eq!(s.execute(Action::Start).unwrap_err().code, "BUSY");
        let _second = LifecycleService::new(p).unwrap();
        assert!(s.jobs().unwrap().is_empty());
    }
    #[test]
    fn relative_paths_and_symlink_manifest_are_rejected() {
        for path in ["../a", "/absolute", "x/../a", "x\\a", ""] {
            assert!(!safe_relative(path));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let s = LifecycleService::new(root()).unwrap();
            symlink("/etc", s.root.join("bundle")).unwrap();
            assert!(s.manifest().is_err());
        }
    }
    #[test]
    #[cfg(unix)]
    fn process_timeout_and_native_output() {
        let data = run("/usr/bin/printf", &["{\"safe\":true}".into()], None, 2).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&data).unwrap()["safe"],
            true
        );
        assert_eq!(
            run("/bin/sleep", &["3".into()], None, 1).unwrap_err().code,
            "ENGINE_TIMEOUT"
        );
    }
    #[test]
    fn registry_pin_normalizes_tags_without_weakening_repository_identity() {
        let h = "a".repeat(64);
        assert!(same_registry_pin(
            &format!("postgres@sha256:{h}"),
            &format!("docker.io/library/postgres:18.6@sha256:{h}")
        ));
        assert!(!same_registry_pin(
            &format!("evil.example/postgres@sha256:{h}"),
            &format!("postgres:18.6@sha256:{h}")
        ));
        assert!(!same_registry_pin(
            &format!("postgres@sha256:{}", "b".repeat(64)),
            &format!("postgres@sha256:{h}")
        ));
    }
    #[test]
    fn public_history_views_are_bounded_without_deleting_private_history() {
        let s = LifecycleService::new(root()).unwrap();
        let jobs: Vec<Job> = (0..240)
            .map(|i| Job {
                id: Uuid::new_v4().to_string(),
                action: Action::Install,
                state: JobState::Completed,
                attempt: 1,
                progress: 100,
                created_at: i,
                updated_at: i,
                error_code: None,
                guidance: None,
            })
            .collect();
        write_json(&s.root, "jobs.json", &jobs).unwrap();
        let events: Vec<LogEvent> = (0..550)
            .map(|i| LogEvent {
                at: i,
                job_id: None,
                code: "COMPLETED".into(),
                message: "완료".into(),
            })
            .collect();
        write_json(&s.root, "events.json", &events).unwrap();
        assert_eq!(s.jobs().unwrap().len(), 200);
        assert_eq!(s.jobs().unwrap()[0].created_at, 40);
        assert_eq!(s.logs().unwrap().len(), 500);
        assert_eq!(s.job_history().unwrap().len(), 240);
        assert_eq!(s.event_history().unwrap().len(), 550);
    }
    #[test]
    fn errors_never_reveal_process_or_secret_fields() {
        let e = err("ENGINE_PERMISSION");
        let text = serde_json::to_string(&e).unwrap();
        assert!(!text.contains("postgresql://"));
        assert!(!text.contains("password"));
        assert!(text.contains("ENGINE_PERMISSION"));
    }
}
