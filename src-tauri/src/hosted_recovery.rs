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
// Private Keychain-only record. It is not an archive or renderer contract.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all="SCREAMING_SNAKE_CASE")]
enum ConfigurationKey { ClientOrigin, TenantId, AdminSubject, AdminPassword, FreezeSigningKeyBase64, StaticPublicationId, StaticRevisionSha256, StaticIndexSha256 }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigurationEntry { key:ConfigurationKey, value:String }
#[derive(Deserialize, Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct OriginalConfiguration { env_vars:Vec<ConfigurationEntry>, origin:String }
#[derive(Deserialize, Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct OriginalConfigurationSnapshot { version:u8, source:Source, expected_configuration:OriginalConfiguration }
impl OriginalConfiguration {
    // Domain/crypto validation remains the existing load-only validator and
    // fixed Node runner's responsibility. This closes only storage field names
    // and cardinality, so tokens/database URLs cannot be added to the record.
    fn closed(&self)->bool {
        use ConfigurationKey::*;
        let required=[ClientOrigin,TenantId,AdminSubject,AdminPassword,FreezeSigningKeyBase64];
        let optional=[StaticPublicationId,StaticRevisionSha256,StaticIndexSha256];
        let mut names=std::collections::BTreeSet::new();
        self.env_vars.iter().all(|e| !e.value.is_empty() && names.insert(e.key))
            && required.iter().all(|key|names.contains(key))
            && [0,3].contains(&optional.iter().filter(|key|names.contains(*key)).count())
    }
}
fn add_configuration_bytes(account:&str,bytes:&mut[u8],add:impl FnOnce(&str,&[u8])->Result<(),()>)->Result<(),LifecycleError>{
    let result=if (1..=65536).contains(&bytes.len()){add(account,bytes)}else{Err(())};
    bytes.fill(0);
    result.map_err(|_|failure("HOSTED_KEYCHAIN_STORE_FAILED"))
}
fn preserve_original_configuration(job_id:&str,source:&Source,configuration:&serde_json::Value,add:impl FnOnce(&str,&[u8])->Result<(),()>)->Result<(),LifecycleError>{
    source.validate()?;
    let original:OriginalConfiguration=serde_json::from_value(configuration.clone()).map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    if !original.closed(){return Err(failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"));}
    let snapshot=OriginalConfigurationSnapshot{version:1,source:source.clone(),expected_configuration:original};
    let mut bytes=serde_json::to_vec(&snapshot).map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
    add_configuration_bytes(&format!("render-backup-configuration-{job_id}"),&mut bytes,add)
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
     // This UUID and source belong to the already journalled native job.
     // Neither item is overwritten or deleted if the other step fails.
     preserve_original_configuration(&execution.job_id,&source,&expected_configuration,super::hosted_keychain::add_backup_only)?;
     let mut token=super::hosted_keychain::load("render").map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
     let mut key=[0u8;32];
     let result=(||{
         super::hosted_keychain::random(&mut key).map_err(|_|failure("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
         let account=format!("render-backup-key-{}",execution.job_id);
         super::hosted_keychain::add_backup_only(&account,&key).map_err(|_|failure("HOSTED_KEYCHAIN_STORE_FAILED"))?;
         let mut plan=serde_json::to_value(execution).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))?;
         plan["keyBase64"]=super::hosted::base64(&key).into();
         let text=std::str::from_utf8(&token).map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
         serde_json::to_vec(&serde_json::json!({"request":request,"expectedConfiguration":expected_configuration,"tokens":{"render":text},"sourceCidr":cidr,"confirmExternalWritersStopped":true,"execution":plan})).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))
     })();
     key.fill(0);token.fill(0);result
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
    fn synthetic_configuration()->serde_json::Value { serde_json::json!({"origin":"https://synthetic.onrender.com","envVars":[{"key":"CLIENT_ORIGIN","value":"https://synthetic.pages.dev"},{"key":"TENANT_ID","value":"10000000-0000-4000-8000-000000000001"},{"key":"ADMIN_SUBJECT","value":"synthetic-owner"},{"key":"ADMIN_PASSWORD","value":"synthetic-original-password"},{"key":"FREEZE_SIGNING_KEY_BASE64","value":"synthetic-key-shape-only"}]}) }
    fn decode_saved(bytes:&[u8],expected:&Source)->Result<OriginalConfigurationSnapshot,LifecycleError>{
        let value:OriginalConfigurationSnapshot=serde_json::from_slice(bytes).map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
        if value.version!=1 || value.source!=*expected || !value.expected_configuration.closed(){return Err(failure("HOSTED_RECOVERY_CONFIGURATION_CHANGED"));}Ok(value)
    }
    #[test]
    fn job_configuration_is_closed_source_bound_and_original_copy_survives_later_changes() {
        let source=Source{workspace_id:"tea-original".into(),service_id:"srv-original".into(),postgres_id:"dpg-original".into()};
        let mut original=synthetic_configuration();let before=original.clone();let mut retained=Vec::new();let mut calls=0;
        preserve_original_configuration("c30e227b-e771-43dc-b315-023e0469d4d2",&source,&original,|account,bytes|{calls+=1;assert_eq!(account,"render-backup-configuration-c30e227b-e771-43dc-b315-023e0469d4d2");retained=bytes.to_vec();Ok(())}).unwrap();
        assert_eq!(calls,1);assert_eq!(original,before);original["envVars"][3]["value"]="later mutable runtime change".into();
        let saved=decode_saved(&retained,&source).unwrap();assert_eq!(serde_json::to_value(saved.expected_configuration).unwrap(),before);
        let mut other=source.clone();other.postgres_id="dpg-other".into();assert!(decode_saved(&retained,&other).is_err());
        let mut wrong:serde_json::Value=serde_json::from_slice(&retained).unwrap();wrong["version"]=2.into();assert!(decode_saved(&serde_json::to_vec(&wrong).unwrap(),&source).is_err());
        for key in ["tokens","databaseURL","extra"]{let mut wrong:serde_json::Value=serde_json::from_slice(&retained).unwrap();wrong[key]="forbidden".into();assert!(decode_saved(&serde_json::to_vec(&wrong).unwrap(),&source).is_err());}
        let duplicate=String::from_utf8(retained).unwrap().replacen("{", "{\"version\":1,",1);assert!(decode_saved(duplicate.as_bytes(),&source).is_err());
    }
    #[test]
    fn job_configuration_rejects_unknown_fields_duplicates_partial_pins_before_add() {
        let source=Source{workspace_id:"tea-original".into(),service_id:"srv-original".into(),postgres_id:"dpg-original".into()};
        let mut unknown=synthetic_configuration();unknown["tokens"]=serde_json::json!({"render":"forbidden"});
        let mut duplicate=synthetic_configuration();let first=duplicate["envVars"][0].clone();duplicate["envVars"].as_array_mut().unwrap().push(first);
        let mut partial=synthetic_configuration();partial["envVars"].as_array_mut().unwrap().push(serde_json::json!({"key":"STATIC_INDEX_SHA256","value":"a".repeat(64)}));
        let mut database=synthetic_configuration();database["envVars"].as_array_mut().unwrap().push(serde_json::json!({"key":"DATABASE_URL","value":"forbidden"}));
        for value in [unknown,duplicate,partial,database]{assert!(preserve_original_configuration("c30e227b-e771-43dc-b315-023e0469d4d2",&source,&value,|_,_|panic!("invalid closed record must not touch OS")).is_err());}
    }
    #[test]
    fn configuration_store_buffer_is_zero_on_success_duplicate_denial_and_oversize() {
        for status in [Ok(()),Err(())]{let mut bytes=b"synthetic secret store bytes".to_vec();let result=add_configuration_bytes("synthetic-test-only",&mut bytes,|_,value|{assert_eq!(value,b"synthetic secret store bytes");status});assert_eq!(result.is_ok(),status.is_ok());assert!(bytes.iter().all(|b|*b==0));}
        let mut oversized=vec![1u8;65537];assert!(add_configuration_bytes("synthetic-test-only",&mut oversized,|_,_|panic!("oversize must not touch OS")).is_err());assert!(oversized.iter().all(|b|*b==0));
    }
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
