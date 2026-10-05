// SPDX-License-Identifier: Apache-2.0
//! Bounded, allowlisted startup diagnostics; never prints raw setup errors or paths.
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Failure {
    pub code: &'static str,
    pub guidance: &'static str,
}
impl Failure {
    pub fn from_code(code: &str) -> Self {
        let (code, guidance) = match code {
            "MANAGER_ROOT_INVALID" => (
                "MANAGER_ROOT_INVALID",
                "검사 공간 경로가 올바르지 않습니다. 절대 경로를 지정한 뒤 다시 실행하세요.",
            ),
            "MANAGER_DATA_PATH_UNAVAILABLE" => (
                "MANAGER_DATA_PATH_UNAVAILABLE",
                "앱의 저장 위치를 확인할 수 없습니다. 사용자 계정의 저장 공간 접근을 확인하세요.",
            ),
            "MANAGER_WEBVIEW_UNAVAILABLE" => (
                "MANAGER_WEBVIEW_UNAVAILABLE",
                "앱 창을 만들 수 없습니다. 이 오류 코드를 전달해 창 구성과 실행 환경을 확인하세요.",
            ),
            "WINDOWS_PROFILE_ACL_INVALID" => (
                "WINDOWS_PROFILE_ACL_INVALID",
                "관리 공간의 소유자 또는 접근 권한을 확인할 수 없습니다. 기존 폴더와 권한을 보존하고 새 검사 공간에서 진단하세요.",
            ),
            "WINDOWS_PROFILE_PARENT_UNSAFE" => (
                "WINDOWS_PROFILE_PARENT_UNSAFE",
                "상위 폴더의 접근 권한을 안전하게 확인할 수 없습니다. 기존 데이터를 보존하고 새 검사 공간에서 진단하세요.",
            ),
            "WINDOWS_PROFILE_CREATE_REFUSED" => (
                "WINDOWS_PROFILE_CREATE_REFUSED",
                "새 관리 공간을 만들 수 없습니다. 기존 폴더를 삭제하거나 권한을 바꾸지 말고 이 오류 코드를 전달하세요.",
            ),
            "WINDOWS_PROFILE_HANDLE_UNAVAILABLE" => (
                "WINDOWS_PROFILE_HANDLE_UNAVAILABLE",
                "관리 공간을 열 수 없습니다. 다른 작업의 종료 여부와 저장 위치의 접근을 확인하세요.",
            ),
            "WINDOWS_PROFILE_TOKEN_UNAVAILABLE" => (
                "WINDOWS_PROFILE_TOKEN_UNAVAILABLE",
                "현재 Windows 계정의 보안 정보를 확인할 수 없습니다. 이 오류 코드를 전달하세요.",
            ),
            "WINDOWS_PROFILE_IDENTITY_INVALID" => (
                "WINDOWS_PROFILE_IDENTITY_INVALID",
                "관리 공간의 파일 식별자가 바뀌었습니다. 기존 파일과 검사 후보를 보존하고 진단하세요.",
            ),
            "WINDOWS_PROFILE_RECORD_OPEN_REFUSED" => (
                "WINDOWS_PROFILE_RECORD_OPEN_REFUSED",
                "관리 기록을 안전하게 열 수 없습니다. 파일과 권한을 보존하고 다른 실행 중인 작업을 확인하세요.",
            ),
            "BUSY" | "PROFILE_BUSY" | "WINDOWS_PROFILE_BUSY" => (
                "MANAGER_PROFILE_BUSY",
                "관리 공간을 사용 중인 작업이 있습니다. 진행 중인 작업이 끝난 뒤 다시 실행하세요.",
            ),
            "MANAGER_PROFILE_BUSY" => (
                "MANAGER_PROFILE_BUSY",
                "관리 공간을 사용 중인 작업이 있습니다. 진행 중인 작업이 끝난 뒤 다시 실행하세요.",
            ),
            "MANAGER_PROFILE_UNAVAILABLE"
            | "STATE_UNAVAILABLE"
            | "STATE_INVALID"
            | "PROFILE_PATH_INVALID"
            | "INSTALLATION_ROOT_UNAVAILABLE" => (
                "MANAGER_PROFILE_UNAVAILABLE",
                "관리 공간을 초기화할 수 없습니다. 기존 데이터를 보존하고 이 오류 코드를 전달하세요.",
            ),
            _ => (
                "MANAGER_STARTUP_FAILED",
                "관리 앱을 시작할 수 없습니다. 기존 데이터와 설정을 보존하고 이 오류 코드를 전달하세요.",
            ),
        };
        Self { code, guidance }
    }
    #[cfg(any(windows, test))]
    pub fn message(&self) -> String {
        format!(
            "ExhibitOS Manager를 시작할 수 없습니다.\n\n{}\n\n오류 코드: {}",
            self.guidance, self.code
        )
    }
    pub fn report(&self) {
        eprintln!("{}: {}", self.code, self.guidance);
        #[cfg(windows)]
        {
            // Standard ownerless native dialog works even before a WebView exists.
            let message: Vec<u16> = self.message().encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "ExhibitOS Manager".encode_utf16().chain(Some(0)).collect();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    0x10 | 0x10000,
                );
            }
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tauri's opaque SetupError forwards this code. The outer handler maps it
        // back through the allowlist rather than exposing arbitrary error text.
        f.write_str(self.code)
    }
}
impl std::error::Error for Failure {}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        window: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}

