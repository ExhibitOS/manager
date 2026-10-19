// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::{io::{Read, Write}, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
use exhibitos_lifecycle::LifecycleError;
use sha2::{Digest, Sha256};
use std::sync::{atomic::{AtomicBool,Ordering},Mutex,mpsc};
static ABORT:AtomicBool=AtomicBool::new(false);
static OWNER:Mutex<Option<String>>=Mutex::new(None);
static BUSY:AtomicBool=AtomicBool::new(false);
struct BusyGuard;
impl Drop for BusyGuard{fn drop(&mut self){if let Ok(mut owner)=OWNER.lock(){*owner=None;}ABORT.store(false,Ordering::Release);BUSY.store(false,Ordering::Release);}}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all="kebab-case")]
pub enum Action { Plan, Status, Logs, Verify, Install, Start, Stop, Restart, Retry, Backup, BackupJobs, Update, UpdateStatus, Rollback }
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct Consent { pub preserve_volumes:bool, pub existing_images_only:bool,pub external_writers_quiesced:bool,pub apps_closed:bool }
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct Connection { pub remote_root:String,pub manager_path:String,pub manager_sha256:String,pub known_hosts_path:String,pub known_hosts_sha256:String,pub https_origin:String }
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub struct Request { pub request_id:String,pub action:Action,pub oed:serde_json::Value,pub connection:Connection,pub consent:Consent,pub maintenance:Option<serde_json::Value> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub struct CancelInput{pub request_id:String}
pub fn busy()->bool{BUSY.load(Ordering::Acquire)}
pub fn cancel(input:CancelInput)->Result<bool,LifecycleError>{let owner=OWNER.lock().map_err(|_|refused("SSH_OPERATION_UNCERTAIN"))?;if owner.as_deref()!=Some(input.request_id.as_str()){return Ok(false);}ABORT.store(true,Ordering::Release);Ok(true)}
fn refused(code:&str)->LifecycleError { LifecycleError{code:code.into(),guidance:"서버 작업을 완료하지 못했습니다. 기존 데이터·작업을 보존하고 연결·인증 정보·실제 상태를 확인하세요.".into()} }
pub(crate) struct Pin{path:PathBuf,file:std::fs::File,metadata:std::fs::Metadata}
fn same(a:&std::fs::Metadata,b:&std::fs::Metadata)->bool{
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;a.dev()==b.dev()&&a.ino()==b.ino()&&a.len()==b.len()&&a.mode()==b.mode()&&a.nlink()==b.nlink()&&a.mtime()==b.mtime()&&a.mtime_nsec()==b.mtime_nsec()&&a.ctime()==b.ctime()&&a.ctime_nsec()==b.ctime_nsec()}
 #[cfg(not(unix))]{a.len()==b.len()&&a.modified().ok()==b.modified().ok()}
}
impl Pin{pub(crate) fn check(&self)->Result<(),LifecycleError>{let named=std::fs::symlink_metadata(&self.path).map_err(|_|refused("REMOTE_RUNTIME_CHANGED"))?;let opened=self.file.metadata().map_err(|_|refused("REMOTE_RUNTIME_CHANGED"))?;if named.file_type().is_symlink()||!same(&self.metadata,&named)||!same(&self.metadata,&opened){return Err(refused("REMOTE_RUNTIME_CHANGED"));}Ok(())}}
pub(crate) fn bounded(path:&Path,max:u64)->Result<(Vec<u8>,Pin),LifecycleError>{
 let before=std::fs::symlink_metadata(path).map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 if !before.is_file()||before.file_type().is_symlink()||before.len()>max{return Err(refused("REMOTE_RUNTIME_UNSAFE"));}
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;if before.nlink()!=1||before.mode()&0o022!=0{return Err(refused("REMOTE_RUNTIME_UNSAFE"));}}
 let mut file=std::fs::File::open(path).map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;let metadata=file.metadata().map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;if !same(&before,&metadata){return Err(refused("REMOTE_RUNTIME_CHANGED"));}
 let mut bytes=Vec::new();Read::by_ref(&mut file).take(max+1).read_to_end(&mut bytes).map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 if bytes.len()as u64!=metadata.len()||bytes.len()as u64>max{return Err(refused("REMOTE_RUNTIME_UNSAFE"));}let pin=Pin{path:path.into(),file,metadata};pin.check()?;Ok((bytes,pin))
}
pub(crate) fn reader(mut pipe:impl Read,max:usize)->Result<Vec<u8>,()> { let mut out=Vec::new();pipe.by_ref().take((max+1)as u64).read_to_end(&mut out).map_err(|_|())?;if out.len()>max{Err(())}else{Ok(out)} }
fn signal(child:&std::process::Child,kind:&str){
 #[cfg(unix)]{let id=format!("-{}",child.id());let _=Command::new("/bin/kill").args([kind,"--",&id]).stdout(Stdio::null()).stderr(Stdio::null()).status();}
 #[cfg(not(unix))]{let _=(child,kind);}
}
pub(crate) fn stop(child:&mut std::process::Child){signal(child,"-TERM");let deadline=Instant::now()+Duration::from_secs(3);while Instant::now()<deadline{if child.try_wait().ok().flatten().is_some(){return;}std::thread::sleep(Duration::from_millis(25));}signal(child,"-KILL");let _=child.kill();let _=child.wait();}
pub(crate) fn verified_resources(resources:&Path)->Result<Vec<Pin>,LifecycleError>{if cfg!(windows){return Err(refused("SSH_CLIENT_UNSUPPORTED"));}
 let manifest=option_env!("EXHIBITOS_REMOTE_RUNTIME_MANIFEST").ok_or_else(||refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 let expected:serde_json::Value=serde_json::from_str(manifest).map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 let hashes=expected["files"].as_object().ok_or_else(||refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 let mut pins=Vec::new();
 if !hashes.contains_key("scripts/remote-runner.mjs")||!hashes.contains_key("vendor/deployment/src/ssh-maintenance.mjs"){return Err(refused("REMOTE_RUNTIME_UNAVAILABLE"));}
 for(name,hash)in hashes{if name.starts_with('/')||name.contains("..")||(!name.starts_with("vendor/deployment/")&&name!="scripts/remote-runner.mjs"&&name!="scripts/hosted-runner.mjs"&&name!="scripts/hosted-recovery-runner.mjs"&&name!="scripts/hosted-restore-runner.mjs"&&name!="scripts/hosted-remote-restore-runner.mjs"&&name!="scripts/render-restore-provider.mjs"&&name!="scripts/remote-target-entry.mjs"&&name!="scripts/publication-runner.mjs"&&name!="scripts/publication-client.mjs"&&name!="remote-runtime/generated/NODE-LICENSE"){return Err(refused("REMOTE_RUNTIME_CHANGED"));}let(bytes,pin)=bounded(&resources.join(name),1024*1024)?;pins.push(pin);if format!("{:x}",Sha256::digest(&bytes))!=hash.as_str().unwrap_or(""){return Err(refused("REMOTE_RUNTIME_CHANGED"));}}
 let node=resources.join("remote-runtime/generated/node");
 let(bytes,pin)=bounded(&node,150*1024*1024)?;pins.push(pin);
 if expected["nodeVersion"]!="24.21.0"||format!("{:x}",Sha256::digest(&bytes))!=expected["nodeSha256"].as_str().unwrap_or(""){return Err(refused("REMOTE_RUNTIME_CHANGED"));}
 Ok(pins)
}
pub fn available(resources:&Path)->bool{verified_resources(resources).is_ok()}
/** Fixed packaged public Node/adapter only. No renderer-controlled executable, flags or script. */
pub fn execute(resources:PathBuf,request:Request)->Result<serde_json::Value,LifecycleError>{
 if BUSY.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err(refused("SSH_BUSY"));}
 let _busy=BusyGuard;
 if request.request_id.len()!=36||!request.request_id.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_digit()||(b'a'..=b'f').contains(&b)}){return Err(refused("SSH_REQUEST_INVALID"));}
 *OWNER.lock().map_err(|_|refused("SSH_OPERATION_UNCERTAIN"))?=Some(request.request_id.clone());
 let timeout=match request.action{Action::Update=>2490000,Action::Backup|Action::Rollback=>1890000,_=>260000};
 if !request.consent.preserve_volumes||!request.consent.existing_images_only{return Err(refused("SSH_CONSENT_REQUIRED"));}
 if cfg!(windows){return Err(refused("SSH_CLIENT_UNSUPPORTED"));}
 let pins=verified_resources(&resources)?;let node=resources.join("remote-runtime/generated/node");
 let input=serde_json::to_vec(&request).map_err(|_|refused("SSH_REQUEST_INVALID"))?;if input.len()>131072{return Err(refused("SSH_REQUEST_TOO_LARGE"));}
 let mut command=Command::new(node);command.args(["--disable-proto=throw","--no-warnings"]).arg(resources.join("scripts/remote-runner.mjs")).env_clear().env("PATH","/usr/bin:/bin:/usr/sbin:/sbin").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
 if let Some(home)=std::env::var_os("HOME"){command.env("HOME",home);}
 if let Some(refs)=request.oed["secretRefs"].as_array(){for value in refs{if value["source"]=="environment"{let variable=value["variable"].as_str().ok_or_else(||refused("SSH_SECRET_REFERENCE_INVALID"))?;if !variable.starts_with("EXHIBITOS_")||variable.len()>80||!variable.bytes().all(|b|b.is_ascii_uppercase()||b.is_ascii_digit()||b==b'_'){return Err(refused("SSH_SECRET_REFERENCE_INVALID"));}
if let Some(secret)=std::env::var_os(variable){command.env(variable,secret);}}}}
 #[cfg(unix)] { use std::os::unix::process::CommandExt;command.process_group(0); }
 for pin in &pins{pin.check()?;}if ABORT.load(Ordering::Acquire){return Err(refused("SSH_CANCELLED"));}
 let mut child=command.spawn().map_err(|_|refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
 for pin in &pins{if let Err(error)=pin.check(){stop(&mut child);return Err(error);}}
 if child.stdin.take().and_then(|mut pipe|pipe.write_all(&input).ok()).is_none(){stop(&mut child);return Err(refused("SSH_OPERATION_UNCERTAIN"));}
 let stdout=child.stdout.take().ok_or_else(||refused("SSH_OPERATION_UNCERTAIN"))?;let stderr=child.stderr.take().ok_or_else(||refused("SSH_OPERATION_UNCERTAIN"))?;
 let(out_tx,out_rx)=mpsc::channel();let(err_tx,err_rx)=mpsc::channel();std::thread::spawn(move||{let _=out_tx.send(reader(stdout,262144));});std::thread::spawn(move||{let _=err_tx.send(reader(stderr,8192));});
 let start=Instant::now();let mut output=None;let mut errors=None;let mut status=None;let mut exited=None;
 loop{
  if output.is_none() && let Ok(v)=out_rx.try_recv(){output=Some(v);}
  if errors.is_none() && let Ok(v)=err_rx.try_recv(){errors=Some(v);}
  if output.as_ref().is_some_and(Result::is_err)||errors.as_ref().is_some_and(Result::is_err)||ABORT.load(Ordering::Acquire)||start.elapsed()>Duration::from_millis(timeout){stop(&mut child);return Err(refused("SSH_OPERATION_UNCERTAIN"));}
  if status.is_none(){match child.try_wait(){Ok(Some(v))=>{status=Some(v);exited=Some(Instant::now());},Ok(None)=>{},Err(_)=>{stop(&mut child);return Err(refused("SSH_OPERATION_UNCERTAIN"));}}}
  if status.is_some()&&output.is_some()&&errors.is_some(){break;}
  if exited.is_some_and(|time|time.elapsed()>Duration::from_secs(3)){signal(&child,"-KILL");return Err(refused("SSH_RESPONSE_INVALID"));}std::thread::sleep(Duration::from_millis(25));
 }
 for pin in &pins{pin.check()?;}
 let status=status.unwrap();let stdout=output.unwrap().map_err(|_|refused("SSH_RESPONSE_INVALID"))?;let stderr=errors.unwrap().map_err(|_|refused("SSH_RESPONSE_INVALID"))?;
 if !status.success(){let code=String::from_utf8_lossy(&stderr);let code=code.trim();let safe=code.starts_with("SSH_")&&code.len()<=64&&code.bytes().all(|b|b.is_ascii_uppercase()||b==b'_');return Err(refused(if safe{code}else{"SSH_OPERATION_UNCERTAIN"}));}
 serde_json::from_slice(&stdout).map_err(|_|refused("SSH_RESPONSE_INVALID"))
}
