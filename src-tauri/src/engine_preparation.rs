// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{LifecycleError, LifecycleService, engine_preparation as engine};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Status,
    Prepare,
    Install,
    OpenInstaller,
    Start,
    Cancel,
    License,
}
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:match code{"ENGINE_PREP_OS_PERMISSION"=>"응용 프로그램 설치 권한을 확인해 주세요. 공식 설치 창에서 필요한 macOS 승인을 마친 뒤 다시 확인하세요. 기존 Docker는 바꾸지 않았어요.","ENGINE_PREP_UNSUPPORTED"=>"이 설치 파일의 실행 도구 준비는 Apple 칩 Mac용 Docker Desktop에 연결돼 있어요. 기존 실행 도구와 데이터는 유지됩니다.",_=>"실행 도구 준비를 완료하지 못했어요. 현재 상태를 확인한 뒤 해당 단계에서 다시 시도하세요. 기존 설치는 변경하지 않았어요."}.into()}
}
pub fn cache(app: &tauri::AppHandle) -> Result<PathBuf, LifecycleError> {
    use tauri::Manager;
    app.path()
        .app_cache_dir()
        .map(|p| p.join("engine-preparation/docker-4.95.0-arm64"))
        .map_err(|_| failure("ENGINE_PREP_STATE"))
}
pub fn helper(root: &Path) -> Result<(), LifecycleError> {
    if root.file_name().and_then(|n| n.to_str()) != Some("docker-4.95.0-arm64")
        || root
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            != Some("engine-preparation")
    {
        return Err(failure("ENGINE_PREP_STATE"));
    }
    engine::run(root)?;
    engine::ensure_automatic_continuation(root)?;
    if !Path::new("/Applications/Docker.app").exists() {
        if let Err(e) = engine::install_desktop(root) {
            if e.code == "ENGINE_PREP_OS_PERMISSION" {
                let path = engine::installer(root)?;
                open::that_detached(path).map_err(|_| failure("ENGINE_PREP_STATE"))?;
            }
            return Err(e);
        }
    }
    engine::ensure_automatic_continuation(root)?;
    start(root).map(|_| ())
}
fn supported(resources: &Path) -> Result<(), LifecycleError> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err(failure("ENGINE_PREP_UNSUPPORTED"));
    }
    let p = resources.join("platform-runtime/generated/manifest.json");
    let m = std::fs::symlink_metadata(&p).map_err(|_| failure("ENGINE_PREP_UNSUPPORTED"))?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 65536 {
        return Err(failure("ENGINE_PREP_UNSUPPORTED"));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p).map_err(|_| failure("ENGINE_PREP_UNSUPPORTED"))?)
            .map_err(|_| failure("ENGINE_PREP_UNSUPPORTED"))?;
    if v["preferredEngine"] != "docker" {
        return Err(failure("ENGINE_PREP_UNSUPPORTED"));
    }
    Ok(())
}
fn start(root: &Path) -> Result<engine::Context, LifecycleError> {
    let app = engine::verify_desktop_app()?;
    open::that_detached(app).map_err(|_| failure("ENGINE_PREP_STATE"))?;
    engine::mark_awaiting_engine(root)
}
pub fn request(
    root: &Path,
    resources: &Path,
    service: &LifecycleService,
    action: Action,
) -> Result<engine::Context, LifecycleError> {
    if matches!(action, Action::License) {
        open::that_detached("https://docs.docker.com/subscription-billing/desktop-license/")
            .map_err(|_| failure("ENGINE_PREP_STATE"))?;
        return engine::context(root);
    }
    supported(resources)?;
    match action {
        Action::Status => engine::context(root),
        Action::Cancel => engine::cancel(root),
        Action::Prepare => {
            if service
                .detect()?
                .iter()
                .any(|e| e.kind == "docker" && e.available)
            {
                return Ok(engine::ready());
            }
            if Path::new("/Applications/Docker.app").exists() {
                return start(root);
            }
            if engine::running(root)? {
                return engine::context(root);
            }
            let before = engine::context(root)?;
            let mut child =
                Command::new(std::env::current_exe().map_err(|_| failure("ENGINE_PREP_STATE"))?)
                    .arg("--manager-engine-preparation-helper")
                    .arg(root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|_| failure("ENGINE_PREP_STATE"))?;
            let begin = Instant::now();
            loop {
                let current = engine::context(root)?;
                if current.job_id != before.job_id || engine::running(root)? {
                    return Ok(current);
                }
                if child
                    .try_wait()
                    .map_err(|_| failure("ENGINE_PREP_STATE"))?
                    .is_some()
                {
                    return engine::context(root);
                }
                if begin.elapsed() > Duration::from_secs(3) {
                    return Ok(current);
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        Action::Install => {
            if Path::new("/Applications/Docker.app").exists() {
                return start(root);
            }
            match engine::install_desktop(root) {
                Ok(_) => start(root),
                Err(e) => Err(e),
            }
        }
        Action::OpenInstaller => {
            let path = engine::installer(root)?;
            open::that_detached(path).map_err(|_| failure("ENGINE_PREP_STATE"))?;
            engine::context(root)
        }
        Action::Start => start(root),
        Action::License => unreachable!(),
    }
}
