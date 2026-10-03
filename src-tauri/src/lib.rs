// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{
    Action, EngineProbe, Job, LifecycleError, LifecycleService, LogEvent, Status,
};
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;
struct DesktopState(Arc<LifecycleService>);
fn error(code: &str) -> LifecycleError {
    LifecycleError {
        code: code.into(),
        guidance: "관리 앱의 연결을 확인할 수 없습니다. 앱을 다시 열고 상태를 확인하세요.".into(),
    }
}
fn local_frontend(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let bundled = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (["http", "https"].contains(&url.scheme())
            && url.host_str() == Some("tauri.localhost")
            && url.port().is_none());
    #[cfg(debug_assertions)]
    let developer =
        url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == Some(1420);
    #[cfg(not(debug_assertions))]
    let developer = false;
    bundled || developer
}
fn caller(window: &WebviewWindow) -> Result<(), LifecycleError> {
    if window.label() != "main"
        || !local_frontend(&window.url().map_err(|_| error("MANAGER_ORIGIN"))?)
    {
        return Err(error("MANAGER_ORIGIN"));
    }
    Ok(())
}
async fn blocking<T: Send + 'static>(
    state: Arc<LifecycleService>,
    task: impl FnOnce(&LifecycleService) -> Result<T, LifecycleError> + Send + 'static,
) -> Result<T, LifecycleError> {
    tauri::async_runtime::spawn_blocking(move || task(&state))
        .await
        .map_err(|_| error("MANAGER_OPERATION"))?
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationInput {
    image: String,
    key_path: String,
    source_path: String,
}
impl VerificationInput {
    fn validate(&self) -> Result<(), LifecycleError> {
        if !self.image.strip_prefix("sha256:").is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        }) || [&self.key_path, &self.source_path].iter().any(|path| {
            path.len() > 2048
                || !PathBuf::from(path.as_str()).is_absolute()
                || path.chars().any(|c| c.is_control() || c == ',')
        }) {
            return Err(LifecycleError {
                code: "BACKUP_INPUT_INVALID".into(),
                guidance: "준비한 백업·키의 전체 경로와 검증된 실행 패키지 ID를 확인하세요.".into(),
            });
        }
        Ok(())
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreationInput {
    image: String,
    key_path: String,
    external_writers_quiesced: bool,
    downtime_accepted: bool,
}
impl CreationInput {
    fn validate(&self) -> Result<(), LifecycleError> {
        if !self.external_writers_quiesced || !self.downtime_accepted {
            return Err(LifecycleError {
                code: "BACKUP_OPERATOR_ACK_REQUIRED".into(),
                guidance: "외부 쓰기 중지와 전시 중단을 확인한 후 새 백업을 시작하세요.".into(),
            });
        }
        VerificationInput {
            image: self.image.clone(),
            key_path: self.key_path.clone(),
            source_path: self.key_path.clone(),
        }
        .validate()
    }
}
#[tauri::command]
async fn manager_create_backup(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    input: CreationInput,
) -> Result<exhibitos_lifecycle::backup_creation::BackupCreationReceipt, LifecycleError> {
    caller(&window)?;
    input.validate()?;
    blocking(state.0.clone(), move |service| {
        service.create_backup(
            &input.image,
            std::path::Path::new(&input.key_path),
            input.external_writers_quiesced,
        )
    })
    .await
}
#[tauri::command]
async fn manager_backup_jobs(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Vec<exhibitos_lifecycle::backup_creation::BackupCreationJob>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.backup_jobs()).await
}
#[tauri::command]
async fn manager_verify_backup(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    input: VerificationInput,
) -> Result<exhibitos_lifecycle::maintenance::VerificationReceipt, LifecycleError> {
    caller(&window)?;
    input.validate()?;
    blocking(state.0.clone(), move |service| {
        service.verify_backup(
            &input.image,
            std::path::Path::new(&input.key_path),
            std::path::Path::new(&input.source_path),
        )
    })
    .await
}
#[tauri::command]
async fn manager_status(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Status, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.status()).await
}
#[tauri::command]
async fn manager_detect(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Vec<EngineProbe>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.detect()).await
}
#[tauri::command]
async fn manager_install(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Job, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.install()).await
}
#[tauri::command]
async fn manager_action(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    action: Action,
) -> Result<Job, LifecycleError> {
    caller(&window)?;
    if action == Action::Install {
        return Err(error("MANAGER_ACTION"));
    }
    blocking(state.0.clone(), move |service| service.execute(action)).await
}
#[tauri::command]
async fn manager_jobs(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Vec<Job>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.jobs()).await
}
#[tauri::command]
async fn manager_logs(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<Vec<LogEvent>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| service.logs()).await
}
fn allowed_exhibition_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some_and(|port| port > 0)
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && ["/", "/studio"].contains(&url.path())
}
#[tauri::command]
async fn manager_open_exhibition(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<(), LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), |service| {
        let url = service.open_url()?;
        if !allowed_exhibition_url(&url) {
            return Err(error("MANAGER_OPEN_URL"));
        }
        open::that_detached(url).map_err(|_| error("MANAGER_OPEN_FAILED"))
    })
    .await
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let root = match std::env::var_os("EXHIBITOS_MANAGER_ROOT") {
                Some(root) => {
                    let path = PathBuf::from(root);
                    if !path.is_absolute() {
                        return Err("Manager runtime root must be absolute".into());
                    }
                    path
                }
                None => app.path().app_data_dir()?.join("local-runtime"),
            };
            let service = LifecycleService::new(root)?;
            app.manage(DesktopState(Arc::new(service)));
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("ExhibitOS Manager")
                .inner_size(1120.0, 900.0)
                .min_inner_size(640.0, 620.0)
                .on_navigation(local_frontend)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            manager_status,
            manager_detect,
            manager_install,
            manager_action,
            manager_jobs,
            manager_logs,
            manager_open_exhibition,
            manager_verify_backup,
            manager_create_backup,
            manager_backup_jobs
        ])
        .run(tauri::generate_context!())
        .expect("Manager desktop startup failed");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_requires_both_acknowledgements_and_safe_input() {
        let mut input = CreationInput {
            image: format!("sha256:{}", "a".repeat(64)),
            key_path: std::env::temp_dir()
                .join("synthetic-key")
                .to_string_lossy()
                .into(),
            external_writers_quiesced: true,
            downtime_accepted: true,
        };
        assert!(input.validate().is_ok());
        input.external_writers_quiesced = false;
        assert_eq!(
            input.validate().unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        input.external_writers_quiesced = true;
        input.downtime_accepted = false;
        assert_eq!(
            input.validate().unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        input.downtime_accepted = true;
        input.key_path = "private key bytes".into();
        assert_eq!(input.validate().unwrap_err().code, "BACKUP_INPUT_INVALID");
        input.key_path = "/private/tmp/key,target=/host".into();
        assert!(input.validate().is_err());
    }
    #[test]
    fn creation_wire_input_rejects_unknown_commands_and_non_boolean_acknowledgements() {
        let input = serde_json::json!({
            "image":format!("sha256:{}", "a".repeat(64)),
            "keyPath":"/private/tmp/synthetic-key",
            "externalWritersQuiesced":true,
            "downtimeAccepted":true
        });
        assert!(serde_json::from_value::<CreationInput>(input.clone()).is_ok());
        for (key, value) in [
            ("command", serde_json::json!("rm --force")),
            ("keyBytes", serde_json::json!("private")),
            ("externalWritersQuiesced", serde_json::json!("true")),
            ("downtimeAccepted", serde_json::json!(null)),
        ] {
            let mut invalid = input.clone();
            invalid[key] = value;
            assert!(serde_json::from_value::<CreationInput>(invalid).is_err());
        }
    }
    #[test]
    fn verification_request_rejects_commands_key_contents_and_mount_injection() {
        let valid = VerificationInput {
            image: format!("sha256:{}", "a".repeat(64)),
            key_path: std::env::temp_dir()
                .join("synthetic-key")
                .to_string_lossy()
                .into(),
            source_path: std::env::temp_dir()
                .join("synthetic-archive")
                .to_string_lossy()
                .into(),
        };
        assert!(valid.validate().is_ok());
        for image in ["image:latest", "sha256:abc", "shell command"] {
            let value = VerificationInput {
                image: image.into(),
                key_path: valid.key_path.clone(),
                source_path: valid.source_path.clone(),
            };
            assert_eq!(value.validate().unwrap_err().code, "BACKUP_INPUT_INVALID");
        }
        for path in [
            "key contents",
            "relative",
            "/private/tmp/key,target=/evil",
            "/private/tmp/key\nSECRET=private",
        ] {
            let value = VerificationInput {
                image: valid.image.clone(),
                key_path: path.into(),
                source_path: valid.source_path.clone(),
            };
            assert!(value.validate().is_err());
        }
    }
    #[test]
    fn remote_sources_and_privileged_url_replacements_are_denied() {
        for source in [
            "https://example.com/",
            "file:///tmp/index.html",
            "http://127.0.0.1:3000/",
            "https://user@tauri.localhost/",
        ] {
            assert!(!local_frontend(&Url::parse(source).unwrap()));
        }
        assert!(local_frontend(
            &Url::parse("tauri://localhost/index.html").unwrap()
        ));
        for target in [
            "https://example.com/",
            "http://127.0.0.1.evil:3000/",
            "http://user:secret@127.0.0.1:3000/",
            "http://127.0.0.1:3000/?command=other",
            "http://127.0.0.1:3000/evil",
            "file:///tmp/exhibition",
        ] {
            assert!(!allowed_exhibition_url(target));
        }
        assert!(allowed_exhibition_url("http://127.0.0.1:3000/"));
    }
}
