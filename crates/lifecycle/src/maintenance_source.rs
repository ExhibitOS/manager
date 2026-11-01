// SPDX-License-Identifier: Apache-2.0
//! Native-only linkage from an authenticated archive to its retained verification.
//! This is not a hosted/provider completion record or a database restore receipt.
use super::*;
use super::maintenance::VerificationReceipt;
use std::os::unix::fs::{MetadataExt,OpenOptionsExt};
const LIMIT:u64=64*1024*1024;
const MAX_FILES:usize=100000;
fn invalid()->LifecycleError{err("BACKUP_VERIFICATION_SOURCE_INVALID")}
fn uuid(id:&str)->bool{Uuid::parse_str(id).is_ok_and(|v|v.to_string()==id)}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct FilePin{path:String,dev:u64,ino:u64,bytes:u64,mode:u32,uid:u32,mtime:i64,mtime_nsec:i64,ctime:i64,ctime_nsec:i64}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct VerificationSource{
 pub verification_id:String,pub backup_id:String,pub image:String,pub authenticated_manifest_sha256:String,pub files:u64,pub source_path:PathBuf,
 pins:Vec<FilePin>,directory_pins:Vec<(u64,u64,u32,u32)>,
}
pub(crate) struct DirectoryPin{file:File,path:PathBuf}
impl DirectoryPin{
 pub(crate) fn check_path(&self,expected:&Path)->Result<()>{if self.path!=expected{return Err(invalid());}self.check()}
 pub(crate) fn open(path:&Path)->Result<Self>{
  let file=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_DIRECTORY|libc::O_NONBLOCK).open(path).map_err(|_|invalid())?;
  let pin=Self{file,path:path.into()};pin.check()?;Ok(pin)
 }
 pub(crate) fn check(&self)->Result<()>{let held=self.file.metadata().map_err(|_|invalid())?;let named=fs::symlink_metadata(&self.path).map_err(|_|invalid())?;
  if !held.is_dir()||named.is_symlink()||held.dev()!=named.dev()||held.ino()!=named.ino()||fs::canonicalize(&self.path).ok().as_deref()!=Some(self.path.as_path()){return Err(invalid());}Ok(())
 }
}
fn file_pin(root:&Path,name:&str)->Result<FilePin>{
 let path=root.join(name);let file=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(&path).map_err(|_|invalid())?;
 let m=file.metadata().map_err(|_|invalid())?;let named=fs::symlink_metadata(&path).map_err(|_|invalid())?;
 let max=if name=="manifest.gcm"{16*1024*1024+36}else if name.starts_with("files/"){32*1024*1024*1024+36}else{4096};
 if !m.is_file()||named.is_symlink()||m.nlink()!=1||m.mode()&0o077!=0||m.uid()!=unsafe{libc::geteuid()}||m.len()==0||m.len()>max||m.dev()!=named.dev()||m.ino()!=named.ino()||m.size()!=named.size()||m.mtime()!=named.mtime()||m.mtime_nsec()!=named.mtime_nsec()||m.ctime()!=named.ctime()||m.ctime_nsec()!=named.ctime_nsec(){return Err(invalid());}
 Ok(FilePin{path:name.into(),dev:m.dev(),ino:m.ino(),bytes:m.len(),mode:m.mode(),uid:m.uid(),mtime:m.mtime(),mtime_nsec:m.mtime_nsec(),ctime:m.ctime(),ctime_nsec:m.ctime_nsec()})
}
fn cipher_name(name:&[u8])->bool{name.len()==12&&name.ends_with(b".gcm")&&name[..8].iter().all(u8::is_ascii_digit)}
// Finder metadata is not authenticated backup content. Its optional presence and
// identity are held only for the current observation, never the durable source link.
fn finder_pin(root:&Path)->Result<Option<FilePin>>{
 let path=root.join(".DS_Store");
 match fs::symlink_metadata(&path){Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err(invalid())}
 let file=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(&path).map_err(|_|invalid())?;
 let m=file.metadata().map_err(|_|invalid())?;let named=fs::symlink_metadata(&path).map_err(|_|invalid())?;
 if !m.is_file()||named.is_symlink()||m.nlink()!=1||m.mode()&0o7133!=0||m.uid()!=unsafe{libc::geteuid()}||m.len()>1024*1024||m.dev()!=named.dev()||m.ino()!=named.ino()||m.size()!=named.size()||m.mode()!=named.mode()||m.uid()!=named.uid()||m.nlink()!=named.nlink()||m.mtime()!=named.mtime()||m.mtime_nsec()!=named.mtime_nsec()||m.ctime()!=named.ctime()||m.ctime_nsec()!=named.ctime_nsec(){return Err(invalid());}
 Ok(Some(FilePin{path:".DS_Store".into(),dev:m.dev(),ino:m.ino(),bytes:m.len(),mode:m.mode(),uid:m.uid(),mtime:m.mtime(),mtime_nsec:m.mtime_nsec(),ctime:m.ctime(),ctime_nsec:m.ctime_nsec()}))
}
fn names(root:&Path)->Result<(Vec<String>,bool)>{
 let mut top=fs::read_dir(root).map_err(|_|invalid())?.take(6).map(|e|e.map_err(|_|invalid()).and_then(|e|e.file_name().into_string().map_err(|_|invalid()))).collect::<Result<Vec<_>>>()?;top.sort();
 let finder=top.first().is_some_and(|name|name==".DS_Store");if finder{top.remove(0);}
 if top!=["complete.json","files","manifest.gcm","writing.json"]{return Err(invalid());}
 let mut files=Vec::new();for entry in fs::read_dir(root.join("files")).map_err(|_|invalid())?{if files.len()>=MAX_FILES{return Err(invalid());}let name=entry.map_err(|_|invalid())?.file_name().into_string().map_err(|_|invalid())?;
  if !cipher_name(name.as_bytes()){return Err(invalid());}files.push(format!("files/{name}"));}
 if files.is_empty(){return Err(invalid());}files.sort();let mut all=vec!["complete.json".into(),"manifest.gcm".into(),"writing.json".into()];all.extend(files);Ok((all,finder))
}
pub(crate) struct SourceSnapshot{root:PathBuf,pins:Vec<FilePin>,finder:Option<FilePin>,directories:Vec<DirectoryPin>,directory_pins:Vec<(u64,u64,u32,u32)>}
impl SourceSnapshot{
 pub(crate) fn bounded_cipher_bytes(&self)->Result<u64>{
  self.check()?;
  self.pins.iter().try_fold(0u64,|total,pin|total.checked_add(pin.bytes).ok_or_else(invalid))
 }
 pub(crate) fn capture(root:&Path)->Result<Self>{
  super::maintenance::input_path(root,true)?;super::maintenance::input_path(&root.join("files"),true)?;
  let mut directories=root.ancestors().map(DirectoryPin::open).collect::<Result<Vec<_>>>()?;directories.push(DirectoryPin::open(&root.join("files"))?);
  let (paths,has_finder)=names(root)?;let finder=finder_pin(root)?;if has_finder!=finder.is_some(){return Err(invalid());}let pins=paths.iter().map(|name|file_pin(root,name)).collect::<Result<Vec<_>>>()?;
  let directory_pins=[root,root.join("files").as_path()].iter().map(|path|{let m=fs::symlink_metadata(path).map_err(|_|invalid())?;Ok((m.dev(),m.ino(),m.mode(),m.uid()))}).collect::<Result<Vec<_>>>()?;
  let snapshot=Self{root:root.into(),pins,finder,directories,directory_pins};snapshot.check()?;Ok(snapshot)
 }
 pub(crate) fn check(&self)->Result<()>{for directory in &self.directories{directory.check()?;}
  for (path,pin) in [self.root.clone(),self.root.join("files")].iter().zip(&self.directory_pins){let m=fs::symlink_metadata(path).map_err(|_|invalid())?;if (m.dev(),m.ino(),m.mode(),m.uid())!=*pin{return Err(invalid());}}
  let (paths,has_finder)=names(&self.root)?;
  if paths!=self.pins.iter().map(|p|p.path.clone()).collect::<Vec<_>>()||has_finder!=self.finder.is_some()||finder_pin(&self.root)?!=self.finder{return Err(invalid());}
  for pin in &self.pins{if &file_pin(&self.root,&pin.path)?!=pin{return Err(invalid());}}Ok(())
 }
 pub(crate) fn preserve(&self,workspace:&Path,receipt:&VerificationReceipt)->Result<()>{
  self.check()?;let manifest=authenticated_manifest(workspace,receipt)?;
  if manifest.1!=self.pins.iter().filter(|p|p.path.starts_with("files/")).map(|p|p.path.clone()).collect::<Vec<_>>(){return Err(invalid());}
  let record=VerificationSource{verification_id:receipt.id.clone(),backup_id:manifest.0,image:receipt.image.clone(),authenticated_manifest_sha256:receipt.authenticated_manifest_sha256.clone(),files:receipt.files,source_path:self.root.clone(),pins:self.pins.clone(),directory_pins:self.directory_pins.clone()};
  let bytes=serde_json::to_vec(&record).map_err(|_|invalid())?;write_private_new(workspace,"source.json",&bytes,LIMIT as usize)?;self.check()?;Ok(())
 }
}
fn authenticated_manifest(workspace:&Path,receipt:&VerificationReceipt)->Result<(String,Vec<String>)>{
 if !uuid(&receipt.id)||receipt.operation!="verified"||receipt.files==0||receipt.files>MAX_FILES as u64||!hash_valid(&receipt.authenticated_manifest_sha256)||!receipt.image.strip_prefix("sha256:").is_some_and(hash_valid){return Err(invalid());}
 let bytes=super::installation_backup::source_bytes(workspace,"plaintext/manifest.json",16*1024*1024,true)?;
 if digest(&bytes)!=receipt.authenticated_manifest_sha256{return Err(invalid());}let manifest:Value=serde_json::from_slice(&bytes).map_err(|_|invalid())?;
 let id=manifest["id"].as_str().filter(|id|uuid(id)).ok_or_else(invalid)?.to_owned();
 if manifest["kind"]!="service-backup"||!manifest["schemaVersion"].as_str().is_some_and(|v|["1.0.0-draft.1","1.0.0-draft.2"].contains(&v)){return Err(invalid());}
 let entries=manifest["files"].as_array().filter(|f|f.len()==receipt.files as usize).ok_or_else(invalid)?;let mut paths=Vec::new();
 for entry in entries{let name=entry["path"].as_str().ok_or_else(invalid)?;if !name.as_bytes().strip_prefix(b"files/").is_some_and(cipher_name){return Err(invalid());}paths.push(name.to_owned());}paths.sort();if paths.windows(2).any(|p|p[0]==p[1]){return Err(invalid());}Ok((id,paths))
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct VerificationSummary{pub verification_id:String,pub backup_id:String,pub image:String,pub files:u64,pub at:u64}
impl LifecycleService{
 /// Historical metadata only; explicit restore admission must recheck source and crypto.
 pub fn verification_sources(&self)->Result<Vec<VerificationSummary>>{
  let mut out=Vec::new();for entry in fs::read_dir(&self.root).map_err(|_|invalid())?{let name=entry.map_err(|_|invalid())?.file_name();let Some(id)=name.to_str().and_then(|s|s.strip_prefix("backup-verification-")).filter(|id|uuid(id))else{continue};
   let workspace=self.root.join(format!("backup-verification-{id}"));if !workspace.join("receipt.json").exists()||!workspace.join("source.json").exists(){continue;}
   if out.len()>=1000{return Err(err("JOB_HISTORY_FULL"));}
   let receipt:VerificationReceipt=serde_json::from_slice(&super::installation_backup::source_bytes(&workspace,"receipt.json",4096,true)?).map_err(|_|invalid())?;
   let record:VerificationSource=serde_json::from_slice(&super::installation_backup::source_bytes(&workspace,"source.json",LIMIT,true)?).map_err(|_|invalid())?;
   if receipt.id!=id||record.verification_id!=id||!uuid(&record.backup_id)||receipt.operation!="verified"||receipt.files==0||receipt.files>MAX_FILES as u64||record.files!=receipt.files||record.image!=receipt.image||!receipt.image.strip_prefix("sha256:").is_some_and(hash_valid)||!hash_valid(&receipt.authenticated_manifest_sha256)||record.authenticated_manifest_sha256!=receipt.authenticated_manifest_sha256||record.pins.len()>MAX_FILES+3{return Err(invalid());}out.push(VerificationSummary{verification_id:record.verification_id,backup_id:record.backup_id,image:record.image,files:record.files,at:receipt.at});
  }out.sort_by_key(|r|r.at);Ok(out)
 }
 /// Revalidates a retained native source link. Existing receipts without a link
 /// remain authentication receipts, but are not silently promoted to this input.
 /// Metadata custody is not fresh cryptographic authorization: a later restore
 /// must authenticate the ciphertext and expected backup ID/SHA before SQL.
 pub fn verification_source(&self,id:&str)->Result<VerificationSource>{
  let _operation=self.lock()?;self.verification_source_unlocked(id)
 }
 pub(crate) fn verification_source_unlocked(&self,id:&str)->Result<VerificationSource>{
  let record=self.verification_source_metadata_unlocked(id)?;
  let current=SourceSnapshot::capture(&record.source_path)?;if current.pins!=record.pins||current.directory_pins!=record.directory_pins{return Err(invalid());}Ok(record)
 }
 pub(crate) fn verification_source_metadata_unlocked(&self,id:&str)->Result<VerificationSource>{
  if !uuid(id){return Err(invalid());}let workspace=self.root.join(format!("backup-verification-{id}"));
  let receipt:VerificationReceipt=serde_json::from_slice(&super::installation_backup::source_bytes(&workspace,"receipt.json",4096,true)?).map_err(|_|invalid())?;
  let record:VerificationSource=serde_json::from_slice(&super::installation_backup::source_bytes(&workspace,"source.json",LIMIT,true)?).map_err(|_|invalid())?;
  let (backup_id,files)=authenticated_manifest(&workspace,&receipt)?;
  if receipt.id!=id||record.verification_id!=id||record.backup_id!=backup_id||record.image!=receipt.image||record.authenticated_manifest_sha256!=receipt.authenticated_manifest_sha256||record.files!=receipt.files||record.pins.len()>MAX_FILES+3||files!=record.pins.iter().filter(|p|p.path.starts_with("files/")).map(|p|p.path.clone()).collect::<Vec<_>>(){return Err(invalid());}
  Ok(record)
 }
}

#[cfg(test)]
mod tests{
 use super::*;use std::os::unix::fs::{PermissionsExt,symlink};
 fn fixture()->(LifecycleService,PathBuf,PathBuf,VerificationReceipt){
  let base=std::env::temp_dir().join(format!("verification-source-{}",Uuid::new_v4()));fs::create_dir(&base).unwrap();let base=fs::canonicalize(base).unwrap();
  let service=LifecycleService::new(base.join("profile")).unwrap();let source=base.join("archive");fs::create_dir(&source).unwrap();fs::set_permissions(&source,fs::Permissions::from_mode(0o700)).unwrap();fs::create_dir(source.join("files")).unwrap();fs::set_permissions(source.join("files"),fs::Permissions::from_mode(0o700)).unwrap();
  for name in ["complete.json","writing.json","manifest.gcm","files/00000000.gcm"]{write_private_new(&source,name,b"synthetic ciphertext fixture",4096).unwrap();}
  let id=Uuid::new_v4().to_string();let workspace=service.root.join(format!("backup-verification-{id}"));fs::create_dir(&workspace).unwrap();fs::set_permissions(&workspace,fs::Permissions::from_mode(0o700)).unwrap();fs::create_dir(workspace.join("plaintext")).unwrap();
  let bytes=serde_json::to_vec(&serde_json::json!({"schemaVersion":"1.0.0-draft.1","kind":"service-backup","id":"c30e227b-e771-43dc-b315-023e0469d4d2","files":[{"path":"files/00000000.gcm"}]})).unwrap();write_private_new(&workspace.join("plaintext"),"manifest.json",&bytes,4096).unwrap();
  let receipt=VerificationReceipt{id,operation:"verified".into(),files:1,image:format!("sha256:{}","a".repeat(64)),authenticated_manifest_sha256:digest(&bytes),at:now()};(service,source,workspace,receipt)
 }
 #[test]
 fn historical_source_summary_is_metadata_only_but_explicit_action_rechecks_source(){
  let(service,source,workspace,receipt)=fixture();SourceSnapshot::capture(&source).unwrap().preserve(&workspace,&receipt).unwrap();write_json(&workspace,"receipt.json",&receipt).unwrap();fs::rename(&source,source.with_extension("moved")).unwrap();let summaries=service.verification_sources().unwrap();assert_eq!(summaries.len(),1);assert_eq!(summaries[0].verification_id,receipt.id);assert!(service.verification_source(&receipt.id).is_err());
 }
 #[test]
 fn verification_source_links_only_matching_native_receipt_and_original_pins(){
  let(service,source,workspace,receipt)=fixture();let snapshot=SourceSnapshot::capture(&source).unwrap();snapshot.preserve(&workspace,&receipt).unwrap();write_json(&workspace,"receipt.json",&receipt).unwrap();
  let record=service.verification_source(&receipt.id).unwrap();assert_eq!(record.backup_id,"c30e227b-e771-43dc-b315-023e0469d4d2");assert_eq!(record.verification_id,receipt.id);assert_eq!(record.source_path,source);
  let json=serde_json::to_value(&receipt).unwrap();assert_eq!(json.as_object().unwrap().len(),6);assert!(json.get("sourcePath").is_none());
  let bytes=fs::read(workspace.join("source.json")).unwrap();assert!(!String::from_utf8(bytes).unwrap().contains("keyPath"));assert_eq!(fs::metadata(workspace.join("source.json")).unwrap().permissions().mode()&0o7777,0o600);
  fs::write(source.join("files/00000000.gcm"),b"changed fixture").unwrap();assert!(service.verification_source(&receipt.id).is_err());
 }
 #[test]
 fn verification_source_rejects_manifest_receipt_record_and_source_changes_without_migration(){
  for mode in ["unknown","receipt","manifest","extra-file","symlink","directory","missing-source","legacy"]{
   let(service,source,workspace,receipt)=fixture();let snapshot=SourceSnapshot::capture(&source).unwrap();if mode!="legacy"{snapshot.preserve(&workspace,&receipt).unwrap();}write_json(&workspace,"receipt.json",&receipt).unwrap();
   match mode{
    "unknown"=>{let mut value:Value=serde_json::from_slice(&fs::read(workspace.join("source.json")).unwrap()).unwrap();value["keyPath"]="forbidden".into();write_json(&workspace,"source.json",&value).unwrap();},
    "receipt"=>{let mut value=serde_json::to_value(&receipt).unwrap();value["files"]=2.into();write_json(&workspace,"receipt.json",&value).unwrap();},
    "manifest"=>{fs::write(workspace.join("plaintext/manifest.json"),b"changed").unwrap();},
    "extra-file"=>{write_private_new(&source.join("files"),"00000001.gcm",b"extra",4096).unwrap();},
    "symlink"=>{fs::remove_file(source.join("files/00000000.gcm")).unwrap();symlink(source.join("manifest.gcm"),source.join("files/00000000.gcm")).unwrap();},
    "directory"=>{fs::rename(source.join("files"),source.join("original-files")).unwrap();fs::create_dir(source.join("files")).unwrap();fs::set_permissions(source.join("files"),fs::Permissions::from_mode(0o700)).unwrap();fs::rename(source.join("original-files/00000000.gcm"),source.join("files/00000000.gcm")).unwrap();fs::remove_dir(source.join("original-files")).unwrap();},
    "missing-source"=>{fs::rename(&source,source.with_extension("moved")).unwrap();},_=>{}
   }
   assert!(service.verification_source(&receipt.id).is_err(),"{mode}");if mode=="legacy"{assert!(!workspace.join("source.json").exists());assert_eq!(fs::read(workspace.join("receipt.json")).unwrap(),serde_json::to_vec(&receipt).unwrap());}
  }
 }
 #[test]
 fn verification_source_rejects_unicode_paths_and_directory_permission_churn(){
  for name in ["000000é.gcm","0000000é.gcm"]{
   let(_service,source,workspace,mut receipt)=fixture();write_private_new(&source.join("files"),name,b"extra",4096).unwrap();assert!(SourceSnapshot::capture(&source).is_err());
   let bytes=serde_json::to_vec(&serde_json::json!({"schemaVersion":"1.0.0-draft.1","kind":"service-backup","id":"c30e227b-e771-43dc-b315-023e0469d4d2","files":[{"path":format!("files/{name}")}]})).unwrap();fs::write(workspace.join("plaintext/manifest.json"),&bytes).unwrap();receipt.authenticated_manifest_sha256=digest(&bytes);assert!(authenticated_manifest(&workspace,&receipt).is_err());
  }
  for child in ["","files"]{let(_service,source,workspace,receipt)=fixture();let snapshot=SourceSnapshot::capture(&source).unwrap();fs::set_permissions(source.join(child),fs::Permissions::from_mode(0o755)).unwrap();assert!(snapshot.preserve(&workspace,&receipt).is_err());assert!(!workspace.join("source.json").exists());assert!(!workspace.join("receipt.json").exists());}
 }
 #[test]
 fn verification_source_refuses_capture_churn_or_failed_metadata_custody(){
  let(_service,source,workspace,receipt)=fixture();let snapshot=SourceSnapshot::capture(&source).unwrap();write_private_new(&source.join("files"),"00000001.gcm",b"extra",4096).unwrap();assert!(snapshot.preserve(&workspace,&receipt).is_err());assert!(!workspace.join("source.json").exists());assert!(!workspace.join("receipt.json").exists());
 }
 fn finder(source:&Path){let path=source.join(".DS_Store");fs::write(&path,b"synthetic Finder metadata").unwrap();fs::set_permissions(path,fs::Permissions::from_mode(0o644)).unwrap();}
 #[test]
 fn finder_metadata_is_optional_transient_and_not_authenticated_source_content(){
  for present in [false,true]{
   let(service,source,workspace,receipt)=fixture();if present{finder(&source);}
   let snapshot=SourceSnapshot::capture(&source).unwrap();assert_eq!(snapshot.finder.is_some(),present);assert_eq!(snapshot.pins.len(),4);
   snapshot.preserve(&workspace,&receipt).unwrap();write_json(&workspace,"receipt.json",&receipt).unwrap();
   let bytes=fs::read(workspace.join("source.json")).unwrap();assert!(!String::from_utf8(bytes).unwrap().contains(".DS_Store"));
   // Later normal Finder changes do not change the authenticated durable link.
   if present{fs::write(source.join(".DS_Store"),b"later Finder metadata").unwrap();}else{finder(&source);}
   assert!(service.verification_source(&receipt.id).is_ok());
   fs::remove_file(source.join(".DS_Store")).unwrap();assert!(service.verification_source(&receipt.id).is_ok());
  }
 }
 #[test]
 fn finder_metadata_never_allows_other_names_unsafe_types_or_permissions(){
  for mode in ["unknown","unknown-with-finder","nested","symlink","hardlink","fifo","oversize","directory","executable","group-write","other-write","special"]{
   let(_service,source,_workspace,_receipt)=fixture();let path=source.join(".DS_Store");
   match mode{
    "unknown"|"unknown-with-finder"=>{if mode=="unknown-with-finder"{finder(&source);}write_private_new(&source,".localized",b"unknown",4096).unwrap();},
    "nested"=>finder(&source.join("files")),
    "symlink"=>symlink(source.join("manifest.gcm"),&path).unwrap(),
    "hardlink"=>fs::hard_link(source.join("manifest.gcm"),&path).unwrap(),
    "fifo"=>{let name=std::ffi::CString::new(path.to_str().unwrap()).unwrap();assert_eq!(unsafe{libc::mkfifo(name.as_ptr(),0o600)},0);},
    "directory"=>fs::create_dir(&path).unwrap(),
    "oversize"=>{finder(&source);OpenOptions::new().write(true).open(&path).unwrap().set_len(1024*1024+1).unwrap();},
    _=>{finder(&source);let permissions=match mode{"executable"=>0o744,"group-write"=>0o664,"other-write"=>0o646,_=>0o1644};fs::set_permissions(&path,fs::Permissions::from_mode(permissions)).unwrap();},
   }
   assert!(SourceSnapshot::capture(&source).is_err(),"{mode}");
  }
 }
 #[test]
 fn finder_metadata_churn_during_observation_refuses_without_publishing_source(){
  for mode in ["appearance","disappearance","mutation","replacement","permissions"]{
   let(_service,source,workspace,receipt)=fixture();if mode!="appearance"{finder(&source);}let snapshot=SourceSnapshot::capture(&source).unwrap();let path=source.join(".DS_Store");
   match mode{
    "appearance"=>finder(&source),
    "disappearance"=>fs::remove_file(&path).unwrap(),
    "mutation"=>fs::write(&path,b"changed Finder metadata").unwrap(),
    "replacement"=>{fs::rename(&path,source.with_extension("retained-finder")).unwrap();finder(&source);},
    _=>fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap(),
   }
   assert!(snapshot.preserve(&workspace,&receipt).is_err(),"{mode}");assert!(!workspace.join("source.json").exists());
  }
 }
}
