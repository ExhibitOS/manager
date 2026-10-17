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
struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
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
pub fn resume_remote(resources: PathBuf) -> Result<Option<ResumeInfo>, LifecycleError> {
    let work = load_work()?.ok_or_else(|| refused("HOSTED_WORK_UNAVAILABLE"))?;
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
    let mut failed = false;
    for mut request in requests {
        request.action = Action::Reconcile;
        request.expected_manifest_sha256 = Some(work.source_sha256.clone());
        if matches!(request.provider, Provider::Cloudflare) {
            request.api_origin = work.api_origin.clone();
        }
        if execute(resources.clone(), request).is_err() {
            failed = true;
        }
    }
    if failed {
        if let Some(mut work) = load_work()? {
            work.phase = "uncertain".into();
            save_work(&work)?;
        }
    }
    resume_local()
}
fn base64(bytes: &[u8]) -> String {
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
fn runtime_configuration(
    workspace: &str,
    project: &str,
) -> Result<serde_json::Value, LifecycleError> {
    let account = format!(
        "render-runtime-{:x}",
        Sha256::digest(format!("{workspace}\0{project}").as_bytes())
    );
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
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(refused("HOSTED_BUSY"));
    }
    let _busy = Busy;
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
    let written = child
        .stdin
        .take()
        .is_some_and(|mut pipe| pipe.write_all(&input).is_ok());
    input.fill(0);
    if !written {
        super::remote::stop(&mut child);
        token.fill(0);
        return Err(refused("HOSTED_OPERATION_UNCERTAIN"));
    }
    let out = child
        .stdout
        .take()
        .ok_or_else(|| refused("HOSTED_OPERATION_UNCERTAIN"))?;
    let err = child
        .stderr
        .take()
        .ok_or_else(|| refused("HOSTED_OPERATION_UNCERTAIN"))?;
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
            status = match child.try_wait() {
                Ok(value) => value,
                Err(_) => {
                    super::remote::stop(&mut child);
                    token.fill(0);
                    return Err(refused("HOSTED_OPERATION_UNCERTAIN"));
                }
            };
        }
        if start.elapsed() > Duration::from_secs(180)
            || output.as_ref().is_some_and(Result::is_err)
            || errors.as_ref().is_some_and(Result::is_err)
        {
            super::remote::stop(&mut child);
            token.fill(0);
            return Err(refused("HOSTED_OPERATION_UNCERTAIN"));
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
            .map_err(|_| refused("HOSTED_OPERATION_UNCERTAIN"))?;
        let code = std::str::from_utf8(&error).unwrap_or("").trim();
        return Err(refused(
            if code.starts_with("HOSTED_")
                && code.len() <= 80
                && code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            {
                code
            } else {
                "HOSTED_OPERATION_UNCERTAIN"
            },
        ));
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
    #[test]
    fn signing_key_base64_uses_standard_padding_without_a_new_crypto_format() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xee, 0xdd]), "/+7d");
    }
}
