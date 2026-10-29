// SPDX-License-Identifier: Apache-2.0
//! Static-only provider jobs. Tokens live only in inherited pipes and memory;
//! the durable journal contains the reviewed source and observed provider state.
use super::*;
use crate::{hosted_runtime_file, native_payload::Payload, static_publication_source};
use std::{io::{Read, Write}, process::{Command, Stdio}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant}};

#[derive(Deserialize)]
pub struct Secret(String);
impl Drop for Secret {
    fn drop(&mut self) { let mut bytes = std::mem::take(&mut self.0).into_bytes(); bytes.fill(0); }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Accounts { token: Secret },
    Deploy { token: Secret, account_id: String, publication_id: String, expected_manifest_sha256: String, rights_confirmed: bool, cost_confirmed: bool },
    Reconcile { token: Secret },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account { pub id: String, pub name: String }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub format: u32, pub id: String,
    pub workspace_id: String, pub tenant_id: String, pub principal_id: String,
    pub account_id: String, pub project_name: String,
    pub source: super::native_static_export::Receipt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_cache_policy_sha256: Option<String>,
    pub state: String, pub deployment_id: Option<String>, pub public_url: Option<String>,
    pub error_code: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status { pub running: bool, pub accounts: Vec<Account>, pub work: Option<Work>, pub error_code: Option<String> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review { pub source: super::native_static_export::Receipt, pub info: static_publication_source::SourceInfo }
fn bad() -> LifecycleError { err("NATIVE_HOSTING_INVALID") }
fn account_id(id: &str) -> bool { id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
fn project_name(name: &str) -> bool {
    (1..=59).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !name.starts_with('-') && !name.ends_with('-')
}
fn safe_code(code: &str) -> bool {
    matches!(code, "HOSTED_PUBLIC_CONTENT_MISMATCH" | "HOSTED_PUBLIC_CONTENT_UNAVAILABLE" | "HOSTED_API_ORIGIN_INVALID" | "HOSTED_BUSY" | "HOSTED_CANONICAL_ORIGIN_CONFLICT" | "HOSTED_CONNECTION_FAILED" | "HOSTED_DEPLOYMENT_BRANCH_CONFLICT" | "HOSTED_OPERATION_UNCERTAIN" | "HOSTED_PAGINATION_LIMIT" | "HOSTED_PAID_PLAN_REFUSED" | "HOSTED_PERMISSION_REQUIRED" | "HOSTED_PROJECT_CONFLICT" | "HOSTED_RATE_LIMITED" | "HOSTED_REDIRECT_REFUSED" | "HOSTED_REMOTE_REJECTED" | "HOSTED_REQUEST_INVALID" | "HOSTED_RESOURCE_CONFLICT" | "HOSTED_RESPONSE_INVALID" | "HOSTED_RESULT_UNCERTAIN" | "HOSTED_SOURCE_INVALID" | "HOSTED_SOURCE_REQUIRED" | "HOSTED_STATIC_BUNDLE_CHANGED" | "HOSTED_STATIC_BUNDLE_INVALID" | "HOSTED_TOKEN_INVALID" | "HOSTED_TOKEN_REQUIRED" | "NATIVE_HOSTING_CANCELLED" | "NATIVE_HOSTING_UNAVAILABLE" | "NATIVE_HOSTING_INVALID")
}
impl Work {
    fn validate(&self) -> Result<()> {
        if self.format != 1 || !uuid(&self.id) || ![&self.workspace_id, &self.tenant_id, &self.principal_id, &self.source.publication_id].iter().all(|id| uuid(id))
            || !account_id(&self.account_id) || !project_name(&self.project_name)
            || ![&self.source.manifest_sha256, &self.source.revision_sha256, &self.source.index_sha256].iter().all(|h| hash_valid(h))
            || !self.source.output.is_absolute() || self.source.files < 3 || self.source.files > 20_000 || self.source.bytes > 512 * 1024 * 1024
            || !matches!(self.state.as_str(), "running" | "uncertain" | "deploying" | "failed" | "completed")
            || self.error_code.as_ref().is_some_and(|c| !safe_code(c))
            || self.static_cache_policy_sha256.as_ref().is_some_and(|s| !hash_valid(s))
            || self.deployment_id.as_ref().is_some_and(|s| s.is_empty() || s.len() > 128 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
            || self.public_url.as_ref().is_some_and(|s| s != &format!("https://{}.pages.dev", self.project_name))
            || (self.state == "completed" && (self.public_url.is_none() || self.deployment_id.is_none())) { return Err(bad()); }
        Ok(())
    }
    fn verify_source(&self) -> Result<static_publication_source::SourcePin> {
        self.validate()?;
        let value = super::native_static_export::Status { format: 1, job_id: self.id.clone(), workspace_id: self.workspace_id.clone(), tenant_id: self.tenant_id.clone(), principal_id: self.principal_id.clone(), publication_id: self.source.publication_id.clone(), revision_sha256: self.source.revision_sha256.clone(), output: self.source.output.clone(), state: "completed".into(), receipt: None, error_code: None };
        let current = super::native_static_export::verify_output(&value)?;
        if let Some(expected) = &self.static_cache_policy_sha256
            && super::native_static_export::cache_policy_sha256(&current)? != *expected { return Err(err("HOSTED_STATIC_BUNDLE_CHANGED")); }
        if current.manifest_sha256 != self.source.manifest_sha256 || current.index_sha256 != self.source.index_sha256 || current.files != self.source.files || current.bytes != self.source.bytes { return Err(err("HOSTED_STATIC_BUNDLE_CHANGED")); }
        let pin = static_publication_source::inspect(self.source.output.to_str().ok_or_else(bad)?)?;
        if pin.sha256 != self.source.manifest_sha256 || pin.info.project_name != self.project_name || pin.info.publication_id != self.source.publication_id { return Err(err("HOSTED_STATIC_BUNDLE_CHANGED")); }
        Ok(pin)
    }
}
impl InstallationController {
    fn hosting_location(&self, token: &str) -> Result<(PathBuf, String, (String, String, String))> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            Ok((profile.root.clone(), format!("native-hosting-{}.json", digest(service.root.as_os_str().as_encoded_bytes())), service.personal_identity()?))
        })
    }
    fn hosting_work(&self, token: &str) -> Result<Option<Work>> {
        let (root, name, identity) = self.hosting_location(token)?;
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        match fs::symlink_metadata(root.join(&name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(bad()),
            Ok(_) => {
                let mut work: Work = serde_json::from_slice(&source_bytes(&root, &name, 16384, true)?).map_err(|_| bad())?;
                work.validate()?;
                if (work.workspace_id.clone(), work.tenant_id.clone(), work.principal_id.clone()) != identity { return Err(bad()); }
                // A lost process result is never permission to replay a write.
                if work.state == "running" { work.state = "uncertain".into(); write_json(&root, &name, &work)?; }
                Ok(Some(work))
            }
        }
    }
    fn save_hosting_work(&self, token: &str, work: &Work) -> Result<()> {
        work.validate()?;
        let (root, name, identity) = self.hosting_location(token)?;
        if (work.workspace_id.clone(), work.tenant_id.clone(), work.principal_id.clone()) != identity { return Err(bad()); }
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        write_json(&root, &name, work)?;
        File::open(root).and_then(|f| f.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))
    }
}

// Existing reviewed public provider code, verified against the helper's compiled
// bytes. The native app reuses its admitted Node; no second runtime is installed.
const RESOURCES: &[(&str, &[u8])] = &[
    ("scripts/native-hosting-runner.mjs", include_bytes!("../../../scripts/native-hosting-runner.mjs")),
    ("vendor/deployment/src/pages-render.mjs", include_bytes!("../../../vendor/deployment/src/pages-render.mjs")),
    ("vendor/deployment/src/pages-asset-hash.mjs", include_bytes!("../../../vendor/deployment/src/pages-asset-hash.mjs")),
    ("vendor/deployment/src/pages-render-worker.mjs", include_bytes!("../../../vendor/deployment/src/pages-render-worker.mjs")),
    ("vendor/deployment/src/bounded-json.mjs", include_bytes!("../../../vendor/deployment/src/bounded-json.mjs")),
];
fn resources(payload: &Payload) -> Result<(PathBuf, Vec<hosted_runtime_file::Pin>)> {
    let runtime = payload.script().parent().and_then(Path::parent).and_then(Path::parent).map(Path::to_path_buf).ok_or_else(bad)?;
    let root = runtime.parent().ok_or_else(bad)?.join("native-hosting");
    if fs::canonicalize(&root).map_err(|_| err("NATIVE_HOSTING_UNAVAILABLE"))? != root { return Err(bad()); }
    let mut pins = Vec::new();
    for (name, expected) in RESOURCES {
        let path = root.join(name);
        if fs::canonicalize(&path).map_err(|_| err("NATIVE_HOSTING_UNAVAILABLE"))? != path { return Err(bad()); }
        let (bytes, pin) = hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
        if bytes != *expected { return Err(err("NATIVE_HOSTING_UNAVAILABLE")); }
        pins.push(pin);
    }
    Ok((root.join("scripts/native-hosting-runner.mjs"), pins))
}
struct OwnedChild(std::process::Child);
impl std::ops::Deref for OwnedChild { type Target = std::process::Child; fn deref(&self) -> &Self::Target { &self.0 } }
impl std::ops::DerefMut for OwnedChild { fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 } }
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM); }
            let _ = self.0.wait();
        }
    }
}
fn run(payload: &Payload, request: Value, secret: Secret, cancel: &AtomicBool) -> Result<Value> {
    let (script, pins) = resources(payload)?;
    let mut envelope = serde_json::json!({"request": request, "tokens": {"cloudflare": secret.0}});
    let mut bytes = serde_json::to_vec(&envelope).map_err(|_| bad())?;
    if let Some(token) = envelope["tokens"]["cloudflare"].as_str() {
        if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| (33..=126).contains(&b)) { bytes.fill(0); return Err(err("HOSTED_TOKEN_INVALID")); }
    }
    if let Value::String(token) = &mut envelope["tokens"]["cloudflare"] { let mut discarded = std::mem::take(token).into_bytes(); discarded.fill(0); }
    let mut child = OwnedChild(Command::new(payload.node()).arg(script).env_clear().env("PATH", "/usr/bin:/bin").env("LANG", "en_US.UTF-8").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| { bytes.fill(0); err("HOSTED_OPERATION_UNCERTAIN") })?);
    let stdout = child.stdout.take().ok_or_else(bad)?;
    let capture = thread::spawn(move || {
        let mut reader = stdout; let mut buffer = [0u8; 8192]; let mut output = Vec::new(); let mut overflow = false;
        loop { match reader.read(&mut buffer) { Ok(0) => break, Ok(n) => { if output.len() + n <= 262144 { output.extend_from_slice(&buffer[..n]); } else { overflow = true; } }, Err(_) => { overflow = true; break; } } }
        (output, overflow)
    });
    let written = child.stdin.take().ok_or_else(bad)?.write_all(&bytes);
    bytes.fill(0);
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut stopped = false;
    let exit = loop {
        if !stopped && (written.is_err() || cancel.load(Ordering::SeqCst) || Instant::now() >= deadline) {
            // A timed-out write may already have reached the provider. Retain
            // the journal and wait for actual normal child exit before release.
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM); }
            stopped = true;
        }
        match child.try_wait() { Ok(Some(exit)) => break exit, Ok(None) => thread::sleep(Duration::from_millis(50)), Err(_) => { let _ = child.wait(); break child.try_wait().map_err(|_| err("HOSTED_OPERATION_UNCERTAIN"))?.ok_or_else(bad)?; } }
    };
    let (mut output, overflow) = capture.join().map_err(|_| bad())?;
    for pin in pins { pin.check()?; }
    if stopped || overflow { output.fill(0); return Err(err("HOSTED_OPERATION_UNCERTAIN")); }
    let value: Value = serde_json::from_slice(&output).map_err(|_| err("HOSTED_RESPONSE_INVALID"))?;
    output.fill(0);
    if !exit.success() { return Err(err(value["errorCode"].as_str().filter(|code| safe_code(code)).unwrap_or("HOSTED_OPERATION_UNCERTAIN"))); }
    Ok(value)
}

