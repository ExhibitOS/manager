// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::LifecycleError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static BUSY: AtomicBool = AtomicBool::new(false);
pub fn busy() -> bool {
    BUSY.load(Ordering::Acquire)
}
pub(crate) struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
pub(crate) fn hold_operation() -> Result<Busy, LifecycleError> {
    if BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return Err(refused("HOSTED_BUSY")); }
    Ok(Busy)
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Cloudflare,
    Render,
}
impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cloudflare => "cloudflare",
            Self::Render => "render",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenInput {
    pub provider: Provider,
    pub token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInput {
    pub provider: Provider,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Accounts,
    Services,
    Reconcile,
    DeployStatic,
    DeployApi,
    Status,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Request {
    #[serde(default, skip_serializing)]
    pub public_upload_approved: bool,
    #[serde(default, skip_serializing)]
    pub cost_reviewed: bool,
    #[serde(default, skip_serializing)]
    pub api_requested: bool,
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub api_origin: Option<String>,
    pub provider: Provider,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postgres_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_only: Option<bool>,
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub expected_manifest_sha256: Option<String>,
}
fn safe_failure_code(code: &str) -> bool {
    if code.len() > 80 {
        return false;
    }
    if let Some(diagnostic) = code.strip_prefix("HOSTED_JS_") {
        return [
            "INPUT",
            "OUTPUT",
            "ACCOUNTS",
            "MEMBERSHIPS",
            "PROJECT_READ",
            "PROJECT_CREATE",
            "UPLOAD_TOKEN",
            "ASSET_CHECK",
            "ASSET_UPLOAD",
            "ASSET_UPSERT",
            "DEPLOY_LIST",
            "DEPLOY_STATUS",
            "DEPLOY_CREATE",
            "BUNDLE_VERIFY",
            "ASSET_PREPARE",
            "DEPLOY_FORM",
            "RENDER_REQUEST",
        ]
        .iter()
        .any(|stage| {
            diagnostic
                .strip_prefix(&format!("{stage}_"))
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "TYPE_ERROR"
                            | "RANGE_ERROR"
                            | "SYNTAX_ERROR"
                            | "REFERENCE_ERROR"
                            | "ERROR"
                            | "UNKNOWN"
                    )
                })
        });
    }
    if [
        "HOSTED_API_ORIGIN_INVALID",
        "HOSTED_API_ORIGIN_UNVERIFIED",
        "HOSTED_BUSY",
        "HOSTED_CANONICAL_ORIGIN_CONFLICT",
        "HOSTED_CONFIGURATION_REQUIRED",
        "HOSTED_CONNECTION_FAILED",
        "HOSTED_DEPLOYMENT_BRANCH_CONFLICT",
        "HOSTED_EXISTING_DATABASE_REQUIRED",
        "HOSTED_FOLDER_UNAVAILABLE",
        "HOSTED_KEYCHAIN_REMOVE_FAILED",
        "HOSTED_KEYCHAIN_STORE_FAILED",
        "HOSTED_KEYCHAIN_UNAVAILABLE",
        "HOSTED_LINK_UNAVAILABLE",
        "HOSTED_OPERATION_UNCERTAIN",
        "HOSTED_PAGINATION_LIMIT",
        "HOSTED_PAID_PLAN_REFUSED",
        "HOSTED_PERMISSION_REQUIRED",
        "HOSTED_PLATFORM_UNSUPPORTED",
        "HOSTED_PROJECT_CONFLICT",
        "HOSTED_RATE_LIMITED",
        "HOSTED_REDIRECT_REFUSED",
        "HOSTED_REMOTE_REJECTED",
        "HOSTED_RENDER_NOT_REQUIRED",
        "HOSTED_REQUEST_INVALID",
        "HOSTED_RESOURCE_CONFLICT",
        "HOSTED_RESPONSE_INVALID",
        "HOSTED_RESULT_UNCERTAIN",
        "HOSTED_RUNTIME_UNAVAILABLE",
        "HOSTED_RUNTIME_INPUT_UNCERTAIN",
        "HOSTED_RUNTIME_WAIT_UNCERTAIN",
        "HOSTED_RUNTIME_TIMEOUT_UNCERTAIN",
        "HOSTED_RUNTIME_OUTPUT_UNCERTAIN",
        "HOSTED_SOURCE_INVALID",
        "HOSTED_SOURCE_REQUIRED",
        "HOSTED_STATIC_BUNDLE_CHANGED",
        "HOSTED_STATIC_BUNDLE_INVALID",
        "HOSTED_TOKEN_INVALID",
        "HOSTED_TOKEN_REQUIRED",
        "HOSTED_URL_UNAVAILABLE",
        "HOSTED_WORK_INVALID",
        "HOSTED_WORK_UNAVAILABLE",
    ]
    .contains(&code)
    {
        return true;
    }
    let Some((stage, tail)) = code
        .strip_prefix("HOSTED_CF_")
        .and_then(|s| s.split_once("_HTTP_"))
    else {
        return false;
    };
    if !matches!(
        stage,
        "ASSET_CHECK"
            | "ASSET_UPSERT"
            | "ASSET_UPLOAD"
            | "UPLOAD_TOKEN"
            | "DEPLOY_STATUS"
            | "DEPLOY_CREATE"
            | "DEPLOY_LIST"
            | "PROJECT_CREATE"
            | "PROJECT_READ"
    ) {
        return false;
    }
    let (http, provider) = tail
        .split_once("_CODE_")
        .map_or((tail, None), |(h, c)| (h, Some(c)));
    if http.len() != 3
        || !http.bytes().all(|b| b.is_ascii_digit())
        || !(http.starts_with('2') || http.starts_with('4'))
    {
        return false;
    }
    provider.is_none_or(|n| {
        n.len() >= 4 && n.len() <= 8 && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    })
}
fn write_runtime_input(mut pipe: Option<impl Write>, bytes: &[u8]) -> Result<(), LifecycleError> {
    pipe.as_mut()
        .ok_or_else(|| refused("HOSTED_RUNTIME_INPUT_UNCERTAIN"))?
        .write_all(bytes)
        .map_err(|_| refused("HOSTED_RUNTIME_INPUT_UNCERTAIN"))
}
fn runtime_wait(
    result: std::io::Result<Option<std::process::ExitStatus>>,
) -> Result<Option<std::process::ExitStatus>, LifecycleError> {
    result.map_err(|_| refused("HOSTED_RUNTIME_WAIT_UNCERTAIN"))
}
fn runtime_abort(
    elapsed: Duration,
    stdout_failed: bool,
    stderr_failed: bool,
) -> Option<LifecycleError> {
    if elapsed > Duration::from_secs(180) {
        Some(refused("HOSTED_RUNTIME_TIMEOUT_UNCERTAIN"))
    } else if stdout_failed || stderr_failed {
        Some(refused("HOSTED_RUNTIME_OUTPUT_UNCERTAIN"))
    } else {
        None
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FailureFrame {
    error_code: String,
}
fn runtime_failure(output: &[u8], stderr: &[u8]) -> LifecycleError {
    if !output.iter().all(u8::is_ascii_whitespace) {
        return match serde_json::from_slice::<FailureFrame>(output) {
            Ok(frame) if safe_failure_code(&frame.error_code) => refused(&frame.error_code),
            _ => refused("HOSTED_RESPONSE_INVALID"),
        };
    }
    // Compatibility for older fixed-code runners; arbitrary stderr is never forwarded.
    let code = std::str::from_utf8(stderr).unwrap_or("").trim();
    refused(if safe_failure_code(code) {
        code
    } else {
        "HOSTED_OPERATION_UNCERTAIN"
    })
}
pub fn refused(code: &str) -> LifecycleError {
    LifecycleError { code: code.into(), guidance: "연결이나 배포를 완료하지 못했습니다. 저장된 자료는 그대로입니다. 계정 권한과 제공자의 실제 작업 상태를 확인하세요.".into() }
}
pub fn store(input: TokenInput) -> Result<bool, LifecycleError> {
    let mut bytes = input.token.into_bytes();
    if bytes.is_empty() || bytes.len() > 4096 || !bytes.iter().all(|b| (33..=126).contains(b)) {
        bytes.fill(0);
        return Err(refused("HOSTED_TOKEN_INVALID"));
    }
    let result = super::hosted_keychain::store(input.provider.name(), &bytes);
    bytes.fill(0);
    result
        .map(|_| true)
        .map_err(|_| refused("HOSTED_KEYCHAIN_STORE_FAILED"))
}
pub fn connected(input: ProviderInput) -> Result<bool, LifecycleError> {
    super::hosted_keychain::contains(input.provider.name())
        .map_err(|_| refused("HOSTED_KEYCHAIN_UNAVAILABLE"))
}
pub fn disconnect(input: ProviderInput) -> Result<bool, LifecycleError> {
    super::hosted_keychain::remove(input.provider.name())
        .map(|_| true)
        .map_err(|_| refused("HOSTED_KEYCHAIN_REMOVE_FAILED"))
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Link {
    CloudflareSignup,
    CloudflareToken,
    RenderSignup,
    RenderToken,
}
pub fn open_link(link: Link) -> Result<(), LifecycleError> {
    let url = match link {
        Link::CloudflareSignup => "https://dash.cloudflare.com/sign-up",
        Link::CloudflareToken => "https://dash.cloudflare.com/profile/api-tokens",
        Link::RenderSignup => "https://dashboard.render.com/register",
        Link::RenderToken => "https://dashboard.render.com/u/settings#api-keys",
    };
    open::that(url).map_err(|_| refused("HOSTED_LINK_UNAVAILABLE"))
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub title: String,
    pub publication_id: String,
    pub project_name: String,
    pub revision_sha256: String,
    pub index_sha256: String,
}
struct SourcePin {
    path: String,
    sha256: String,
    info: SourceInfo,
}
static SOURCE: Mutex<Option<SourcePin>> = Mutex::new(None);
static PUBLIC_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicUrlInput {
    pub url: String,
}
pub fn open_url(input: PublicUrlInput) -> Result<(), LifecycleError> {
    let allowed = PUBLIC_URLS
        .lock()
        .map_err(|_| refused("HOSTED_URL_UNAVAILABLE"))?;
    if !allowed.contains(&input.url) {
        return Err(refused("HOSTED_URL_UNAVAILABLE"));
    }
    open::that(&input.url).map_err(|_| refused("HOSTED_URL_UNAVAILABLE"))
}
fn read_source(path: &str) -> Result<SourcePin, LifecycleError> {
    let root = std::fs::canonicalize(path).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    let (bytes, manifest_pin) =
        super::remote::bounded(&root.join("static-export-manifest.json"), 8 * 1024 * 1024)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if manifest["format"] != "exhibitos-static-publication-v1" {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let publication_id = manifest["publicationId"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control))
        .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
        .to_owned();
    let (index, index_pin) =
        super::remote::bounded(&root.join("static-publication-index.json"), 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&index)) != manifest["indexSha256"].as_str().unwrap_or("") {
        return Err(refused("HOSTED_STATIC_BUNDLE_CHANGED"));
    }
    let index: serde_json::Value =
        serde_json::from_slice(&index).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    let public_path = index["publication"]["path"]
        .as_str()
        .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if public_path.len() != 74
        || !public_path.starts_with("data/")
        || !public_path.ends_with(".json")
        || !public_path[5..69].bytes().all(|b| b.is_ascii_hexdigit())
        || std::fs::symlink_metadata(root.join("data"))
            .map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
            .is_symlink()
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let (public_bytes, public_pin) =
        super::remote::bounded(&root.join(public_path), 25 * 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&public_bytes))
        != index["publication"]["sha256"].as_str().unwrap_or("")
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_CHANGED"));
    }
    let public: serde_json::Value = serde_json::from_slice(&public_bytes)
        .map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if public["publication"]["status"] != "published"
        || public["publication"]["id"] != publication_id
        || public["publication"]["revisionSha256"] != manifest["revisionSha256"]
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let title = public["exhibition"]["title"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
        .unwrap_or("공개 전시")
        .to_owned();
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "exhibition" } else { slug };
    let suffix: String = publication_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    if suffix.is_empty() {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let project_name = format!(
        "{}-{}",
        slug.chars()
            .take(40)
            .collect::<String>()
            .trim_end_matches('-'),
        suffix.to_lowercase()
    );
    manifest_pin.check()?;
    index_pin.check()?;
    public_pin.check()?;
    Ok(SourcePin {
        path: root
            .to_str()
            .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
            .to_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        info: SourceInfo {
            title,
            publication_id,
            project_name,
            revision_sha256: manifest["revisionSha256"].as_str().unwrap_or("").into(),
            index_sha256: manifest["indexSha256"].as_str().unwrap_or("").into(),
        },
    })
}
pub(crate) fn select_local_export(path:&str, publication_id:&str, revision:&str, manifest_sha256:&str)->Result<SourceInfo,LifecycleError>{
    let pin=read_source(path)?;
    if pin.info.publication_id!=publication_id || pin.info.revision_sha256!=revision || pin.sha256!=manifest_sha256 {return Err(refused("LOCAL_CMS_SOURCE_CHANGED"));}
    let info=pin.info.clone();
    *SOURCE.lock().map_err(|_|refused("HOSTED_SOURCE_INVALID"))?=Some(pin);
    Ok(info)
}
pub fn choose_source() -> Result<Option<SourceInfo>, LifecycleError> {
    #[cfg(target_os = "macos")]
    {
        let result = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "POSIX path of (choose folder with prompt \"공개 승인한 전시 묶음을 선택하세요\")",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|_| refused("HOSTED_FOLDER_UNAVAILABLE"))?;
        if !result.status.success() {
            return if String::from_utf8_lossy(&result.stderr).contains("(-128)") {
                Ok(None)
            } else {
                Err(refused("HOSTED_FOLDER_UNAVAILABLE"))
            };
        }
        let text =
            String::from_utf8(result.stdout).map_err(|_| refused("HOSTED_FOLDER_UNAVAILABLE"))?;
        let text = text.trim_end_matches(['\r', '\n']);
        if !text.starts_with('/') || text.len() > 4095 || text.contains(['\r', '\n', '\0']) {
            return Err(refused("HOSTED_SOURCE_INVALID"));
        }
        let pin = read_source(text)?;
        let info = pin.info.clone();
        *SOURCE
            .lock()
            .map_err(|_| refused("HOSTED_SOURCE_INVALID"))? = Some(pin);
        return Ok(Some(info));
    }
    #[cfg(not(target_os = "macos"))]
    Err(refused("HOSTED_PLATFORM_UNSUPPORTED"))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Work {
    version: u8,
    request: Request,
    source_path: String,
    source_sha256: String,
    source: SourceInfo,
    receipt: Option<serde_json::Value>,
    phase: String,
    #[serde(default)]
    static_request: Option<Request>,
    #[serde(default)]
    api_request: Option<Request>,
    #[serde(default)]
    static_receipt: Option<serde_json::Value>,
    #[serde(default)]
    api_receipt: Option<serde_json::Value>,
    #[serde(default)]
    api_requested: bool,
    #[serde(default)]
    api_origin: Option<String>,
}
fn work_bytes(work: &Work) -> Result<Vec<u8>, LifecycleError> {
    let mut value = serde_json::to_value(work).map_err(|_| refused("HOSTED_WORK_UNAVAILABLE"))?;
    // Internal pins have dedicated journal fields. Keep persisted requests on the
    // renderer wire schema so deny_unknown_fields still rejects renderer overrides.
    for key in ["request", "staticRequest", "apiRequest"] {
        if let Some(request) = value
            .get_mut(key)
            .and_then(serde_json::Value::as_object_mut)
        {
            request.remove("expectedManifestSha256");
            request.remove("apiOrigin");
        }
    }
    serde_json::to_vec(&value).map_err(|_| refused("HOSTED_WORK_UNAVAILABLE"))
}
fn save_work(work: &Work) -> Result<(), LifecycleError> {
    let mut bytes = work_bytes(work)?;
    let result = super::hosted_keychain::store("hosted-work", &bytes);
    bytes.fill(0);
    result.map_err(|_| refused("HOSTED_WORK_UNAVAILABLE"))
}
fn load_work() -> Result<Option<Work>, LifecycleError> {
    if !super::hosted_keychain::contains("hosted-work")
        .map_err(|_| refused("HOSTED_WORK_UNAVAILABLE"))?
    {
        return Ok(None);
    }
    let mut bytes = super::hosted_keychain::load("hosted-work")
        .map_err(|_| refused("HOSTED_WORK_UNAVAILABLE"))?;
    let value = serde_json::from_slice(&bytes).map_err(|_| refused("HOSTED_WORK_INVALID"));
    bytes.fill(0);
    value.map(Some)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInfo {
    pub provider: Provider,
    pub account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub source: SourceInfo,
    pub receipt: Option<serde_json::Value>,
    pub phase: String,
    pub source_available: bool,
    pub static_only: bool,
    pub static_receipt: Option<serde_json::Value>,
    pub api_receipt: Option<serde_json::Value>,
}
pub fn resume_local() -> Result<Option<ResumeInfo>, LifecycleError> {
    let Some(work) = load_work()? else {
        return Ok(None);
    };
    if work.version != 1 {
        return Err(refused("HOSTED_WORK_INVALID"));
    }
    let pin = read_source(&work.source_path)
        .ok()
        .filter(|pin| pin.sha256 == work.source_sha256);
    let available = if let Some(pin) = pin {
        let mut current = SOURCE
            .lock()
            .map_err(|_| refused("HOSTED_SOURCE_INVALID"))?;
        if let Some(existing) = current.as_ref() {
            existing.sha256 == work.source_sha256
        } else {
            *current = Some(pin);
            true
        }
    } else {
        false
    };
    for receipt in [&work.static_receipt, &work.api_receipt, &work.receipt] {
        if let Some(url) = receipt.as_ref().and_then(|r| r["url"].as_str()) {
            if let Ok(parsed) = url::Url::parse(url) {
                if parsed.scheme() == "https"
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
                    && parsed.query().is_none()
                    && parsed.fragment().is_none()
                    && parsed.port().is_none()
                    && parsed
                        .host_str()
                        .is_some_and(|h| h.ends_with(".pages.dev") || h.ends_with(".onrender.com"))
                {
                    PUBLIC_URLS
                        .lock()
                        .map_err(|_| refused("HOSTED_URL_UNAVAILABLE"))?
                        .push(url.into());
                }
            }
        }
    }
    Ok(Some(ResumeInfo {
        provider: work.request.provider,
        account_id: work
            .static_request
            .as_ref()
            .and_then(|r| r.account_id.clone())
            .or(work.request.account_id),
        workspace_id: work
            .api_request
            .as_ref()
            .and_then(|r| r.workspace_id.clone())
            .or(work.request.workspace_id),
        source: work.source,
        receipt: work.receipt,
        phase: work.phase,
        source_available: available,
        static_only: !work.api_requested,
        static_receipt: work.static_receipt,
        api_receipt: work.api_receipt,
    }))
}
fn reconcile_work(
    work: Work,
    mut observe: impl FnMut(Request) -> Result<serde_json::Value, LifecycleError>,
) -> Result<(), LifecycleError> {
    let mut requests = Vec::new();
    if let Some(request) = work.static_request {
        requests.push(request);
    }
    if let Some(request) = work.api_request {
        requests.push(request);
    }
    if requests.is_empty() {
        requests.push(work.request);
    }
    let mut first_error = None;
    for mut request in requests {
        request.action = Action::Reconcile;
        request.expected_manifest_sha256 = Some(work.source_sha256.clone());
        if matches!(request.provider, Provider::Cloudflare) {
            request.api_origin = work.api_origin.clone();
        }
        if let Err(error) = observe(request) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}
pub fn resume_remote(resources: PathBuf) -> Result<Option<ResumeInfo>, LifecycleError> {
    let work = load_work()?.ok_or_else(|| refused("HOSTED_WORK_UNAVAILABLE"))?;
    if let Err(error) = reconcile_work(work, |request| execute(resources.clone(), request)) {
        if let Some(mut work) = load_work()? {
            work.phase = "uncertain".into();
            save_work(&work)?;
        }
        return Err(error);
    }
    resume_local()
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as usize) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as usize) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as usize);
        out.push(CHARS[(n >> 18) & 63] as char);
        out.push(CHARS[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 {
            CHARS[(n >> 6) & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[n & 63] as char
        } else {
            '='
        });
    }
    out
}
fn recovery_request(work: &Work) -> Result<&Request,LifecycleError> {
    work.api_request.as_ref().or_else(||matches!(work.request.provider,Provider::Render).then_some(&work.request)).ok_or_else(||refused("HOSTED_RECOVERY_REQUEST"))
}
fn recovery_source_from_work(work: &Work)->Result<exhibitos_lifecycle::hosted_backup::Source,LifecycleError>{
    let request=recovery_request(work)?;
    let receipt=work.api_receipt.as_ref().ok_or_else(||refused("HOSTED_RECOVERY_REQUEST"))?;
    let source=exhibitos_lifecycle::hosted_backup::Source{workspace_id:request.workspace_id.clone().ok_or_else(||refused("HOSTED_RECOVERY_REQUEST"))?,service_id:receipt["serviceId"].as_str().ok_or_else(||refused("HOSTED_RECOVERY_REQUEST"))?.into(),postgres_id:receipt["postgresId"].as_str().or(request.postgres_id.as_deref()).ok_or_else(||refused("HOSTED_RECOVERY_REQUEST"))?.into()};
    if request.service_id.as_ref().is_some_and(|id|id!=&source.service_id)||request.postgres_id.as_ref().is_some_and(|id|id!=&source.postgres_id){return Err(refused("HOSTED_RESOURCE_CONFLICT"));}
    source.validate()?;Ok(source)
}
pub(crate) fn recovery_source()->Result<exhibitos_lifecycle::hosted_backup::Source,LifecycleError>{
    let work=load_work()?.ok_or_else(||refused("HOSTED_WORK_UNAVAILABLE"))?;
    recovery_source_from_work(&work)
}
fn runtime_account(workspace: &str,project: &str)->String {
    format!("render-runtime-{:x}",Sha256::digest(format!("{workspace}\0{project}").as_bytes()))
}
// Recovery only loads the saved source configuration. It must never call the
// deployment creator/public_runtime or derive new values from current SOURCE.
fn recovery_configuration_from_work(work:&Work,expected:&exhibitos_lifecycle::hosted_backup::Source,load:impl FnOnce(&str)->Result<Vec<u8>,()>)->Result<serde_json::Value,LifecycleError>{
    if recovery_source_from_work(work)?!=*expected{return Err(refused("HOSTED_RECOVERY_CONFIGURATION_CHANGED"));}
    let request=recovery_request(work)?;
    let project=request.project_name.as_deref().unwrap_or(&work.source.project_name);
    if project.is_empty()||project.len()>63||!project.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'){return Err(refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    let receipt_origin=work.api_receipt.as_ref().and_then(|r|r["url"].as_str());
    if work.api_origin.as_deref().zip(receipt_origin).is_some_and(|(a,b)|a!=b){return Err(refused("HOSTED_RECOVERY_CONFIGURATION_CHANGED"));}
    let origin=work.api_origin.as_deref().or(receipt_origin).ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    let parsed=url::Url::parse(origin).map_err(|_|refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    if parsed.scheme()!="https"||!parsed.username().is_empty()||parsed.password().is_some()||parsed.port().is_some()||parsed.query().is_some()||parsed.fragment().is_some()||parsed.path()!="/"||!parsed.host_str().is_some_and(|h|h.ends_with(".onrender.com"))||parsed.origin().ascii_serialization()!=origin{return Err(refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    let mut bytes=load(&runtime_account(&expected.workspace_id,project)).map_err(|_|refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    let decoded=if bytes.is_empty()||bytes.len()>65536{None}else{serde_json::from_slice::<serde_json::Value>(&bytes).ok()};bytes.fill(0);
    let value=decoded.ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    if value.as_object().is_none_or(|v|v.len()!=1||!v.contains_key("envVars")){return Err(refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    let env=value["envVars"].as_array().ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    let required=["CLIENT_ORIGIN","TENANT_ID","ADMIN_SUBJECT","ADMIN_PASSWORD","FREEZE_SIGNING_KEY_BASE64"];
    let optional=["STATIC_PUBLICATION_ID","STATIC_REVISION_SHA256","STATIC_INDEX_SHA256"];
    let mut names=std::collections::BTreeSet::new();
    for row in env {
        let object=row.as_object().ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
        let key=row["key"].as_str().ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
        let text=row["value"].as_str().ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
        if object.len()!=2||(!required.contains(&key)&&!optional.contains(&key))||!names.insert(key)||text.is_empty()||text.len()>8192||text.contains('\0'){return Err(refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    }
    let optional_count=optional.iter().filter(|key|names.contains(**key)).count();
    if !required.iter().all(|key|names.contains(*key))||![0,3].contains(&optional_count){return Err(refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    Ok(serde_json::json!({"envVars":value["envVars"],"origin":origin}))
}
/// One saved Work observation binds the DB source, original configuration and
/// optional public-file copy. No current SOURCE or newly generated keys are used.
pub(crate) struct RecoveryContext {
    pub source:exhibitos_lifecycle::hosted_backup::Source,
    pub configuration:serde_json::Value,
    pub publication:Option<RecoveryPublication>,
}
pub(crate) struct RecoveryPublication {
    pub source_path:String, pub manifest_sha256:String, pub publication_id:String,
    pub revision_sha256:String, pub index_sha256:String, pub snapshot:Vec<u8>,
}
fn recovery_publication(work:&Work,configuration:&serde_json::Value)->Option<RecoveryPublication>{
    if work.version!=1{return None;}
    let env=configuration["envVars"].as_array()?;
    let get=|name:&str|env.iter().find(|v|v["key"]==name)?.get("value")?.as_str();
    if get("STATIC_PUBLICATION_ID")?!=work.source.publication_id || get("STATIC_REVISION_SHA256")?!=work.source.revision_sha256 || get("STATIC_INDEX_SHA256")?!=work.source.index_sha256{return None;}
    let digest=|s:&str|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b));
    if !digest(&work.source_sha256)||!digest(&work.source.revision_sha256)||!digest(&work.source.index_sha256)||!std::path::Path::new(&work.source_path).is_absolute(){return None;}
    let mut value:serde_json::Value=serde_json::from_slice(&work_bytes(work).ok()?).ok()?;
    // Provider response objects are not a secret-safe storage schema. Preserve
    // only identifiers from them; credentials/configuration never join this file.
    for key in ["receipt","staticReceipt","apiReceipt"]{
        let projected=value.get(key).and_then(serde_json::Value::as_object).map(|receipt|{
            let mut out=serde_json::Map::new();
            for name in ["deploymentId","serviceId","postgresId","projectName"]{
                if let Some(text)=receipt.get(name).and_then(serde_json::Value::as_str).filter(|s|!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')){out.insert(name.into(),text.into());}
            }
            serde_json::Value::Object(out)
        }).unwrap_or(serde_json::Value::Null);
        value[key]=projected;
    }
    let snapshot=serde_json::to_vec(&value).ok()?;
    if snapshot.len()>65536{return None;}
    Some(RecoveryPublication{source_path:work.source_path.clone(),manifest_sha256:work.source_sha256.clone(),publication_id:work.source.publication_id.clone(),revision_sha256:work.source.revision_sha256.clone(),index_sha256:work.source.index_sha256.clone(),snapshot})
}
pub(crate) fn recovery_snapshot_matches(bytes:&[u8],source:&exhibitos_lifecycle::hosted_backup::Source)->bool{
    serde_json::from_slice::<Work>(bytes).ok().is_some_and(|work|work.version==1&&recovery_source_from_work(&work).ok().as_ref()==Some(source))
}
pub(crate) fn recovery_context()->Result<RecoveryContext,LifecycleError>{
    let work=load_work()?.ok_or_else(||refused("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    let source=recovery_source_from_work(&work)?;
    let configuration=recovery_configuration_from_work(&work,&source,super::hosted_keychain::load)?;
    let publication=recovery_publication(&work,&configuration);
    Ok(RecoveryContext{source,configuration,publication})
}

fn runtime_configuration(
    workspace: &str,
    project: &str,
) -> Result<serde_json::Value, LifecycleError> {
    let account = runtime_account(workspace,project);
    if super::hosted_keychain::contains(&account)
        .map_err(|_| refused("HOSTED_KEYCHAIN_UNAVAILABLE"))?
    {
        let mut bytes = super::hosted_keychain::load(&account)
            .map_err(|_| refused("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
        let value =
            serde_json::from_slice(&bytes).map_err(|_| refused("HOSTED_KEYCHAIN_UNAVAILABLE"));
        bytes.fill(0);
        return public_runtime(value?, &account);
    }
    let mut random = [0u8; 80];
    super::hosted_keychain::random(&mut random)
        .map_err(|_| refused("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
    random[6] = (random[6] & 15) | 64;
    random[8] = (random[8] & 63) | 128;
    let uuid: String = random[..16]
        .iter()
        .enumerate()
        .map(|(i, b)| {
            format!(
                "{}{b:02x}",
                if [4, 6, 8, 10].contains(&i) { "-" } else { "" }
            )
        })
        .collect();
    let password: String = random[16..48].iter().map(|b| format!("{b:02x}")).collect();
    let mut pkcs8 = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    pkcs8.extend_from_slice(&random[48..80]);
    let value = serde_json::json!({"envVars":[{"key":"CLIENT_ORIGIN","value":format!("https://{project}.pages.dev")},{"key":"TENANT_ID","value":uuid},{"key":"ADMIN_SUBJECT","value":"exhibitos-owner"},{"key":"ADMIN_PASSWORD","value":password},{"key":"FREEZE_SIGNING_KEY_BASE64","value":base64(&pkcs8)}]});
    random.fill(0);
    pkcs8.fill(0);
    let mut bytes =
        serde_json::to_vec(&value).map_err(|_| refused("HOSTED_KEYCHAIN_STORE_FAILED"))?;
    let stored = super::hosted_keychain::store(&account, &bytes);
    bytes.fill(0);
    stored.map_err(|_| refused("HOSTED_KEYCHAIN_STORE_FAILED"))?;
    public_runtime(value, &account)
}
fn public_runtime(
    mut value: serde_json::Value,
    account: &str,
) -> Result<serde_json::Value, LifecycleError> {
    let source = SOURCE
        .lock()
        .map_err(|_| refused("HOSTED_SOURCE_INVALID"))?;
    let pin = source
        .as_ref()
        .ok_or_else(|| refused("HOSTED_SOURCE_REQUIRED"))?;
    let env = value["envVars"]
        .as_array_mut()
        .ok_or_else(|| refused("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
    for (key, text) in [
        ("STATIC_PUBLICATION_ID", &pin.info.publication_id),
        ("STATIC_REVISION_SHA256", &pin.info.revision_sha256),
        ("STATIC_INDEX_SHA256", &pin.info.index_sha256),
    ] {
        env.retain(|entry| entry["key"] != key);
        env.push(serde_json::json!({"key":key,"value":text}));
    }
    let mut bytes =
        serde_json::to_vec(&value).map_err(|_| refused("HOSTED_KEYCHAIN_STORE_FAILED"))?;
    let result = super::hosted_keychain::store(account, &bytes);
    bytes.fill(0);
    result.map_err(|_| refused("HOSTED_KEYCHAIN_STORE_FAILED"))?;
    Ok(value)
}
fn valid_request(input: &Request) -> bool {
    if matches!(input.action, Action::DeployStatic | Action::DeployApi)
        && (!input.public_upload_approved
            || (matches!(input.action, Action::DeployApi) && !input.cost_reviewed))
    {
        return false;
    }
    [
        &input.account_id,
        &input.workspace_id,
        &input.project_name,
        &input.source_path,
        &input.deployment_id,
        &input.service_id,
        &input.postgres_id,
    ]
    .iter()
    .all(|value| {
        value.as_ref().is_none_or(|value| {
            !value.is_empty() && value.len() <= 4095 && !value.chars().any(char::is_control)
        })
    }) && (!matches!(input.action, Action::DeployStatic)
        || matches!(input.provider, Provider::Cloudflare))
        && (!matches!(input.action, Action::DeployApi)
            || matches!(input.provider, Provider::Render))
}
pub fn execute(
    resources: PathBuf,
    mut request: Request,
) -> Result<serde_json::Value, LifecycleError> {
    if !cfg!(target_os = "macos") {
        return Err(refused("HOSTED_PLATFORM_UNSUPPORTED"));
    }
    if !valid_request(&request) {
        return Err(refused("HOSTED_REQUEST_INVALID"));
    }
    let _busy = hold_operation()?;
    let manifest: serde_json::Value =
        serde_json::from_str(option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").unwrap_or("{}"))
            .map_err(|_| refused("HOSTED_RUNTIME_UNAVAILABLE"))?;
    if !manifest["files"].as_object().is_some_and(|m| {
        m.contains_key("scripts/hosted-runner.mjs")
            && m.contains_key("vendor/deployment/src/pages-render.mjs")
    }) {
        return Err(refused("HOSTED_RUNTIME_UNAVAILABLE"));
    }
    if matches!(request.action, Action::DeployStatic | Action::DeployApi) {
        let guard = SOURCE
            .lock()
            .map_err(|_| refused("HOSTED_SOURCE_INVALID"))?;
        let pin = guard
            .as_ref()
            .ok_or_else(|| refused("HOSTED_SOURCE_REQUIRED"))?;
        let current = read_source(&pin.path)?;
        if current.sha256 != pin.sha256 {
            return Err(refused("HOSTED_STATIC_BUNDLE_CHANGED"));
        }
        request.source_path = Some(pin.path.clone());
        request.expected_manifest_sha256 = Some(pin.sha256.clone());
        request.project_name = Some(pin.info.project_name.clone());
    }
    let writing = matches!(request.action, Action::DeployStatic | Action::DeployApi);
    if writing {
        let source = SOURCE
            .lock()
            .map_err(|_| refused("HOSTED_SOURCE_INVALID"))?;
        let pin = source
            .as_ref()
            .ok_or_else(|| refused("HOSTED_SOURCE_REQUIRED"))?;
        let previous = load_work()?.filter(|w| w.source_sha256 == pin.sha256);
        let (mut static_request, mut api_request, mut static_receipt, mut api_receipt) =
            if let Some(work) = previous {
                (
                    work.static_request,
                    work.api_request,
                    work.static_receipt,
                    work.api_receipt,
                )
            } else {
                (None, None, None, None)
            };
        if matches!(request.provider, Provider::Cloudflare) {
            if request.static_only == Some(false) {
                request.api_origin = api_receipt
                    .as_ref()
                    .filter(|r| r["state"] == "deployed")
                    .and_then(|r| r["url"].as_str())
                    .map(str::to_owned);
                if request.api_origin.is_none() {
                    return Err(refused("HOSTED_API_ORIGIN_UNVERIFIED"));
                }
            }
            if static_request
                .as_ref()
                .is_some_and(|r| r.account_id != request.account_id)
            {
                static_receipt = None;
            }
            static_request = Some(request.clone());
        } else {
            if api_request
                .as_ref()
                .is_some_and(|r| r.workspace_id != request.workspace_id)
            {
                api_receipt = None;
            }
            api_request = Some(request.clone());
        }
        let receipt = if matches!(request.provider, Provider::Cloudflare) {
            static_receipt.clone()
        } else {
            api_receipt.clone()
        };
        save_work(&Work {
            version: 1,
            request: request.clone(),
            source_path: pin.path.clone(),
            source_sha256: pin.sha256.clone(),
            source: pin.info.clone(),
            receipt,
            phase: "pending".into(),
            static_request,
            api_request,
            static_receipt,
            api_receipt,
            api_origin: request.api_origin.clone(),
            api_requested: request.api_requested
                || matches!(request.provider, Provider::Render)
                || request.static_only == Some(false),
        })?;
    }
    let pins = super::remote::verified_resources(&resources)?;
    let mut token = super::hosted_keychain::load(request.provider.name())
        .map_err(|_| refused("HOSTED_TOKEN_REQUIRED"))?;
    let token_text = std::str::from_utf8(&token).map_err(|_| refused("HOSTED_TOKEN_REQUIRED"))?;
    let mut tokens = serde_json::Map::new();
    tokens.insert(request.provider.name().into(), token_text.into());
    let runtime = if matches!(request.action, Action::DeployApi) {
        Some(runtime_configuration(
            request
                .workspace_id
                .as_deref()
                .ok_or_else(|| refused("HOSTED_REQUEST_INVALID"))?,
            request
                .project_name
                .as_deref()
                .ok_or_else(|| refused("HOSTED_REQUEST_INVALID"))?,
        )?)
    } else {
        None
    };
    let mut input = serde_json::to_vec(
        &serde_json::json!({"request":request,"tokens":tokens,"runtime":runtime}),
    )
    .map_err(|_| refused("HOSTED_REQUEST_INVALID"))?;
    let mut command = Command::new(resources.join("remote-runtime/generated/node"));
    command
        .args(["--disable-proto=throw", "--no-warnings"])
        .arg(resources.join("scripts/hosted-runner.mjs"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    for pin in &pins {
        pin.check()?;
    }
    let mut child = command
        .spawn()
        .map_err(|_| refused("HOSTED_RUNTIME_UNAVAILABLE"))?;
    let written = write_runtime_input(child.stdin.take(), &input);
    input.fill(0);
    if let Err(error) = written {
        super::remote::stop(&mut child);
        token.fill(0);
        return Err(error);
    }
    let out = child
        .stdout
        .take()
        .ok_or_else(|| refused("HOSTED_RUNTIME_OUTPUT_UNCERTAIN"))?;
    let err = child
        .stderr
        .take()
        .ok_or_else(|| refused("HOSTED_RUNTIME_OUTPUT_UNCERTAIN"))?;
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(super::remote::reader(out, 262144));
    });
    std::thread::spawn(move || {
        let _ = etx.send(super::remote::reader(err, 4096));
    });
    let start = Instant::now();
    let mut output = None;
    let mut errors = None;
    let mut status = None;
    loop {
        if output.is_none() {
            output = rx.try_recv().ok();
        }
        if errors.is_none() {
            errors = erx.try_recv().ok();
        }
        if status.is_none() {
            status = match runtime_wait(child.try_wait()) {
                Ok(value) => value,
                Err(error) => {
                    super::remote::stop(&mut child);
                    token.fill(0);
                    return Err(error);
                }
            };
        }
        if let Some(error) = runtime_abort(
            start.elapsed(),
            output.as_ref().is_some_and(Result::is_err),
            errors.as_ref().is_some_and(Result::is_err),
        ) {
            super::remote::stop(&mut child);
            token.fill(0);
            return Err(error);
        }
        if status.is_some() && output.is_some() && errors.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    for pin in &pins {
        pin.check()?;
    }
    let output = output
        .unwrap()
        .map_err(|_| refused("HOSTED_RESPONSE_INVALID"))?;
    // Never forward a returned credential, even if a provider or adapter violates its projection contract.
    let leaked = output.windows(token.len()).any(|bytes| bytes == token);
    token.fill(0);
    if leaked {
        return Err(refused("HOSTED_RESPONSE_INVALID"));
    }
    if !status.unwrap().success() {
        let error = errors
            .unwrap()
            .map_err(|_| refused("HOSTED_RUNTIME_OUTPUT_UNCERTAIN"))?;
        return Err(runtime_failure(&output, &error));
    }
    let response: serde_json::Value =
        serde_json::from_slice(&output).map_err(|_| refused("HOSTED_RESPONSE_INVALID"))?;
    if let Some(text) = response["url"].as_str() {
        let url = url::Url::parse(text).map_err(|_| refused("HOSTED_RESPONSE_INVALID"))?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url
                .host_str()
                .is_some_and(|host| host.ends_with(".pages.dev") || host.ends_with(".onrender.com"))
        {
            return Err(refused("HOSTED_RESPONSE_INVALID"));
        }
        let mut allowed = PUBLIC_URLS
            .lock()
            .map_err(|_| refused("HOSTED_URL_UNAVAILABLE"))?;
        if !allowed.iter().any(|v| v == text) {
            if allowed.len() >= 200 {
                allowed.remove(0);
            }
            allowed.push(text.into());
        }
    }
    if writing || matches!(request.action, Action::Reconcile | Action::Status) {
        if let Some(mut work) = load_work()? {
            let target = if matches!(request.provider, Provider::Cloudflare) {
                work.static_request.as_ref()
            } else {
                work.api_request.as_ref()
            };
            if target.is_some_and(|r| {
                r.account_id == request.account_id
                    && r.workspace_id == request.workspace_id
                    && r.project_name == request.project_name
            }) {
                work.phase = if response["state"] == "uncertain" {
                    "uncertain"
                } else {
                    "observed"
                }
                .into();
                if matches!(request.provider, Provider::Cloudflare) {
                    work.static_receipt = Some(response.clone());
                } else {
                    work.api_receipt = Some(response.clone());
                }
                if work.request.provider.name() == request.provider.name() {
                    work.receipt = Some(response.clone());
                }
                save_work(&work)?;
            }
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_operation_guard_refuses_concurrent_deploy_and_backup() {
        let first=hold_operation().unwrap();
        assert!(matches!(hold_operation(),Err(e) if e.code=="HOSTED_BUSY"));
        drop(first);
        assert!(hold_operation().is_ok());
    }

    #[test]
    fn native_io_failures_keep_uncertainty_and_hide_system_error_text() {
        struct FailedWriter;
        impl Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("SYNTHETIC_PRIVATE_SYSTEM_TEXT"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let error = write_runtime_input(Some(FailedWriter), b"synthetic-input").unwrap_err();
        assert_eq!(error.code, "HOSTED_RUNTIME_INPUT_UNCERTAIN");
        assert!(!error.guidance.contains("SYNTHETIC_PRIVATE"));
        assert_eq!(
            write_runtime_input(None::<Vec<u8>>, b"input")
                .unwrap_err()
                .code,
            "HOSTED_RUNTIME_INPUT_UNCERTAIN"
        );
        let error =
            runtime_wait(Err(std::io::Error::other("SYNTHETIC_PRIVATE_SYSTEM_TEXT"))).unwrap_err();
        assert_eq!(error.code, "HOSTED_RUNTIME_WAIT_UNCERTAIN");
        assert!(!error.guidance.contains("SYNTHETIC_PRIVATE"));
        assert!(runtime_wait(Ok(None)).unwrap().is_none());
        assert!(runtime_abort(Duration::from_secs(180), false, false).is_none());
        assert_eq!(
            runtime_abort(Duration::from_secs(181), false, false)
                .unwrap()
                .code,
            "HOSTED_RUNTIME_TIMEOUT_UNCERTAIN"
        );
        for (stdout, stderr) in [(true, false), (false, true)] {
            assert_eq!(
                runtime_abort(Duration::ZERO, stdout, stderr).unwrap().code,
                "HOSTED_RUNTIME_OUTPUT_UNCERTAIN"
            );
        }
    }
    #[test]
    fn failure_frame_survives_warnings_and_rejects_extra_or_duplicate_fields() {
        let frame = br#"{"errorCode":"HOSTED_CF_ASSET_UPLOAD_HTTP_204"}"#;
        assert_eq!(
            runtime_failure(frame, b"incidental synthetic warning").code,
            "HOSTED_CF_ASSET_UPLOAD_HTTP_204"
        );
        assert_eq!(
            runtime_failure(
                br#"{"errorCode":"HOSTED_JS_DEPLOY_FORM_TYPE_ERROR"}"#,
                b"warning"
            )
            .code,
            "HOSTED_JS_DEPLOY_FORM_TYPE_ERROR"
        );
        assert_eq!(
            runtime_failure(b"", b"HOSTED_TOKEN_REQUIRED").code,
            "HOSTED_TOKEN_REQUIRED"
        );
        for frame in [
            br#"{"errorCode":"HOSTED_TOKEN_REQUIRED","message":"private"}"#.as_slice(),
            br#"{"errorCode":"HOSTED_TOKEN_REQUIRED","errorCode":"HOSTED_BUSY"}"#,
            br#"{"errorCode":"HOSTED_JS_ARBITRARY_ERROR"}"#,
            br#"{"errorCode":"HOSTED_UNCLASSIFIED_EXCEPTION"}"#,
            br#"{"errorCode":"HOSTED_CF_ASSET_UPLOAD_HTTP_204?token=private"}"#,
        ] {
            assert_eq!(
                runtime_failure(frame, b"HOSTED_TOKEN_REQUIRED").code,
                "HOSTED_RESPONSE_INVALID"
            );
        }
    }
    #[test]
    fn reconciliation_returns_first_safe_failure_and_still_observes_both_saved_targets() {
        let work: Work = serde_json::from_value(serde_json::json!({
            "version":1,"request":{"provider":"cloudflare","action":"deploy-static"},
            "sourcePath":"/synthetic/preserved","sourceSha256":"a".repeat(64),
            "source":{"title":"Existing","publicationId":"original-publication","projectName":"show-a","revisionSha256":"b".repeat(64),"indexSha256":"c".repeat(64)},
            "phase":"pending","staticRequest":{"provider":"cloudflare","action":"deploy-static","accountId":"account-a","projectName":"show-a","staticOnly":false},
            "apiRequest":{"provider":"render","action":"deploy-api","workspaceId":"workspace-a","projectName":"show-a"},
            "apiOrigin":"https://show-a.onrender.com"
        })).unwrap();
        let mut observed = 0;
        let error = reconcile_work(work, |request| {
            assert!(matches!(request.action, Action::Reconcile));
            assert_eq!(
                request.expected_manifest_sha256.as_deref(),
                Some("a".repeat(64).as_str())
            );
            observed += 1;
            if matches!(request.provider, Provider::Cloudflare) {
                assert_eq!(
                    request.api_origin.as_deref(),
                    Some("https://show-a.onrender.com")
                );
                Err(refused("HOSTED_CF_PROJECT_READ_HTTP_404_CODE_7003"))
            } else {
                Err(refused("HOSTED_PERMISSION_REQUIRED"))
            }
        })
        .unwrap_err();
        assert_eq!(observed, 2);
        assert_eq!(error.code, "HOSTED_CF_PROJECT_READ_HTTP_404_CODE_7003");
    }
    #[test]
    fn diagnostics_accept_only_fixed_stage_status_and_numeric_provider_code() {
        assert!(safe_failure_code(
            "HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_10000"
        ));
        assert!(safe_failure_code(
            "HOSTED_CF_DEPLOY_STATUS_HTTP_200_CODE_8000000"
        ));
        for code in [
            "HOSTED_CF_ARBITRARY_HTTP_404_CODE_10000",
            "HOSTED_CF_ASSET_CHECK_HTTP_500",
            "HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_secret",
            "HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_0000",
            "HOSTED_CF_ASSET_CHECK_HTTP_404?token=secret",
        ] {
            assert!(!safe_failure_code(code));
        }
    }
    #[test]
    fn provider_wire_refuses_credentials_and_renderer_manifest_authority() {
        assert!(
            serde_json::from_value::<Request>(
                serde_json::json!({"provider":"other","action":"accounts"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<Request>(
                serde_json::json!({"provider":"cloudflare","action":"accounts","token":"synthetic"})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<Request>(serde_json::json!({"provider":"cloudflare","action":"deploy-static","publicUploadApproved":true,"expectedManifestSha256":"renderer-supplied"})).is_err());
        let request: Request = serde_json::from_value(serde_json::json!({"provider":"cloudflare","action":"deploy-static","publicUploadApproved":true})).unwrap();
        assert!(request.expected_manifest_sha256.is_none());
        assert!(valid_request(&request));
        assert!(
            serde_json::to_value(request)
                .unwrap()
                .get("publicUploadApproved")
                .is_none()
        );
    }
    #[test]
    fn native_writes_require_the_existing_publication_and_cost_acknowledgements() {
        let read: Request = serde_json::from_value(
            serde_json::json!({"provider":"render","action":"services","workspaceId":"owner-a"}),
        )
        .unwrap();
        assert!(valid_request(&read));
        let mut write:Request=serde_json::from_value(serde_json::json!({"provider":"render","action":"deploy-api","publicUploadApproved":false,"costReviewed":false})).unwrap();
        assert!(!valid_request(&write));
        write.public_upload_approved = true;
        assert!(!valid_request(&write));
        write.cost_reviewed = true;
        assert!(valid_request(&write));
        write.provider = Provider::Cloudflare;
        assert!(!valid_request(&write));
    }
    #[test]
    fn serialized_restart_intent_preserves_both_stages_without_credentials() {
        let request:Request=serde_json::from_value(serde_json::json!({"provider":"cloudflare","action":"deploy-static","staticOnly":false,"accountId":"account-a","projectName":"show-a"})).unwrap();
        let mut request = request;
        request.expected_manifest_sha256 = Some("a".repeat(64));
        request.api_origin = Some("https://show-a.onrender.com".into());
        let work = Work {
            version: 1,
            request: request.clone(),
            source_path: "/synthetic/public-bundle".into(),
            source_sha256: "a".repeat(64),
            source: SourceInfo {
                title: "테스트 전시".into(),
                publication_id: "publication-original".into(),
                project_name: "show-a".into(),
                revision_sha256: "b".repeat(64),
                index_sha256: "c".repeat(64),
            },
            receipt: None,
            phase: "pending".into(),
            static_request: Some(request),
            api_request: None,
            static_receipt: Some(
                serde_json::json!({"provider":"cloudflare","action":"deploy-static","state":"ready","url":"https://show-a.pages.dev"}),
            ),
            api_receipt: Some(
                serde_json::json!({"provider":"render","action":"status","state":"deployed","serviceId":"srv-existing","url":"https://show-a.onrender.com"}),
            ),
            api_requested: true,
            api_origin: Some("https://show-a.onrender.com".into()),
        };
        let bytes = work_bytes(&work).unwrap();
        let restored: Work = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.source.publication_id, "publication-original");
        assert_eq!(
            restored.static_receipt.unwrap()["url"],
            "https://show-a.pages.dev"
        );
        assert_eq!(restored.api_receipt.unwrap()["serviceId"], "srv-existing");
        assert_eq!(
            restored.api_origin.as_deref(),
            Some("https://show-a.onrender.com")
        );
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("ADMIN_PASSWORD"));
        assert!(!text.contains("FREEZE_SIGNING_KEY"));
        assert!(!text.contains("tokens"));
    }
    fn saved_recovery_fixture()->Work {
        serde_json::from_value(serde_json::json!({"version":1,"request":{"provider":"render","action":"status","workspaceId":"tea-fixture","projectName":"show-a","serviceId":"srv-fixture","postgresId":"dpg-fixture"},"sourcePath":"/synthetic/absent-package","sourceSha256":"a".repeat(64),"source":{"title":"Synthetic","publicationId":"public-fixture","projectName":"show-a","revisionSha256":"b".repeat(64),"indexSha256":"c".repeat(64)},"receipt":null,"phase":"observed","apiReceipt":{"provider":"render","serviceId":"srv-fixture","postgresId":"dpg-fixture","url":"https://show-a.onrender.com"},"apiOrigin":"https://show-a.onrender.com"})).unwrap()
    }
    fn saved_configuration_fixture()->serde_json::Value {
        serde_json::json!({"envVars":[{"key":"CLIENT_ORIGIN","value":"https://show-a.pages.dev"},{"key":"TENANT_ID","value":"00000000-0000-4000-8000-000000000001"},{"key":"ADMIN_SUBJECT","value":"fixture-owner"},{"key":"ADMIN_PASSWORD","value":"synthetic-password"},{"key":"FREEZE_SIGNING_KEY_BASE64","value":"synthetic-key-shape-only-Node-validates-crypto"}]})
    }
    #[test]
    fn recovery_loads_exact_original_configuration_without_static_pin_invention() {
        let work=saved_recovery_fixture();let expected=recovery_source_from_work(&work).unwrap();
        let original=serde_json::to_vec(&saved_configuration_fixture()).unwrap();let retained=original.clone();
        let value=recovery_configuration_from_work(&work,&expected,|account|{assert_eq!(account,runtime_account("tea-fixture","show-a"));Ok(original.clone())}).unwrap();
        assert_eq!(original,retained);assert_eq!(value["envVars"],saved_configuration_fixture()["envVars"]);assert_eq!(value["origin"],"https://show-a.onrender.com");
        assert!(!value.to_string().contains("STATIC_"));
        let mut changed=expected;changed.postgres_id="dpg-other".into();
        assert_eq!(recovery_configuration_from_work(&work,&changed,|_|panic!("No key lookup for another source")).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_CHANGED");
    }
    #[test]
    fn recovery_missing_denied_malformed_and_partial_saved_configuration_never_create_values() {
        let work=saved_recovery_fixture();let source=recovery_source_from_work(&work).unwrap();
        assert_eq!(recovery_configuration_from_work(&work,&source,|_|Err(())).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_REQUIRED");
        let mut partial=saved_configuration_fixture();partial["envVars"].as_array_mut().unwrap().push(serde_json::json!({"key":"STATIC_INDEX_SHA256","value":"d".repeat(64)}));
        let mut duplicate=saved_configuration_fixture();let first=duplicate["envVars"][0].clone();duplicate["envVars"].as_array_mut().unwrap().push(first);
        for bytes in [Vec::new(),b"invalid-json".to_vec(),b"{}".to_vec(),serde_json::to_vec(&partial).unwrap(),serde_json::to_vec(&duplicate).unwrap()] {
            assert_eq!(recovery_configuration_from_work(&work,&source,|_|Ok(bytes)).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_REQUIRED");
        }
        let mut original=work;original.api_origin=Some("https://other.onrender.com".into());
        assert_eq!(recovery_configuration_from_work(&original,&source,|_|panic!("Mismatched origin must not load")).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_CHANGED");
    }
    #[test]
    fn exported_publication_subfolder_reuses_existing_source_admission_and_rejects_changed_bytes(){
        let folder=std::env::temp_dir().join(format!("exhibitos-source-admission-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let publication=folder.join("publication");std::fs::create_dir_all(publication.join("data")).unwrap();
        let document=serde_json::to_vec(&serde_json::json!({"publication":{"id":"publication-original","status":"published","revisionSha256":"b".repeat(64)},"exhibition":{"title":"Synthetic restored static source"}})).unwrap();
        let hash=format!("{:x}",Sha256::digest(&document));let name=format!("data/{hash}.json");
        let index=serde_json::to_vec(&serde_json::json!({"publication":{"path":name,"sha256":hash}})).unwrap();
        let manifest=serde_json::to_vec(&serde_json::json!({"format":"exhibitos-static-publication-v1","publicationId":"publication-original","revisionSha256":"b".repeat(64),"indexSha256":format!("{:x}",Sha256::digest(&index))})).unwrap();
        for (name,bytes) in [(name.as_str(),document.as_slice()),("static-publication-index.json",index.as_slice()),("static-export-manifest.json",manifest.as_slice())]{std::fs::write(publication.join(name),bytes).unwrap();}
        assert!(read_source(folder.to_str().unwrap()).is_err(),"Select the publication subfolder, not the companion wrapper");
        let source=read_source(publication.to_str().unwrap()).unwrap();assert_eq!(source.info.publication_id,"publication-original");assert_eq!(source.sha256,format!("{:x}",Sha256::digest(&manifest)));
        std::fs::write(publication.join(name),b"changed").unwrap();assert!(read_source(publication.to_str().unwrap()).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn publication_capture_requires_original_pins_and_projects_receipts_without_secrets(){
        let mut work=saved_recovery_fixture();let source=recovery_source_from_work(&work).unwrap();
        let mut configuration=saved_configuration_fixture();
        assert!(recovery_publication(&work,&configuration).is_none());
        for (key,value) in [("STATIC_PUBLICATION_ID",work.source.publication_id.clone()),("STATIC_REVISION_SHA256",work.source.revision_sha256.clone()),("STATIC_INDEX_SHA256",work.source.index_sha256.clone())]{configuration["envVars"].as_array_mut().unwrap().push(serde_json::json!({"key":key,"value":value}));}
        work.api_receipt.as_mut().unwrap()["tokens"]=serde_json::json!({"render":"synthetic-forbidden-secret"});
        work.api_receipt.as_mut().unwrap()["envVars"]=serde_json::json!([{"key":"ADMIN_PASSWORD","value":"synthetic-forbidden-password"}]);
        let copy=recovery_publication(&work,&configuration).unwrap();
        assert_eq!(copy.manifest_sha256,work.source_sha256);
        let text=String::from_utf8(copy.snapshot.clone()).unwrap();
        for forbidden in ["synthetic-forbidden","tokens","envVars","ADMIN_PASSWORD","FREEZE_SIGNING_KEY"]{assert!(!text.contains(forbidden));}
        assert!(recovery_snapshot_matches(&copy.snapshot,&source));
        let mut other=source;other.service_id="srv-other".into();assert!(!recovery_snapshot_matches(&copy.snapshot,&other));
        work.version=2;assert!(recovery_publication(&work,&configuration).is_none());work.version=1;
        work.source.index_sha256="d".repeat(64);assert!(recovery_publication(&work,&configuration).is_none());
    }
    #[test]
    fn signing_key_base64_uses_standard_padding_without_a_new_crypto_format() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xee, 0xdd]), "/+7d");
    }
}
