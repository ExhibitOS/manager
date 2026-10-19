// SPDX-License-Identifier: Apache-2.0
//! Local external archive checks. No provider/Keychain access or service activation.
use exhibitos_lifecycle::LifecycleError;
use serde::Deserialize;
use std::path::Path;
#[derive(Debug,Deserialize)]
#[serde(tag="action",rename_all="kebab-case",deny_unknown_fields)]
pub enum Input{
 Context{},
 Check{#[serde(rename="verificationId")]verification_id:String,#[serde(rename="keyPath")]key_path:String},
 Reconcile{#[serde(rename="jobId")]job_id:String,#[serde(rename="preserveCandidates")]preserve_candidates:bool},
}
fn failure(code:&str)->LifecycleError{LifecycleError{code:code.into(),guidance:match code{
 "EXTERNAL_RESTORE_KEY_INVALID"=>"별도로 보관한 원래 키 파일을 선택해 주세요. 키와 백업은 변경하지 않았어요.",
 "EXTERNAL_RESTORE_TOOL_CHANGED"=>"이 사본을 확인한 도구와 현재 도구가 달라요. 원래 백업 폴더와 키로 백업 사본 검증을 다시 실행해 주세요.",
 "EXTERNAL_RESTORE_INPUT_INVALID"=>"검사 기록과 원래 키 파일을 다시 선택해 주세요. 기존 결과는 보존돼요.",
 _=>"격리된 사본 복원 검사를 완료하지 못했어요. 기존 백업과 결과는 보존합니다. 검사 기록과 실행 도구 상태를 확인한 뒤 다시 시도해 주세요.",
 }.into()}}
fn id(value:&str)->bool{super::retry_id(value)&&value!="00000000-0000-0000-0000-000000000000"}
fn key_path(value:&str)->bool{!value.is_empty()&&value.len()<=2048&&Path::new(value).is_absolute()&&!value.chars().any(|c|c.is_control()||c==',')}
impl Input{pub fn validate(&self)->Result<(),LifecycleError>{match self{
 Self::Context{}=>Ok(()),Self::Check{verification_id,key_path:p}if id(verification_id)&&key_path(p)=>Ok(()),
 Self::Reconcile{job_id,preserve_candidates:true}if id(job_id)=>Ok(()),_=>Err(failure("EXTERNAL_RESTORE_INPUT_INVALID"))}}}
#[cfg(target_os="macos")]
struct PrivateKey{bytes:Vec<u8>,pin:super::remote::Pin}
#[cfg(target_os="macos")]
impl Drop for PrivateKey{fn drop(&mut self){self.bytes.fill(0);}}
#[cfg(target_os="macos")]
impl PrivateKey{
 fn open(path:&Path,source:&Path)->Result<Self,LifecycleError>{
  use std::os::unix::fs::MetadataExt;
  if std::fs::canonicalize(path).ok().as_deref()!=Some(path)||path.starts_with(source){return Err(failure("EXTERNAL_RESTORE_KEY_INVALID"));}
  let metadata=std::fs::symlink_metadata(path).map_err(|_|failure("EXTERNAL_RESTORE_KEY_INVALID"))?;
  if !metadata.is_file()||metadata.is_symlink()||metadata.len()!=32||metadata.nlink()!=1||metadata.mode()&0o7777!=0o600||metadata.uid()!=unsafe{libc::geteuid()}{return Err(failure("EXTERNAL_RESTORE_KEY_INVALID"));}
  let (mut bytes,pin)=super::remote::bounded(path,32).map_err(|_|failure("EXTERNAL_RESTORE_KEY_INVALID"))?;
  if bytes.len()!=32{bytes.fill(0);return Err(failure("EXTERNAL_RESTORE_KEY_INVALID"));}Ok(Self{bytes,pin})
 }
}
#[cfg(target_os="macos")]
pub fn operation(resources:&Path,controller:&exhibitos_lifecycle::installations::InstallationController,selection:&str,input:Input)->Result<serde_json::Value,LifecycleError>{
 input.validate()?;
 match input{
  Input::Context{}=>{},
  Input::Check{verification_id,key_path}=>{
   // This is the fixed packaged runner; renderer input supplies neither a binary
   // nor archive identifiers, expected hashes, authority, configuration or key bytes.
   let manifest:serde_json::Value=serde_json::from_str(option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").unwrap_or("{}")).map_err(|_|failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
   if !manifest["files"].as_object().is_some_and(|m|m.contains_key("scripts/hosted-restore-runner.mjs")){return Err(failure("HOSTED_RUNTIME_UNAVAILABLE"));}
   let pins=super::remote::verified_resources(resources)?;
   controller.with_external_archive_source(selection,&verification_id,|service|{
    let source=service.verification_source(&verification_id)?;
    let image=service.bundled_maintenance_image(&resources.join("platform-runtime/generated"))?.ok_or_else(||failure("HOSTED_RUNTIME_UNAVAILABLE"))?;
    if image!=source.image{return Err(failure("EXTERNAL_RESTORE_TOOL_CHANGED"));}
    let key=PrivateKey::open(Path::new(&key_path),&source.source_path)?;
    service.prepare_external_archive_tool(&resources.join("platform-runtime/generated"),&image)?;
    service.create_external_restore_check(&verification_id,&image,&resources.join("remote-runtime/generated/node"),&resources.join("scripts/hosted-restore-runner.mjs"),|execution|{
     key.pin.check()?;
     let mut plan=serde_json::to_value(execution).map_err(|_|failure("EXTERNAL_RESTORE_INPUT_INVALID"))?;
     plan["keyBase64"]=super::hosted::base64(&key.bytes).into();
     serde_json::to_vec(&serde_json::json!({"execution":plan})).map_err(|_|failure("EXTERNAL_RESTORE_INPUT_INVALID"))
    },||{key.pin.check()?;for pin in &pins{pin.check()?;}Ok(())})?;Ok(())
   })?;
  },
  Input::Reconcile{job_id,preserve_candidates}=>{controller.with_external_archive_job(selection,&job_id,|service|service.reconcile_helper("external-restoration",&job_id,preserve_candidates))?;},
 }
 serde_json::to_value(controller.external_restore_context(selection)?).map_err(|_|failure("MANAGER_PROTOCOL"))
}
#[cfg(not(target_os="macos"))]
pub fn operation(_resources:&Path,_controller:&exhibitos_lifecycle::installations::InstallationController,_selection:&str,_input:Input)->Result<serde_json::Value,LifecycleError>{Err(failure("HOSTED_PLATFORM_UNSUPPORTED"))}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn external_request_is_closed_and_never_accepts_provider_or_expected_metadata(){
  let id="c30e227b-e771-43dc-b315-023e0469d4d2";
  for value in [serde_json::json!({"action":"context"}),serde_json::json!({"action":"check","verificationId":id,"keyPath":"/private/key.bin"}),serde_json::json!({"action":"reconcile","jobId":id,"preserveCandidates":true})]{assert!(serde_json::from_value::<Input>(value.clone()).unwrap().validate().is_ok());for name in ["sourceJobId","sourcePath","keyBase64","expectedBackupId","image","expectedConfiguration"]{let mut changed=value.clone();changed[name]="forbidden".into();assert!(serde_json::from_value::<Input>(changed).is_err());}}
  for value in [serde_json::json!({"action":"check","verificationId":id,"keyPath":"relative"}),serde_json::json!({"action":"reconcile","jobId":id,"preserveCandidates":false}),serde_json::json!({"action":"check","verificationId":"00000000-0000-0000-0000-000000000000","keyPath":"/private/key"})]{assert!(serde_json::from_value::<Input>(value).unwrap().validate().is_err());}
 }
 #[cfg(target_os="macos")]
 #[test]fn external_key_is_exact_private_readonly_pinned_and_not_in_archive(){
  use std::os::unix::fs::{PermissionsExt,symlink};
  let base=std::env::temp_dir().join(format!("external-key-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&base).unwrap();let base=std::fs::canonicalize(base).unwrap();let source=base.join("archive");std::fs::create_dir(&source).unwrap();let key=base.join("backup.key");std::fs::write(&key,[9u8;32]).unwrap();std::fs::set_permissions(&key,std::fs::Permissions::from_mode(0o600)).unwrap();let pinned=PrivateKey::open(&key,&source).unwrap();assert_eq!(pinned.bytes,[9u8;32]);pinned.pin.check().unwrap();assert!(PrivateKey::open(&key,&base).is_err());std::fs::set_permissions(&key,std::fs::Permissions::from_mode(0o644)).unwrap();assert!(pinned.pin.check().is_err());assert!(PrivateKey::open(&key,&source).is_err());std::fs::set_permissions(&key,std::fs::Permissions::from_mode(0o600)).unwrap();let link=base.join("linked.key");symlink(&key,&link).unwrap();assert!(PrivateKey::open(&link,&source).is_err());assert_eq!(std::fs::read(&key).unwrap(),[9u8;32]);
 }
}