pub fn override_root(root: Option<OsString>) -> Result<Option<PathBuf>, Failure> {
    match root {
        Some(root) => {
            let path = PathBuf::from(root);
            if !path.is_absolute() {
                return Err(Failure::from_code("MANAGER_ROOT_INVALID"));
            }
            Ok(Some(path))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_errors_paths_and_secrets_never_enter_diagnostics() {
        for raw in [
            "STATE_INVALID secret=private",
            "C:\\private\\runtime.env",
            "WINDOWS_PROFILE_ACL_INVALID\nSECRET=private",
            "UNKNOWN_SECRET_VALUE",
        ] {
            let failure = Failure::from_code(raw);
            assert_eq!(failure.code, "MANAGER_STARTUP_FAILED");
            assert!(!failure.message().contains(raw));
            assert!(!format!("{failure:?}").contains(raw));
        }
    }
    #[test]
    fn profile_errors_retain_safe_codes_and_preservation_guidance() {
        let failure = Failure::from_code("WINDOWS_PROFILE_ACL_INVALID");
        assert_eq!(failure.to_string(), "WINDOWS_PROFILE_ACL_INVALID");
        assert!(failure.guidance.contains("보존"));
        assert_eq!(Failure::from_code("BUSY").code, "MANAGER_PROFILE_BUSY");
    }
    #[test]
    fn invalid_override_is_rejected_before_any_profile_creation() {
        assert!(override_root(None).unwrap().is_none());
        assert_eq!(
            override_root(Some("relative-check".into()))
                .unwrap_err()
                .code,
            "MANAGER_ROOT_INVALID"
        );
        let absolute = std::env::temp_dir().join("exhibitos-startup-synthetic-path-only");
        assert_eq!(
            override_root(Some(absolute.clone().into_os_string())).unwrap(),
            Some(absolute)
        );
    }
    #[test]
    fn native_message_is_bounded_utf16_without_embedded_nulls() {
        for code in [
            "MANAGER_ROOT_INVALID",
            "MANAGER_WEBVIEW_UNAVAILABLE",
            "WINDOWS_PROFILE_ACL_INVALID",
            "MANAGER_STARTUP_FAILED",
        ] {
            let message = Failure::from_code(code).message();
            assert!(!message.contains('\0'));
            assert!(message.encode_utf16().count() < 512);
            assert_eq!(
                String::from_utf16(&message.encode_utf16().collect::<Vec<_>>()).unwrap(),
                message
            );
        }
    }
}
