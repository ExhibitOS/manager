// SPDX-License-Identifier: Apache-2.0
//! Fixed OS selectors return a path only. Existing backup admission checks the file later.
use exhibitos_lifecycle::LifecycleError;
use serde::Deserialize;
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Key,
    Archive,
    NewKey,
    ExportParent,
    Application,
}
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:"파일 선택을 완료하지 못했어요. 기존 입력과 자료는 그대로입니다. 다시 선택하거나 경로 직접 입력을 사용하세요.".into()}
}
#[cfg(target_os = "macos")]
static PICKER: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn choose(kind: Kind) -> Result<Option<String>, LifecycleError> {
    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};
        let _guard = PICKER
            .try_lock()
            .map_err(|_| failure("MAINTENANCE_PICKER_BUSY"))?;
        let script=match kind{Kind::Application=>"POSIX path of (choose file with prompt \"사용할 처리 앱을 선택하세요. Studio에는 필요하지 않습니다.\" of type {\"com.apple.application-bundle\"} without multiple selections allowed)",Kind::ExportParent=>"POSIX path of (choose folder with prompt \"암호화 백업 사본을 보관할 위치를 선택하세요. 키는 별도로 보관하세요.\")",Kind::Key=>"POSIX path of (choose file with prompt \"백업과 별도로 보관한 키 파일을 선택하세요\" without multiple selections allowed)",Kind::NewKey=>"POSIX path of (choose file name with prompt \"백업 폴더와 다른 위치에 새 키를 보관하세요. 기존 파일은 덮어쓰지 않습니다.\" default name \"ExhibitOS-backup.key\")",Kind::Archive=>"POSIX path of (choose folder with prompt \"암호화 백업 폴더를 선택하세요\")"};
        let result = Command::new("/usr/bin/osascript")
            .args(["-e", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|_| failure("MAINTENANCE_PICKER_UNAVAILABLE"))?;
        if !result.status.success() {
            return if String::from_utf8_lossy(&result.stderr).contains("(-128)") {
                Ok(None)
            } else {
                Err(failure("MAINTENANCE_PICKER_UNAVAILABLE"))
            };
        }
        let text = String::from_utf8(result.stdout)
            .map_err(|_| failure("MAINTENANCE_PICKER_UNAVAILABLE"))?;
        let path = text.strip_suffix('\n').unwrap_or(&text);
        let path = path.strip_suffix('\r').unwrap_or(path);
        if !path.starts_with('/') || path.len() > 2048 || path.contains(['\r', '\n', '\0', ',']) {
            return Err(failure("MAINTENANCE_PICKER_PATH_INVALID"));
        }
        Ok(Some(path.into()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Err(failure("MAINTENANCE_PICKER_UNAVAILABLE"))
    }
}
