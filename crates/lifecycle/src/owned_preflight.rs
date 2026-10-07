// SPDX-License-Identifier: Apache-2.0
//! Admission derived from actual current recovery, with no returned-receipt replay.
use super::*;
#[path = "candidate_execution.rs"]
mod candidate_execution;
use crate::signed_release::trust::authority_recovery_vault::InactiveAuthorityProof;
pub use candidate_execution::ReadyCandidate;

/// A consuming permit borrowing the original Store/profile session and staged
/// artifact, and owning the EXACT operation locks used by runtime qualification.
/// No Clone, Deserialize, public constructor or caller-supplied verification flags.
///
/// Saved JSON cannot recreate this permit:
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::OwnedPreflight;
/// let permit = serde_json::from_str::<OwnedPreflight<'_, '_, '_>>("{}");
/// ```
/// Admission consumes it; even a still-live executor cannot replay admission:
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::OwnedPreflight;
/// fn replay(permit: OwnedPreflight<'_, '_, '_>) {
///     let started = permit.begin();
///     let replay = permit.begin();
/// }
/// ```
pub struct OwnedPreflight<'session, 'store, 'inputs> {
    session: &'session mut ExecutionSession<'store>,
    artifact: &'session mut PreparedArtifact,
    inputs: &'inputs RecoveryRuntimeInputs<'inputs>,
    destination: &'inputs Path,
    key_file: &'inputs Path,
    checkpoint: VerifiedCheckpointPair,
    host: crate::profile_backup::HostReceipt,
    host_path: PathBuf,
    host_identity: Metadata,
    oci: PreparedOciReceipt,
    authority: InactiveAuthorityProof,
    migration: Option<std::cell::RefCell<super::super::migration_catalog::CatalogInput>>,
    lease: candidate_inventory::RetainedCandidateLease,
    evidence: crate::update::Preflight,
    receipt: Value,
}
/// Applying has been durably journaled. This retains the admission's fences;
/// it does not claim that a runtime update, activation or health check occurred.
/// Dropping without an executor leaves restart recovery required, never success.
pub struct StartedUpdate<'session, 'store, 'inputs> {
    admission: OwnedPreflight<'session, 'store, 'inputs>,
}
impl StartedUpdate<'_, '_, '_> {
    pub fn receipt(&self) -> &Value {
        &self.admission.receipt
    }
}
impl<'store> ExecutionSession<'store> {
    /// Restores the complete host/trust and current independent authority to one
    /// NEW inactive destination, using the existing cipher/key and candidate.
    /// Three temporary image exports retire after the existing actual byte checks;
    /// no fresh complete host archive or persistent database copy is produced.
    /// External writers must remain quiesced until the consuming permit is dropped.
    pub fn prepare_owned_update<'session, 'inputs>(
        &'session mut self,
        artifact: &'session mut PreparedArtifact,
        inputs: &'inputs RecoveryRuntimeInputs<'inputs>,
        destination: &'inputs Path,
        key_file: &'inputs Path,
    ) -> Result<OwnedPreflight<'session, 'store, 'inputs>> {
        self.check()?;
        self.require_selected_source()?;
        // Missing independent recovery fails BEFORE extraction, OCI or Engine work.
        self.store
            .require_authority_recovery()
            .map_err(|e| err(e.code()))?;
        self.reverify_prepared_artifact(artifact)?;
        let ((), receipt, lease, host, oci) = self.with_recovery_runtime_impl(
            artifact,
            inputs,
            Some((destination, key_file)),
            |_| Ok(()),
        )?;
        let host = host.ok_or_else(|| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?;
        self.finish_owned_permit(artifact,inputs,destination,key_file,destination.join("host"),host,receipt,lease,oci,None)
    }
    /// Reuse a complete inactive host extraction only after current archive/profile
    /// and exact extracted bytes/modes are reobserved. Fresh trust/authority records
    /// remain current-generation. This permit cannot begin changed-schema execution
    /// until the whole migrated activation/failure recovery gate is qualified.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_owned_migrated_update_reusing_host<'session,'inputs>(
        &'session mut self, artifact: &'session mut PreparedArtifact,
        inputs: &'inputs RecoveryRuntimeInputs<'inputs>, migration: &MigrationRuntimeInputs<'_>,
        retained_host: &Path, destination: &'inputs Path, key_file: &'inputs Path,
    ) -> Result<OwnedPreflight<'session,'store,'inputs>> {
        self.check()?; self.require_selected_source()?;
        self.store.require_authority_recovery().map_err(|e|err(e.code()))?;
        self.reverify_prepared_artifact(artifact)?;
        let observed=self.with_recovery_runtime_retained(
            artifact,inputs,Some((destination,key_file)),Some(migration),Some(retained_host),|_|Ok(()))?;
        let host=observed.host.ok_or_else(||err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?;
        let catalog=observed.catalog.ok_or_else(||err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"))?;
        self.finish_owned_permit(artifact,inputs,destination,key_file,retained_host.to_owned(),host,observed.receipt,observed.lease,observed.oci,Some(catalog))
    }
    #[allow(clippy::too_many_arguments)]
    fn finish_owned_permit<'session,'inputs>(
        &'session mut self, artifact: &'session mut PreparedArtifact,
        inputs: &'inputs RecoveryRuntimeInputs<'inputs>, destination: &'inputs Path,key_file: &'inputs Path,
        host_path: PathBuf,host: crate::profile_backup::HostReceipt,mut receipt: Value,
        lease: candidate_inventory::RetainedCandidateLease,oci: PreparedOciReceipt,
        migration: Option<super::super::migration_catalog::CatalogInput>,
    )->Result<OwnedPreflight<'session,'store,'inputs>> {
        let cp = &inputs.checkpoint;
        let checkpoint = self
            .store
            .verify_checkpoint_pair(cp.binding, cp.host, cp.trust, cp.key)?;
        current_bound_checkpoint(&checkpoint.receipt(), artifact.generation)?;
        let authority = self
            .store
            .restore_inactive_authority(&destination.join("authority"))
            .map_err(|e| err(e.code()))?;
        let evidence = crate::update::Preflight {
            plan: artifact.plan.clone(),
            signature_verified: false,
            artifact_verified: false,
            compatibility_verified: true,
            backup_restore_verified: true,
            current_source_matches_backup: true,
            available_free_bytes: 0,
            // Even unchanged schema does NOT authorize image-only rollback.
            image_only_rollback_verified: false,
        };
        receipt["currentAuthorityInactiveRestorationVerified"] = serde_json::json!(true);
        receipt["preflightVerified"] = serde_json::json!(true);
        receipt["executionStarted"] = serde_json::json!(false);
        let mut permit = OwnedPreflight {
            session: self,
            artifact,
            inputs,
            destination,
            key_file,
            checkpoint,
            host_identity: fs::symlink_metadata(&host_path).map_err(|_|err("HOST_CHECKPOINT_INVALID"))?,
            host_path,
            host,
            oci,
            authority,
            migration: migration.map(std::cell::RefCell::new),
            lease,
            evidence,
            receipt,
        };
        permit.recheck()?;
        Ok(permit)
    }
}
impl<'session, 'store, 'inputs> OwnedPreflight<'session, 'store, 'inputs> {
    pub fn receipt(&self) -> &Value {
        &self.receipt
    }
    fn recheck(&mut self) -> Result<()> {
        self.session.check()?;
        self.session.require_selected_source()?;
        self.session.reverify_prepared_artifact(self.artifact)?;
        if let Some(catalog) = &self.migration {
            catalog.borrow_mut().recheck_for_schema(&self.artifact.plan.target_schema)?;
        }
        self.lease.check(&self.session.source)?;
        let cp = &self.inputs.checkpoint;
        self.session.store.recheck_checkpoint_pair(
            &self.checkpoint,
            cp.binding,
            cp.host,
            cp.trust,
            cp.key,
        )?;
        current_bound_checkpoint(&self.checkpoint.receipt(), self.artifact.generation)?;
        crate::profile_backup::verify_host_current_borrowed(
            &self.session.store.profile,
            cp.host,
            cp.key,
            &self.session._session,
            &self.checkpoint.receipt().host_manifest_sha256,
        )?;
        if !identity(&self.host_identity,&fs::symlink_metadata(&self.host_path).map_err(|_|err("HOST_CHECKPOINT_INVALID"))?) {
            return Err(err("HOST_CHECKPOINT_INVALID"));
        }
        crate::profile_backup::verify_extracted_host(
            &self.session.store.profile,
            &self.host_path,
            &self.host,
        )?;
        self.session
            .store
            .verify_trust_checkpoint(&self.destination.join("trust"))
            .map_err(|e| err(e.code()))?;
        self.session
            .store
            .recheck_inactive_authority(&self.authority)
            .map_err(|e| err(e.code()))?;
        if super::super::read_record(self.key_file)
            .map_err(|_| err("PROFILE_KEY_INVALID"))?
            .as_slice()
            != cp.key
        {
            return Err(err("PROFILE_KEY_INVALID"));
        }
        let available = [
            self.session.store.profile.as_path(),
            self.lease.root(),
            self.destination,
        ]
        .into_iter()
        .map(|p| fs2::available_space(p).map_err(|_| err("STORAGE_UNAVAILABLE")))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .ok_or_else(|| err("STORAGE_UNAVAILABLE"))?;
        if available
            < self
                .artifact
                .plan
                .required_free_bytes
                .checked_add(6 * 1024 * 1024 * 1024)
                .ok_or_else(|| err("RESTORE_SPACE_REQUIRED"))?
        {
            return Err(err("RESTORE_SPACE_REQUIRED"));
        }
        self.evidence.available_free_bytes = available;
        self.receipt["availableFreeBytes"] = serde_json::json!(available);
        self.lease.check(&self.session.source)?;
        self.session.check()
    }
    /// Consume the non-replayable permit. No runtime mutation precedes the durable
    /// Applying record; the same Store, staged artifact and source/target locks
    /// transfer into the future executor without an unlock/relock interval.
    pub fn begin(self) -> Result<StartedUpdate<'session, 'store, 'inputs>> {
        require_executable_schema(&self.artifact.plan)?;
        self.begin_journaled()
    }
    // Shared implementation for production admission and the explicit native
    // qualification test. Public changed-schema admission stays CLOSED until
    // the full real apply/failure/recovery matrix is qualified. No feature flag,
    // CLI switch, serialized permit or caller-supplied verification can open it.
    fn begin_journaled(mut self) -> Result<StartedUpdate<'session, 'store, 'inputs>> {
        self.recheck()?;
        // Time is observed after all potentially slow full-corpus rechecks.
        self.session.reverify_prepared_artifact(self.artifact)?;
        let started = self
            .session
            .store
            .begin_update(
                self.evidence.clone(),
                &self.artifact.verified,
                super::super::release_now()?,
            )
            .map_err(|e| err(e.code()))?;
        self.receipt["executionStarted"] = serde_json::json!(true);
        self.receipt["applyingTrustGeneration"] = serde_json::json!(started.generation);
        // updateExecuted, liveAuthorityRestored and hostActivated remain false.
        // Keep the checkpoint/key borrows and inactive full recovery proofs too;
        // the future executor cannot substitute caller-selected recovery paths.
        Ok(StartedUpdate { admission: self })
    }
}

