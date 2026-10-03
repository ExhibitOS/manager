// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{
    Action, EngineProbe, Job, LifecycleError, LifecycleService, LogEvent, Status,
};
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;
struct DesktopState(Arc<exhibitos_lifecycle::installations::InstallationController>);
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
    state: Arc<exhibitos_lifecycle::installations::InstallationController>,
    selection_token: String,
    task: impl FnOnce(&LifecycleService) -> Result<T, LifecycleError> + Send + 'static,
) -> Result<T, LifecycleError> {
    tauri::async_runtime::spawn_blocking(move || state.with_current(&selection_token, task))
        .await
        .map_err(|_| error("MANAGER_OPERATION"))?
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreationSelectionInput {
    preserve_existing: bool,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectionInput {
    target_id: String,
    preserve_existing: bool,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconciliationInput {
    kind: String,
    target_id: String,
    preserve_candidates: bool,
}
#[tauri::command]
async fn manager_reconcile_helper(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: ReconciliationInput,
) -> Result<exhibitos_lifecycle::helper_reconciliation::ReconciliationReceipt, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, move |service| {
        service.reconcile_helper(&input.kind, &input.target_id, input.preserve_candidates)
    })
    .await
}
#[tauri::command]
async fn manager_helper_reconciliations(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<Vec<exhibitos_lifecycle::helper_reconciliation::ReconciliationJob>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| {
        service.helper_reconciliations()
    })
    .await
}
#[tauri::command]
async fn manager_installations(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<exhibitos_lifecycle::installations::InstallationContext, LifecycleError> {
    caller(&window)?;
    let controller = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || controller.context())
        .await
        .map_err(|_| error("MANAGER_OPERATION"))?
}
#[tauri::command]
async fn manager_create_installation(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: CreationSelectionInput,
) -> Result<exhibitos_lifecycle::installations::InstallationContext, LifecycleError> {
    caller(&window)?;
    let controller = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        controller.create(&selection_token, input.preserve_existing)
    })
    .await
    .map_err(|_| error("MANAGER_OPERATION"))?
}
#[tauri::command]
async fn manager_select_installation(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: SelectionInput,
) -> Result<exhibitos_lifecycle::installations::InstallationContext, LifecycleError> {
    caller(&window)?;
    let controller = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        controller.select(&selection_token, &input.target_id, input.preserve_existing)
    })
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
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestorationInput {
    image: String,
    key_path: String,
    source_path: String,
    port: u16,
    fresh_installation_accepted: bool,
}
impl RestorationInput {
    fn validate(&self) -> Result<(), LifecycleError> {
        if !self.fresh_installation_accepted {
            return Err(LifecycleError {
                code: "BACKUP_OPERATOR_ACK_REQUIRED".into(),
                guidance:
                    "이 앱의 비어 있는 설치 공간에 새 전시를 복원하고 시작하는 데 동의하세요."
                        .into(),
            });
        }
        if self.port < 1024 {
            return Err(error("BACKUP_INPUT_INVALID"));
        }
        VerificationInput {
            image: self.image.clone(),
            key_path: self.key_path.clone(),
            source_path: self.source_path.clone(),
        }
        .validate()
    }
}
#[tauri::command]
async fn manager_restore_backup(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: RestorationInput,
) -> Result<exhibitos_lifecycle::restoration::RestorationReceipt, LifecycleError> {
    caller(&window)?;
    input.validate()?;
    blocking(state.0.clone(), selection_token, move |service| {
        service.restore_backup(
            &input.image,
            std::path::Path::new(&input.key_path),
            std::path::Path::new(&input.source_path),
            input.port,
            input.fresh_installation_accepted,
        )
    })
    .await
}
#[tauri::command]
async fn manager_restoration_context(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<exhibitos_lifecycle::restoration::RestorationContext, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| {
        service.restoration_context()
    })
    .await
}
#[tauri::command]
async fn manager_create_backup(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: CreationInput,
) -> Result<exhibitos_lifecycle::backup_creation::BackupCreationReceipt, LifecycleError> {
    caller(&window)?;
    input.validate()?;
    blocking(state.0.clone(), selection_token, move |service| {
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
    selection_token: String,
) -> Result<Vec<exhibitos_lifecycle::backup_creation::BackupCreationJob>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| {
        service.backup_jobs()
    })
    .await
}
#[tauri::command]
async fn manager_verify_backup(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    input: VerificationInput,
) -> Result<exhibitos_lifecycle::maintenance::VerificationReceipt, LifecycleError> {
    caller(&window)?;
    input.validate()?;
    blocking(state.0.clone(), selection_token, move |service| {
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
    selection_token: String,
) -> Result<Status, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| service.status()).await
}
#[tauri::command]
async fn manager_detect(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<Vec<EngineProbe>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| service.detect()).await
}
#[tauri::command]
async fn manager_install(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<Job, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| {
        service.install()
    })
    .await
}
#[tauri::command]
async fn manager_action(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
    action: Action,
) -> Result<Job, LifecycleError> {
    caller(&window)?;
    if action == Action::Install {
        return Err(error("MANAGER_ACTION"));
    }
    blocking(state.0.clone(), selection_token, move |service| {
        service.execute(action)
    })
    .await
}
#[tauri::command]
async fn manager_jobs(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<Vec<Job>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| service.jobs()).await
}
#[tauri::command]
async fn manager_logs(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    selection_token: String,
) -> Result<Vec<LogEvent>, LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| service.logs()).await
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
    selection_token: String,
) -> Result<(), LifecycleError> {
    caller(&window)?;
    blocking(state.0.clone(), selection_token, |service| {
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
            let override_root = match std::env::var_os("EXHIBITOS_MANAGER_ROOT") {
                Some(root) => {
                    let path = PathBuf::from(root);
                    if !path.is_absolute() {
                        return Err("Manager runtime root must be absolute".into());
                    }
                    Some(path)
                }
                None => None,
            };
            let controller = exhibitos_lifecycle::installations::InstallationController::new(
                app.path().app_data_dir()?,
                override_root,
            )?;
            app.manage(DesktopState(Arc::new(controller)));
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
            manager_backup_jobs,
            manager_restore_backup,
            manager_restoration_context,
            manager_reconcile_helper,
            manager_helper_reconciliations,
            manager_installations,
            manager_create_installation,
            manager_select_installation
        ])
        .run(tauri::generate_context!())
        .expect("Manager desktop startup failed");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoration_wire_requires_explicit_fresh_acknowledgement_and_bounded_port() {
        let valid = serde_json::json!({"image":format!("sha256:{}", "a".repeat(64)),"keyPath":"/private/tmp/key","sourcePath":"/private/tmp/archive","port":4500,"freshInstallationAccepted":true});
        assert!(
            serde_json::from_value::<RestorationInput>(valid.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        for (key, value) in [
            ("command", serde_json::json!("shell")),
            ("keyBytes", serde_json::json!("secret")),
            ("port", serde_json::json!(65536)),
            ("port", serde_json::json!("4500")),
            ("freshInstallationAccepted", serde_json::json!("true")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(serde_json::from_value::<RestorationInput>(invalid).is_err());
        }
        for (key, value, code) in [
            (
                "freshInstallationAccepted",
                serde_json::json!(false),
                "BACKUP_OPERATOR_ACK_REQUIRED",
            ),
            ("port", serde_json::json!(80), "BACKUP_INPUT_INVALID"),
            (
                "sourcePath",
                serde_json::json!("/tmp/source,target=/host"),
                "BACKUP_INPUT_INVALID",
            ),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert_eq!(
                serde_json::from_value::<RestorationInput>(invalid)
                    .unwrap()
                    .validate()
                    .unwrap_err()
                    .code,
                code
            );
        }
    }
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
    fn reconciliation_wire_input_rejects_raw_targets_and_non_boolean_consent() {
        let valid = serde_json::json!({"kind":"backup","targetId":"12345678-1234-1234-1234-123456789012","preserveCandidates":true});
        assert!(serde_json::from_value::<ReconciliationInput>(valid.clone()).is_ok());
        for (key, value) in [
            ("container", serde_json::json!("foreign")),
            ("command", serde_json::json!("rm")),
            ("preserveCandidates", serde_json::json!("true")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(serde_json::from_value::<ReconciliationInput>(invalid).is_err());
        }
    }
    #[test]
    fn selection_input_rejects_raw_paths_commands_and_non_boolean_consent() {
        let valid = serde_json::json!({"targetId":"12345678-1234-1234-1234-123456789012", "preserveExisting":true});
        assert!(serde_json::from_value::<SelectionInput>(valid.clone()).is_ok());
        for (key, value) in [
            ("path", serde_json::json!("/private/data")),
            ("command", serde_json::json!("shell")),
            ("preserveExisting", serde_json::json!("true")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(serde_json::from_value::<SelectionInput>(invalid).is_err());
        }
        assert!(
            serde_json::from_value::<CreationSelectionInput>(
                serde_json::json!({"preserveExisting":true})
            )
            .is_ok()
        );
        assert!(serde_json::from_value::<CreationSelectionInput>(valid).is_err());
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
