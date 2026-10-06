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
        let ((), mut receipt, lease, host, oci) = self.with_recovery_runtime_impl(
            artifact,
            inputs,
            Some((destination, key_file)),
            |_| Ok(()),
        )?;
        let host = host.ok_or_else(|| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?;
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
            host,
            oci,
            authority,
            migration: None,
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
        crate::profile_backup::verify_extracted_host(
            &self.session.store.profile,
            &self.destination.join("host"),
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
