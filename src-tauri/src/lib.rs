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
            manager_open_exhibition
        ])
        .run(tauri::generate_context!())
        .expect("Manager desktop startup failed");
}
#[cfg(test)]
mod tests {
    use super::*;
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
