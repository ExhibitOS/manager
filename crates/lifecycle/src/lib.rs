// SPDX-License-Identifier: Apache-2.0
//! Trusted-bundle desktop lifecycle. No shell, arbitrary compose paths or destructive volume removal.
pub mod backup_creation;
pub mod cancellation;
pub mod helper_reconciliation;
pub mod installation_backup;
pub mod installations;
pub mod maintenance;
mod maintenance_stream;
mod process_window;
pub mod profile_backup;
pub mod restoration;
mod restoration_auxiliary;
pub mod retry;
pub mod signed_release;
pub mod update;
#[cfg(windows)]
pub mod windows_private;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[cfg(windows)]
pub(crate) type OperationGuard = File;
#[cfg(not(windows))]
pub(crate) struct OperationGuard(File);
#[cfg(not(windows))]
impl std::ops::Deref for OperationGuard {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}
#[cfg(not(windows))]
impl Drop for OperationGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Start,
    Stop,
    Restart,
    Retry,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Completed,
    Failed,
    Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub action: Action,
    pub state: JobState,
    pub attempt: u32,
    pub progress: u8,
    pub created_at: u64,
    pub updated_at: u64,
    pub error_code: Option<String>,
    pub guidance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbe {
    pub kind: String,
    pub installed: bool,
    pub available: bool,
    pub engine_version: Option<String>,
    pub compose_version: Option<String>,
    pub error_code: Option<String>,
    pub guidance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub name: String,
    pub state: String,
    pub health: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub ready: bool,
    pub version: Option<String>,
    pub protocol_version: Option<String>,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
    pub used_bytes: Option<u64>,
    pub free_bytes: u64,
    pub minimum_free_bytes: u64,
    pub quota_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub state: String,
    pub services: Vec<ServiceState>,
    pub readiness: Readiness,
    pub storage: StorageState,
    pub active_job: Option<Job>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub at: u64,
    pub job_id: Option<String>,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Archive {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Image {
    pub reference: String,
    pub archive: Option<Archive>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleManifest {
    pub schema_version: String,
    pub bundle_id: String,
    pub version: String,
    pub protocol_version: String,
    pub compose_sha256: String,
    #[serde(default)]
    pub preferred_engine: Option<String>,
    pub project_name: String,
    pub services: Vec<String>,
    pub images: Vec<Image>,
    pub ports: Vec<u16>,
    pub open_url: String,
    pub readiness_url: String,
    pub minimum_free_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleError {
    pub code: String,
    pub guidance: String,
}
impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for LifecycleError {}
type Result<T> = std::result::Result<T, LifecycleError>;
// Docker reports sha256-prefixed IDs; Podman reports the same digest without a prefix.
fn same_local_image_id(actual: &str, reference: &str) -> bool {
    let Some(expected) = reference.strip_prefix("sha256:") else {
        return false;
    };
    let actual = actual.strip_prefix("sha256:").unwrap_or(actual);
    hash_valid(expected) && hash_valid(actual) && actual == expected
}
fn err(code: &str) -> LifecycleError {
    let guidance = match code {
        "WINDOWS_PROFILE_PUBLICATION_UNCERTAIN" => {
            "기록 교체의 완료 여부를 확인할 수 없습니다. 앱을 닫고 현재 기록과 임시 후보를 보존한 뒤 상태를 진단하세요. 확인 없이 같은 작업을 다시 실행하지 마세요."
        }
        "WINDOWS_PROFILE_BUSY" => {
            "다른 Manager 작업이 기록을 사용하고 있습니다. 해당 작업이 끝난 뒤 다시 확인하세요."
        }
        "WINDOWS_PROFILE_PUBLICATION_REFUSED" => {
            "기록을 교체하지 못했습니다. 열린 파일·작업, 파일 권한과 저장 공간을 확인하고 기존 기록과 임시 후보를 보존하세요."
        }
        "WINDOWS_PROFILE_ACL_INVALID"
        | "WINDOWS_PROFILE_PARENT_UNSAFE"
        | "WINDOWS_PROFILE_IDENTITY_INVALID"
        | "WINDOWS_PROFILE_PUBLICATION_LOCK_INVALID" => {
            "관리 공간의 소유자·접근 권한·파일 식별자를 확인할 수 없습니다. 기존 파일과 권한을 보존하고 진단 결과를 확인하세요."
        }
        "PROFILE_BUSY" => {
            "모든 Manager 앱과 해당 공간의 작업을 종료한 뒤 다시 실행하세요. 데이터와 기존 사본은 유지됩니다."
        }
        "PROFILE_ACK_REQUIRED" => "모든 앱을 종료하고 원본 공간·외부 키·사본 보존을 확인하세요.",
        "PROFILE_AUTHENTICATION_FAILED" => {
            "설정 사본 인증에 실패했습니다. 외부 키와 원래 사본을 확인하세요. 기존 목록은 교체하지 않았습니다."
        }
        "UPDATE_SOURCE_IMAGES_RETIRE_UNCERTAIN" | "UPDATE_SOURCE_IMAGES_RETIRE_UNVERIFIED" => {
            "이미지 검사 사본 정리 완료를 확인할 수 없습니다. 원본·복원점과 남은 검사 폴더·목록을 보존하고 진단하세요."
        }
        "RECOVERY_PAIR_INVALID" => {
            "호스트와 신뢰 백업의 결합 기록을 확인할 수 없습니다. 원래 백업·키·신뢰 기록을 보존하고 같은 체크포인트의 파일을 선택하세요."
        }
        "HOST_RESTORE_TARGET_EXISTS" => {
            "원래 관리 폴더가 이미 있습니다. 기존 폴더를 보존하고 덮어쓰지 마세요."
        }
        "HOST_RESTORE_UNCERTAIN" => {
            "복구 폴더 게시 완료를 확인할 수 없습니다. 현재 폴더·복구 후보·외부 신뢰 기록을 보존하고 진단하세요."
        }
        "PROFILE_WRITE_UNCERTAIN" => {
            "설정 기록 완료를 확인할 수 없습니다. 앱을 닫아 둔 채 보존한 이전 목록·후보·사본을 검사하세요."
        }
        "PROFILE_DESTINATION_UNAVAILABLE" => {
            "설정 사본 경로에 접근하거나 새 파일을 만들 수 없습니다. 경로·권한·저장 공간을 확인하고 기존 사본을 보존하세요."
        }
        "PROFILE_DESTINATION_EXISTS" => {
            "새 사본 파일 이름을 사용하세요. 기존 사본을 덮어쓰지 않습니다."
        }

        "RETRY_ACK_REQUIRED" => {
            "원본·실패 후보 보존과 외부 writer 중지 또는 새 복원 공간 사용을 다시 확인하세요."
        }
        "RETRY_RECOVERY_REQUIRED" => {
            "이전 재시도 후보를 보존하고 해당 새 작업의 실제 정지·상태를 확인하세요. 자동 재개하지 않습니다."
        }
        "RETRY_RECOVERY_UNPROVEN"
        | "RETRY_PROOF_MISSING"
        | "RETRY_CANDIDATE_CONFLICT"
        | "RETRY_RECOVERY_UNCERTAIN" => {
            "재시도 예약·후보·복구 증거가 없거나 불확실합니다. 모든 기록과 후보를 보존하고 diagnose-retry로 확인하세요. 기록 삭제로 우회하지 않습니다."
        }
        "RETRY_DESTINATION_INVALID" | "RETRY_DIAGNOSIS_TARGET_INVALID" => {
            "실패·중단 재시도의 UUID와 기록에 일치하는 별도 목적지 공간을 지정하세요. 기존 공간을 변경하거나 지우지 않습니다."
        }
        "RETRY_TARGET_INVALID" | "RETRY_SOURCE_CHANGED" => {
            "재시도 대상 기록을 확인할 수 없습니다. 원래 기록과 모든 후보를 보존하고 비공개 상태를 검사하세요."
        }
        "CANCELLED" => {
            "취소를 확인했습니다. 후보와 데이터는 보존되며 서버를 자동 재개하지 않습니다."
        }
        "CANCEL_UNCERTAIN" => {
            "정지 확인이 불확실합니다. 후보를 보존하고 helper와 실제 서버 상태를 확인하세요."
        }
        "CANCEL_ACK_REQUIRED" | "CANCEL_INPUT_INVALID" | "CANCEL_TARGET_INVALID" => {
            "현재 진행 중인 작업과 후보 보존 동의를 확인하세요."
        }
        "CANCEL_RECOVERY_REQUIRED" => "중단된 취소 기록을 확인하고 후보를 보존하세요.",
        "RESTORE_FRESH_ROOT_REQUIRED" => {
            "복원은 비어 있는 새 비공개 설치 폴더에서 실행하세요. 기존 설치와 실패 후보를 보존하세요."
        }
        "RESTORE_RECOVERY_REQUIRED" => {
            "복원이 완료되지 않았습니다. 실패 후보와 소유 리소스를 확인하고 별도 새 설치에서 다시 복원하세요. 전시를 자동 시작하지 않습니다."
        }
        "RESTORE_RESULT_INVALID" | "RESTORE_LAYOUT_UNSUPPORTED" => {
            "이 사본의 설치 구성과 이미지 정보를 안전하게 복원할 수 없습니다. 원본과 후보를 보존해 확인하세요."
        }
        "RESTORE_FAILED" => {
            "새 설치 복원을 완료하지 못했습니다. 원본 사본과 기존 설치는 보존되며 실패 후보를 확인하세요."
        }
        "RUNTIME_MISSING" => "컨테이너 실행 도구를 설치한 후 다시 시도하세요.",
        "ENGINE_UNAVAILABLE" => "컨테이너 실행 도구를 시작한 후 다시 시도하세요.",
        "COMPOSE_UNAVAILABLE" => "Compose 실행 도구를 설치하거나 설정한 후 다시 시도하세요.",
        "ENGINE_TIMEOUT" => "실행 도구의 응답이 늦습니다. 상태를 확인한 후 다시 시도하세요.",
        "ENGINE_OUTPUT_INVALID" | "ENGINE_OUTPUT_LIMIT" => {
            "실행 도구의 응답을 확인하지 못했습니다. 실행 도구를 확인한 후 다시 시도하세요."
        }
        "ENGINE_PERMISSION" => "실행 도구 접근 권한을 확인한 후 다시 시도하세요.",
        "PORT_IN_USE" => {
            "전시에 사용할 포트를 다른 앱이 사용하고 있습니다. 해당 앱을 종료한 후 다시 시도하세요."
        }
        "STORAGE_QUOTA" => "저장 공간이 부족합니다. 공간을 확보한 후 다시 시도하세요.",
        "BUSY" => "진행 중인 작업이 끝날 때까지 기다려 주세요.",
        "BACKUP_IMAGE_INVALID" | "IMAGE_INTEGRITY" => {
            "검증된 유지보수 이미지의 고정 ID를 확인하세요."
        }
        "BACKUP_PATH_INVALID" | "BACKUP_PRIVATE_PERMISSIONS" | "BACKUP_PATH_OVERLAP" => {
            "백업과 키의 실제 경로·비공개 권한을 확인하고 서로 분리하세요."
        }
        "BACKUP_VERIFICATION_FAILED" | "BACKUP_RESULT_INVALID" => {
            "백업 인증 검사를 통과하지 못했습니다. 기존 사본과 키를 보존하고 새 검증 작업으로 확인하세요."
        }
        "BACKUP_SOURCE_INVALID" | "BACKUP_SOURCE_CHANGED" => {
            "설치 설정 파일이 올바르지 않거나 변경되었습니다. 원본을 보존하고 다른 설정 작업이 끝난 뒤 다시 확인하세요."
        }
        "BACKUP_CONFIGURATION_INVALID" => {
            "현재 설치 환경 형식을 확인하세요. 기존 비밀번호를 바꾸거나 설정을 지우지 마세요."
        }
        "BACKUP_ORPHAN_PENDING" => {
            "이전 백업의 유지보수 helper가 아직 실행 중입니다. 작업 ID와 정확한 label을 확인해 해당 helper를 정지한 뒤 재시도하세요. 이전 사본과 volume은 보존하세요."
        }
        "BACKUP_CREATION_FAILED" => {
            "백업 생성이 완료되지 않았습니다. 작업 기록과 보존된 후보를 확인하세요. 전시 writer는 정지 상태일 수 있으며 기존 데이터와 사본은 삭제하지 않았습니다."
        }
        "BACKUP_OPERATOR_ACK_REQUIRED" => {
            "다른 앱·스크립트의 DB·작품·설정 변경을 중지했는지 확인한 뒤 백업을 실행하세요."
        }
        "BACKUP_LAYOUT_UNSUPPORTED" => {
            "현재 설치의 volume·network·서비스 구성을 백업 producer가 지원하는지 확인하세요. 원본 설정을 변경하지 마세요."
        }
        "BACKUP_PLATFORM_UNVERIFIED" => "이 운영체제의 백업 검증 연결은 아직 검증되지 않았습니다.",
        "VERSION_MISMATCH" => "호환되는 전시 실행 패키지가 필요합니다.",
        "RECONCILIATION_INPUT_INVALID" => "지원되는 백업·복원 작업 ID를 선택하세요.",
        "RECONCILIATION_ACK_REQUIRED" => "후보 데이터 보존과 helper만 정지하는 것을 확인하세요.",
        "RECONCILIATION_TARGET_INVALID" => "현재 공간의 실패·중단 작업만 확인·정지할 수 있습니다.",
        "RECONCILIATION_UNCERTAIN" => {
            "helper의 정지 여부가 확인되지 않았습니다. 데이터와 기록을 보존하고 다시 확인하세요."
        }
        "INSTALLATION_SELECTION_CHANGED" => {
            "관리 공간이 바뀌었습니다. 상태를 다시 확인한 뒤 작업하세요."
        }
        "INSTALLATION_SELECTION_INVALID" => {
            "저장된 관리 공간 목록을 확인할 수 없습니다. 기존 폴더와 선택 기록을 보존하고 진단하세요."
        }
        "INSTALLATION_ROOT_UNAVAILABLE" => {
            "선택한 설치 공간을 읽을 수 없습니다. 원본을 보존하고 다른 공간을 선택하거나 새 복원 공간을 만드세요."
        }
        "INSTALLATION_SELECTION_ACK_REQUIRED" => {
            "전환이 서버를 중지하지 않으며 기존 데이터를 보존하는 데 동의하세요."
        }
        "INSTALLATION_SELECTION_DISABLED" => {
            "이 실행은 관리 공간이 고정되어 있습니다. 지정 실행 설정 또는 플랫폼 지원을 확인하세요."
        }
        "INSTALLATION_SELECTION_LIMIT" => {
            "보존 중인 관리 공간이 한도에 도달했습니다. 데이터를 지우지 말고 보관 정책을 확인하세요."
        }
        "INSTALLATION_SELECTION_UNCERTAIN" => {
            "선택 기록 저장 여부가 불명확합니다. 기존 폴더와 기록을 보존하고 관리 앱을 다시 열어 확인하세요."
        }
        "BUNDLE_INVALID" | "BUNDLE_CHANGED" => "검증된 설치 패키지를 다시 준비해 주세요.",
        "READINESS_TIMEOUT" => "서버 준비가 끝나지 않았습니다. 상태를 확인한 후 다시 시도하세요.",
        _ => "작업을 완료하지 못했습니다. 상태를 확인한 후 다시 시도하세요.",
    };
    LifecycleError {
        code: code.into(),
        guidance: guidance.into(),
    }
}
// Detection converts generic failed connections into actionable state, preserving native
// permission/timeout/output bounds. Only successfully parsed versions are compared.
fn detection_error(error: LifecycleError, compose: bool) -> LifecycleError {
    if error.code == "ENGINE_OPERATION_FAILED" {
        err(if compose {
            "COMPOSE_UNAVAILABLE"
        } else {
            "ENGINE_UNAVAILABLE"
        })
    } else {
        error
    }
}
fn probe_failure(
    kind: &str,
    installed: bool,
    error: LifecycleError,
    engine_version: Option<String>,
    compose_version: Option<String>,
) -> EngineProbe {
    EngineProbe {
        kind: kind.into(),
        installed,
        available: false,
        engine_version,
        compose_version,
        error_code: Some(error.code),
        guidance: Some(error.guidance),
    }
}
fn version_major(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 64 {
        return None;
    }
    let valid_suffix = |s: &str| {
        !s.is_empty()
            && s.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
    };
    let without_build = if let Some((core, build)) = value.split_once('+') {
        if !valid_suffix(build) {
            return None;
        }
        core
    } else {
        value
    };
    let core = if let Some((core, pre)) = without_build.split_once('-') {
        if !valid_suffix(pre) {
            return None;
        }
        core
    } else {
        without_build
    };
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3
        || !parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
    {
        return None;
    }
    parts[0].parse().ok()
}
fn probe_engine(kind: &str, mut command: impl FnMut(&[String]) -> Result<Vec<u8>>) -> EngineProbe {
    let version = match command(&[
        "version".into(),
        "--format".into(),
        if kind == "docker" {
            "{{json .}}".into()
        } else {
            "json".into()
        },
    ]) {
        Ok(bytes) => bytes,
        Err(e) => {
            return probe_failure(
                kind,
                e.code != "RUNTIME_MISSING",
                detection_error(e, false),
                None,
                None,
            );
        }
    };
    let value: Value = match serde_json::from_slice(&version) {
        Ok(v) => v,
        Err(_) => return probe_failure(kind, true, err("ENGINE_OUTPUT_INVALID"), None, None),
    };
    let version = value["Client"]["Version"]
        .as_str()
        .or_else(|| value["client"]["version"].as_str())
        .or_else(|| value["Version"].as_str())
        .unwrap_or("");
    let Some(major) = version_major(version) else {
        return probe_failure(kind, true, err("ENGINE_OUTPUT_INVALID"), None, None);
    };
    let version = version.to_string();
    let info = command(&[
        "info".into(),
        "--format".into(),
        if kind == "docker" {
            "{{json .}}".into()
        } else {
            "json".into()
        },
    ]);
    let compose = command(&["compose".into(), "version".into(), "--short".into()]);
    let cv = compose
        .as_ref()
        .ok()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(|v| v.trim().trim_start_matches('v'))
        .filter(|v| version_major(v).is_some())
        .map(String::from);
    if let Err(e) = info {
        return probe_failure(kind, true, detection_error(e, false), Some(version), cv);
    }
    if let Err(e) = compose {
        return probe_failure(kind, true, detection_error(e, true), Some(version), None);
    }
    let Some(cv) = cv else {
        return probe_failure(
            kind,
            true,
            err("ENGINE_OUTPUT_INVALID"),
            Some(version),
            None,
        );
    };
    if major < if kind == "docker" { 24 } else { 5 } || version_major(&cv).is_none_or(|v| v < 2) {
        return probe_failure(kind, true, err("VERSION_MISMATCH"), Some(version), Some(cv));
    }
    EngineProbe {
        kind: kind.into(),
        installed: true,
        available: true,
        engine_version: Some(version),
        compose_version: Some(cv),
        error_code: None,
        guidance: None,
    }
}
fn choose_engine(probes: &[EngineProbe], preferred: Option<&str>) -> Result<String> {
    if let Some(kind) = preferred {
        if !matches!(kind, "docker" | "podman") {
            return Err(err("BUNDLE_INVALID"));
        }
        let probe = probes
            .iter()
            .find(|p| p.kind == kind)
            .ok_or_else(|| err("RUNTIME_MISSING"))?;
        return if probe.available {
            Ok(probe.kind.clone())
        } else {
            Err(err(probe
                .error_code
                .as_deref()
                .unwrap_or("ENGINE_UNAVAILABLE")))
        };
    }
    probes
        .iter()
        .find(|p| p.kind == "podman" && p.available)
        .or_else(|| probes.iter().find(|p| p.available))
        .map(|p| p.kind.clone())
        .ok_or_else(|| {
            err(if probes.iter().any(|p| p.installed) {
                "ENGINE_UNAVAILABLE"
            } else {
                "RUNTIME_MISSING"
            })
        })
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn same_registry_pin(actual: &str, expected: &str) -> bool {
    fn normalize(repo: &str) -> String {
        let repo = repo.strip_prefix("docker.io/").unwrap_or(repo);
        let repo = repo.strip_prefix("library/").unwrap_or(repo);
        if let Some((base, tag)) = repo.rsplit_once(':')
            && !tag.contains('/')
        {
            return base.to_string();
        }
        repo.to_string()
    }
    match (
        actual.split_once("@sha256:"),
        expected.split_once("@sha256:"),
    ) {
        (Some((a, h)), Some((b, k))) => hash_valid(h) && h == k && normalize(a) == normalize(b),
        _ => false,
    }
}
fn hash_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn safe_relative(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 240
        && !s.contains('\\')
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !s.starts_with('/')
}
fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}
/// Immutable private bytes; never adopts or overwrites an existing record.
fn write_private_new(root: &Path, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.len() > limit {
        return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
    }
    #[cfg(windows)]
    {
        let directory = windows_private::PrivateDirectory::inspect(root)?;
        let _record = directory.write_new_record(name, bytes, limit)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut file = private_options()
            .open(root.join(name))
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))
    }
}
fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    if !safe_relative(relative) {
        return Err(err("BUNDLE_INVALID"));
    }
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part);
        let m = fs::symlink_metadata(&path).map_err(|_| err("BUNDLE_INVALID"))?;
        if m.file_type().is_symlink() {
            return Err(err("BUNDLE_INVALID"));
        }
    }
    Ok(path)
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    #[cfg(windows)]
    {
        serde_json::from_slice(&windows_private::read_json_path(path)?)
            .map_err(|_| err("STATE_INVALID"))
    }
    #[cfg(not(windows))]
    {
        let m = fs::symlink_metadata(path).map_err(|_| err("STATE_UNAVAILABLE"))?;
        if !m.is_file() || m.file_type().is_symlink() || m.len() > 4 * 1024 * 1024 {
            return Err(err("STATE_INVALID"));
        }
        serde_json::from_slice(&fs::read(path).map_err(|_| err("STATE_UNAVAILABLE"))?)
            .map_err(|_| err("STATE_INVALID"))
    }
}
fn write_json<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value).map_err(|_| err("STATE_INVALID"))?;
    if payload.len() > 4 * 1024 * 1024 {
        return Err(err("JOB_HISTORY_FULL"));
    }
    #[cfg(windows)]
    {
        windows_private::write_json_root(root, name, &payload)
    }
    #[cfg(not(windows))]
    {
        let temp = root.join(format!(".{}.tmp", Uuid::new_v4()));
        let mut f = private_options()
            .open(&temp)
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        f.write_all(&payload)
            .and_then(|_| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        fs::rename(&temp, root.join(name)).map_err(|_| err("STATE_UNAVAILABLE"))?;
        Ok(())
    }
}
fn engine_executable(kind: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute())
            .map(|p| {
                p.join(if cfg!(windows) {
                    format!("{kind}.exe")
                } else {
                    kind.to_string()
                })
            })
            .collect();
    #[cfg(target_os = "macos")]
    {
        for dir in [
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/Applications/Docker.app/Contents/Resources/bin",
        ] {
            candidates.push(Path::new(dir).join(kind));
        }
    }
    #[cfg(windows)]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let base = PathBuf::from(program_files);
            candidates.push(if kind == "docker" {
                base.join("Docker/Docker/resources/bin/docker.exe")
            } else {
                base.join("RedHat/Podman/podman.exe")
            });
        }
    }
    candidates
        .into_iter()
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))
}
// Both streams are drained, but output is capped. Raw engine errors never leave this module.
fn run(binary: &str, args: &[String], cwd: Option<&Path>, seconds: u64) -> Result<Vec<u8>> {
    run_observed(binary, args, cwd, seconds, || Ok(()))
}
fn run_observed(
    binary: &str,
    args: &[String],
    cwd: Option<&Path>,
    seconds: u64,
    observe: impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    run_observed_input(binary, args, cwd, seconds, None, observe)
}
fn run_observed_input(
    binary: &str,
    args: &[String],
    cwd: Option<&Path>,
    seconds: u64,
    input: Option<File>,
    observe: impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    run_observed_inputs(binary, args, cwd, seconds, input, None, observe)
}
fn run_observed_inputs(
    binary: &str,
    args: &[String],
    cwd: Option<&Path>,
    seconds: u64,
    input: Option<File>,
    extra: Option<File>,
    mut observe: impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    let executable = if matches!(binary, "docker" | "podman") {
        engine_executable(binary).ok_or_else(|| err("RUNTIME_MISSING"))?
    } else {
        PathBuf::from(binary)
    };
    let mut command = process_window::background_command(executable);
    if matches!(binary, "docker" | "podman") {
        let mut paths: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .filter(|p| p.is_absolute())
                .collect();
        for path in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"] {
            if !paths.iter().any(|p| p == Path::new(path)) {
                paths.push(path.into());
            }
        }
        if let Ok(value) = std::env::join_paths(paths) {
            command.env("PATH", value);
        }
    }
    command
        .args(args)
        .stdin(input.map(Stdio::from).unwrap_or_else(Stdio::null))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(unix)]
    if let Some(ref file) = extra {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let fd = file.as_raw_fd();
        // Only the child clears close-on-exec; parent descriptor protection remains.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags == -1 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    if extra.is_some() {
        return Err(err("UPDATE_OCI_PLATFORM_UNVERIFIED"));
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            err("RUNTIME_MISSING")
        } else {
            err("ENGINE_PERMISSION")
        }
    })?;
    let (tx, rx) = mpsc::channel();
    for (index, mut stream) in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
        Box::new(child.stderr.take().unwrap()) as Box<dyn Read + Send>,
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut data = Vec::new();
            let mut buf = [0; 8192];
            let mut overflow = false;
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if data.len() + n <= 4 * 1024 * 1024 {
                            data.extend_from_slice(&buf[..n]);
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            let _ = tx.send((index, data, overflow));
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if let Err(error) = observe() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err("ENGINE_TIMEOUT"));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(err("ENGINE_OPERATION_FAILED")),
        }
    };
    let a = rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| err("ENGINE_TIMEOUT"))?;
    let b = rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| err("ENGINE_TIMEOUT"))?;
    if a.2 || b.2 {
        return Err(err("ENGINE_OUTPUT_LIMIT"));
    }
    if !status.success() {
        let text = format!(
            "{} {}",
            String::from_utf8_lossy(&a.1),
            String::from_utf8_lossy(&b.1)
        )
        .to_lowercase();
        return Err(err(if text.contains("permission denied") {
            "ENGINE_PERMISSION"
        } else if text.contains("address already in use")
            || text.contains("port is already allocated")
        {
            "PORT_IN_USE"
        } else {
            "ENGINE_OPERATION_FAILED"
        }));
    }
    // Stream identity is preserved below by forwarding a tagged tuple (stdout tag true).
    Ok(if a.0 == 0 { a.1 } else { b.1 })
}
fn compose_args(m: &BundleManifest, tail: &[&str]) -> Vec<String> {
    let mut a = vec![
        "compose".into(),
        "--env-file".into(),
        "../runtime.env".into(),
        "--project-name".into(),
        m.project_name.clone(),
        "--file".into(),
        "compose.yaml".into(),
    ];
    a.extend(tail.iter().map(|x| x.to_string()));
    a
}
fn loopback_url(input: &str, ports: &[u16]) -> Result<(SocketAddr, String)> {
    let rest = input
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| err("BUNDLE_INVALID"))?;
    let (port, path) = rest
        .split_once('/')
        .map(|(p, x)| (p, format!("/{x}")))
        .unwrap_or((rest, "/".into()));
    let port = port.parse::<u16>().map_err(|_| err("BUNDLE_INVALID"))?;
    if !ports.contains(&port)
        || path.contains(['\r', '\n', '?', '#', '\\'])
        || path.contains("..")
        || path.len() > 240
    {
        return Err(err("BUNDLE_INVALID"));
    }
    Ok((SocketAddr::from(([127, 0, 0, 1], port)), path))
}
fn readiness(m: &BundleManifest) -> Readiness {
    let attempt = (|| -> Result<Readiness> {
        let (address, path) = loopback_url(&m.readiness_url, &m.ports)?;
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        write!(
            socket,
            "GET {path} HTTP/1.0\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        let mut bytes = Vec::new();
        socket
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| err("READINESS_UNAVAILABLE"))?;
        if bytes.len() > 65536 {
            return Err(err("READINESS_INVALID"));
        }
        let response = String::from_utf8(bytes).map_err(|_| err("READINESS_INVALID"))?;
        if !response.starts_with("HTTP/1.1 200 ") && !response.starts_with("HTTP/1.0 200 ") {
            return Err(err("READINESS_UNAVAILABLE"));
        }
        let (_, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| err("READINESS_INVALID"))?;
        let v: Value = serde_json::from_str(body).map_err(|_| err("READINESS_INVALID"))?;
        if v["schemaVersion"] != m.schema_version
            || v["protocolVersion"] != m.protocol_version
            || v["platformVersion"] != m.version
        {
            return Err(err("VERSION_MISMATCH"));
        }
        let service_ready = v["services"].as_array().is_some_and(|entries| {
            entries.len() == 5
                && ["platform", "api", "database", "web", "storage"]
                    .iter()
                    .all(|name| {
                        entries
                            .iter()
                            .filter(|entry| entry["name"] == *name && entry["status"] == "ready")
                            .count()
                            == 1
                    })
        });
        Ok(Readiness {
            ready: v["ready"] == true && service_ready,
            version: Some(m.version.clone()),
            protocol_version: Some(m.protocol_version.clone()),
            error_code: None,
        })
    })();
    attempt.unwrap_or_else(|e| Readiness {
        error_code: Some(e.code),
        ..Default::default()
    })
}

