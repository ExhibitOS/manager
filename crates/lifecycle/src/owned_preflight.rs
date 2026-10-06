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
    pub fn begin(mut self) -> Result<StartedUpdate<'session, 'store, 'inputs>> {
        require_executable_schema(&self.artifact.plan)?;
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
}
