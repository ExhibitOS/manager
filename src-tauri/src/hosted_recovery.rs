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
 "HOSTED_RESTORE_CHECK_FAILED"=>"사본을 새 빈 데이터베이스에서 확인하지 못했어요. 원본과 이전 결과는 보존합니다. 복원 확인 기록과 실행 도구 상태를 확인한 뒤 다시 시도하세요.",
 "HOSTED_RECOVERY_SOURCE_CHANGED"=>"배포 대상이나 서버 상태가 바뀌었어요. 기존 사본은 보존합니다. 백업 준비에서 대상을 다시 확인한 뒤 요청하세요.",
 "HOSTED_RECOVERY_NETWORK_REQUIRED"=>"Render의 데이터베이스 접근 설정에서 현재 Mac의 좁은 IP 범위를 허용한 뒤 같은 범위를 확인하세요. 전체 인터넷 허용은 사용하지 않습니다.",
 "HOSTED_RECOVERY_WRITERS_ACTIVE"=>"Render에서 선택한 API를 일시 중지하고 다른 데이터 쓰기도 멈춘 뒤 백업하세요. 정적 전시 관람은 계속할 수 있습니다.",
 "INSUFFICIENT_STORAGE"=>"백업과 확인 사본을 만들 공간이 부족합니다. 원본과 기존 사본은 보존됐습니다. 저장 공간을 확보한 뒤 새 백업을 요청하세요.",
 "HOSTED_RECOVERY_HELPER_ACTIVE"=>"이전 백업 도구가 아직 실행 중입니다. 기존 작업의 실제 상태를 확인하고 중지·복구한 뒤 다시 요청하세요.",
 _=>"서버 백업을 완료하지 못했어요. 원격 원본·기존 사본·실패 후보는 보존합니다. 연결·데이터 쓰기 중지·접근 범위를 확인한 뒤 실패한 작업을 다시 확인하세요."
}.into()}
}
#[derive(Clone,Copy,Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Context,
    Jobs,
    Create,
    ExportKey,
    ExportArchive,
    CheckRestore,
    RemoteTargets,
    RemoteRestore,
    RemoteConnect,
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
    #[serde(default)]
    pub job_budget_bytes: Option<u64>,
    #[serde(default)]
    pub target_postgres_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub create_new_target: bool,
    #[serde(default)]
    pub confirm_service_connection: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub source: Option<Source>,
    pub publication_copies: Vec<PublicationCopy>,
    pub jobs: Vec<Job>,
    pub restore_checks: Vec<exhibitos_lifecycle::hosted_restoration::CheckJob>,
    pub key_saved: bool,
    pub archive_saved: Option<exhibitos_lifecycle::backup_export::ExportReceipt>,
    pub publication_saved: Option<PublicationSaved>,
    pub remote_restores: Vec<exhibitos_lifecycle::hosted_remote_restoration::Operation>,
    pub remote_targets: Vec<serde_json::Value>,
}
#[derive(Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PublicationCopy { pub job_id:String,pub state:String,pub files:u64,pub bytes:u64 }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct PublicationSaved {pub job_id:String,pub state:String,pub path:Option<String>,pub files:u64,pub bytes:u64}
pub struct ArchiveDelivery {pub archive:Option<exhibitos_lifecycle::backup_export::ExportReceipt>,pub publication:Option<PublicationSaved>}
#[derive(Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct PublicationReceipt { operation:String,state:String,files:u64,bytes:u64,manifest_sha256:String,publication_id:String,revision_sha256:String,index_sha256:String }
fn private_bytes(path:&Path)->Result<Vec<u8>,LifecycleError>{
    let (bytes,pin)=super::remote::bounded(path,65536)?;
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;if std::fs::metadata(path).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?.permissions().mode()&0o777!=0o600{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}}
    pin.check()?;Ok(bytes)
}
fn preserve_publication_bytes(folder:&Path,name:&str,bytes:&[u8])->Result<(),LifecycleError>{
    use std::io::Write;
    if bytes.is_empty()||bytes.len()>65536{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
    let path=folder.join(name);
    let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    match options.open(&path){
        Ok(mut file)=>{file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;}
        Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{if private_bytes(&path)?!=bytes{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}}
        Err(_)=>return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE")),
    }
    if private_bytes(&path)?!=bytes{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}Ok(())
}
fn publication_receipt(bytes:&[u8],publication:&super::hosted::RecoveryPublication,max_bytes:u64)->Result<PublicationReceipt,LifecycleError>{
    let value:PublicationReceipt=serde_json::from_slice(bytes).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    if value.operation!="publication-custody"||!["copied","reused"].contains(&value.state.as_str())||!(3..=20000).contains(&value.files)||value.bytes==0||value.bytes>max_bytes||value.manifest_sha256!=publication.manifest_sha256||value.publication_id!=publication.publication_id||value.revision_sha256!=publication.revision_sha256||value.index_sha256!=publication.index_sha256{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}Ok(value)
}
#[cfg(target_os="macos")]
fn run_publication_copy(resources:&Path,folder:&Path,publication:&super::hosted::RecoveryPublication,max_bytes:u64)->Result<PublicationReceipt,LifecycleError>{
    use std::{io::Write,process::{Command,Stdio},sync::mpsc,time::{Duration,Instant}};
    let pins=super::remote::verified_resources(resources)?;
    let input=serde_json::to_vec(&serde_json::json!({"sourcePath":publication.source_path,"expectedManifestSha256":publication.manifest_sha256,"publicationId":publication.publication_id,"revisionSha256":publication.revision_sha256,"indexSha256":publication.index_sha256,"destinationPath":folder.join("publication"),"maxBytes":max_bytes})).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    let mut command=Command::new(resources.join("remote-runtime/generated/node"));
    command.args(["--disable-proto=throw","--no-warnings"]).arg(resources.join("scripts/hosted-recovery-runner.mjs")).arg("--publication-custody").env_clear().env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    {use std::os::unix::process::CommandExt;command.process_group(0);}
    for pin in &pins{pin.check()?;}
    let mut child=command.spawn().map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    if child.stdin.take().is_none_or(|mut stdin|stdin.write_all(&input).is_err()){super::remote::stop(&mut child);return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
    let out=child.stdout.take().ok_or_else(||failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    let err=child.stderr.take().ok_or_else(||failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    let (tx,rx)=mpsc::channel();let(etx,erx)=mpsc::channel();
    std::thread::spawn(move||{let _=tx.send(super::remote::reader(out,65536));});std::thread::spawn(move||{let _=etx.send(super::remote::reader(err,4096));});
    let start=Instant::now();let(mut stdout,mut stderr,mut status)=(None,None,None);
    loop{
        if stdout.is_none(){stdout=rx.try_recv().ok();}
        if stderr.is_none(){stderr=erx.try_recv().ok();}
        if status.is_none(){match child.try_wait(){Ok(value)=>status=value,Err(_)=>{super::remote::stop(&mut child);return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}}}
        if stdout.as_ref().is_some_and(Result::is_err)||stderr.as_ref().is_some_and(Result::is_err)||start.elapsed()>Duration::from_secs(600){super::remote::stop(&mut child);return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
        if stdout.is_some()&&stderr.is_some()&&status.is_some(){break;}std::thread::sleep(Duration::from_millis(25));
    }
    for pin in &pins{pin.check()?;}
    let output=stdout.unwrap().map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    if !status.unwrap().success(){return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
    publication_receipt(&output,publication,max_bytes)
}
#[cfg(target_os="macos")]
fn copy_publication(resources:&Path,folder:&Path,publication:&super::hosted::RecoveryPublication,max_bytes:u64)->Result<(),LifecycleError>{
    let receipt=run_publication_copy(resources,folder,publication,max_bytes)?;
    preserve_publication_bytes(folder,"publication-custody.json",&serde_json::to_vec(&receipt).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?)
}
#[cfg(target_os="macos")]
fn stored_publication(service:&LifecycleService,job:&Job)->Result<(super::hosted::RecoveryPublication,Vec<u8>,PublicationReceipt),LifecycleError>{
    if job.state!="completed"{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
    let folder=service.hosted_publication_workspace(&job.id)?;
    let work=private_bytes(&folder.join("publication-work.json"))?;
    if !super::hosted::recovery_snapshot_matches(&work,&job.source){return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
    let snapshot:serde_json::Value=serde_json::from_slice(&work).map_err(|_|failure("HOSTED_PUBLICATION_UNAVAILABLE"))?;
    let text=|key:&str|snapshot["source"][key].as_str().map(str::to_owned).ok_or_else(||failure("HOSTED_PUBLICATION_UNAVAILABLE"));
    let publication=super::hosted::RecoveryPublication{source_path:folder.join("publication").to_str().ok_or_else(||failure("HOSTED_PUBLICATION_UNAVAILABLE"))?.into(),manifest_sha256:snapshot["sourceSha256"].as_str().unwrap_or("").into(),publication_id:text("publicationId")?,revision_sha256:text("revisionSha256")?,index_sha256:text("indexSha256")?,snapshot:work};
    let receipt_bytes=private_bytes(&folder.join("publication-custody.json"))?;
    let receipt=publication_receipt(&receipt_bytes,&publication,job.job_budget_bytes.unwrap_or(0))?;
    Ok((publication,receipt_bytes,receipt))
}
pub(crate) fn publication_copies(service:&LifecycleService)->Vec<PublicationCopy>{
    #[cfg(target_os="macos")]
    {service.hosted_backup_jobs().unwrap_or_default().into_iter().map(|job|{
        let mut result=PublicationCopy{job_id:job.id.clone(),state:"unavailable".into(),files:0,bytes:0};
        let loaded=stored_publication(service,&job).map(|(_,_,receipt)|receipt);
        if let Ok(receipt)=loaded{result.state="copied".into();result.files=receipt.files;result.bytes=receipt.bytes;}result
    }).collect()}
    #[cfg(not(target_os="macos"))] {let _=service;Vec::new()}
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
// Load only the immutable backup-time record. Never consult the current
// publication/runtime creator or generate replacement signing/archive keys.
fn load_original_configuration(job_id:&str,expected:&Source,load:impl FnOnce(&str)->Result<Vec<u8>,super::hosted_keychain::BackupConfigurationReadError>)->Result<serde_json::Value,LifecycleError>{
    if !super::hosted_keychain::canonical_backup_uuid(job_id){return Err(failure("HOSTED_RECOVERY_REQUEST"));}
    expected.validate()?;
    let mut bytes=load(&format!("render-backup-configuration-{job_id}")).map_err(|error|failure(match error{super::hosted_keychain::BackupConfigurationReadError::Missing=>"HOSTED_RECOVERY_CONFIGURATION_REQUIRED",super::hosted_keychain::BackupConfigurationReadError::Unavailable=>"HOSTED_KEYCHAIN_UNAVAILABLE"}))?;
    let parsed=if (1..=65536).contains(&bytes.len()){serde_json::from_slice::<OriginalConfigurationSnapshot>(&bytes).ok()}else{None};bytes.fill(0);
    let snapshot=parsed.ok_or_else(||failure("HOSTED_RECOVERY_CONFIGURATION_CHANGED"))?;
    if snapshot.version!=1||snapshot.source!=*expected||!snapshot.expected_configuration.closed(){return Err(failure("HOSTED_RECOVERY_CONFIGURATION_CHANGED"));}
    serde_json::to_value(snapshot.expected_configuration).map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))
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
    if input.job_budget_bytes.is_some_and(|value| !matches!(input.action, Action::Create)
        || !(4 * 1024 * 1024..=9_007_199_254_740_991).contains(&value)) {
        return Err(failure("HOSTED_RECOVERY_REQUEST"));
    }
    match input.action {
        Action::Jobs => Ok(Context {
            source: None,
            jobs: service.hosted_backup_jobs().map_err(fail)?,
            restore_checks: service.hosted_restore_checks().map_err(fail)?,
            key_saved: false,
            archive_saved: None,
            publication_saved: None,
            publication_copies: publication_copies(service),
            remote_restores:service.hosted_remote_restores().map_err(fail)?,remote_targets:Vec::new(),
        }),
        Action::Context => Ok(Context {
            source: Some(super::hosted::recovery_source().map_err(fail)?),
            jobs: service.hosted_backup_jobs().map_err(fail)?,
            restore_checks: service.hosted_restore_checks().map_err(fail)?,
            key_saved: false,
            archive_saved: None,
            publication_saved: None,
            publication_copies: publication_copies(service),
            remote_restores:service.hosted_remote_restores().map_err(fail)?,remote_targets:Vec::new(),
        }),
        Action::ExportKey | Action::ExportArchive => Err(failure("HOSTED_RECOVERY_REQUEST")),
        Action::RemoteConnect => {
            #[cfg(not(target_os="macos"))] { return Err(failure("HOSTED_PLATFORM_UNSUPPORTED")); }
            #[cfg(target_os="macos")] {
                let _hosted_operation=super::hosted::hold_operation()?;
                if !input.confirm_service_connection||!input.confirm_external_writers_stopped{return Err(failure("HOSTED_RECOVERY_OPERATOR_REQUIRED"));}
                let id=input.operation_id.ok_or_else(||failure("HOSTED_RECOVERY_REQUEST"))?;
                let op=service.hosted_remote_restores()?.into_iter().find(|v|v.id==id).ok_or_else(||failure("HOSTED_RECOVERY_REQUEST"))?;
                if input.job_id.as_deref()!=Some(op.source_job_id.as_str()){return Err(failure("HOSTED_RECOVERY_SOURCE_CHANGED"));}
                let configuration=load_original_configuration(&op.source_job_id,&op.source,super::hosted_keychain::load_backup_configuration)?;
                let pins=remote_restore_pins(resources)?;
                let image=service.bundled_maintenance_image(&resources.join("platform-runtime/generated"))?.ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
                service.prepare_bundled_maintenance(&resources.join("platform-runtime/generated"),&image)?;
                let entry=resources.join("scripts/remote-target-entry.mjs");let(entry_bytes,entry_pin)=super::remote::bounded(&entry,65536)?;
                use sha2::{Digest,Sha256};let entry_sha=format!("{:x}",Sha256::digest(&entry_bytes));
                let mut token=super::hosted_keychain::load("render").map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
                let result=(||{
                    let text=std::str::from_utf8(&token).map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
                    service.connect_hosted_remote(&id,&image,&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-remote-restore-runner.mjs"),|operation,execution|{
                        let mut execution=execution.clone();execution["entryPath"]=entry.to_string_lossy().into_owned().into();execution["entrySha256"]=entry_sha.clone().into();
                        let account=format!("render-remote-configuration-{}",operation.id);
                        let mut bytes=match super::hosted_keychain::load_backup_configuration(&account){
                            Ok(value)=>value,
                            Err(super::hosted_keychain::BackupConfigurationReadError::Missing)=>{
                                let cidr=input.source_cidr.as_deref().filter(|v|network(v)).ok_or_else(||failure("HOSTED_RECOVERY_NETWORK_REQUIRED"))?;
                                let plan=run_remote_private(resources,&serde_json::json!({"action":"connection-plan","token":text,"operation":operation,"execution":execution,"expectedConfiguration":configuration,"sourceCidr":cidr}))?;
                                let mut encoded=serde_json::to_vec(&plan).map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_REQUIRED"))?;
                                let added=super::hosted_keychain::add_backup_only(&account,&encoded);if added.is_err(){encoded.fill(0);return Err(failure("HOSTED_KEYCHAIN_STORE_FAILED"));}encoded
                            },
                            Err(_)=>return Err(failure("HOSTED_KEYCHAIN_UNAVAILABLE")),
                        };
                        let parsed=serde_json::from_slice::<serde_json::Value>(&bytes);bytes.fill(0);let plan=parsed.map_err(|_|failure("HOSTED_RECOVERY_CONFIGURATION_CHANGED"))?;
                        if plan["operationId"]!=operation.id||plan["sourceJobId"]!=operation.source_job_id||plan["target"]["postgresId"].as_str()!=operation.target.postgres_id.as_deref(){return Err(failure("HOSTED_RECOVERY_CONFIGURATION_CHANGED"));}
                        serde_json::to_vec(&serde_json::json!({"action":"connect","token":text,"operation":operation,"execution":execution,"plan":plan,"confirmServiceConnection":true})).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))
                    },||{entry_pin.check()?;for pin in &pins{pin.check()?;}Ok(())})
                })();token.fill(0);result?;
                Ok(Context{source:None,jobs:service.hosted_backup_jobs()?,restore_checks:service.hosted_restore_checks()?,key_saved:false,archive_saved:None,publication_saved:None,publication_copies:publication_copies(service),remote_restores:service.hosted_remote_restores()?,remote_targets:Vec::new()})
            }
        },
        Action::RemoteTargets | Action::RemoteRestore => {
            #[cfg(not(target_os="macos"))] { return Err(failure("HOSTED_PLATFORM_UNSUPPORTED")); }
            #[cfg(target_os="macos")] {
            let _hosted_operation=super::hosted::hold_operation()?;
            let id=input.job_id.ok_or_else(||failure("HOSTED_RECOVERY_REQUEST"))?;
            let original=service.completed_hosted_backup(&id).map_err(fail)?;
            let pins=remote_restore_pins(resources)?;
            let mut token=super::hosted_keychain::load("render").map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
            let mut targets=Vec::new();
            let result=(||{
                let text=std::str::from_utf8(&token).map_err(|_|failure("HOSTED_TOKEN_REQUIRED"))?;
                if matches!(input.action,Action::RemoteTargets){
                    let value=run_remote_private(resources,&serde_json::json!({"action":"targets","source":original.source,"token":text}))?;
                    targets=value["targets"].as_array().ok_or_else(||failure("HOSTED_RECOVERY_RESULT_INVALID"))?.clone();
                    if targets.len()>10000{return Err(failure("HOSTED_RECOVERY_RESULT_INVALID"));}
                    for target in &mut targets{target.as_object_mut().ok_or_else(||failure("HOSTED_RECOVERY_RESULT_INVALID"))?.insert("sourceJobId".into(),id.clone().into());}
                }else{
                    if !input.confirm_external_writers_stopped{return Err(failure("HOSTED_RECOVERY_OPERATOR_REQUIRED"));}
                    let cidr=input.source_cidr.as_deref().filter(|v|network(v)).ok_or_else(||failure("HOSTED_RECOVERY_NETWORK_REQUIRED"))?;
                    let configuration=load_original_configuration(&id,&original.source,super::hosted_keychain::load_backup_configuration)?;
                    let mut key=super::hosted_keychain::load(&format!("render-backup-key-{id}")).map_err(|_|failure("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
                    let result=(||{
                        let image=service.bundled_maintenance_image(&resources.join("platform-runtime/generated"))?.ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
                        service.prepare_bundled_maintenance(&resources.join("platform-runtime/generated"),&image)?;
                        let entry=resources.join("scripts/remote-target-entry.mjs");let(entry_bytes,entry_pin)=super::remote::bounded(&entry,65536)?;
                        use sha2::{Digest,Sha256};let entry_sha=format!("{:x}",Sha256::digest(&entry_bytes));
                        let target=exhibitos_lifecycle::hosted_remote_restoration::Target{workspace_id:original.source.workspace_id.clone(),postgres_id:input.target_postgres_id.clone(),create_requested:input.create_new_target};
                        service.restore_hosted_remote(exhibitos_lifecycle::hosted_remote_restoration::RestoreRequest{source_job_id:&id,target,image:&image,operation_id:input.operation_id.as_deref()},&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-remote-restore-runner.mjs"),|operation,execution|{
                            let mut execution=execution.clone();execution["keyBase64"]=super::hosted::base64(&key).into();execution["entryPath"]=entry.to_string_lossy().into_owned().into();execution["entrySha256"]=entry_sha.clone().into();
                            serde_json::to_vec(&serde_json::json!({"action":"restore","token":text,"operation":operation,"execution":execution,"sourceCidr":cidr,"confirmExternalWritersStopped":true,"expectedConfiguration":configuration})).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))
                        },||{entry_pin.check()?;for pin in &pins{pin.check()?;}Ok(())})?;Ok::<(),LifecycleError>(())
                    })();key.fill(0);result?;
                }
                Ok::<(),LifecycleError>(())
            })();token.fill(0);result?;
            Ok(Context{source:None,jobs:service.hosted_backup_jobs()?,restore_checks:service.hosted_restore_checks()?,key_saved:false,archive_saved:None,publication_saved:None,publication_copies:publication_copies(service),remote_restores:service.hosted_remote_restores()?,remote_targets:targets})
            }
        },
        Action::CheckRestore => {
            let _hosted_operation=super::hosted::hold_operation()?;
            let id=input.job_id.ok_or_else(||failure("HOSTED_RECOVERY_REQUEST"))?;
            let source_job=service.completed_hosted_backup(&id).map_err(fail)?;
            let manifest:serde_json::Value=serde_json::from_str(option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").unwrap_or("{}")).map_err(|_|failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
            if !manifest["files"].as_object().is_some_and(|m|m.contains_key("scripts/hosted-restore-runner.mjs")){return Err(failure("HOSTED_RUNTIME_UNAVAILABLE"));}
            let pins=super::remote::verified_resources(resources).map_err(fail)?;
            let image=service.bundled_maintenance_image(&resources.join("platform-runtime/generated")).map_err(fail)?.ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
            service.prepare_bundled_maintenance(&resources.join("platform-runtime/generated"),&image).map_err(fail)?;
            #[cfg(target_os="macos")]
            {
                // Missing local configuration can use only authenticated archive authority.
                // Corrupt/changed snapshots still refuse, with no current-config fallback.
                let original=match load_original_configuration(&id,&source_job.source,super::hosted_keychain::load_backup_configuration){
                    Ok(value)=>Some(value),
                    Err(error) if error.code=="HOSTED_RECOVERY_CONFIGURATION_REQUIRED"=>None,
                    Err(error)=>return Err(error),
                };
                service.create_hosted_restore_check(&id,&image,&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-restore-runner.mjs"),|execution|{
                    let mut key=super::hosted_keychain::load(&format!("render-backup-key-{id}")).map_err(|_|failure("HOSTED_KEYCHAIN_UNAVAILABLE"))?;
                    let result=(||{
                        if key.len()!=32{return Err(failure("HOSTED_KEYCHAIN_UNAVAILABLE"));}
                        let mut plan=serde_json::to_value(execution).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))?;
                        plan["keyBase64"]=super::hosted::base64(&key).into();
                        let mut input=serde_json::json!({"execution":plan});
                        if let Some(configuration)=original.as_ref(){input["expectedConfiguration"]=configuration.clone();}
                        serde_json::to_vec(&input).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))
                    })();key.fill(0);result
                },||{for pin in &pins{pin.check()?;}Ok(())}).map_err(fail)?;
                Ok(Context{source:None,jobs:service.hosted_backup_jobs().map_err(fail)?,restore_checks:service.hosted_restore_checks().map_err(fail)?,key_saved:false,archive_saved:None,publication_saved:None,publication_copies:publication_copies(service),remote_restores:service.hosted_remote_restores().map_err(fail)?,remote_targets:Vec::new()})
            }
            #[cfg(not(target_os="macos"))]
            {let _=(source_job,pins,image);Err(failure("HOSTED_PLATFORM_UNSUPPORTED"))}
        }
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
            let recovery = super::hosted::recovery_context().map_err(fail)?;
            let source=recovery.source;
            if !expected_source_matches(input.expected_source.as_ref(), &source) { return Err(failure("HOSTED_RECOVERY_SOURCE_CHANGED")); }
            let expected_configuration=recovery.configuration;
            let publication=recovery.publication;
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
                let mut copy_plan=None;
                let job=service.create_hosted_backup(source.clone(),input.job_budget_bytes,&image,&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-recovery-runner.mjs"),|execution|{
     // This UUID and source belong to the already journalled native job.
     // Neither item is overwritten or deleted if the other step fails.
     preserve_original_configuration(&execution.job_id,&source,&expected_configuration,super::hosted_keychain::add_backup_only)?;
     if let Some(publication)=publication.as_ref()
         && let Ok(folder)=service.hosted_publication_workspace(&execution.job_id)
         && Some(folder.as_path())==Path::new(&execution.output_path).parent()
         && preserve_publication_bytes(&folder,"publication-work.json",&publication.snapshot).is_ok(){copy_plan=Some((folder.to_path_buf(),execution.job_budget_bytes));}
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
                // Optional custody cannot change a completed DB backup to failure.
                if job.state=="completed"
                    && let (Some(publication),Some((folder,budget)))=(publication.as_ref(),copy_plan)
                    && let (Ok(current_folder),Ok(remaining_budget))=(service.hosted_publication_workspace(&job.id),service.hosted_publication_budget(&job.id))
                    && current_folder==folder
                    && private_bytes(&folder.join("publication-work.json")).ok().as_ref()==Some(&publication.snapshot){let _=copy_publication(resources,&folder,publication,budget.min(remaining_budget));}
                Ok(Context {
                    source: Some(source),
                    jobs: service.hosted_backup_jobs().map_err(fail)?,
                    restore_checks: service.hosted_restore_checks().map_err(fail)?,
            key_saved: false,
            archive_saved: None,
            publication_saved: None,
            publication_copies: publication_copies(service),
            remote_restores:service.hosted_remote_restores().map_err(fail)?,remote_targets:Vec::new(),
                })
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
pub fn export_archive(resources:&Path,controller: &exhibitos_lifecycle::installations::InstallationController, selection: &str, id: &str)
    -> Result<ArchiveDelivery, LifecycleError> {
    controller.with_current(selection, |service| {service.completed_hosted_backup(id).map(|_|())})?;
    let Some(path)=super::maintenance_picker::choose(super::maintenance_picker::Kind::ExportParent)? else { return Ok(ArchiveDelivery{archive:None,publication:None}); };
    let archive=controller.export_hosted_backup(selection,id,Path::new(&path))?;
    // The encrypted archive already completed. Optional public-file delivery must
    // never turn that success into failure, including a later selection change.
    let mut publication=PublicationSaved{job_id:id.into(),state:"unavailable".into(),path:None,files:0,bytes:0};
    let copied=(||{
        let _hosted_operation=super::hosted::hold_operation()?;
        let (saved,receipt_bytes,expected)=controller.with_current(selection,|service|{let job=service.completed_hosted_backup(id)?;stored_publication(service,&job)})?;
        let target=controller.prepare_hosted_publication_export(selection,id,&archive)?;
        publication.path=Some(target.path.to_string_lossy().into_owned());
        controller.with_current(selection,|service|{
            let job=service.completed_hosted_backup(id)?;
            if !super::hosted::recovery_snapshot_matches(&saved.snapshot,&job.source){return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
            target.check()?;
            if !target.created
                && (private_bytes(&target.path.join("publication-work.json"))?!=saved.snapshot||private_bytes(&target.path.join("publication-custody.json"))?!=receipt_bytes){return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
            let budget=job.job_budget_bytes.unwrap_or(0).min(target.available_budget()?);
            let actual=run_publication_copy(resources,&target.path,&saved,budget)?;
            target.check()?;
            if actual.files!=expected.files||actual.bytes!=expected.bytes{return Err(failure("HOSTED_PUBLICATION_UNAVAILABLE"));}
            target.preserve_metadata("publication-work.json",&saved.snapshot)?;
            target.preserve_metadata("publication-custody.json",&receipt_bytes)?;
            target.check()?;Ok(actual)
        })
    })();
    if let Ok(receipt)=copied{publication.state="copied".into();publication.files=receipt.files;publication.bytes=receipt.bytes;}
    Ok(ArchiveDelivery{archive:Some(archive),publication:Some(publication)})
}

#[cfg(target_os="macos")]
fn run_remote_private(resources:&Path,input:&serde_json::Value)->Result<serde_json::Value,LifecycleError>{
    use std::{io::Write,process::{Command,Stdio},sync::mpsc,time::{Duration,Instant}};
    let pins=remote_restore_pins(resources)?;
    let mut bytes=serde_json::to_vec(input).map_err(|_|failure("HOSTED_RECOVERY_REQUEST"))?;
    if bytes.len()>131072{bytes.fill(0);return Err(failure("HOSTED_RECOVERY_REQUEST"));}
    let mut command=Command::new(resources.join("remote-runtime/generated/node"));
    command.args(["--disable-proto=throw","--no-warnings"]).arg(resources.join("scripts/hosted-remote-restore-runner.mjs")).env_clear().env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    {use std::os::unix::process::CommandExt;command.process_group(0);}
    for pin in &pins{pin.check()?;}
    let mut child=command.spawn().map_err(|_|failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
    let write=child.stdin.take().ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?.write_all(&bytes);bytes.fill(0);
    if write.is_err(){super::remote::stop(&mut child);return Err(failure("HOSTED_RUNTIME_UNAVAILABLE"));}
    let out=child.stdout.take().ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;let err=child.stderr.take().ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
    let(tx,rx)=mpsc::channel();let(etx,erx)=mpsc::channel();
    std::thread::spawn(move||{let _=tx.send(super::remote::reader(out,1048576));});std::thread::spawn(move||{let _=etx.send(super::remote::reader(err,4096));});
    let start=Instant::now();let(mut stdout,mut stderr,mut status)=(None,None,None);
    loop{
        if stdout.is_none(){stdout=rx.try_recv().ok();}
        if stderr.is_none(){stderr=erx.try_recv().ok();}
        if status.is_none(){match child.try_wait(){Ok(value)=>status=value,Err(_)=>{super::remote::stop(&mut child);return Err(failure("HOSTED_RECOVERY_EXECUTION_FAILED"));}}}
        if stdout.as_ref().is_some_and(Result::is_err)||stderr.as_ref().is_some_and(Result::is_err)||start.elapsed()>Duration::from_secs(600){super::remote::stop(&mut child);return Err(failure("HOSTED_RECOVERY_EXECUTION_FAILED"));}
        for pin in &pins{if pin.check().is_err(){super::remote::stop(&mut child);return Err(failure("REMOTE_RUNTIME_CHANGED"));}}
        if stdout.is_some()&&stderr.is_some()&&status.is_some(){break;}std::thread::sleep(Duration::from_millis(25));
    }
    let out=stdout.unwrap().map_err(|_|failure("HOSTED_RECOVERY_RESULT_INVALID"))?;let err=stderr.unwrap().map_err(|_|failure("HOSTED_RECOVERY_RESULT_INVALID"))?;
    if !status.unwrap().success()||!err.is_empty(){return Err(failure("HOSTED_RECOVERY_EXECUTION_FAILED"));}
    serde_json::from_slice(&out).map_err(|_|failure("HOSTED_RECOVERY_RESULT_INVALID"))
}

#[cfg(target_os="macos")]
fn remote_restore_pins(resources:&Path)->Result<Vec<super::remote::Pin>,LifecycleError>{
 let manifest:serde_json::Value=serde_json::from_str(option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").unwrap_or("{}")).map_err(|_|failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
 for name in ["scripts/hosted-remote-restore-runner.mjs","scripts/render-restore-provider.mjs","scripts/remote-target-entry.mjs"]{if manifest["files"][name].as_str().is_none(){return Err(failure("HOSTED_RUNTIME_UNAVAILABLE"));}}
 super::remote::verified_resources(resources)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_receipt_is_closed_pin_bound_and_does_not_claim_full_restore(){
        let publication=super::super::hosted::RecoveryPublication{source_path:"/synthetic".into(),manifest_sha256:"a".repeat(64),publication_id:"public-fixture".into(),revision_sha256:"b".repeat(64),index_sha256:"c".repeat(64),snapshot:Vec::new()};
        let valid=serde_json::json!({"operation":"publication-custody","state":"copied","files":3,"bytes":100,"manifestSha256":"a".repeat(64),"publicationId":"public-fixture","revisionSha256":"b".repeat(64),"indexSha256":"c".repeat(64)});
        assert!(publication_receipt(&serde_json::to_vec(&valid).unwrap(),&publication,100).is_ok());
        assert!(publication_receipt(&serde_json::to_vec(&valid).unwrap(),&publication,99).is_err());
        for (key,value) in [("operation",serde_json::json!("restored-and-running")),("indexSha256",serde_json::json!("d".repeat(64))),("token",serde_json::json!("must-not-cross")),("files",serde_json::json!(0))]{let mut changed=valid.clone();changed[key]=value;assert!(publication_receipt(&serde_json::to_vec(&changed).unwrap(),&publication,100).is_err());}
    }
    #[test]
    fn publication_metadata_is_exclusive_and_existing_bytes_are_never_replaced(){
        let folder=std::env::temp_dir().join(format!("exhibitos-publication-metadata-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(&folder,std::fs::Permissions::from_mode(0o700)).unwrap();}
        preserve_publication_bytes(&folder,"publication-work.json",b"original-work").unwrap();
        preserve_publication_bytes(&folder,"publication-work.json",b"original-work").unwrap();
        assert!(preserve_publication_bytes(&folder,"publication-work.json",b"changed-work").is_err());
        assert_eq!(private_bytes(&folder.join("publication-work.json")).unwrap(),b"original-work");
        #[cfg(unix)] {std::os::unix::fs::symlink(folder.join("publication-work.json"),folder.join("link")).unwrap();assert!(private_bytes(&folder.join("link")).is_err());}
        std::fs::remove_dir_all(folder).unwrap();
    }
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
    fn restore_load_uses_original_job_configuration_only_without_fallback() {
        let id="c30e227b-e771-43dc-b315-023e0469d4d2";
        let source=Source{workspace_id:"tea-original".into(),service_id:"srv-original".into(),postgres_id:"dpg-original".into()};
        let configuration=synthetic_configuration();let mut retained=Vec::new();
        preserve_original_configuration(id,&source,&configuration,|_,b|{retained=b.to_vec();Ok(())}).unwrap();
        let loaded=load_original_configuration(id,&source,|account|{assert_eq!(account,format!("render-backup-configuration-{id}"));Ok(retained.clone())}).unwrap();assert_eq!(loaded,configuration);
        assert_eq!(load_original_configuration(id,&source,|_|Err(super::super::hosted_keychain::BackupConfigurationReadError::Missing)).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_REQUIRED");
        assert_eq!(load_original_configuration(id,&source,|_|Err(super::super::hosted_keychain::BackupConfigurationReadError::Unavailable)).unwrap_err().code,"HOSTED_KEYCHAIN_UNAVAILABLE");
        assert_eq!(load_original_configuration(id,&source,|_|Ok(b"invalid retained snapshot".to_vec())).unwrap_err().code,"HOSTED_RECOVERY_CONFIGURATION_CHANGED");
        let mut changed=source.clone();changed.service_id="srv-other".into();assert!(load_original_configuration(id,&changed,|_|Ok(retained.clone())).is_err());
        for bad in ["invalid", "00000000-0000-0000-0000-000000000000"]{assert!(load_original_configuration(bad,&source,|_|panic!("invalid job cannot read OS keychain")).is_err());}
        assert!(load_original_configuration(id,&source,|_|Ok(vec![0;65537])).is_err());
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
