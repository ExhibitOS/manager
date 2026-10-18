// SPDX-License-Identifier: Apache-2.0
//! Isolated DB checks from authenticated external archives, never provider jobs or activation.
use super::*;
use super::hosted_restoration::{Receipt,RestoreHelperSpec,validate_restore_helper,observed_desktop_mapping};
use super::installation_backup::source_bytes;
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExternalCheckJob{pub id:String,pub verification_id:String,pub image:String,pub expected_backup_id:String,pub expected_manifest_sha256:String,pub expected_files:u64,pub job_budget_bytes:u64,pub state:String,pub stage:String,pub error_code:Option<String>,pub receipt:Option<Receipt>,pub created_at:u64,pub updated_at:u64}
impl ExternalCheckJob{
 pub(crate) fn validate(&self)->Result<()>{
  if !uuid(&self.id)||!uuid(&self.verification_id)||self.id==self.verification_id||!uuid(&self.expected_backup_id)||!hash_valid(&self.expected_manifest_sha256)||!self.image.strip_prefix("sha256:").is_some_and(hash_valid)||self.expected_files==0||self.expected_files>100000||!(4194304..=32*1024*1024*1024).contains(&self.job_budget_bytes)||!["running","completed","failed","interrupted"].contains(&self.state.as_str())||self.stage!=if self.state=="running"{"checking-archive-and-restoring"}else{&self.state}||self.created_at>self.updated_at||self.updated_at>8_640_000_000_000_000||(["failed","interrupted"].contains(&self.state.as_str())!=self.error_code.is_some())||self.error_code.as_ref().is_some_and(|v|v.is_empty()||v.len()>128||!v.bytes().all(|b|b.is_ascii_uppercase()||b==b'_'))||((self.state=="completed")!=self.receipt.is_some()){return Err(err("STATE_INVALID"));}
  if let Some(r)=&self.receipt{self.validate_receipt(r)?;}Ok(())
 }
 fn validate_receipt(&self,r:&Receipt)->Result<()>{if r.operation!="restored-and-verified"||r.backup_id!=self.expected_backup_id||r.authenticated_manifest_sha256!=self.expected_manifest_sha256||r.files!=self.expected_files{return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));}Ok(())}
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Execution{pub source_kind:String,pub job_id:String,pub verification_id:String,pub helper_name:String,pub image:String,pub source_path:String,pub work_volume:String,pub docker_path:String,pub docker_endpoint:String,pub job_budget_bytes:u64,pub expected_backup_id:String,pub expected_manifest_sha256:String,pub expected_files:u64}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct ExternalRestoreContext{pub sources:Vec<super::maintenance_source::VerificationSummary>,pub checks:Vec<ExternalCheckJob>}
pub type Context=ExternalRestoreContext;
fn uuid(v:&str)->bool{Uuid::parse_str(v).is_ok_and(|u|u.to_string()==v)}
struct SecretInput(std::io::Cursor<Vec<u8>>);impl Read for SecretInput{fn read(&mut self,b:&mut[u8])->std::io::Result<usize>{self.0.read(b)}}impl Drop for SecretInput{fn drop(&mut self){self.0.get_mut().fill(0);}}
impl LifecycleService{
    fn check_external_restore_work_volume(&self, id: &str) -> Result<()> {
        if !uuid(id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let name = format!("exhibitos-external-restore-work-{id}");
        let value: Value = serde_json::from_slice(&run(
            "docker",
            &["volume".into(), "inspect".into(), name.clone()],
            None,
            30,
        )?)
        .map_err(|_| err("OWNERSHIP_CONFLICT"))?;
        if value.as_array().is_none_or(|v| v.len() != 1)
            || value[0]["Name"] != name
            || value[0]["Driver"] != "local"
            || value[0]["Scope"] != "local"
            || value[0]["Labels"]["com.exhibitos.external-restore"] != id
            || !(value[0]["Options"].is_null()
                || value[0]["Options"]
                    .as_object()
                    .is_some_and(|v| v.is_empty()))
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(())
    }
    pub(crate) fn external_restore_helper_state(&self, job: &ExternalCheckJob) -> Result<String> {
        let name = format!("exhibitos-external-restore-{}", job.id);
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &name,
            "com.exhibitos.external-restore",
            &job.id,
        )?;
        let Some(id) = ids.first() else {
            return Ok("absent".into());
        };
        let value = backup_creation::inspected("docker", &["inspect".into(), id.clone()])?;
        self.check_external_restore_work_volume(&job.id)?;
        validate_restore_helper(
            &RestoreHelperSpec::external(&job.id,&job.image),
            &value,
            &self.verification_source_metadata_unlocked(&job.verification_id)?.source_path,
            id,
            observed_desktop_mapping(&value)?,
        )?;
        Ok(if value["State"]["Running"] == true {
            "running"
        } else if value["State"]["Status"]
            .as_str()
            .is_some_and(|v| ["exited", "dead"].contains(&v))
        {
            "stopped"
        } else {
            "pending"
        }
        .into())
    }
    pub(crate) fn stop_owned_external_restore(&self, id: &str) -> Result<()> {
        let job = self.external_restore_check(id)?;
        let state = self.external_restore_helper_state(&job)?;
        if state == "absent" || state == "stopped" {
            return Ok(());
        }
        if state != "running" {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &format!("exhibitos-external-restore-{id}"),
            "com.exhibitos.external-restore",
            id,
        )?;
        let cid = ids.first().ok_or_else(|| err("CANCEL_UNCERTAIN"))?;
        let observed = backup_creation::inspected("docker", &["inspect".into(), cid.clone()])?;
        validate_restore_helper(
            &RestoreHelperSpec::external(&job.id,&job.image),
            &observed,
            &self.verification_source_metadata_unlocked(&job.verification_id)?.source_path,
            cid,
            observed_desktop_mapping(&observed)?,
        )?;
        run(
            "docker",
            &["stop".into(), "--time".into(), "30".into(), cid.clone()],
            None,
            60,
        )?;
        if self.external_restore_helper_state(&job)? != "stopped" {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub fn create_external_restore_check(
        &self,
        verification_id: &str,
        image: &str,
        node: &Path,
        runner: &Path,
        make_input: impl FnOnce(&Execution) -> Result<Vec<u8>>,
        check_resources: impl Fn() -> Result<()>,
    ) -> Result<ExternalCheckJob> {
        let _lock = self.lock()?;
        let original=self.verification_source_unlocked(verification_id)?;
        if original.image!=image{return Err(err("BACKUP_IMAGE_INVALID"));}
        self.ensure_external_archive_reconciled()?;
        self.ensure_external_helpers_settled()?;
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        for old in self.hosted_backup_jobs()? {
            if self.hosted_helper_state(&old)? == "running" {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        for old in self.hosted_restore_checks()? {
            if ["running", "pending"].contains(&self.hosted_restore_helper_state(&old)?.as_str()) {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        let available = fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?;
        let job_budget_bytes = hosted_backup::job_budget(available)?.min(32 * 1024 * 1024 * 1024);
        let observed: Value = serde_json::from_slice(&run(
            "docker",
            &["image".into(), "inspect".into(), image.into()],
            None,
            30,
        )?)
        .map_err(|_| err("BACKUP_IMAGE_INVALID"))?;
        if observed.as_array().is_none_or(|v| v.len() != 1) || observed[0]["Id"] != image {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("external-restoration-{id}"));
        let _workspace = restoration::directory(&workspace)?;
        let workspace_pin = hosted_backup::output_guard(&workspace)?;
        let source_path=original.source_path.clone();
        let source_pins=[source_path.clone(),source_path.join("files")]
        .into_iter()
        .map(|path| hosted_backup::output_guard(&path).map(|pin| (path, pin)))
        .collect::<Result<Vec<_>>>()?;
        let check_source = || -> Result<()> {
            for (path, pin) in &source_pins {
                super::backup_export::native::check(pin, path)?;
            }
            Ok(())
        };
        let now = now();
        let mut job = ExternalCheckJob {
            id: id.clone(),
            verification_id: verification_id.into(),
            image: image.into(),
            expected_backup_id: original.backup_id.clone(),
            expected_manifest_sha256: original.authenticated_manifest_sha256.clone(),
            expected_files: original.files,
            job_budget_bytes,
            state: "running".into(),
            stage: "checking-archive-and-restoring".into(),
            error_code: None,
            receipt: None,
            created_at: now,
            updated_at: now,
        };
        self.save_external_restore_check(&job)?;
        if let Err(error) = self.begin_maintenance("external-restoration", &id, &job.stage) {
            job.state = "failed".into();
            job.stage = "failed".into();
            job.error_code = Some(error.code.clone());
            job.updated_at = super::now();
            self.save_external_restore_check(&job)?;
            return Err(error);
        }
        let outcome = (|| {
            let mut command = std::process::Command::new("docker");
            local_network::pin_command(&mut command)?;
            let endpoint = command
                .get_envs()
                .find(|(k, _)| *k == "DOCKER_HOST")
                .and_then(|(_, v)| v)
                .and_then(|v| v.to_str())
                .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?
                .to_owned();
            let docker = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
            let plan = Execution {
                source_kind:"external-archive".into(),
                job_id: id.clone(),
                verification_id: verification_id.into(),
                helper_name: format!("exhibitos-external-restore-{id}"),
                image: image.into(),
                source_path: source_path
                    .to_str()
                    .ok_or_else(|| err("BACKUP_PATH_INVALID"))?
                    .into(),
                work_volume: format!("exhibitos-external-restore-work-{id}"),
                docker_path: docker
                    .to_str()
                    .ok_or_else(|| err("RUNTIME_MISSING"))?
                    .into(),
                docker_endpoint: endpoint,
                job_budget_bytes,
                expected_backup_id: job.expected_backup_id.clone(),
                expected_manifest_sha256: job.expected_manifest_sha256.clone(),
                expected_files: job.expected_files,
            };
            let existing = run(
                "docker",
                &[
                    "volume".into(),
                    "ls".into(),
                    "--filter".into(),
                    format!("name=^{}$", plan.work_volume),
                    "--format".into(),
                    "{{.Name}}".into(),
                ],
                None,
                30,
            )?;
            if !existing.iter().all(u8::is_ascii_whitespace) {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            run(
                "docker",
                &[
                    "volume".into(),
                    "create".into(),
                    "--label".into(),
                    format!("com.exhibitos.external-restore={id}"),
                    plan.work_volume.clone(),
                ],
                None,
                30,
            )?;
            self.check_external_restore_work_volume(&id)?;
            let input = make_input(&plan)?;
            if input.len() > 131072 {
                return Err(err("ENGINE_INPUT_LIMIT"));
            }
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            check_source()?;
            let mut next = Instant::now();
            let bytes = run_observed_stream_environment(
                node.to_str()
                    .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?,
                &[
                    "--disable-proto=throw".into(),
                    "--no-warnings".into(),
                    runner
                        .to_str()
                        .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?
                        .into(),
                ],
                None,
                4200,
                (None, None),
                Some((Box::new(SecretInput(std::io::Cursor::new(input))), 131072)),
                true,
                || {
                    if Instant::now() >= next {
                        next = Instant::now() + Duration::from_millis(400);
                        check_resources()?;
                        super::backup_export::native::check(&workspace_pin, &workspace)?;
                        check_source()?;
                        if self.maintenance_requested("external-restoration", &id)?
                            && ["running", "stopped"]
                                .contains(&self.external_restore_helper_state(&job)?.as_str())
                        {
                            self.maintenance_checkpoint("external-restoration", &id)?;
                        }
                    }
                    Ok(())
                },
            )?;
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            check_source()?;
            self.maintenance_checkpoint("external-restoration", &id)?;
            if bytes.len() > 16384 {
                return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
            }
            let receipt: Receipt = serde_json::from_slice(&bytes)
                .map_err(|_| err("HOSTED_RECOVERY_EXECUTION_FAILED"))?;
            job.validate_receipt(&receipt)?;
            if self.external_restore_helper_state(&job)? != "stopped" {
                return Err(err("HOSTED_RECOVERY_HELPER_UNCERTAIN"));
            }
            let final_helper = backup_creation::inspected(
                "docker",
                &["inspect".into(), plan.helper_name.clone()],
            )?;
            let final_id = final_helper["Id"]
                .as_str()
                .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
            validate_restore_helper(
                &RestoreHelperSpec::external(&job.id,&job.image),
                &final_helper,
                &source_path,
                final_id,
                observed_desktop_mapping(&final_helper)?,
            )?;
            if final_helper["State"]["ExitCode"] != 0 || final_helper["State"]["OOMKilled"] != false
            {
                return Err(err("HOSTED_RECOVERY_EXECUTION_FAILED"));
            }
            self.check_external_restore_work_volume(&id)?;
            let final_source=self.verification_source_unlocked(verification_id)?;
            if final_source!=original{return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));}
            Ok(receipt)
        })();
        let _finish = self.maintenance_finish_guard("external-restoration", &id)?;
        match outcome {
            Ok(receipt) => {
                job.state = "completed".into();
                job.stage = "completed".into();
                job.receipt = Some(receipt);
                self.finish_maintenance("completed", None)?;
            }
            Err(error) => {
                job.state = if error.code == "CANCELLED" {
                    "interrupted"
                } else {
                    "failed"
                }
                .into();
                job.stage = job.state.clone();
                job.error_code = Some(error.code.clone());
                let terminal = match error.code.as_str() {
                    "CANCELLED" => "confirmed",
                    "CANCEL_UNCERTAIN" => "uncertain",
                    _ => "failed",
                };
                self.finish_maintenance(terminal, Some(&error.code))?;
            }
        }
        job.updated_at = super::now();
        self.save_external_restore_check(&job)?;
        Ok(job)
    }

 pub(crate) fn external_restore_check(&self,id:&str)->Result<ExternalCheckJob>{if !uuid(id){return Err(err("HOSTED_RECOVERY_REQUEST"));}let job:ExternalCheckJob=serde_json::from_slice(&source_bytes(&self.root,&format!("external-restoration-{id}/job.json"),16384,true)?).map_err(|_|err("STATE_INVALID"))?;job.validate()?;if job.id!=id{return Err(err("STATE_INVALID"));}Ok(job)}
 pub fn external_restore_checks(&self)->Result<Vec<ExternalCheckJob>>{let mut out=Vec::new();for entry in fs::read_dir(&self.root).map_err(|_|err("STATE_UNAVAILABLE"))?{let name=entry.map_err(|_|err("STATE_UNAVAILABLE"))?.file_name();let Some(id)=name.to_str().and_then(|n|n.strip_prefix("external-restoration-")).filter(|id|uuid(id))else{continue};if out.len()>=1000{return Err(err("JOB_HISTORY_FULL"));}out.push(self.external_restore_check(id)?);}out.sort_by_key(|j|j.created_at);Ok(out)}
 fn save_external_restore_check(&self,job:&ExternalCheckJob)->Result<()>{job.validate()?;write_json(&self.root.join(format!("external-restoration-{}",job.id)),"job.json",job)}
 pub(crate) fn recover_external_restore_checks(&self)->Result<()>{let _lock=match self.lock(){Ok(v)=>v,Err(e)if e.code=="BUSY"=>return Ok(()),Err(e)=>return Err(e)};for mut job in self.external_restore_checks()?{if job.state=="running"{job.state="interrupted".into();job.stage="interrupted".into();job.error_code=Some("HOSTED_RECOVERY_INTERRUPTED".into());job.updated_at=now();self.save_external_restore_check(&job)?;}}Ok(())}
 pub(crate) fn ensure_external_helpers_settled(&self)->Result<()>{for job in self.external_restore_checks()?{if ["running","pending"].contains(&self.external_restore_helper_state(&job)?.as_str()){return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));}}Ok(())}
 pub fn prepare_external_archive_tool(&self,resources:&Path,image:&str)->Result<()>{
  let _lock=self.lock()?;
  if self.bundled_maintenance_image(resources)?.as_deref()!=Some(image){return Err(err("BACKUP_VERIFICATION_TOOL_CHANGED"));}
  match fs::symlink_metadata(self.root.join("installed.json")){
   Ok(_)=>{let manifest=self.manifest()?;let installed:BundleManifest=read_json(&self.root.join("installed.json"))?;if serde_json::to_value(installed).ok()!=serde_json::to_value(&manifest).ok(){return Err(err("BUNDLE_CHANGED"));}self.engine(&manifest,false)?;},
   Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err(err("STATE_UNAVAILABLE"))
  }
  // This isolated checker uses Docker independently; an installed Podman pin is
  // preserved and is never evidence that this exact helper is loaded in Docker.
  choose_engine(&self.detect()?,Some("docker"))?;runtime_prepare::ensure_maintenance(resources,image,|a,t|run("docker",a,None,t))
 }

 pub(crate) fn ensure_external_archive_reconciled(&self)->Result<()>{
  let history=self.reconciliation_history()?;
  for job in self.external_restore_checks()?{
   if job.state=="running"{return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));}
   if ["failed","interrupted"].contains(&job.state.as_str())&&!history.iter().any(|h|h.kind=="external-restoration"&&h.target_id==job.id&&h.state=="completed"&&h.updated_at>=job.updated_at&&h.helper_state.as_deref().is_some_and(|s|["absent","stopped"].contains(&s))){return Err(err("CANCEL_RECOVERY_REQUIRED"));}
  }Ok(())
 }

}

#[cfg(test)]
pub(crate) fn fixture_job()->ExternalCheckJob{ExternalCheckJob{id:Uuid::new_v4().to_string(),verification_id:Uuid::new_v4().to_string(),image:format!("sha256:{}","a".repeat(64)),expected_backup_id:Uuid::new_v4().to_string(),expected_manifest_sha256:"b".repeat(64),expected_files:2,job_budget_bytes:64*1024*1024,state:"running".into(),stage:"checking-archive-and-restoring".into(),error_code:None,receipt:None,created_at:1,updated_at:1}}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn external_job_has_no_provider_or_source_job_and_completion_requires_exact_receipt(){
  let mut job=fixture_job();job.validate().unwrap();let json=serde_json::to_value(&job).unwrap();assert_eq!(json.as_object().unwrap().len(),13);assert!(json.get("source").is_none());assert!(json.get("sourceJobId").is_none());
  for field in ["source","sourceJobId","keyPath"]{let mut value=json.clone();value[field]="forbidden".into();assert!(serde_json::from_value::<ExternalCheckJob>(value).is_err());}
  job.state="completed".into();job.stage="completed".into();assert!(job.validate().is_err());job.receipt=Some(Receipt{operation:"restored-and-verified".into(),backup_id:job.expected_backup_id.clone(),authenticated_manifest_sha256:job.expected_manifest_sha256.clone(),files:job.expected_files});job.validate().unwrap();job.receipt.as_mut().unwrap().files+=1;assert!(job.validate().is_err());
 }
 #[test]
 fn external_execution_is_distinct_and_uses_no_hosted_source_identity(){
  let job=fixture_job();let plan=Execution{source_kind:"external-archive".into(),job_id:job.id.clone(),verification_id:job.verification_id,helper_name:format!("exhibitos-external-restore-{}",job.id),image:job.image,source_path:"/private/synthetic/archive".into(),work_volume:format!("exhibitos-external-restore-work-{}",job.id),docker_path:"/fixed/docker".into(),docker_endpoint:"unix:///fixed/docker.sock".into(),job_budget_bytes:job.job_budget_bytes,expected_backup_id:job.expected_backup_id,expected_manifest_sha256:job.expected_manifest_sha256,expected_files:job.expected_files};let value=serde_json::to_value(plan).unwrap();assert_eq!(value.as_object().unwrap().len(),13);assert_eq!(value["sourceKind"],"external-archive");assert!(value.get("sourceJobId").is_none());
 }
 #[test]
 fn relaunch_external_job_is_interrupted_and_requires_explicit_reconciliation(){
  let root=std::env::temp_dir().join(format!("external-recover-{}",Uuid::new_v4()));let service=LifecycleService::new(root.clone()).unwrap();let job=fixture_job();let workspace=root.join(format!("external-restoration-{}",job.id));restoration::directory(&workspace).unwrap();service.save_external_restore_check(&job).unwrap();service.begin_maintenance("external-restoration",&job.id,&job.stage).unwrap();let marker=workspace.join("candidate");write_private_new(&workspace,"candidate",b"keep",1024).unwrap();drop(service);
  let reopened=LifecycleService::new(root).unwrap();let interrupted=reopened.external_restore_checks().unwrap().pop().unwrap();assert_eq!(interrupted.state,"interrupted");assert_eq!(reopened.maintenance_context().unwrap().unwrap().state,"interrupted");assert_eq!(fs::read(marker).unwrap(),b"keep");assert_eq!(reopened.ensure_external_archive_reconciled().unwrap_err().code,"CANCEL_RECOVERY_REQUIRED");reopened.reconciliation_target("external-restoration",&job.id).unwrap();
 }
 #[test]
 fn external_cancel_requires_live_owner_and_keeps_candidate(){
  let root=std::env::temp_dir().join(format!("external-cancel-{}",Uuid::new_v4()));let service=LifecycleService::new(root.clone()).unwrap();let job=fixture_job();let workspace=root.join(format!("external-restoration-{}",job.id));restoration::directory(&workspace).unwrap();service.save_external_restore_check(&job).unwrap();service.begin_maintenance("external-restoration",&job.id,&job.stage).unwrap();assert!(service.request_maintenance_cancel("external-restoration",&job.id,true).is_err());let _owner=service.lock().unwrap();assert_eq!(service.request_maintenance_cancel("external-restoration",&job.id,false).unwrap_err().code,"CANCEL_ACK_REQUIRED");assert_eq!(service.request_maintenance_cancel("external-restoration",&job.id,true).unwrap().state,"requested");assert!(workspace.join("job.json").exists());
 }
}