pub struct Hoster {
    controller: Arc<InstallationController>,
    state: Arc<Mutex<Option<Status>>>, owner: Option<(String, String, String)>,
    cancel: Arc<AtomicBool>, worker: Option<thread::JoinHandle<()>>,
}
impl Hoster {
    pub fn new(controller: Arc<InstallationController>) -> Self { Self { controller, state: Arc::new(Mutex::new(None)), owner: None, cancel: Arc::new(AtomicBool::new(false)), worker: None } }
    pub fn active(&self) -> bool { self.worker.as_ref().is_some_and(|w| !w.is_finished()) }
    pub fn cancel(&self) { self.cancel.store(true, Ordering::SeqCst); }
    pub fn review(&self, token: &str) -> Result<Review> {
        let exported = self.controller.read_static_export(token)?.ok_or_else(|| err("NATIVE_STATIC_EXPORT_UNAVAILABLE"))?;
        if exported.state != "completed" { return Err(err("NATIVE_STATIC_EXPORT_UNAVAILABLE")); }
        let source = super::native_static_export::verify_output(&exported)?;
        let publication = self.controller.native_publication(token)?;
        let completed = publication.completed.ok_or_else(|| err("NATIVE_PUBLICATION_REQUIRED"))?;
        if publication.pending.is_some() || completed.publication.status != "published" || completed.publication.publication_id != source.publication_id || completed.publication.revision_sha256 != source.revision_sha256 { return Err(err("NATIVE_PUBLICATION_STATE_CHANGED")); }
        let pin = static_publication_source::inspect(source.output.to_str().ok_or_else(bad)?)?;
        if pin.sha256 != source.manifest_sha256 { return Err(err("HOSTED_STATIC_BUNDLE_CHANGED")); }
        Ok(Review { source, info: pin.info })
    }
    pub fn status(&mut self, token: &str) -> Result<Status> {
        let identity = self.controller.hosting_location(token)?.2;
        if self.active() {
            if self.owner.as_ref() != Some(&identity) { return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE")); }
            return self.state.lock().map_err(|_| bad())?.clone().ok_or_else(bad);
        }
        if let Some(worker) = self.worker.take() { worker.join().map_err(|_| bad())?; }
        if self.owner.as_ref() == Some(&identity) && let Some(value) = self.state.lock().map_err(|_| bad())?.clone() && !value.running { return Ok(value); }
        Ok(Status { running: false, accounts: vec![], work: self.controller.hosting_work(token)?, error_code: None })
    }
    pub fn start(&mut self, token: String, input: Input, payload: Arc<Payload>) -> Result<Status> {
        let previous = self.status(&token)?;
        if self.active() { return Err(err("BUSY")); }
        resources(&payload)?;
        let identity = self.controller.hosting_location(&token)?.2;
        let (request, secret, mut work) = match input {
            Input::Accounts { token } => (serde_json::json!({"provider":"cloudflare","action":"accounts","staticOnly":true}), token, previous.work),
            Input::Deploy { token: secret, account_id: account, publication_id, expected_manifest_sha256, rights_confirmed, cost_confirmed } => {
                if !rights_confirmed || !cost_confirmed || !account_id(&account) { return Err(err("HOSTED_PERMISSION_REQUIRED")); }
                if previous.work.as_ref().is_some_and(|w| !matches!(w.state.as_str(), "completed" | "failed")) { return Err(err("NATIVE_HOSTING_RECONCILE_REQUIRED")); }
                let exported = self.controller.read_static_export(&token)?.ok_or_else(|| err("NATIVE_STATIC_EXPORT_UNAVAILABLE"))?;
                if exported.state != "completed" { return Err(err("NATIVE_STATIC_EXPORT_UNAVAILABLE")); }
                let source = super::native_static_export::verify_output(&exported)?;
                let publication = self.controller.native_publication(&token)?;
                let completed = publication.completed.ok_or_else(|| err("NATIVE_PUBLICATION_REQUIRED"))?;
                if publication.pending.is_some() || completed.publication.publication_id != publication_id || source.publication_id != publication_id || completed.publication.status != "published" || source.revision_sha256 != completed.publication.revision_sha256 || expected_manifest_sha256 != source.manifest_sha256 { return Err(err("NATIVE_PUBLICATION_STATE_CHANGED")); }
                let pin = static_publication_source::inspect(source.output.to_str().ok_or_else(bad)?)?;
                let cache_policy = super::native_static_export::cache_policy_sha256(&source)?;
                let record = Work { format: 1, id: Uuid::new_v4().to_string(), workspace_id: identity.0.clone(), tenant_id: identity.1.clone(), principal_id: identity.2.clone(), account_id: account, project_name: pin.info.project_name, source, static_cache_policy_sha256: Some(cache_policy), state: "running".into(), deployment_id: None, public_url: None, error_code: None };
                record.verify_source()?;
                self.controller.save_hosting_work(&token, &record)?;
                (request_for(&record, "deploy-static"), secret, Some(record))
            }
            Input::Reconcile { token: secret } => {
                let mut record = previous.work.ok_or_else(|| err("NATIVE_HOSTING_RECONCILE_REQUIRED"))?;
                record.verify_source()?;
                record.state = "running".into(); record.error_code = None;
                self.controller.save_hosting_work(&token, &record)?;
                (request_for(&record, "reconcile"), secret, Some(record))
            }
        };
        let accounts = request["action"] == "accounts";
        let initial = Status { running: true, accounts: previous.accounts, work: work.clone(), error_code: None };
        *self.state.lock().map_err(|_| bad())? = Some(initial.clone()); self.owner = Some(identity);
        self.cancel.store(false, Ordering::SeqCst);
        let shared = self.state.clone(); let controller = self.controller.clone(); let cancel = self.cancel.clone();
        self.worker = Some(thread::spawn(move || {
            let result = run(&payload, request, secret, &cancel);
            let mut status = initial; status.running = false;
            let projected = result.and_then(|value| {
                if accounts {
                    let values: Vec<Account> = serde_json::from_value(value["accounts"].clone()).map_err(|_| bad())?;
                    if value["provider"] != "cloudflare" || value["state"] != "ready" || values.len() > 1024 || values.iter().any(|a| !account_id(&a.id) || a.name.len() > 512 || a.name.chars().any(char::is_control)) { return Err(bad()); }
                    status.accounts = values;
                } else if let Some(record) = work.as_mut() { project_result(record, &value)?; }
                Ok(())
            });
            if let Err(error) = projected {
                let code = if safe_code(&error.code) { error.code } else { "HOSTED_OPERATION_UNCERTAIN".into() };
                status.error_code = Some(code.clone());
                if !accounts && let Some(record) = work.as_mut() { record.state = "uncertain".into(); record.error_code = Some(code); }
            }
            if !accounts && let Some(record) = work.as_ref() {
                if controller.save_hosting_work(&token, record).is_err() { status.error_code = Some("HOSTED_OPERATION_UNCERTAIN".into()); record_uncertain(&mut work); }
            }
            status.work = work;
            if let Ok(mut state) = shared.lock() { *state = Some(status); }
        }));
        Ok(self.state.lock().map_err(|_| bad())?.clone().ok_or_else(bad)?)
    }
    pub fn shutdown(&mut self) { self.cancel(); if let Some(worker) = self.worker.take() { let _ = worker.join(); } }
}
impl Drop for Hoster { fn drop(&mut self) { self.shutdown(); } }
fn record_uncertain(work: &mut Option<Work>) { if let Some(record) = work { record.state = "uncertain".into(); record.public_url = None; record.error_code = Some("HOSTED_OPERATION_UNCERTAIN".into()); } }
fn request_for(record: &Work, action: &str) -> Value {
    let mut request = serde_json::json!({"provider":"cloudflare","action":action,"accountId":record.account_id,"projectName":record.project_name,"sourcePath":record.source.output,"expectedManifestSha256":record.source.manifest_sha256,"staticOnly":true});
    if let Some(hash) = &record.static_cache_policy_sha256 {
        request["staticCacheProfile"] = serde_json::json!("static-data-sha256-24h-v1");
        request["staticCachePolicySha256"] = serde_json::json!(hash);
    }
    request
}
fn project_result(record: &mut Work, result: &Value) -> Result<()> {
    if result["provider"] != "cloudflare" || result["projectName"] != record.project_name { return Err(bad()); }
    record.deployment_id = result["deploymentId"].as_str().map(str::to_owned);
    record.public_url = None; record.error_code = None;
    record.state = match result["state"].as_str() { Some("ready") => {
        if !result["verification"].is_object() { return Err(err("HOSTED_PUBLIC_CONTENT_MISMATCH")); }
        let manifest_bytes = source_bytes(&record.source.output, "static-export-manifest.json", 8 * 1024 * 1024, false)?.len() as u64;
        if result["url"] != format!("https://{}.pages.dev", record.project_name) || result["verification"]["manifestSha256"] != record.source.manifest_sha256 || result["verification"]["files"].as_u64() != record.source.files.checked_sub(1) || result["verification"]["bytes"].as_u64() != record.source.bytes.checked_sub(manifest_bytes) { return Err(err("HOSTED_PUBLIC_CONTENT_MISMATCH")); }
        record.verify_source()?; record.public_url = result["url"].as_str().map(str::to_owned); "completed"
    }, Some("deploying") => "deploying", Some("failed") => "failed", Some("uncertain") => "uncertain", _ => return Err(bad()) }.into();
    record.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn work() -> Work {
        Work { format: 1, id: Uuid::new_v4().to_string(), workspace_id: Uuid::new_v4().to_string(), tenant_id: Uuid::new_v4().to_string(), principal_id: Uuid::new_v4().to_string(), account_id: "a".repeat(32), project_name: "synthetic-exhibition".into(), source: super::super::native_static_export::Receipt { publication_id: Uuid::new_v4().to_string(), revision_sha256: "b".repeat(64), index_sha256: "c".repeat(64), manifest_sha256: "d".repeat(64), files: 3, bytes: 4096, output: PathBuf::from("/synthetic/absent-publication") }, static_cache_policy_sha256: None, state: "running".into(), deployment_id: None, public_url: None, error_code: None }
    }
    #[test]
    fn provider_ready_without_public_file_proof_never_completes() {
        let mut work = work();
        let result = serde_json::json!({"provider":"cloudflare","projectName":work.project_name,"deploymentId":"synthetic-deployment","state":"ready","url":"https://synthetic-exhibition.pages.dev"});
        assert_eq!(project_result(&mut work, &result).unwrap_err().code, "HOSTED_PUBLIC_CONTENT_MISMATCH");
        assert_ne!(work.state, "completed"); assert!(work.public_url.is_none());
    }
    #[test]
    fn pending_provider_state_never_exposes_an_unverified_url() {
        let mut work = work();
        project_result(&mut work, &serde_json::json!({"provider":"cloudflare","projectName":"synthetic-exhibition","deploymentId":"synthetic-deployment","state":"deploying","url":"https://unverified.invalid"})).unwrap();
        assert_eq!(work.state, "deploying"); assert!(work.public_url.is_none());
        work.state = "completed".into(); assert!(work.validate().is_err());
    }
    #[test]
    fn static_input_and_error_boundary_are_closed() {
        for input in [serde_json::json!({"action":"deploy_api","token":"synthetic"}), serde_json::json!({"action":"accounts","token":"synthetic","provider":"render"}), serde_json::json!({"action":"reconcile","token":"synthetic","sourcePath":"/unreviewed"})] { assert!(serde_json::from_value::<Input>(input).is_err()); }
        assert!(safe_code("HOSTED_PUBLIC_CONTENT_MISMATCH"));
        for value in ["HOSTED_PRIVATE_TOKEN_VALUE", "HOSTED_PUBLIC_CONTENT_MISMATCH\nSQL", "/private/path"] { assert!(!safe_code(value)); }
    }
    #[test]
    fn cache_policy_is_optional_for_legacy_work_and_pinned_for_new_work() {
        let mut work = work();
        let legacy = serde_json::to_value(&work).unwrap();
        assert!(legacy.get("staticCachePolicySha256").is_none());
        let request = request_for(&work, "reconcile");
        assert!(request.get("staticCacheProfile").is_none());
        work.static_cache_policy_sha256 = Some("e".repeat(64));
        let request = request_for(&work, "reconcile");
        assert_eq!(request["staticCacheProfile"], "static-data-sha256-24h-v1");
        assert_eq!(request["staticCachePolicySha256"], "e".repeat(64));
        assert!(work.validate().is_ok());
        work.static_cache_policy_sha256 = Some("INVALID".into());
        assert!(work.validate().is_err());
    }
}
