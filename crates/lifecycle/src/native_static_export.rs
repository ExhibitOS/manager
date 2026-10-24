// SPDX-License-Identifier: Apache-2.0
//! Owned, cancellable execution of the byte-admitted static exporter. No CMS
//! credential is passed to this public-projection-only child.
use super::*;
use crate::native_payload::Payload;
use std::{
    collections::BTreeSet,
    process::Child,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub publication_id: String,
    pub output: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub publication_id: String,
    pub revision_sha256: String,
    pub index_sha256: String,
    pub manifest_sha256: String,
    pub files: u64,
    pub bytes: u64,
    pub output: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    pub format: u32,
    pub job_id: String,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub publication_id: String,
    pub revision_sha256: String,
    pub output: PathBuf,
    pub state: String,
    pub receipt: Option<Receipt>,
    pub error_code: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    publication_id: String,
    revision_sha256: String,
    index_sha256: String,
    files: Vec<ManifestFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
}
fn bad() -> LifecycleError {
    err("NATIVE_STATIC_EXPORT_INVALID")
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}
fn bounded(root: &Path, name: &str, limit: u64) -> Result<Vec<u8>> {
    installation_backup::source_bytes(root, name, limit, false).map_err(|_| bad())
}
/// Full exact-file verification is also the crash/lost-result reconciliation.
/// A receipt alone, directory existence or successful exit is never sufficient.
pub fn verify_output(status: &Status) -> Result<Receipt> {
    if !status.output.is_absolute()
        || fs::symlink_metadata(&status.output)
            .map_err(|_| bad())?
            .is_symlink()
        || fs::canonicalize(&status.output).map_err(|_| bad())? != status.output
    {
        return Err(bad());
    }
    let raw = bounded(
        &status.output,
        "static-export-manifest.json",
        8 * 1024 * 1024,
    )?;
    let manifest: Manifest = serde_json::from_slice(&raw).map_err(|_| bad())?;
    if manifest.format != "exhibitos-static-publication-v1"
        || manifest.publication_id != status.publication_id
        || manifest.revision_sha256 != status.revision_sha256
        || !hash_valid(&manifest.index_sha256)
        || manifest.files.is_empty()
        || manifest.files.len() >= 20_000
    {
        return Err(bad());
    }
    let mut expected = BTreeSet::from(["static-export-manifest.json".to_string()]);
    let mut bytes = raw.len() as u64;
    for item in &manifest.files {
        if !safe_name(&item.path)
            || !expected.insert(item.path.clone())
            || !hash_valid(&item.sha256)
            || item.bytes > 25 * 1024 * 1024
        {
            return Err(bad());
        }
        let value = bounded(&status.output, &item.path, 25 * 1024 * 1024)?;
        if value.len() as u64 != item.bytes || digest(&value) != item.sha256 {
            return Err(bad());
        }
        bytes = bytes.checked_add(item.bytes).ok_or_else(bad)?;
    }
    fn walk(root: &Path, at: &Path, depth: usize, names: &mut BTreeSet<String>) -> Result<()> {
        if depth > 16 {
            return Err(bad());
        }
        for entry in fs::read_dir(at).map_err(|_| bad())? {
            let path = entry.map_err(|_| bad())?.path();
            let stat = fs::symlink_metadata(&path).map_err(|_| bad())?;
            if stat.is_symlink() {
                return Err(bad());
            }
            if stat.is_dir() {
                walk(root, &path, depth + 1, names)?;
            } else if stat.is_file() {
                let name = path
                    .strip_prefix(root)
                    .map_err(|_| bad())?
                    .to_str()
                    .ok_or_else(bad)?;
                if names.len() >= 20_000 || !safe_name(name) {
                    return Err(bad());
                }
                names.insert(name.into());
            } else {
                return Err(bad());
            }
        }
        Ok(())
    }
    let mut actual = BTreeSet::new();
    walk(&status.output, &status.output, 0, &mut actual)?;
    if actual != expected
        || !expected.contains("index.html")
        || expected.contains("studio.html")
        || digest(&bounded(
            &status.output,
            "static-publication-index.json",
            1024 * 1024,
        )?) != manifest.index_sha256
    {
        return Err(bad());
    }
    let html = bounded(&status.output, "index.html", 1024 * 1024)?;
    if !String::from_utf8_lossy(&html).contains(&manifest.index_sha256) {
        return Err(bad());
    }
    Ok(Receipt {
        publication_id: status.publication_id.clone(),
        revision_sha256: status.revision_sha256.clone(),
        index_sha256: manifest.index_sha256,
        manifest_sha256: digest(&raw),
        files: expected.len() as u64,
        bytes,
        output: status.output.clone(),
    })
}
impl InstallationController {
    fn static_export_location(
        &self,
        token: &str,
    ) -> Result<(PathBuf, String, (String, String, String))> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            Ok((
                profile.root.clone(),
                format!(
                    "native-static-export-{}.json",
                    digest(service.root.as_os_str().as_encoded_bytes())
                ),
                service.personal_identity()?,
            ))
        })
    }
    fn read_static_export(&self, token: &str) -> Result<Option<Status>> {
        let (root, name, identity) = self.static_export_location(token)?;
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        match fs::symlink_metadata(root.join(&name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(bad()),
            Ok(_) => {
                let value: Status = serde_json::from_slice(&installation_backup::source_bytes(
                    &root, &name, 65536, true,
                )?)
                .map_err(|_| bad())?;
                if value.format != 1
                    || !uuid(&value.job_id)
                    || !uuid(&value.publication_id)
                    || !hash_valid(&value.revision_sha256)
                    || !value.output.is_absolute()
                    || !matches!(
                        value.state.as_str(),
                        "running" | "completed" | "cancelled" | "failed" | "interrupted"
                    )
                    || (
                        value.workspace_id.clone(),
                        value.tenant_id.clone(),
                        value.principal_id.clone(),
                    ) != identity
                {
                    return Err(bad());
                }
                Ok(Some(value))
            }
        }
    }
    fn write_static_export(&self, token: &str, value: &Status) -> Result<()> {
        let existing = self.read_static_export(token)?;
        if existing
            .as_ref()
            .is_some_and(|old| old.state == "running" && old.job_id != value.job_id)
        {
            return Err(err("BUSY"));
        }
        let (root, name, identity) = self.static_export_location(token)?;
        if (&value.workspace_id, &value.tenant_id, &value.principal_id)
            != (&identity.0, &identity.1, &identity.2)
        {
            return Err(bad());
        }
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        write_json(&root, &name, value)?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))
    }
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
pub struct Exporter {
    controller: Arc<InstallationController>,
    state: Arc<Mutex<Option<Status>>>,
    cancel: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Exporter {
    pub fn new(controller: Arc<InstallationController>) -> Self {
        Self {
            controller,
            state: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }
    pub fn active(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| !w.is_finished())
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn reap(&mut self) -> Result<()> {
        if self
            .worker
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
        {
            self.worker.take().unwrap().join().map_err(|_| bad())?;
        }
        Ok(())
    }
    pub fn status(&mut self, token: &str) -> Result<Option<Status>> {
        self.reap()?;
        if self.active() {
            let value = self.state.lock().map_err(|_| bad())?.clone();
            let identity = self.controller.static_export_location(token)?.2;
            if value.as_ref().is_some_and(|v| {
                (&v.workspace_id, &v.tenant_id, &v.principal_id)
                    != (&identity.0, &identity.1, &identity.2)
            }) {
                return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
            }
            return Ok(value);
        }
        let value = self.controller.read_static_export(token)?;
        if let Some(value) = &value {
            // Reopening a running job never re-executes the exporter. Verify its
            // atomic final output asynchronously; retain incomplete files.
            if value.state == "running" {
                self.launch(token.into(), value.clone(), None);
            }
        }
        Ok(value)
    }
    pub fn start(
        &mut self,
        token: String,
        input: Input,
        payload: Arc<Payload>,
        port: u16,
    ) -> Result<Status> {
        self.status(&token)?;
        if self.active() {
            return Err(err("BUSY"));
        }
        let publication = self.controller.native_publication(&token)?;
        if publication.pending.is_some() {
            return Err(err("NATIVE_PUBLICATION_PENDING"));
        }
        let completed = publication
            .completed
            .ok_or_else(|| err("NATIVE_PUBLICATION_REQUIRED"))?;
        if completed.publication.publication_id != input.publication_id
            || completed.publication.status != "published"
        {
            return Err(err("NATIVE_PUBLICATION_STATE_CHANGED"));
        }
        let parent = input.output.parent().ok_or_else(bad)?;
        if !input.output.is_absolute()
            || input.output.as_os_str().len() > 4096
            || input.output.file_name().is_none()
            || fs::canonicalize(parent).map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))? != parent
            || !fs::metadata(parent).map_err(|_| bad())?.is_dir()
        {
            return Err(err("NATIVE_STATIC_OUTPUT_INVALID"));
        }
        match fs::symlink_metadata(&input.output) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(err("NATIVE_STATIC_OUTPUT_EXISTS")),
        }
        // All optional capability bytes were included in Payload::open's exact
        // inventory. An older runtime is refused instead of searching PATH.
        let (script, viewer) = payload.static_export_resources()?;
        let value = Status {
            format: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            workspace_id: publication.workspace_id,
            tenant_id: publication.tenant_id,
            principal_id: publication.principal_id,
            publication_id: input.publication_id,
            revision_sha256: completed.publication.revision_sha256,
            output: input.output,
            state: "running".into(),
            receipt: None,
            error_code: None,
        };
        self.controller.write_static_export(&token, &value)?;
        self.launch(token, value.clone(), Some((payload, script, viewer, port)));
        Ok(value)
    }
    fn launch(
        &mut self,
        token: String,
        mut value: Status,
        execute: Option<(Arc<Payload>, PathBuf, PathBuf, u16)>,
    ) {
        self.cancel.store(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let shared = self.state.clone();
        let controller = self.controller.clone();
        if let Ok(mut state) = shared.lock() {
            *state = Some(value.clone());
        }
        self.worker = Some(thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Receipt> {
                    if let Some((payload, script, viewer, port)) = execute {
                        let (root, _, _) = controller.static_export_location(&token)?;
                        let stdout_name = format!("native-static-{}.stdout", value.job_id);
                        let stderr_name = format!("native-static-{}.stderr", value.job_id);
                        use std::os::unix::fs::OpenOptionsExt;
                        let create = |name: &str| {
                            OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .mode(0o600)
                                .open(root.join(name))
                                .map_err(|_| bad())
                        };
                        let stdout = create(&stdout_name)?;
                        let stderr = create(&stderr_name)?;
                        let mut child = OwnedChild(
                            std::process::Command::new(payload.node())
                                .args([
                                    script.as_os_str(),
                                    std::ffi::OsStr::new(&format!("http://127.0.0.1:{port}")),
                                    std::ffi::OsStr::new(&value.publication_id),
                                    viewer.as_os_str(),
                                    value.output.as_os_str(),
                                ])
                                .env_clear()
                                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                                .env("LANG", "en_US.UTF-8")
                                .stdin(Stdio::piped())
                                .stdout(Stdio::from(stdout))
                                .stderr(Stdio::from(stderr))
                                .spawn()
                                .map_err(|_| err("NATIVE_STATIC_EXPORT_FAILED"))?,
                        );
                        let deadline = std::time::Instant::now() + Duration::from_secs(300);
                        let mut stopping = None;
                        let exit = loop {
                            if let Some(exit) = child.0.try_wait().map_err(|_| bad())? {
                                break exit;
                            }
                            let overflow = [&stdout_name, &stderr_name].iter().any(|n| {
                                fs::metadata(root.join(n)).map_or(true, |s| s.len() > 65536)
                            });
                            if stopping.is_none()
                                && (cancel.load(Ordering::SeqCst)
                                    || std::time::Instant::now() > deadline
                                    || overflow)
                            {
                                // Signal only this Child handle; no process-name or
                                // listener sweep and no output deletion.
                                unsafe {
                                    libc::kill(child.0.id() as i32, libc::SIGTERM);
                                }
                                stopping = Some(std::time::Instant::now());
                            }
                            if stopping.is_some_and(|at| at.elapsed() > Duration::from_secs(5)) {
                                let _ = child.0.kill();
                            }
                            thread::sleep(Duration::from_millis(50));
                        };
                        // Cancellation can race the final rename. A verified final
                        // result wins; otherwise keep every incomplete output.
                        if cancel.load(Ordering::SeqCst) {
                            return verify_output(&value)
                                .map_err(|_| err("NATIVE_STATIC_CANCELLED"));
                        }
                        if !exit.success() || stopping.is_some() {
                            return Err(err("NATIVE_STATIC_EXPORT_FAILED"));
                        }
                        let raw =
                            installation_backup::source_bytes(&root, &stdout_name, 65536, true)?;
                        let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| bad())?;
                        let verified = verify_output(&value)?;
                        if serde_json::to_value(receipt).map_err(|_| bad())?
                            != serde_json::to_value(&verified).map_err(|_| bad())?
                        {
                            return Err(bad());
                        }
                        return Ok(verified);
                    }
                    verify_output(&value).map_err(|_| err("NATIVE_STATIC_INTERRUPTED"))
                }));
            match result {
                Ok(Ok(receipt)) => {
                    value.state = "completed".into();
                    value.receipt = Some(receipt);
                    value.error_code = None;
                }
                other => {
                    let code = match other {
                        Ok(Err(e)) => e.code,
                        _ => "NATIVE_STATIC_EXPORT_FAILED".into(),
                    };
                    value.state = match code.as_str() {
                        "NATIVE_STATIC_CANCELLED" => "cancelled",
                        "NATIVE_STATIC_INTERRUPTED" => "interrupted",
                        _ => "failed",
                    }
                    .into();
                    value.error_code = Some(code);
                }
            }
            if controller.write_static_export(&token, &value).is_err() {
                value.state = "failed".into();
                value.error_code = Some("STATE_UNAVAILABLE".into());
            }
            if let Ok(mut state) = shared.lock() {
                *state = Some(value);
            }
        }));
    }
}
impl Drop for Exporter {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