pub struct LifecycleService {
    root: PathBuf,
    #[cfg(windows)]
    root_guard: windows_private::PrivateDirectory,
}
impl LifecycleService {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(err("STATE_INVALID"));
        }
        #[cfg(windows)]
        let root_guard = windows_private::PrivateDirectory::ensure_tree(&root)?;
        #[cfg(not(windows))]
        fs::create_dir_all(&root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        let m = fs::symlink_metadata(&root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(err("STATE_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        #[cfg(windows)]
        let service = Self {
            root: root_guard.path().to_path_buf(),
            root_guard,
        };
        #[cfg(not(windows))]
        let service = Self { root };
        #[cfg(windows)]
        drop(service.lock()?);
        service.recover_jobs()?;
        service.recover_backup_jobs()?;
        service.recover_restoration()?;
        service.recover_helper_reconciliations()?;
        service.recover_maintenance_cancellation()?;
        service.recover_maintenance_retries()?;
        Ok(service)
    }
    /// Bind an already-existing checked destination without initiating recovery.
    fn bound_existing(root: PathBuf) -> Result<Self> {
        #[cfg(windows)]
        {
            let root_guard = windows_private::PrivateDirectory::inspect(&root)?;
            Ok(Self {
                root: root_guard.path().to_path_buf(),
                root_guard,
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self { root })
        }
    }
    fn lock(&self) -> Result<OperationGuard> {
        #[cfg(windows)]
        {
            let file = self.root_guard.lock_record("operation.lock")?;
            file.try_lock_exclusive().map_err(|_| err("BUSY"))?;
            self.root_guard.check_record(&file, "operation.lock")?;
            Ok(file)
        }
        #[cfg(not(windows))]
        {
            let path = self.root.join("operation.lock");
            if path.exists()
                && fs::symlink_metadata(&path)
                    .map_err(|_| err("STATE_INVALID"))?
                    .file_type()
                    .is_symlink()
            {
                return Err(err("STATE_INVALID"));
            }
            let file = match private_options().open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .map_err(|_| err("STATE_UNAVAILABLE"))?,
                Err(_) => return Err(err("STATE_UNAVAILABLE")),
            };
            file.try_lock_exclusive().map_err(|_| err("BUSY"))?;
            Ok(OperationGuard(file))
        }
    }
    fn manifest(&self) -> Result<BundleManifest> {
        let path = checked_path(&self.root, "bundle/manifest.json")?;
        let m: BundleManifest = read_json(&path)?;
        if m.schema_version != "1.0.0-draft.1"
            || m.protocol_version != "1"
            || !name(&m.bundle_id)
            || !name(&m.project_name)
            || !m.project_name.starts_with("exhibitos-")
            || m.preferred_engine
                .as_ref()
                .is_some_and(|v| !matches!(v.as_str(), "docker" | "podman"))
            || m.services.is_empty()
            || m.services.len() > 16
            || !m.services.iter().all(|s| name(s))
            || m.images.is_empty()
            || m.images.len() > 16
            || m.ports.is_empty()
            || m.ports.iter().any(|p| *p < 1024)
            || !hash_valid(&m.compose_sha256)
            || m.version.len() > 64
            || m.version.is_empty()
        {
            return Err(err("BUNDLE_INVALID"));
        }
        loopback_url(&m.open_url, &m.ports)?;
        loopback_url(&m.readiness_url, &m.ports)?;
        for i in &m.images {
            let (repository, d) = if let Some(d) = i.reference.strip_prefix("sha256:") {
                ("local", d)
            } else if let Some(pair) = i.reference.split_once("@sha256:") {
                pair
            } else {
                return Err(err("BUNDLE_INVALID"));
            };
            if !hash_valid(d)
                || repository.is_empty()
                || !repository
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/._-:".contains(&c))
            {
                return Err(err("BUNDLE_INVALID"));
            }
        }
        let compose = fs::read(checked_path(&self.root, "bundle/compose.yaml")?)
            .map_err(|_| err("BUNDLE_INVALID"))?;
        if compose.len() > 256 * 1024 || digest(&compose) != m.compose_sha256 {
            return Err(err("BUNDLE_CHANGED"));
        }
        Ok(m)
    }
    fn engine(&self, m: &BundleManifest, install: bool) -> Result<String> {
        self.engine_with_probe(m, install, || self.detect())
    }
    fn engine_with_probe(
        &self,
        m: &BundleManifest,
        install: bool,
        detect: impl FnOnce() -> Result<Vec<EngineProbe>>,
    ) -> Result<String> {
        if self.root.join("installed.json").exists() {
            let kind: String = read_json(&self.root.join("engine.json"))?;
            if !matches!(kind.as_str(), "docker" | "podman") {
                return Err(err("STATE_INVALID"));
            }
            if m.preferred_engine
                .as_deref()
                .is_some_and(|preferred| preferred != kind)
            {
                return Err(err("BUNDLE_CHANGED"));
            }
            return Ok(kind);
        }
        if !install {
            return Err(err("NOT_INSTALLED"));
        }
        choose_engine(&detect()?, m.preferred_engine.as_deref())
    }
    fn runtime_env(&self, m: &BundleManifest) -> Result<()> {
        let path = self.root.join("runtime.env");
        if path.exists() {
            #[cfg(windows)]
            {
                let directory = windows_private::PrivateDirectory::inspect(&self.root)
                    .map_err(|_| err("SECRET_FILE_INVALID"))?;
                directory
                    .read_record("runtime.env")
                    .and_then(|mut file| file.read_bounded(8192))
                    .map_err(|_| err("SECRET_FILE_INVALID"))?;
            }
            let info = fs::symlink_metadata(&path).map_err(|_| err("SECRET_FILE_INVALID"))?;
            if !info.is_file() || info.file_type().is_symlink() || info.len() > 8192 {
                return Err(err("SECRET_FILE_INVALID"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if info.permissions().mode() & 0o777 != 0o600 {
                    return Err(err("SECRET_FILE_INVALID"));
                }
            }
            return Ok(());
        }
        let secret = || {
            format!(
                "{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            )
        };
        let password = secret();
        let contents = format!(
            "EXHIBITOS_PORT={}\nPOSTGRES_PASSWORD={}\nDATABASE_URL=postgresql://exhibitos:{}@database:5432/exhibitos\nADMIN_SUBJECT=local-admin\nADMIN_PASSWORD={}\nTENANT_ID={}\n",
            m.ports[0],
            password,
            password,
            secret(),
            Uuid::new_v4()
        );
        write_private_new(&self.root, "runtime.env", contents.as_bytes(), 8192)
            .map_err(|_| err("SECRET_FILE_INVALID"))?;
        Ok(())
    }
    pub fn detect(&self) -> Result<Vec<EngineProbe>> {
        Ok(["podman", "docker"]
            .into_iter()
            .map(|kind| probe_engine(kind, |args| run(kind, args, None, 10)))
            .collect())
    }
    fn recover_jobs(&self) -> Result<()> {
        let Ok(_guard) = self.lock() else {
            return Ok(());
        };
        let mut jobs = self.job_history()?;
        let mut changed = false;
        for job in &mut jobs {
            if job.state == JobState::Running {
                job.state = JobState::Interrupted;
                job.updated_at = now();
                job.error_code = Some("INTERRUPTED".into());
                job.guidance = Some(err("INTERRUPTED").guidance);
                changed = true;
            }
        }
        if changed {
            write_json(&self.root, "jobs.json", &jobs)?;
        }
        Ok(())
    }
    fn job_history(&self) -> Result<Vec<Job>> {
        let p = self.root.join("jobs.json");
        if p.exists() {
            read_json(&p)
        } else {
            Ok(Vec::new())
        }
    }
    fn event_history(&self) -> Result<Vec<LogEvent>> {
        let p = self.root.join("events.json");
        if p.exists() {
            read_json(&p)
        } else {
            Ok(Vec::new())
        }
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        let jobs = self.job_history()?;
        Ok(jobs
            .into_iter()
            .rev()
            .take(200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
    pub fn logs(&self) -> Result<Vec<LogEvent>> {
        let events = self.event_history()?;
        Ok(events
            .into_iter()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
    fn event(&self, job: &Job, code: &str, message: &str) -> Result<()> {
        let mut events = self.event_history()?;
        events.push(LogEvent {
            at: now(),
            job_id: Some(job.id.clone()),
            code: code.into(),
            message: message.into(),
        });
        write_json(&self.root, "events.json", &events)
    }
    pub fn install(&self) -> Result<Job> {
        self.execute(Action::Install)
    }
    pub fn execute(&self, action: Action) -> Result<Job> {
        let _lock = self.lock()?;
        let mut jobs = self.job_history()?;
        for j in &mut jobs {
            if j.state == JobState::Running {
                j.state = JobState::Interrupted;
                j.error_code = Some("INTERRUPTED".into());
                j.guidance = Some(err("INTERRUPTED").guidance);
                j.updated_at = now();
            }
        }
        if jobs.len() >= 10000 {
            return Err(err("JOB_HISTORY_FULL"));
        }
        let mut job = if action == Action::Retry {
            let Some(previous) = jobs
                .last()
                .filter(|j| matches!(j.state, JobState::Failed | JobState::Interrupted))
            else {
                return Err(err("NOTHING_TO_RETRY"));
            };
            if previous.attempt >= 5 {
                return Err(err("RETRY_LIMIT"));
            }
            Job {
                attempt: previous.attempt + 1,
                id: previous.id.clone(),
                action: previous.action.clone(),
                state: JobState::Running,
                progress: 0,
                created_at: previous.created_at,
                updated_at: now(),
                error_code: None,
                guidance: None,
            }
        } else {
            Job {
                id: Uuid::new_v4().to_string(),
                action,
                state: JobState::Running,
                attempt: 1,
                progress: 0,
                created_at: now(),
                updated_at: now(),
                error_code: None,
                guidance: None,
            }
        };
        let index = jobs.len();
        jobs.push(job.clone());
        write_json(&self.root, "jobs.json", &jobs)?;
        self.event(&job, "STARTED", "작업을 시작했습니다.")?;
        let result = self
            .ensure_restoration_complete(&job.action)
            .and_then(|()| self.operation(&job.action));
        job.updated_at = now();
        match result {
            Ok(()) => {
                job.state = JobState::Completed;
                job.progress = 100;
                self.event(&job, "COMPLETED", "작업이 완료되었습니다.")?;
            }
            Err(e) => {
                job.state = JobState::Failed;
                job.error_code = Some(e.code.clone());
                job.guidance = Some(e.guidance.clone());
                self.event(&job, &e.code, &e.guidance)?;
            }
        }
        jobs[index] = job.clone();
        write_json(&self.root, "jobs.json", &jobs)?;
        Ok(job)
    }
    fn operation(&self, action: &Action) -> Result<()> {
        let m = self.manifest()?;
        let engine = self.engine(&m, *action == Action::Install)?;
        let probe = self
            .detect()?
            .into_iter()
            .find(|p| p.kind == engine)
            .ok_or_else(|| err("RUNTIME_MISSING"))?;
        if !probe.available {
            return Err(err(probe
                .error_code
                .as_deref()
                .unwrap_or("ENGINE_UNAVAILABLE")));
        }
        if *action != Action::Stop
            && fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < m.minimum_free_bytes
        {
            return Err(err("STORAGE_QUOTA"));
        }
        if *action != Action::Stop {
            self.ensure_backup_helpers_idle(&engine)?;
        }
        let bundle = self.root.join("bundle");
        self.runtime_env(&m)?;
        self.validate_compose(&m, &engine)?;
        if *action == Action::Install {
            for image in &m.images {
                if let Some(a) = &image.archive {
                    let p = checked_path(&bundle, &a.path)?;
                    let metadata = fs::metadata(&p).map_err(|_| err("BUNDLE_INVALID"))?;
                    if !metadata.is_file()
                        || a.bytes == 0
                        || metadata.len() != a.bytes
                        || a.bytes > 8 * 1024 * 1024 * 1024
                        || !hash_valid(&a.sha256)
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                    let mut f = File::open(&p).map_err(|_| err("BUNDLE_INVALID"))?;
                    let mut h = Sha256::new();
                    let mut b = [0; 65536];
                    loop {
                        let n = f.read(&mut b).map_err(|_| err("BUNDLE_INVALID"))?;
                        if n == 0 {
                            break;
                        }
                        h.update(&b[..n]);
                    }
                    if format!("{:x}", h.finalize()) != a.sha256 {
                        return Err(err("BUNDLE_CHANGED"));
                    }
                    run(
                        &engine,
                        &[
                            "load".into(),
                            "--input".into(),
                            p.to_string_lossy().into_owned(),
                        ],
                        None,
                        180,
                    )?;
                }
                if image.archive.is_none() {
                    if image.reference.starts_with("sha256:") {
                        return Err(err("BUNDLE_INVALID"));
                    }
                    run(
                        &engine,
                        &["pull".into(), image.reference.clone()],
                        None,
                        180,
                    )?;
                }
                let inspected: Value = serde_json::from_slice(&run(
                    &engine,
                    &["image".into(), "inspect".into(), image.reference.clone()],
                    None,
                    15,
                )?)
                .map_err(|_| err("IMAGE_INTEGRITY"))?;
                if image.reference.starts_with("sha256:")
                    && !inspected[0]["Id"]
                        .as_str()
                        .is_some_and(|id| same_local_image_id(id, &image.reference))
                {
                    return Err(err("IMAGE_INTEGRITY"));
                }
                if image.reference.contains("@sha256:")
                    && !inspected[0]["RepoDigests"].as_array().is_some_and(|v| {
                        v.iter().any(|d| {
                            d.as_str()
                                .is_some_and(|value| same_registry_pin(value, &image.reference))
                        })
                    })
                {
                    return Err(err("IMAGE_INTEGRITY"));
                }
            }
            write_json(&self.root, "engine.json", &engine)?;
            write_json(&self.root, "installed.json", &m)?;
            return Ok(());
        }
        let installed: BundleManifest = read_json(&self.root.join("installed.json"))?;
        if serde_json::to_value(&installed).ok() != serde_json::to_value(&m).ok() {
            return Err(err("BUNDLE_CHANGED"));
        }
        self.validate_ownership(&m, &engine)?;
        self.validate_volumes(&m, &engine)?;
        let tail = match action {
            Action::Start => vec!["up", "--detach", "--no-build", "--pull", "never"],
            Action::Stop => vec!["stop", "--timeout", "30"],
            Action::Restart => vec!["restart", "--timeout", "30"],
            _ => return Err(err("INVALID_ACTION")),
        };
        run(&engine, &compose_args(&m, &tail), Some(&bundle), 180)?;
        if *action != Action::Stop {
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline {
                let status = self.status()?;
                if status.readiness.error_code.as_deref() == Some("VERSION_MISMATCH") {
                    return Err(err("VERSION_MISMATCH"));
                }
                if status.state == "running" {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }
            return Err(err("READINESS_TIMEOUT"));
        }
        Ok(())
    }
    fn validate_compose(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let v: Value = serde_json::from_slice(&run(
            engine,
            &compose_args(m, &["config", "--format", "json"]),
            Some(&self.root.join("bundle")),
            30,
        )?)
        .map_err(|_| err("BUNDLE_INVALID"))?;
        let services = v["services"]
            .as_object()
            .ok_or_else(|| err("BUNDLE_INVALID"))?;
        if services.len() != m.services.len() {
            return Err(err("BUNDLE_INVALID"));
        }
        for (name, s) in services {
            if !m.services.contains(name)
                || !m.images.iter().any(|i| s["image"] == i.reference)
                || s.get("build").is_some()
                || s["privileged"] == true
                || s["network_mode"] == "host"
                || s.get("container_name").is_some()
            {
                return Err(err("BUNDLE_INVALID"));
            }
            if s["labels"]["com.exhibitos.bundle"] != m.bundle_id
                || s["labels"]["com.exhibitos.project"] != m.project_name
                || s["labels"]["com.exhibitos.schema"] != m.schema_version
            {
                return Err(err("BUNDLE_INVALID"));
            }
            if let Some(ports) = s["ports"].as_array() {
                for p in ports {
                    let published = p["published"]
                        .as_str()
                        .and_then(|p| p.parse::<u16>().ok())
                        .or_else(|| p["published"].as_u64().and_then(|n| u16::try_from(n).ok()));
                    if p["host_ip"] != "127.0.0.1"
                        || !published.is_some_and(|n| m.ports.contains(&n))
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                }
            }
            if let Some(volumes) = s["volumes"].as_array() {
                for volume in volumes {
                    let source = volume["source"]
                        .as_str()
                        .ok_or_else(|| err("BUNDLE_INVALID"))?;
                    if volume["type"] != "volume"
                        || v["volumes"][source]["external"] == true
                        || v["volumes"][source]["labels"]["com.exhibitos.bundle"] != m.bundle_id
                        || v["volumes"][source]["labels"]["com.exhibitos.project"] != m.project_name
                        || v["volumes"][source]["labels"]["com.exhibitos.schema"]
                            != m.schema_version
                        || !v["volumes"][source]["name"]
                            .as_str()
                            .is_some_and(|n| n.starts_with(&(m.project_name.clone() + "_")))
                    {
                        return Err(err("BUNDLE_INVALID"));
                    }
                }
            }
        }
        Ok(())
    }
    fn validate_ownership(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let data = run(
            engine,
            &[
                "ps".into(),
                "--all".into(),
                "--filter".into(),
                format!("label=com.docker.compose.project={}", m.project_name),
                "--format".into(),
                "{{.ID}}".into(),
            ],
            None,
            15,
        )?;
        for id in String::from_utf8(data)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            .lines()
        {
            if id.is_empty() {
                continue;
            }
            if !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            let v: Value =
                serde_json::from_slice(&run(engine, &["inspect".into(), id.into()], None, 15)?)
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            if v[0]["Config"]["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                || v[0]["Config"]["Labels"]["com.exhibitos.project"] != m.project_name
                || v[0]["Config"]["Labels"]["com.exhibitos.schema"] != m.schema_version
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
        }
        Ok(())
    }
    fn validate_volumes(&self, m: &BundleManifest, engine: &str) -> Result<()> {
        let v: Value = serde_json::from_slice(&run(
            engine,
            &compose_args(m, &["config", "--format", "json"]),
            Some(&self.root.join("bundle")),
            30,
        )?)
        .map_err(|_| err("BUNDLE_INVALID"))?;
        if let Some(volumes) = v["volumes"].as_object() {
            for (_, spec) in volumes {
                let name = spec["name"].as_str().ok_or_else(|| err("BUNDLE_INVALID"))?;
                let existing = run(
                    engine,
                    &[
                        "volume".into(),
                        "ls".into(),
                        "--format".into(),
                        "{{.Name}}".into(),
                    ],
                    None,
                    15,
                )?;
                let text = String::from_utf8(existing).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                if text.lines().any(|n| n == name) {
                    let inspected: Value = serde_json::from_slice(&run(
                        engine,
                        &["volume".into(), "inspect".into(), name.into()],
                        None,
                        15,
                    )?)
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                    if inspected[0]["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                        || inspected[0]["Labels"]["com.exhibitos.project"] != m.project_name
                        || inspected[0]["Labels"]["com.exhibitos.schema"] != m.schema_version
                    {
                        return Err(err("OWNERSHIP_CONFLICT"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Status> {
        let jobs = self.jobs()?;
        let active_job = jobs
            .iter()
            .rev()
            .find(|j| j.state == JobState::Running)
            .cloned();
        let installed_path = self.root.join("installed.json");
        if !installed_path.exists() {
            return Ok(Status {
                installed: false,
                bundle_id: None,
                version: None,
                state: "not_installed".into(),
                services: Vec::new(),
                readiness: Readiness::default(),
                storage: StorageState {
                    used_bytes: None,
                    quota_bytes: 5 * 1024 * 1024 * 1024,
                    free_bytes: fs2::available_space(&self.root).unwrap_or(0),
                    minimum_free_bytes: 0,
                },
                active_job,
            });
        }
        let m = self.manifest()?;
        let engine = self.engine(&m, false)?;
        if let Err(error) = self.validate_ownership(&m, &engine)
            && matches!(
                error.code.as_str(),
                "OWNERSHIP_CONFLICT" | "ENGINE_OUTPUT_INVALID"
            )
        {
            return Err(error);
        }
        let r = readiness(&m);
        let output = run(
            &engine,
            &compose_args(&m, &["ps", "--all", "--format", "json"]),
            Some(&self.root.join("bundle")),
            15,
        );
        let mut services = Vec::new();
        let mut state = "stopped".to_string();
        if let Ok(data) = output {
            let text = String::from_utf8(data).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            let rows: Vec<Value> = if text.trim_start().starts_with('[') {
                serde_json::from_str(&text).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            } else {
                text.lines()
                    .filter(|x| !x.is_empty())
                    .map(serde_json::from_str)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            };
            for row in rows {
                if let Some(name) = row["Service"].as_str()
                    && m.services.iter().any(|s| s == name)
                {
                    services.push(ServiceState {
                        name: name.into(),
                        state: row["State"].as_str().unwrap_or("unknown").into(),
                        health: row["Health"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                    });
                }
            }
            if r.ready
                && services.len() == m.services.len()
                && services
                    .iter()
                    .all(|s| s.state == "running" && s.health.as_deref() != Some("unhealthy"))
            {
                state = "running".into();
            } else if services.iter().any(|s| s.state == "running") {
                state = "degraded".into();
            }
        } else {
            state = "runtime_unavailable".into();
        }
        Ok(Status {
            installed: true,
            bundle_id: Some(m.bundle_id),
            version: Some(m.version),
            state,
            services,
            readiness: r,
            storage: StorageState {
                used_bytes: None,
                quota_bytes: 5 * 1024 * 1024 * 1024,
                free_bytes: fs2::available_space(&self.root).unwrap_or(0),
                minimum_free_bytes: m.minimum_free_bytes,
            },
            active_job,
        })
    }
    pub fn open_url(&self) -> Result<String> {
        let m = self.manifest()?;
        if self.status()?.state != "running" {
            return Err(err("READINESS_UNAVAILABLE"));
        }
        loopback_url(&m.open_url, &m.ports)?;
        Ok(m.open_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_image_ids_preserve_exact_digest_across_engine_representation() {
        let digest = "a".repeat(64);
        let reference = format!("sha256:{digest}");
        assert!(same_local_image_id(&digest, &reference));
        assert!(same_local_image_id(&reference, &reference));
        assert!(!same_local_image_id(&"b".repeat(64), &reference));
        assert!(!same_local_image_id(&format!("md5:{digest}"), &reference));
        assert!(!same_local_image_id(&digest.to_uppercase(), &reference));
        assert!(!same_local_image_id(&digest, &digest));
    }
    pub(super) fn root_for_detection() -> PathBuf {
        root()
    }
    pub(super) fn manifest_for_detection() -> BundleManifest {
        manifest()
    }
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("exhibitos-manager-core-{}", Uuid::new_v4()))
    }
    fn manifest() -> BundleManifest {
        BundleManifest {
            schema_version: "1.0.0-draft.1".into(),
            bundle_id: Uuid::new_v4().to_string(),
            version: "0.1.0".into(),
            protocol_version: "1".into(),
            compose_sha256: "a".repeat(64),
            preferred_engine: None,
            project_name: format!("exhibitos-{}", Uuid::new_v4()),
            services: vec!["platform".into(), "database".into()],
            images: vec![Image {
                reference: format!("sha256:{}", "a".repeat(64)),
                archive: None,
            }],
            ports: vec![13200],
            open_url: "http://127.0.0.1:13200".into(),
            readiness_url: "http://127.0.0.1:13200/api/v1/readiness".into(),
            minimum_free_bytes: 1,
        }
    }
    #[test]
    fn no_url_escape() {
        for url in [
            "https://example.org",
            "http://localhost:13200",
            "http://127.0.0.1:80",
            "http://127.0.0.1:13200/../../x",
            "http://127.0.0.1:13200/\r\nAuthorization: secret",
        ] {
            assert!(loopback_url(url, &[13200]).is_err());
        }
        assert!(loopback_url("http://127.0.0.1:13200/api/v1/readiness", &[13200]).is_ok());
    }
    #[test]
    fn private_secret_file_persists_without_rotation() {
        let s = LifecycleService::new(root()).unwrap();
        let m = manifest();
        s.runtime_env(&m).unwrap();
        let before = fs::read(s.root.join("runtime.env")).unwrap();
        s.runtime_env(&m).unwrap();
        assert_eq!(before, fs::read(s.root.join("runtime.env")).unwrap());
        let text = String::from_utf8(before).unwrap();
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("DATABASE_URL=postgresql://exhibitos:"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(s.root.join("runtime.env"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(s.logs().unwrap().is_empty());
    }
    #[test]
    fn interrupted_jobs_recover_and_retry_keeps_identity() {
        let p = root();
        let s = LifecycleService::new(p.clone()).unwrap();
        let job = Job {
            id: Uuid::new_v4().to_string(),
            action: Action::Install,
            state: JobState::Running,
            attempt: 1,
            progress: 30,
            created_at: now(),
            updated_at: now(),
            error_code: None,
            guidance: None,
        };
        write_json(&s.root, "jobs.json", &vec![job.clone()]).unwrap();
        let recovered = LifecycleService::new(p).unwrap();
        assert_eq!(recovered.jobs().unwrap()[0].state, JobState::Interrupted);
        let retry = recovered.execute(Action::Retry).unwrap();
        assert_eq!(retry.id, job.id);
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.state, JobState::Failed);
        assert!(retry.error_code.is_some());
        assert_eq!(recovered.jobs().unwrap().len(), 2);
    }
    #[test]
    fn active_cross_process_lock_is_never_interrupted() {
        let p = root();
        let s = LifecycleService::new(p.clone()).unwrap();
        let _lock = s.lock().unwrap();
        assert_eq!(s.execute(Action::Start).unwrap_err().code, "BUSY");
        #[cfg(windows)]
        assert_eq!(LifecycleService::new(p.clone()).err().unwrap().code, "BUSY");
        #[cfg(not(windows))]
        let _second = LifecycleService::new(p.clone()).unwrap();
        assert!(s.jobs().unwrap().is_empty());
        drop(_lock);
        let reopened = LifecycleService::new(p).unwrap();
        assert!(reopened.jobs().unwrap().is_empty());
    }
    #[test]
    fn relative_paths_and_symlink_manifest_are_rejected() {
        for path in ["../a", "/absolute", "x/../a", "x\\a", ""] {
            assert!(!safe_relative(path));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let s = LifecycleService::new(root()).unwrap();
            symlink("/etc", s.root.join("bundle")).unwrap();
            assert!(s.manifest().is_err());
        }
    }
    #[test]
    #[cfg(unix)]
    fn process_timeout_and_native_output() {
        let data = run("/usr/bin/printf", &["{\"safe\":true}".into()], None, 2).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&data).unwrap()["safe"],
            true
        );
        assert_eq!(
            run("/bin/sleep", &["3".into()], None, 1).unwrap_err().code,
            "ENGINE_TIMEOUT"
        );
    }
    #[test]
    fn registry_pin_normalizes_tags_without_weakening_repository_identity() {
        let h = "a".repeat(64);
        assert!(same_registry_pin(
            &format!("postgres@sha256:{h}"),
            &format!("docker.io/library/postgres:18.6@sha256:{h}")
        ));
        assert!(!same_registry_pin(
            &format!("evil.example/postgres@sha256:{h}"),
            &format!("postgres:18.6@sha256:{h}")
        ));
        assert!(!same_registry_pin(
            &format!("postgres@sha256:{}", "b".repeat(64)),
            &format!("postgres@sha256:{h}")
        ));
    }
    #[test]
    fn public_history_views_are_bounded_without_deleting_private_history() {
        let s = LifecycleService::new(root()).unwrap();
        let jobs: Vec<Job> = (0..240)
            .map(|i| Job {
                id: Uuid::new_v4().to_string(),
                action: Action::Install,
                state: JobState::Completed,
                attempt: 1,
                progress: 100,
                created_at: i,
                updated_at: i,
                error_code: None,
                guidance: None,
            })
            .collect();
        write_json(&s.root, "jobs.json", &jobs).unwrap();
        let events: Vec<LogEvent> = (0..550)
            .map(|i| LogEvent {
                at: i,
                job_id: None,
                code: "COMPLETED".into(),
                message: "완료".into(),
            })
            .collect();
        write_json(&s.root, "events.json", &events).unwrap();
        assert_eq!(s.jobs().unwrap().len(), 200);
        assert_eq!(s.jobs().unwrap()[0].created_at, 40);
        assert_eq!(s.logs().unwrap().len(), 500);
        assert_eq!(s.job_history().unwrap().len(), 240);
        assert_eq!(s.event_history().unwrap().len(), 550);
    }
    #[test]
    fn errors_never_reveal_process_or_secret_fields() {
        let e = err("ENGINE_PERMISSION");
        let text = serde_json::to_string(&e).unwrap();
        assert!(!text.contains("postgresql://"));
        assert!(!text.contains("password"));
        assert!(text.contains("ENGINE_PERMISSION"));
    }
}

#[cfg(test)]
mod detection_tests {
    use super::*;
    fn probe(kind: &str, failure: Option<(&str, &str)>) -> EngineProbe {
        probe_engine(kind, |args| {
            if let Some((stage, code)) = failure
                && args[0] == stage
            {
                return Err(err(code));
            }
            Ok(match args[0].as_str(){"version"=>serde_json::to_vec(&serde_json::json!({"Client":{"Version":if kind=="docker"{"29.8.0"}else{"5.8.2"}}})).unwrap(),"compose"=>b"5.5.1\n".to_vec(),_=>b"{}".to_vec()})
        })
    }
    #[test]
    fn qualified_versions_remain_available() {
        for kind in ["podman", "docker"] {
            let p = probe(kind, None);
            assert!(p.available);
            assert!(p.error_code.is_none());
            assert_eq!(p.compose_version.as_deref(), Some("5.5.1"));
        }
    }
    #[test]
    fn stopped_version_or_info_is_unavailable_not_upgrade() {
        for stage in ["version", "info"] {
            let p = probe("podman", Some((stage, "ENGINE_OPERATION_FAILED")));
            assert!(p.installed);
            assert!(!p.available);
            assert_eq!(p.error_code.as_deref(), Some("ENGINE_UNAVAILABLE"));
            assert!(p.guidance.unwrap().contains("시작"));
            if stage == "info" {
                assert_eq!(p.engine_version.as_deref(), Some("5.8.2"));
                assert_eq!(p.compose_version.as_deref(), Some("5.5.1"));
            }
        }
    }
    #[test]
    fn compose_provider_failure_has_separate_recovery() {
        let p = probe("podman", Some(("compose", "ENGINE_OPERATION_FAILED")));
        assert_eq!(p.error_code.as_deref(), Some("COMPOSE_UNAVAILABLE"));
        assert!(p.guidance.unwrap().contains("Compose"));
        assert!(p.compose_version.is_none());
    }
    #[test]
    fn permission_timeout_and_output_bounds_are_preserved() {
        for stage in ["version", "info", "compose"] {
            for code in ["ENGINE_PERMISSION", "ENGINE_TIMEOUT", "ENGINE_OUTPUT_LIMIT"] {
                let p = probe("docker", Some((stage, code)));
                assert_eq!(p.error_code.as_deref(), Some(code));
                assert!(p.installed);
                assert!(!p.available);
            }
        }
    }
    #[test]
    fn absent_command_differs_from_existing_denied_command() {
        let p = probe("podman", Some(("version", "RUNTIME_MISSING")));
        assert!(!p.installed);
        assert_eq!(p.error_code.as_deref(), Some("RUNTIME_MISSING"));
    }
    #[test]
    fn invalid_versions_are_bounded_not_reclassified_as_unsupported() {
        for text in [
            "",
            "29",
            "29.8",
            "29.8.0+",
            "5.8.2-+abc",
            "secret://credential",
            "29.8.0\nSECRET=credential",
        ] {
            assert!(version_major(text).is_none());
        }
        assert_eq!(version_major("5.8.2-dev.1+build.2"), Some(5));
        let p =
            probe_engine("docker", |_| {
                Ok(serde_json::to_vec(
                    &serde_json::json!({"Client":{"Version":"secret://credential"}}),
                )
                .unwrap())
            });
        assert_eq!(p.error_code.as_deref(), Some("ENGINE_OUTPUT_INVALID"));
        assert!(p.engine_version.is_none());
        assert!(!serde_json::to_string(&p).unwrap().contains("credential"));
    }
    #[test]
    fn successfully_parsed_old_versions_require_upgrade() {
        let p = probe_engine("docker", |args| {
            Ok(if args[0] == "version" {
                b"{\"Client\":{\"Version\":\"23.0.0\"}}".to_vec()
            } else if args[0] == "compose" {
                b"2.0.0".to_vec()
            } else {
                b"{}".to_vec()
            })
        });
        assert_eq!(p.error_code.as_deref(), Some("VERSION_MISMATCH"));
    }
    #[test]
    fn explicit_producer_never_falls_back_and_failed_selection_writes_no_pin() {
        let s = LifecycleService::new(super::tests::root_for_detection()).unwrap();
        let mut m = super::tests::manifest_for_detection();
        m.preferred_engine = Some("podman".into());
        let docker = probe("docker", None);
        let unavailable = probe("podman", Some(("info", "ENGINE_OPERATION_FAILED")));
        assert_eq!(
            s.engine_with_probe(&m, true, || Ok(vec![unavailable.clone(), docker.clone()]))
                .unwrap_err()
                .code,
            "ENGINE_UNAVAILABLE"
        );
        assert!(!s.root.join("engine.json").exists());
        write_json(&s.root, "engine.json", &"docker").unwrap();
        assert_eq!(
            s.engine_with_probe(&m, true, || Ok(vec![probe("podman", None), docker.clone()]))
                .unwrap(),
            "podman"
        );
        assert_eq!(
            read_json::<String>(&s.root.join("engine.json")).unwrap(),
            "docker"
        );
        m.preferred_engine = None;
        assert_eq!(
            s.engine_with_probe(&m, true, || Ok(vec![unavailable, docker]))
                .unwrap(),
            "docker"
        );
    }
    #[test]
    fn installed_engine_is_preserved_without_detection_or_silent_switch() {
        let s = LifecycleService::new(super::tests::root_for_detection()).unwrap();
        let mut m = super::tests::manifest_for_detection();
        write_json(&s.root, "installed.json", &m).unwrap();
        write_json(&s.root, "engine.json", &"docker").unwrap();
        assert_eq!(
            s.engine_with_probe(&m, true, || panic!("installed pin must not be redetected"))
                .unwrap(),
            "docker"
        );
        m.preferred_engine = Some("podman".into());
        assert_eq!(
            s.engine_with_probe(&m, true, || panic!("must not switch existing engine"))
                .unwrap_err()
                .code,
            "BUNDLE_CHANGED"
        );
        assert_eq!(
            read_json::<String>(&s.root.join("engine.json")).unwrap(),
            "docker"
        );
    }
}

#[cfg(test)]
mod engine_failure_tests;
