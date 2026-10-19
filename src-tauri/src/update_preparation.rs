// SPDX-License-Identifier: Apache-2.0
//! Fixed native preparation IPC. Worker status never requires a mutable Controller.
use exhibitos_lifecycle::LifecycleError;
#[cfg(target_os = "macos")]
use exhibitos_lifecycle::update_preparation as core;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;
pub struct Profile(pub Option<PathBuf>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub controller_blocked: bool,
    pub can_open_manager: bool,
    #[cfg(target_os = "macos")]
    pub job: Option<core::Receipt>,
    #[cfg(not(target_os = "macos"))]
    pub job: Option<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdInput {
    pub id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectInput {
    pub selection_token: String,
    pub release_folder: PathBuf,
    pub key_file: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchInput {
    pub selection_token: String,
    pub release_folder: PathBuf,
    pub key_file: PathBuf,
    pub preserve_data: bool,
    pub external_writers_quiesced: bool,
    pub close_app_and_prepare: bool,
}
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:match code{
 "UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"=>"이 앱 배포판에 업데이트 검증 도구가 준비되지 않았어요. 원래 설치와 자료는 변경하지 않았습니다. 검증 도구를 포함한 배포판이 필요해요.",
 "UPDATE_PREPARATION_BUSY"=>"업데이트 준비 작업이 진행 중이에요. 진행 화면에서 실제 상태를 확인해 주세요.",
 "UPDATE_PREPARATION_RECOVERY_REQUIRED"=>"이전 준비가 중단됐어요. 원본과 준비 사본은 보존됩니다. Manager에서 남은 작업과 복구 상태를 확인해 주세요.",
 _=>"업데이트 준비를 완료하지 못했어요. 원본과 이전 결과는 보존합니다. 선택한 파일과 현재 작업 상태를 확인해 주세요.",}.into()}
}
fn id(value: &str) -> bool {
    super::retry_id(value)
}
fn validate_id(input: &IdInput, required: bool) -> Result<(), LifecycleError> {
    if input.id.as_deref().is_some_and(|s| !id(s)) || required && input.id.is_none() {
        return Err(failure("UPDATE_PREPARATION_INPUT_INVALID"));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn boot(profile: &Path) -> Result<bool, LifecycleError> {
    Ok(core::startup(profile)?.controller_blocked)
}
#[cfg(not(target_os = "macos"))]
pub fn boot(_profile: &Path) -> Result<bool, LifecycleError> {
    Ok(false)
}
pub fn summary(
    app: &tauri::AppHandle,
    profile: &Profile,
    input: IdInput,
) -> Result<Summary, LifecycleError> {
    validate_id(&input, false)?;
    #[cfg(target_os = "macos")]
    if let Some(profile) = &profile.0 {
        let status = core::status(profile, input.id.as_deref())?;
        return Ok(Summary {
            controller_blocked: status.controller_blocked,
            can_open_manager: !status.controller_blocked
                && app.try_state::<super::DesktopState>().is_none(),
            job: status.job,
        });
    }
    let _ = (app, profile);
    Ok(Summary {
        controller_blocked: false,
        can_open_manager: false,
        job: None,
    })
}
#[cfg(target_os = "macos")]
pub fn inspect(
    resources: &Path,
    controller: &exhibitos_lifecycle::installations::InstallationController,
    input: InspectInput,
) -> Result<core::Inspection, LifecycleError> {
    controller.with_current(&input.selection_token, |_| Ok(()))?;
    // Resource availability is native-only; no manual interpreter/hash fallback.
    if let Err(e) = runtime(resources) {
        return Ok(core::Inspection {
            available: false,
            code: Some(e.code),
            version: None,
            changed_schema: None,
        });
    }
    match controller.inspect_update_preparation(
        &input.selection_token,
        &input.release_folder,
        &input.key_file,
    ) {
        Ok(value) => Ok(value),
        Err(e) => Ok(core::Inspection {
            available: false,
            code: Some(e.code),
            version: None,
            changed_schema: None,
        }),
    }
}
#[cfg(target_os = "macos")]
pub fn dispatch(
    resources: &Path,
    controller: &exhibitos_lifecycle::installations::InstallationController,
    input: DispatchInput,
) -> Result<(PathBuf, core::Receipt), LifecycleError> {
    if !input.preserve_data || !input.external_writers_quiesced || !input.close_app_and_prepare {
        return Err(failure("UPDATE_PREPARATION_CONSENT_REQUIRED"));
    }
    controller.with_current(&input.selection_token, |_| Ok(()))?;
    let _runtime = runtime(resources)?;
    controller.prepare_update_handoff(
        &input.selection_token,
        core::Input {
            release_folder: input.release_folder,
            key_file: input.key_file,
            preserve_data: input.preserve_data,
            external_writers_quiesced: input.external_writers_quiesced,
            close_app_and_prepare: input.close_app_and_prepare,
        },
    )
}
pub fn action(
    app: &tauri::AppHandle,
    profile: &Profile,
    input: IdInput,
    cancel: bool,
) -> Result<Summary, LifecycleError> {
    validate_id(&input, true)?;
    #[cfg(target_os = "macos")]
    {
        let selected = profile
            .0
            .as_ref()
            .ok_or_else(|| failure("UPDATE_PROFILE_UNAVAILABLE"))?;
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| failure("UPDATE_PREPARATION_INPUT_INVALID"))?;
        if cancel {
            core::cancel(selected, id)?;
        } else {
            core::reconcile(selected, id)?;
        }
        summary(
            app,
            profile,
            IdInput {
                id: Some(id.into()),
            },
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, profile, cancel);
        Err(failure("UPDATE_PREPARATION_UNSUPPORTED"))
    }
}
pub fn open_manager(app: &tauri::AppHandle, profile: &Profile) -> Result<(), LifecycleError> {
    let status = summary(app, profile, IdInput { id: None })?;
    if status.controller_blocked || !status.can_open_manager {
        return Err(failure("UPDATE_PREPARATION_BUSY"));
    }
    app.restart()
}
#[cfg(target_os = "macos")]
fn runtime(resources: &Path) -> Result<core::Runtime, LifecycleError> {
    // This adapter must use the compiled resource descriptor, including the
    // interpreter's stdlib/native library tree. Never infer it from PATH.
    let python = super::update_runtime::verified(resources)?;
    core::Runtime::from_native_resources(
        &resources.join("platform-runtime/generated"),
        python.python(),
        python.sha256(),
    )
}
#[cfg(target_os = "macos")]
pub fn helper(profile: &Path, id: &str, resources: &Path) -> Result<(), LifecycleError> {
    if !self::id(id) {
        return Err(failure("UPDATE_PREPARATION_INPUT_INVALID"));
    }
    exhibitos_lifecycle::enable_native_docker_transport();
    let python = super::update_runtime::verified(resources).map_err(|e| {
        let _ = core::before_start_failed(profile, id, &e.code);
        e
    })?;
    let runtime = core::Runtime::from_native_resources(
        &resources.join("platform-runtime/generated"),
        python.python(),
        python.sha256(),
    )
    .map_err(|e| {
        let _ = core::before_start_failed(profile, id, &e.code);
        e
    })?;
    let receipt = core::execute(profile, id, &runtime, || python.check())?;
    python.check()?;
    if receipt.state != core::State::Prepared {
        return Err(failure(
            receipt
                .code
                .as_deref()
                .unwrap_or("UPDATE_PREPARATION_FAILED"),
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn helper(_profile: &Path, _id: &str, _resources: &Path) -> Result<(), LifecycleError> {
    Err(failure("UPDATE_PREPARATION_UNSUPPORTED"))
}
#[tauri::command]
pub async fn manager_update_preparation_startup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    profile: tauri::State<'_, Profile>,
) -> Result<Summary, LifecycleError> {
    super::caller(&window)?;
    let profile = Profile(profile.0.clone());
    tauri::async_runtime::spawn_blocking(move || summary(&app, &profile, IdInput { id: None }))
        .await
        .map_err(|_| super::error("MANAGER_OPERATION"))?
}
#[tauri::command]
pub async fn manager_update_preparation_status(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    profile: tauri::State<'_, Profile>,
    input: IdInput,
) -> Result<Summary, LifecycleError> {
    super::caller(&window)?;
    let profile = Profile(profile.0.clone());
    tauri::async_runtime::spawn_blocking(move || summary(&app, &profile, input))
        .await
        .map_err(|_| super::error("MANAGER_OPERATION"))?
}
#[tauri::command]
pub async fn manager_update_preparation_inspect(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::DesktopState>,
    input: InspectInput,
) -> Result<serde_json::Value, LifecycleError> {
    super::caller(&window)?;
    #[cfg(target_os = "macos")]
    {
        let resources = app
            .path()
            .resource_dir()
            .map_err(|_| failure("UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"))?;
        let controller = state.0.clone();
        tauri::async_runtime::spawn_blocking(move || {
            serde_json::to_value(inspect(&resources, &controller, input)?)
                .map_err(|_| failure("UPDATE_PREPARATION_INVALID"))
        })
        .await
        .map_err(|_| super::error("MANAGER_OPERATION"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, input);
        Err(failure("UPDATE_PREPARATION_UNSUPPORTED"))
    }
}
#[tauri::command]
pub async fn manager_update_preparation_dispatch(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::DesktopState>,
    input: DispatchInput,
) -> Result<serde_json::Value, LifecycleError> {
    super::caller(&window)?;
    #[cfg(target_os = "macos")]
    {
        if super::remote::busy() || super::hosted::busy() || super::publication::busy() {
            return Err(failure("UPDATE_PREPARATION_BUSY"));
        }
        let resources = app
            .path()
            .resource_dir()
            .map_err(|_| failure("UPDATE_PREPARATION_RUNTIME_UNAVAILABLE"))?;
        let worker_resources = resources.clone();
        let controller = state.0.clone();
        let (profile, receipt) =
            tauri::async_runtime::spawn_blocking(move || dispatch(&resources, &controller, input))
                .await
                .map_err(|_| super::error("MANAGER_OPERATION"))??;
        let launch = std::env::current_exe().and_then(|binary| {
            std::process::Command::new(binary)
                .arg("--manager-update-preparation-helper")
                .arg(&profile)
                .arg(&receipt.id)
                .arg(&worker_resources)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        });
        if launch.is_err() {
            core::launch_failed(&profile, &receipt.id)?;
            return Err(failure("UPDATE_PREPARATION_LAUNCH_FAILED"));
        }
        let value =
            serde_json::to_value(receipt).map_err(|_| failure("UPDATE_PREPARATION_INVALID"))?;
        app.exit(0);
        Ok(value)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, input);
        Err(failure("UPDATE_PREPARATION_UNSUPPORTED"))
    }
}
#[tauri::command]
pub async fn manager_update_preparation_cancel(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    profile: tauri::State<'_, Profile>,
    input: IdInput,
) -> Result<Summary, LifecycleError> {
    super::caller(&window)?;
    let profile = Profile(profile.0.clone());
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || action(&handle, &profile, input, true))
        .await
        .map_err(|_| super::error("MANAGER_OPERATION"))?
}
#[tauri::command]
pub async fn manager_update_preparation_reconcile(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    profile: tauri::State<'_, Profile>,
    input: IdInput,
) -> Result<Summary, LifecycleError> {
    super::caller(&window)?;
    let profile = Profile(profile.0.clone());
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || action(&handle, &profile, input, false))
        .await
        .map_err(|_| super::error("MANAGER_OPERATION"))?
}
#[tauri::command]
pub fn manager_update_preparation_open_manager(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    profile: tauri::State<'_, Profile>,
) -> Result<(), LifecycleError> {
    super::caller(&window)?;
    open_manager(&app, &profile)
}
#[tauri::command]
pub async fn manager_update_preparation_choose_release(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, super::DesktopState>,
    selection_token: String,
) -> Result<Option<String>, LifecycleError> {
    super::caller(&window)?;
    let controller = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        controller.with_current(&selection_token, |_| {
            super::maintenance_picker::choose(super::maintenance_picker::Kind::UpdateRelease)
        })
    })
    .await
    .map_err(|_| super::error("MANAGER_OPERATION"))?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_preparation_inputs_reject_renderer_authority_and_runtime() {
        let mut value = serde_json::json!({"selectionToken":"11111111-1111-4111-8111-111111111111","releaseFolder":"/selected/release","keyFile":"/separate/key","preserveData":true,"externalWritersQuiesced":true,"closeAppAndPrepare":true});
        assert!(serde_json::from_value::<DispatchInput>(value.clone()).is_ok());
        for name in [
            "python",
            "sourceCommit",
            "targetInstance",
            "workspace",
            "maintenanceImage",
            "policy",
            "signatureVerified",
        ] {
            value[name] = true.into();
            assert!(serde_json::from_value::<DispatchInput>(value.clone()).is_err());
            value.as_object_mut().unwrap().remove(name);
        }
    }
    #[test]
    fn reconciliation_requires_an_exact_job_id_without_renderer_paths() {
        assert!(validate_id(&IdInput { id: None }, true).is_err());
        assert!(
            validate_id(
                &IdInput {
                    id: Some("../profile".into())
                },
                true
            )
            .is_err()
        );
        assert!(
            validate_id(
                &IdInput {
                    id: Some("11111111-1111-4111-8111-111111111111".into())
                },
                true
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<IdInput>(
                serde_json::json!({"id":null,"profile":"/arbitrary"})
            )
            .is_err()
        );
    }
}