fn require_executable_schema(plan: &crate::update::Plan) -> Result<()> {
    if plan.source_schema!=plan.target_schema {return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));}
    Ok(())
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn changed_schema_permit_cannot_begin_before_execution_recovery_gate() {
  let mut p:crate::update::Plan=serde_json::from_value(serde_json::json!({"operationId":"update-1","sourceInstance":"source-1","targetInstance":"target-1","backupId":"backup-1","backupManifest":"a".repeat(64),"sourceInventory":"b".repeat(64),"sourceImage":"c".repeat(64),"targetImage":"d".repeat(64),"sourceSchema":"e".repeat(64),"targetSchema":"f".repeat(64),"requiredFreeBytes":1})).unwrap();
  assert_eq!(require_executable_schema(&p).unwrap_err().code,"UPDATE_RUNTIME_MIGRATION_UNQUALIFIED");
  p.target_schema=p.source_schema.clone();require_executable_schema(&p).unwrap();
 }
 #[test]
 #[ignore="explicit current signed synthetic fixture; reuse complete retained host, refresh small trust/authority, observe actual Runtime; no execution/activation"]
 fn actual_owned_migrated_permit_reuses_full_host_and_refuses_execution() {
  let input:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_MIGRATED_PERMIT_INPUT").unwrap()).unwrap()).unwrap();
  let path=|n:&str|PathBuf::from(input[n].as_str().unwrap());
  let root=path("root");let profile=root.join("profile");let mut store=crate::signed_release::trust::Store::open(&profile,"default").unwrap();
  let generation=store.current.generation;let plan=store.intent().unwrap().update.plan().clone();assert_ne!(plan.source_schema,plan.target_schema);
  assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::Prepared);
  let key_file=path("key");let key: [u8;32]=super::super::super::super::read_record(&key_file).unwrap().try_into().unwrap();
  let binding=path("binding");let host=path("hostArchive");let trust=path("trustArchive");let python=path("python");let catalog=path("catalog");let retained_host=path("retainedHost");
  let export=root.join(format!("permit-exports-{}",uuid::Uuid::new_v4()));installations::new_directory(&export).unwrap();
  let destination=root.join(format!("permit-recovery-{}",uuid::Uuid::new_v4()));
  let staging=root.join(format!("permit-staging-{}",uuid::Uuid::new_v4()));installations::new_directory(&staging).unwrap();
  let inputs=RecoveryRuntimeInputs{checkpoint:CheckpointInputs{binding:&binding,host:&host,trust:&trust,key:&key},export_parent:&export,python:&python,source_commit:input["sourceCommit"].as_str().unwrap(),maintenance_image:input["maintenance"].as_str().unwrap(),external_writers_quiesced:true};
  let migration=MigrationRuntimeInputs{catalog:&catalog,catalog_sha256:input["catalogSha256"].as_str().unwrap()};
  let mut session=store.execution().unwrap();let mut artifact=session.stage_prepared_artifact(&path("artifact"),&staging).unwrap();
  let permit=session.prepare_owned_migrated_update_reusing_host(&mut artifact,&inputs,&migration,&retained_host,&destination,&key_file).unwrap();
  assert!(permit.migration.is_some());assert_eq!(permit.host_path,retained_host);
  let receipt=permit.receipt().clone();assert_eq!(receipt["inactiveFullRecovery"]["retainedHostReused"],true);assert_eq!(receipt["currentAuthorityInactiveRestorationVerified"],true);assert_eq!(receipt["executionStarted"],false);
  let error=match permit.begin(){Ok(_)=>panic!("unqualified changed execution began"),Err(e)=>e};assert_eq!(error.code,"UPDATE_RUNTIME_MIGRATION_UNQUALIFIED");
  drop(artifact);drop(session);assert_eq!(store.current.generation,generation);assert_eq!(store.intent().unwrap().update.plan(),&plan);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::Prepared);
  assert_eq!(installations::load(&profile).unwrap().unwrap().0.active_id,plan.source_instance);assert!(!destination.join("host").exists());
  let report=serde_json::json!({"state":"PASS","scope":"actual owned migrated permit with retained complete host; begin refused; not whole Applying/activation/recovery","receipt":receipt,"destination":destination,"staging":staging,"retainedHost":retained_host,"originalAuthorityGeneration":generation,"plan":plan,"preparedIntentUnchanged":true,"selectionUnchanged":true,"newWholeHostCopy":false,"newServiceVolume":false,"executionStarted":false,"updateExecuted":false,"beginRefusal":error.code});
  crate::restoration::private_bytes(&root.join(format!("permit-report-{}.json",uuid::Uuid::new_v4())),&serde_json::to_vec_pretty(&report).unwrap()).unwrap();
 }

 #[test]
 #[ignore="explicit small synthetic signed fixture; real changed Applying/runtime/failed activation/fresh encrypted original service restoration; public changed execution remains closed"]
 fn actual_changed_application_failure_and_fresh_original_restoration() {
  use std::os::unix::fs::PermissionsExt;
  let input:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_MIGRATED_PERMIT_INPUT").unwrap()).unwrap()).unwrap();
  let path=|n:&str|PathBuf::from(input[n].as_str().unwrap());
  let root=fs::canonicalize(path("root")).unwrap();
  // This exact previously qualified development fixture is separate from the
  // rejected original current16 execution. Refuse other roots/policies/stages.
  assert_eq!(root.file_name().unwrap(),"exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a");
  let profile=root.join("profile");let mut store=crate::signed_release::trust::Store::open(&profile,"default").unwrap();
  let signing=ed25519_dalek::SigningKey::from_bytes(&[31;32]);
  let public=signing.verifying_key().to_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>();
  assert_eq!(store.current.policy.public_keys,vec![public]);
  let intent=store.intent().unwrap();assert_eq!(intent.update.stage(),crate::update::Stage::Prepared);
  let envelope:crate::signed_release::Envelope=serde_json::from_str(&intent.envelope).unwrap();
  let release:crate::signed_release::Release=serde_json::from_str(&envelope.payload).unwrap();
  assert_eq!(release.channel,"development");assert_eq!(release.target,"linux-arm64");
  let plan=intent.update.plan().clone();assert_ne!(plan.source_schema,plan.target_schema);
  assert_eq!(require_executable_schema(&plan).unwrap_err().code,"UPDATE_RUNTIME_MIGRATION_UNQUALIFIED");
  let original_generation=store.current.generation;let authority=store.root.clone();
  let old_records=(0..=original_generation).filter_map(|g|{let p=authority.join(format!("{g:020}.json"));fs::read(&p).ok().map(|bytes|(p,bytes))}).collect::<Vec<_>>();
  let source=profile.join("local-runtime");
  let names=["installed.json","engine.json","runtime.env","bundle/manifest.json","bundle/compose.yaml"];
  let before=names.iter().map(|n|{let p=source.join(n);(*n,fs::read(&p).unwrap(),fs::metadata(&p).unwrap().permissions().mode())}).collect::<Vec<_>>();
  let key_file=path("key");let key:[u8;32]=super::super::super::super::read_record(&key_file).unwrap().try_into().unwrap();
  let binding=path("binding");let host=path("hostArchive");let trust=path("trustArchive");let python=path("python");let catalog=path("catalog");let retained_host=path("retainedHost");
  let archive=fs::canonicalize(path("serviceArchive")).unwrap();
  let qualification=root.join(format!("changed-full-recovery-{}",uuid::Uuid::new_v4()));installations::new_directory(&qualification).unwrap();
  let export=qualification.join("exports");installations::new_directory(&export).unwrap();
  let staging=qualification.join("staging");installations::new_directory(&staging).unwrap();
  let destination=qualification.join("preflight");
  let inputs=RecoveryRuntimeInputs{checkpoint:CheckpointInputs{binding:&binding,host:&host,trust:&trust,key:&key},export_parent:&export,python:&python,source_commit:input["sourceCommit"].as_str().unwrap(),maintenance_image:input["maintenance"].as_str().unwrap(),external_writers_quiesced:true};
  let migration=MigrationRuntimeInputs{catalog:&catalog,catalog_sha256:input["catalogSha256"].as_str().unwrap()};
  let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Value>{
   let mut session=store.execution()?;let mut artifact=session.stage_prepared_artifact(&path("artifact"),&staging)?;
   let permit=session.prepare_owned_migrated_update_reusing_host(&mut artifact,&inputs,&migration,&retained_host,&destination,&key_file)?;
   assert!(permit.migration.is_some());let admitted=permit.receipt().clone();
   // Real production body: current signed/artifact/checkpoint/authority/fence
   // rechecks and durable Applying. Only the public rollout gate is held closed.
   let started=permit.begin_journaled()?;
   let ready=started.apply_candidate()?;
   let applying_receipt=ready.started_receipt_for_qualification();
   let target=crate::LifecycleService::open_retry_diagnostics(profile.join("installations").join(&plan.target_instance))?;
   let manifest=target.manifest()?;
   target.validate_ownership(&manifest,"docker")?;target.validate_volumes(&manifest,"docker")?;
   let (app,row)=crate::backup_creation::one_container(&target,&manifest,"docker","platform")?;
   if row["Image"]!=format!("sha256:{}",plan.target_image)||row["State"]["Running"]!=true {return Err(err("UPDATE_TARGET_CHANGED"));}
   // Exact owned migrated target, never source/PG/personal containers.
   crate::run("docker",&["kill".into(),"--signal".into(),"KILL".into(),app.clone()],None,30)?;
   let killed=crate::backup_creation::inspected("docker",&["inspect".into(),app])?;
   assert_eq!(killed["State"]["ExitCode"],137);assert_eq!(killed["State"]["OOMKilled"],false);
   let error=match ready.activate(){Ok(_)=>return Err(err("UPDATE_QUALIFICATION_EXPECTED_FAILURE")),Err(e)=>e};
   drop(artifact);drop(session);
   assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RecoveryRequired);
   crate::run("docker",&crate::compose_args(&manifest,&["stop","--timeout","30"]),Some(&target.root.join("bundle")),180)?;
   let reserved=store.register_rollback_candidate(true)?;
   let port=input["recoveryPort"].as_u64().filter(|n|*n>=1024&&*n<=65535).ok_or_else(||err("PORT_INVALID"))? as u16;
   let restored=store.restore_registered_rollback_candidate(inputs.maintenance_image,&key_file,&archive,port,true)?;
   assert_eq!(restored.candidate_id,reserved.candidate_id);assert!(!restored.rollback_completed);
   let completion=store.activate_restored_rollback(inputs.maintenance_image,true)?;
   assert!(completion.selection_completed);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RolledBack);
   let restored_service=crate::LifecycleService::open_retry_diagnostics(reserved.candidate_path.clone())?;
   let restored_manifest=restored_service.manifest()?;
   restored_service.validate_ownership(&restored_manifest,"docker")?;
   crate::run("docker",&crate::compose_args(&restored_manifest,&["stop","--timeout","30"]),Some(&restored_service.root.join("bundle")),180)?;
   Ok(serde_json::json!({"admission":admitted,"actualApplication":applying_receipt,"actualFailureCode":error.code,"targetExit":137,"originalServiceRestoration":restored,"completion":completion,"freshOriginalCandidate":reserved.candidate_id}))
  })).unwrap_or_else(|_|Err(err("UPDATE_QUALIFICATION_ASSERTION_FAILED")));
  // Stop only this plan's two owned candidate namespaces, also on failure.
  let mut stop_results=Vec::new();
  if let Ok(Some((registry,_)))=installations::load(&profile){
   for entry in registry.installations.iter().filter(|e|e.id==plan.target_instance || store.intent().is_some_and(|i|i.update.restore_candidate()==Some(e.id.as_str()))){
    let service=crate::LifecycleService::open_retry_diagnostics(installations::root(&profile,entry)).unwrap();
    if let Ok(m)=service.manifest()
     && service.validate_ownership(&m,"docker").is_ok()&&service.validate_volumes(&m,"docker").is_ok(){
      let result=crate::run("docker",&crate::compose_args(&m,&["stop","--timeout","30"]),Some(&service.root.join("bundle")),180);
      stop_results.push(serde_json::json!({"candidate":entry.id,"stopped":result.is_ok()}));
    }
   }
  }
  for (name,bytes,mode) in before{let p=source.join(name);assert_eq!(fs::read(&p).unwrap(),bytes);assert_eq!(fs::metadata(&p).unwrap().permissions().mode(),mode);}
  for (p,bytes) in old_records{assert_eq!(fs::read(p).unwrap(),bytes);}
  let report=serde_json::json!({"state":if outcome.is_ok(){"PASS"}else{"FAIL"},"errorCode":outcome.as_ref().err().map(|e|e.code.as_str()),"result":outcome.as_ref().ok(),"stopResults":stop_results,"profile":profile,"scope":"Actual changed durable Applying/application, SIGKILL failed activation, fresh full original encrypted service restoration/native health/selection; public rollout gate remains closed. Missing-host and process-crash/GUI matrix separate.","newWholeHostArchive":false,"originalSourcePreserved":true,"originalAuthorityHistoryPreserved":true});
  crate::restoration::private_bytes(&qualification.join("report.json"),&serde_json::to_vec_pretty(&report).unwrap()).unwrap();
  println!("QUALIFICATION_REPORT={}",qualification.join("report.json").display());
  outcome.unwrap();let id=store.intent().unwrap().update.restore_candidate().unwrap().to_owned();drop(store);
  let cold=crate::signed_release::trust::Store::open(&profile,"default").unwrap();assert_eq!(cold.root,authority);assert_eq!(cold.intent().unwrap().update.stage(),crate::update::Stage::RolledBack);
  assert_eq!(installations::load(&profile).unwrap().unwrap().0.active_id,id);
 }
 #[test]
 #[ignore="explicit retained small development fixture; resume generation12 failed original restore into NEW candidate, no changed update replay or whole-host copy"]
 fn actual_resume_original_restore_after_network_exhaustion() {
  use std::os::unix::fs::PermissionsExt;
  let input:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_MIGRATED_PERMIT_INPUT").unwrap()).unwrap()).unwrap();
  let path=|n:&str|PathBuf::from(input[n].as_str().unwrap());
  let root=fs::canonicalize(path("root")).unwrap();
  assert_eq!(root.file_name().unwrap(),"exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a");
  let profile=root.join("profile");let mut store=crate::signed_release::trust::Store::open(&profile,"default").unwrap();
  let signing=ed25519_dalek::SigningKey::from_bytes(&[31;32]);
  let public=signing.verifying_key().to_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>();
  assert_eq!(store.current.policy.public_keys,vec![public]);
  assert_eq!(store.current.generation,12);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RecoveryRequired);
  let plan=store.intent().unwrap().update.plan().clone();
  assert_ne!(plan.source_schema,plan.target_schema);
  assert_eq!(require_executable_schema(&plan).unwrap_err().code,"UPDATE_RUNTIME_MIGRATION_UNQUALIFIED");
  assert_eq!(installations::load(&profile).unwrap().unwrap().0.active_id,plan.source_instance);
  let authority=store.root.clone();
  let history=(0..=12).filter_map(|g|{let p=authority.join(format!("{g:020}.json"));fs::read(&p).ok().map(|b|(p,b))}).collect::<Vec<_>>();
  let source=profile.join("local-runtime");
  let names=["installed.json","engine.json","runtime.env","bundle/manifest.json","bundle/compose.yaml"];
  let original=names.iter().map(|n|{let p=source.join(n);(*n,fs::read(&p).unwrap(),fs::metadata(&p).unwrap().permissions().mode())}).collect::<Vec<_>>();
  let record=root.join(format!("network-resumed-original-recovery-{}",uuid::Uuid::new_v4()));fs::create_dir(&record).unwrap();fs::set_permissions(&record,fs::Permissions::from_mode(0o700)).unwrap();
  println!("RESUMED_RECOVERY_RECORD={}",record.display());
  let mut reserved_path:Option<PathBuf>=None;
  let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ->Result<Value>{
   let reserved=store.register_rollback_candidate(true)?;reserved_path=Some(reserved.candidate_path.clone());
   crate::restoration::private_bytes(&record.join("reservation.json"),&serde_json::to_vec(&reserved).unwrap())?;
   let port=input["recoveryPort"].as_u64().filter(|n|*n>=1024&&*n<=65535).ok_or_else(||err("PORT_INVALID"))? as u16;
   let restored=store.restore_registered_rollback_candidate(input["maintenance"].as_str().unwrap(),&path("key"),&path("serviceArchive"),port,true)?;
   assert_eq!(restored.candidate_id,reserved.candidate_id);assert!(!restored.rollback_completed);
   assert!(restored.restoration.network_subnet.is_some());
   crate::restoration::private_bytes(&record.join("restoration.json"),&serde_json::to_vec(&restored).unwrap())?;
   let completion=store.activate_restored_rollback(input["maintenance"].as_str().unwrap(),true)?;
   assert!(completion.selection_completed);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RolledBack);
   Ok(serde_json::json!({"reservation":reserved,"restoration":restored,"completion":completion}))
  })).unwrap_or_else(|_|Err(err("UPDATE_QUALIFICATION_ASSERTION_FAILED")));
  let mut stop_result=None;
  if let Some(path)=reserved_path.as_ref()
   && let Ok(service)=crate::LifecycleService::open_retry_diagnostics(path.clone())
   && let Ok(m)=service.manifest()
   && service.validate_ownership(&m,"docker").is_ok()&&service.validate_volumes(&m,"docker").is_ok(){
    stop_result=Some(crate::run("docker",&crate::compose_args(&m,&["stop","--timeout","30"]),Some(&path.join("bundle")),180).is_ok());
  }
  for (name,bytes,mode) in original {let p=source.join(name);assert_eq!(fs::read(&p).unwrap(),bytes);assert_eq!(fs::metadata(&p).unwrap().permissions().mode(),mode);}
  for (p,bytes) in history {assert_eq!(fs::read(p).unwrap(),bytes);}
  let expected=store.intent().unwrap().update.restore_candidate().map(str::to_owned);
  let generation=store.current.generation;drop(store);
  let cold=crate::signed_release::trust::Store::open(&profile,"default").unwrap();
  let cold_completed=cold.intent().unwrap().update.stage()==crate::update::Stage::RolledBack && Some(installations::load(&profile).unwrap().unwrap().0.active_id)==expected;
  let report=serde_json::json!({"state":if outcome.is_ok()&&cold_completed&&stop_result==Some(true){"PASS"}else{"FAIL"},"errorCode":outcome.as_ref().err().map(|e|e.code.as_str()),"result":outcome.as_ref().ok(),"coldSelectionAndStageVerified":cold_completed,"ownedCandidateStopped":stop_result,"authorityGeneration":generation,"originalSourceAndHistoryPreserved":true,"newWholeHostCopy":false,"scope":"NEW original rollback candidate from genuine changed-update generation12 RecoveryRequired; actual encrypted original service restore/native activation/selection/cold; missing host/process crash/GUI still separate"});
  crate::restoration::private_bytes(&record.join("report.json"),&serde_json::to_vec_pretty(&report).unwrap()).unwrap();
  outcome.unwrap();assert!(cold_completed);assert_eq!(stop_result,Some(true));
 }

 #[test]
 #[ignore="explicit same small development fixture; reuse completed original candidate at authority15, native startup wait/activation/selection/cold; no archive copy or update replay"]
 fn actual_reused_original_candidate_waits_for_native_health_and_completes_rollback() {
  use std::os::unix::fs::PermissionsExt;
  let input:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_MIGRATED_PERMIT_INPUT").unwrap()).unwrap()).unwrap();
  let root=fs::canonicalize(PathBuf::from(input["root"].as_str().unwrap())).unwrap();
  assert_eq!(root.file_name().unwrap(),"exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a");
  let profile=root.join("profile");let mut store=crate::signed_release::trust::Store::open(&profile,"default").unwrap();
  let signing=ed25519_dalek::SigningKey::from_bytes(&[31;32]);let public=signing.verifying_key().to_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>();assert_eq!(store.current.policy.public_keys,vec![public]);
  assert_eq!(store.current.generation,15);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RecoveryRequired);
  let plan=store.intent().unwrap().update.plan().clone();let candidate=store.intent().unwrap().update.interrupted_restoration().unwrap().candidate_id.clone();
  assert_eq!(candidate,"c412e389-ae8e-4280-8341-b43e1f98f936");assert_eq!(require_executable_schema(&plan).unwrap_err().code,"UPDATE_RUNTIME_MIGRATION_UNQUALIFIED");
  assert_eq!(installations::load(&profile).unwrap().unwrap().0.active_id,plan.source_instance);
  let authority=store.root.clone();let history=(0..=15).filter_map(|g|{let p=authority.join(format!("{g:020}.json"));fs::read(&p).ok().map(|b|(p,b))}).collect::<Vec<_>>();
  let source=profile.join("local-runtime");let names=["installed.json","engine.json","runtime.env","bundle/manifest.json","bundle/compose.yaml"];
  let original=names.iter().map(|n|{let p=source.join(n);(*n,fs::read(&p).unwrap(),fs::metadata(&p).unwrap().permissions().mode())}).collect::<Vec<_>>();
  let path=profile.join("installations").join(&candidate);let service=crate::LifecycleService::open_retry_diagnostics(path.clone()).unwrap();let manifest=service.manifest().unwrap();
  assert_eq!(manifest.project_name,"exhibitos-28da56bf-ff97-49d9-a986-eaa5a8499e18");
  let job=service.restoration_status().unwrap().unwrap();assert_eq!(job.state,"completed");
  let compose=crate::installation_backup::source_bytes(&path,"bundle/compose.yaml",1048576,true).unwrap();
  let receipt:crate::restoration::RestorationReceipt=crate::read_json(&path.join(format!("restore-{}/receipt.json",job.id))).unwrap();
  assert_eq!(crate::restoration_network::receipt_compose_subnet(&compose,receipt.network_subnet.as_deref()).unwrap().as_deref(),Some("10.240.0.0/28"));
  let record=root.join(format!("reused-original-native-health-{}",uuid::Uuid::new_v4()));fs::create_dir(&record).unwrap();fs::set_permissions(&record,fs::Permissions::from_mode(0o700)).unwrap();println!("REUSED_HEALTH_RECORD={}",record.display());
  let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ->Result<Value>{
   {
    let guard=service.lock()?;service.validate_ownership(&manifest,"docker")?;service.validate_volumes(&manifest,"docker")?;
    crate::run("docker",&crate::compose_args(&manifest,&["up","--detach","--no-build","--pull","never"]),Some(&path.join("bundle")),180)?;
    service.check_restoration_guard(&guard)?;
   }
   let completion=store.activate_restored_rollback(input["maintenance"].as_str().unwrap(),true)?;
   assert!(completion.selection_completed);assert_eq!(store.intent().unwrap().update.stage(),crate::update::Stage::RolledBack);
   Ok(serde_json::to_value(completion).unwrap())
  })).unwrap_or_else(|_|Err(err("UPDATE_QUALIFICATION_ASSERTION_FAILED")));
  let stopped=if service.validate_ownership(&manifest,"docker").is_ok()&&service.validate_volumes(&manifest,"docker").is_ok(){crate::run("docker",&crate::compose_args(&manifest,&["stop","--timeout","30"]),Some(&path.join("bundle")),180).is_ok()}else{false};
  for (name,bytes,mode) in original{let p=source.join(name);assert_eq!(fs::read(&p).unwrap(),bytes);assert_eq!(fs::metadata(&p).unwrap().permissions().mode(),mode);}
  for (p,bytes) in history{assert_eq!(fs::read(p).unwrap(),bytes);}
  drop(store);let cold=crate::signed_release::trust::Store::open(&profile,"default").unwrap();let cold_verified=cold.intent().unwrap().update.stage()==crate::update::Stage::RolledBack && installations::load(&profile).unwrap().unwrap().0.active_id==candidate;
  let report=serde_json::json!({"state":if outcome.is_ok()&&cold_verified&&stopped{"PASS"}else{"FAIL"},"errorCode":outcome.as_ref().err().map(|e|e.code.as_str()),"completion":outcome.as_ref().ok(),"coldSelectionAndStageVerified":cold_verified,"authorityGeneration":cold.current.generation,"candidateId":candidate,"ownedCandidateStopped":stopped,"originalSourceAndHistoryPreserved":true,"newWholeHostCopy":false,"newOriginalServiceRestore":false,"scope":"Reused genuinely restored original candidate after changed update failure; actual native health/startup wait, selection/authority and cold reopen. Missing-host/process crash/public CLI/GUI still separate."});
  crate::restoration::private_bytes(&record.join("report.json"),&serde_json::to_vec_pretty(&report).unwrap()).unwrap();
  outcome.unwrap();assert!(cold_verified);assert!(stopped);
 }

}

#[cfg(all(test, unix))]
#[path = "whole_update_crash_qualification.rs"]
mod whole_update_crash_qualification;
