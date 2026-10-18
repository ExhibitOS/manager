// SPDX-License-Identifier: Apache-2.0
//! Native-only coordinator: provider secrets and backup keys never enter renderer state.
use exhibitos_lifecycle::{
    LifecycleError, LifecycleService,
    hosted_backup::{Job, Source},
};
use serde::{Deserialize, Serialize};
use std::path::Path;
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:match code{
 "HOSTED_RECOVERY_CONFIGURATION_REQUIRED"=>"원래 서버 설정을 이 Mac에서 확인하지 못했어요. 새 키를 만들지 않았습니다. 기존 계정 연결과 보관한 설정을 확인한 뒤 다시 시도하세요.",
 "HOSTED_RECOVERY_CONFIGURATION_CHANGED"=>"보관된 설정과 서버의 설정이 달라요. 원본과 사본은 유지됩니다. 원래 서버·계정·주소를 다시 확인하세요.",
 "HOSTED_RECOVERY_SOURCE_CHANGED"=>"배포 대상이나 서버 상태가 바뀌었어요. 기존 사본은 보존합니다. 백업 준비에서 대상을 다시 확인한 뒤 요청하세요.",
 "HOSTED_RECOVERY_NETWORK_REQUIRED"=>"Render의 데이터베이스 접근 설정에서 현재 Mac의 좁은 IP 범위를 허용한 뒤 같은 범위를 확인하세요. 전체 인터넷 허용은 사용하지 않습니다.",
 "HOSTED_RECOVERY_WRITERS_ACTIVE"=>"Render에서 선택한 API를 일시 중지하고 다른 데이터 쓰기도 멈춘 뒤 백업하세요. 정적 전시 관람은 계속할 수 있습니다.",
 "INSUFFICIENT_STORAGE"=>"백업과 확인 사본을 만들 공간이 부족합니다. 원본과 기존 사본은 보존됐습니다. 저장 공간을 확보한 뒤 새 백업을 요청하세요.",
 "HOSTED_RECOVERY_HELPER_ACTIVE"=>"이전 백업 도구가 아직 실행 중입니다. 기존 작업의 실제 상태를 확인하고 중지·복구한 뒤 다시 요청하세요.",
 _=>"서버 백업을 완료하지 못했어요. 원격 원본·기존 사본·실패 후보는 보존합니다. 연결·데이터 쓰기 중지·접근 범위를 확인한 뒤 실패한 작업을 다시 확인하세요."
}.into()}
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Context,
    Jobs,
    Create,
    ExportKey,
    ExportArchive,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub action: Action,
    #[serde(default)]
    pub source_cidr: Option<String>,
    #[serde(default)]
    pub confirm_external_writers_stopped: bool,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub expected_source: Option<Source>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub source: Option<Source>,
    pub jobs: Vec<Job>,
    pub key_saved: bool,
    pub archive_saved: Option<exhibitos_lifecycle::backup_export::ExportReceipt>,
}
fn expected_source_matches(expected: Option<&Source>, current: &Source) -> bool { expected == Some(current) }
fn network(value: &str) -> bool {
    let Some((ip, bits)) = value.split_once('/') else {
        return false;
    };
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => bits == "32",
        Ok(std::net::IpAddr::V6(_)) => bits == "128",
        _ => false,
    }
}
pub fn operation(
    resources: &Path,
    service: &LifecycleService,
    input: Input,
) -> Result<Context, LifecycleError> {
    let fail = |e: LifecycleError| failure(&e.code);
    if !cfg!(target_os = "macos") {
        return Err(failure("HOSTED_PLATFORM_UNSUPPORTED"));
    }
    match input.action {
        Action::Jobs => Ok(Context {
            source: None,
            jobs: service.hosted_backup_jobs().map_err(fail)?,
            key_saved: false,
            archive_saved: None,
        }),
        Action::Context => Ok(Context {
            source: Some(super::hosted::recovery_source().map_err(fail)?),
            jobs: service.hosted_backup_jobs().map_err(fail)?,
            key_saved: false,
            archive_saved: None,
        }),
        Action::ExportKey | Action::ExportArchive => Err(failure("HOSTED_RECOVERY_REQUEST")),
        Action::Create => {
            // Share the existing deployment lock through source selection and provider postcheck.
            let _hosted_operation = super::hosted::hold_operation()?;
            let cidr = input
                .source_cidr
                .filter(|s| s.len() <= 64 && network(s))
                .ok_or_else(|| failure("HOSTED_RECOVERY_NETWORK_REQUIRED"))?;
            if !input.confirm_external_writers_stopped || input.job_id.is_some() {
                return Err(failure("HOSTED_RECOVERY_OPERATOR_REQUIRED"));
            }
            let source = super::hosted::recovery_source().map_err(fail)?;
            if !expected_source_matches(input.expected_source.as_ref(), &source) { return Err(failure("HOSTED_RECOVERY_SOURCE_CHANGED")); }
            let expected_configuration=super::hosted::recovery_configuration(&source).map_err(fail)?;
            let manifest: serde_json::Value = serde_json::from_str(
                option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").unwrap_or("{}"),
            )
            .map_err(|_| failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
            if !manifest["files"]
                .as_object()
                .is_some_and(|m| m.contains_key("scripts/hosted-recovery-runner.mjs"))
            {
                return Err(failure("HOSTED_RUNTIME_UNAVAILABLE"));
            }
            let pins = super::remote::verified_resources(resources).map_err(fail)?;
            let image = service
                .bundled_maintenance_image(&resources.join("platform-runtime/generated"))
                .map_err(fail)?
                .ok_or_else(|| failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
            service
                .prepare_bundled_maintenance(&resources.join("platform-runtime/generated"), &image)
                .map_err(fail)?;
            #[cfg(target_os = "macos")]
            {
                let request = serde_json::to_value(&source)
                    .map_err(|_| failure("HOSTED_RECOVERY_REQUEST"))?;
                service.create_hosted_backup(source.clone(),&image,&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-recovery-runner.mjs"),|execution|{
     let mut token=super::hosted_keychain::load("render").map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
     let mut key=[0u8;32];super::hosted_keychain::random(&mut key).map_err(|_|failure("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
     let account=format!("render-backup-key-{}",execution.job_id);
     let stored=super::hosted_keychain::store(&account,&key);
     if stored.is_err(){key.fill(0);token.fill(0);return Err(failure("HOSTED_KEYCHAIN_STORE_FAILED"));}
     let mut plan=serde_json::to_value(execution).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))?;
     plan["keyBase64"]=super::hosted::base64(&key).into();key.fill(0);
     let result=std::str::from_utf8(&token).map_err(|_|failure("HOSTED_TOKEN_REQUIRED")).and_then(|text|serde_json::to_vec(&serde_json::json!({"request":request,"expectedConfiguration":expected_configuration,"tokens":{"render":text},"sourceCidr":cidr,"confirmExternalWritersStopped":true,"execution":plan})).map_err(|_|failure("HOSTED_RECOVERY_REQUEST")));
     token.fill(0);Ok(result?)
    },||{for pin in &pins{pin.check()?;}Ok(())}).map_err(fail)?;
                return Ok(Context {
                    source: Some(source),
                    jobs: service.hosted_backup_jobs().map_err(fail)?,
                    key_saved: false,
            archive_saved: None,
                });
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (source, cidr, pins);
                Err(failure("HOSTED_PLATFORM_UNSUPPORTED"))
            }
        }
    }
}
#[cfg(target_os = "macos")]
pub fn export_key(
    controller: &exhibitos_lifecycle::installations::InstallationController,
    selection: &str,
    id: &str,
) -> Result<bool, LifecycleError> {
    controller.with_current(selection, |s| {
        if s.hosted_backup_jobs()?.iter().any(|j| j.id == id) {
            Ok(())
        } else {
            Err(failure("HOSTED_RECOVERY_REQUEST"))
        }
    })?;
    let Some(path) = super::maintenance_picker::choose(super::maintenance_picker::Kind::NewKey)?
    else {
        return Ok(false);
    };
    let mut key = super::hosted_keychain::load(&format!("render-backup-key-{id}"))
        .map_err(|_| failure("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
    let result = if let Ok(bytes) = <&[u8; 32]>::try_from(key.as_slice()) {
        controller.save_hosted_backup_key(selection, id, Path::new(&path), bytes)
    } else {
        Err(failure("HOSTED_KEYCHAIN_UNAVAILABLE"))
    };
    key.fill(0);
    result?;
    Ok(true)
}
#[cfg(target_os = "macos")]
pub fn export_archive(controller: &exhibitos_lifecycle::installations::InstallationController, selection: &str, id: &str)
    -> Result<Option<exhibitos_lifecycle::backup_export::ExportReceipt>, LifecycleError> {
    controller.with_current(selection, |service| {
        let job=service.hosted_backup_jobs()?.into_iter().find(|job|job.id==id).ok_or_else(||failure("HOSTED_RECOVERY_REQUEST"))?;
        if job.state!="completed" || job.error_code.is_some() { return Err(failure("HOSTED_RECOVERY_REQUEST")); }
        Ok(())
    })?;
    let Some(path)=super::maintenance_picker::choose(super::maintenance_picker::Kind::ExportParent)? else { return Ok(None); };
    controller.export_hosted_backup(selection,id,Path::new(&path)).map(Some)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_wire_never_accepts_stop_or_reconcile_authority() {
        for action in ["stop", "reconcile", "cancel", "resume"] {
            assert!(serde_json::from_value::<Input>(serde_json::json!({"action":action,"confirmExternalWritersStopped":true,"jobId":"00000000-0000-4000-8000-000000000000"})).is_err());
        }
        assert!(
            serde_json::from_value::<Input>(serde_json::json!({"action":"create","quiesced":true}))
                .is_err()
        );
    }
    #[test]
    fn prepared_source_must_still_match_the_native_current_selection() {
        let current=Source{workspace_id:"tea-current".into(),service_id:"srv-current".into(),postgres_id:"dpg-current".into()};
        assert!(!expected_source_matches(None,&current));
        assert!(expected_source_matches(Some(&current),&current));
        let mut changed=current.clone();changed.postgres_id="dpg-other".into();
        assert!(!expected_source_matches(Some(&changed),&current));
    }
    #[test]
    fn narrow_network_only() {
        assert!(network("192.0.2.3/32"));
        assert!(network("2001:db8::1/128"));
        for v in [
            "0.0.0.0/0",
            "192.0.2.0/24",
            "http://host/32",
            "192.0.2.1/32/extra",
        ] {
            assert!(!network(v));
        }
    }
}
