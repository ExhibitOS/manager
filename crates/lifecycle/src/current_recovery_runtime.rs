// SPDX-License-Identifier: Apache-2.0
//! Current full host/trust, source/candidate services and actual runtime in one fence.
use super::*;
use crate::{Result, Value, err};

pub struct RecoveryRuntimeInputs<'a> {
    pub checkpoint: CheckpointInputs<'a>,
    pub export_parent: &'a Path,
    pub python: &'a Path,
    pub source_commit: &'a str,
    pub maintenance_image: &'a str,
    pub external_writers_quiesced: bool,
}
/// Borrowed observation only. No Clone/Deserialize/public constructor, journal
/// transition, host extraction, lost-authority restoration or execution permit.
pub struct CurrentRecoveryRuntime {
    plan: crate::update::Plan,
    generation: u64,
    envelope: String,
    receipt: Value,
}
impl CurrentRecoveryRuntime {
    pub fn receipt(&self) -> &Value {
        &self.receipt
    }
    fn matches(&self, artifact: &PreparedArtifact) -> bool {
        self.plan == artifact.plan
            && self.generation == artifact.generation
            && self.envelope == artifact.envelope
    }
}
fn runtime_physical_matches(
    runtime: &Value,
    field: &str,
    physical: &source_database::DatabaseCopyProof,
) -> Result<()> {
    let observed: source_database::DatabaseCopyProof =
        serde_json::from_value(runtime[field]["physical"].clone())
            .map_err(|_| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?;
    if !candidate_inventory::copies_match(&observed, physical) {
        return Err(err("UPDATE_RECOVERY_RUNTIME_MISMATCH"));
    }
    Ok(())
}
fn current_bound_checkpoint(
    receipt: &crate::signed_release::trust::CheckpointPairReceipt,
    generation: u64,
) -> Result<()> {
    if !receipt.source_plan_bound || receipt.generation != generation {
        return Err(err("UPDATE_RECOVERY_CHECKPOINT_UNBOUND"));
    }
    Ok(())
}
impl ExecutionSession<'_> {
    /// Observe under the retained exclusive Store/profile and source/candidate
    /// operation guards. Caller paths identify inputs, never verification results.
    pub fn with_current_recovery_runtime<T>(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        work: impl FnOnce(&CurrentRecoveryRuntime) -> Result<T>,
    ) -> Result<(T, Value)> {
        self.check()?;
        if !inputs.external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let image = inputs.maintenance_image;
        if !image.strip_prefix("sha256:").is_some_and(crate::hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let cp = &inputs.checkpoint;
        // Reject stale, mixed or unbound recovery inputs before OCI/Engine work.
        let checkpoint = self
            .store
            .verify_checkpoint_pair(cp.binding, cp.host, cp.trust, cp.key)?;
        current_bound_checkpoint(&checkpoint.receipt(), artifact.generation)?;
        self.validate_candidate_export_parent(inputs.export_parent)?;
        // Preserve2GiB headroom and6GiB floor before OCI preparation.
        // Exact authenticated three-export growth is reserved inside CandidateContext.
        if fs2::available_space(inputs.export_parent).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < 8 * 1024 * 1024 * 1024
        {
            return Err(err("RESTORE_SPACE_REQUIRED"));
        }
        let oci = self.qualify_runtime_artifact(artifact, inputs.python, inputs.source_commit)?;
        let held_artifact = std::cell::RefCell::new(&mut *artifact);
        let mut outcome = None;
        let (mut before, mut candidate, mut after, mut receipt) = self.inspect_restored_candidate_finalized(true, |ctx| {
            recovery_space::check(&self.source, ctx, &[&self.source.root, ctx.root, inputs.export_parent], 3)?;
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            let host = crate::profile_backup::verify_host_current_borrowed(
                &self.store.profile, cp.host, cp.key, &self._session, &checkpoint.receipt().host_manifest_sha256)?;
            let before = self.observe_source_recovery_at(ctx, image, inputs.export_parent)?;
            let candidate = self.observe_ephemeral_candidate_recovery_at(ctx, image, inputs.export_parent)?;
            let runtime = self.observe_runtime_at(&mut held_artifact.borrow_mut(), ctx, image, &oci)?;
            runtime_physical_matches(&runtime,"sourceBefore", &before.observation.physical)?;
            runtime_physical_matches(&runtime,"sourceAfter", &before.repeated_observation.physical)?;
            runtime_physical_matches(&runtime,"candidateBefore", &candidate.inventory.observation.physical)?;
            runtime_physical_matches(&runtime,"candidateAfter", &candidate.inventory.repeated_observation.physical)?;
            let after = self.observe_source_recovery_at(ctx, image, inputs.export_parent)?;
            if !combined_recovery::same_source(&before, &after) {
                return Err(err("UPDATE_SOURCE_CHANGED"));
            }
            self.recheck_candidate_configuration_at(ctx, image, &candidate.configuration)?;
            let host_after = crate::profile_backup::verify_host_current_borrowed(
                &self.store.profile, cp.host, cp.key, &self._session, &checkpoint.receipt().host_manifest_sha256)?;
            if host != host_after { return Err(err("HOST_SOURCE_CHANGED")); }
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            let available = [&self.store.profile, ctx.root, inputs.export_parent].into_iter()
                .map(|p|fs2::available_space(p).map_err(|_| err("STORAGE_UNAVAILABLE")))
                .collect::<Result<Vec<_>>>()?.into_iter().min().ok_or_else(||err("STORAGE_UNAVAILABLE"))?;
            if available < ctx.plan.required_free_bytes.checked_add(6*1024*1024*1024).ok_or_else(||err("RESTORE_SPACE_REQUIRED"))? {
                return Err(err("RESTORE_SPACE_REQUIRED"));
            }
            let receipt = serde_json::json!({"operationId":ctx.plan.operation_id,"trustGeneration":checkpoint.receipt().generation,"checkpoint":checkpoint.receipt(),"host":host,"sourceBefore":before,"candidate":candidate,"runtime":runtime,"sourceAfter":after,"availableFreeBytes":available,"sameLifetimeRecoveryRuntimeVerified":true,"hostExtractionVerified":false,"lostAuthorityRecoveryVerified":false,"preflightVerified":false,"updateExecuted":false,"imageOnlyRollbackVerified":false});
            Ok((before,candidate,after,receipt))
        },|ctx,(_,_,_,receipt)| {
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            let held = held_artifact.borrow();
            let proof = CurrentRecoveryRuntime {plan:ctx.plan.clone(), generation:checkpoint.receipt().generation,envelope:held.envelope.clone(),receipt:receipt.clone()};
            if !proof.matches(&held) {return Err(err("UPDATE_RECOVERY_RUNTIME_MISMATCH"));}
            drop(held);
            outcome=Some(work(&proof)?);
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            // A callback must not alter any protected host bytes.
            crate::profile_backup::verify_host_current_borrowed(&self.store.profile, cp.host, cp.key, &self._session, &checkpoint.receipt().host_manifest_sha256)?;
            Ok(())
        })?;
        // Only after common final guards and callback: retire exactly new exports.
        for configuration in [&mut before.configuration, &mut after.configuration] {
            source_image_bytes::retire_verified(
                Path::new(&configuration.export_workspace),
                &configuration.images,
            )?;
            configuration.image_archives_retained = false;
        }
        source_image_bytes::retire_verified(
            Path::new(&candidate.configuration.export_workspace),
            &candidate.configuration.images,
        )?;
        candidate.configuration.image_archives_retained = false;
        receipt["sourceBefore"] =
            serde_json::to_value(before).map_err(|_| err("UPDATE_RESULT_INVALID"))?;
        receipt["candidate"] =
            serde_json::to_value(candidate).map_err(|_| err("UPDATE_RESULT_INVALID"))?;
        receipt["sourceAfter"] =
            serde_json::to_value(after).map_err(|_| err("UPDATE_RESULT_INVALID"))?;
        receipt["freshImageArchivesRetired"] = serde_json::json!(true);
        Ok((
            outcome.ok_or_else(|| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?,
            receipt,
        ))
    }
    pub fn qualify_current_recovery_runtime(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
    ) -> Result<Value> {
        self.with_current_recovery_runtime(artifact, inputs, |_| Ok(()))
            .map(|(_, receipt)| receipt)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn physical() -> source_database::DatabaseCopyProof {
        serde_json::from_value(serde_json::json!({"cleanShutdown":true,"files":4,"entries":6,"bytes":200,"contentSha256":"a".repeat(64),"postgresMajor":18,"pgdata":"18/docker","systemIdentifier":"123"})).unwrap()
    }
    #[test]
    fn combined_runtime_rejects_physical_change_even_when_logical_inventory_matches() {
        let p = physical();
        let mut v = serde_json::json!({"sourceBefore":{"physical":p,"inventory":{"inventorySha256":"b".repeat(64)}}});
        runtime_physical_matches(&v, "sourceBefore", &p).unwrap();
        for (field, value) in [
            ("systemIdentifier", serde_json::json!("124")),
            ("files", serde_json::json!(5)),
            ("bytes", serde_json::json!(201)),
            ("contentSha256", serde_json::json!("c".repeat(64))),
            ("cleanShutdown", serde_json::json!(false)),
        ] {
            v["sourceBefore"]["physical"] = serde_json::to_value(&p).unwrap();
            v["sourceBefore"]["physical"][field] = value;
            assert_eq!(
                runtime_physical_matches(&v, "sourceBefore", &p)
                    .unwrap_err()
                    .code,
                "UPDATE_RECOVERY_RUNTIME_MISMATCH"
            );
        }
        assert!(runtime_physical_matches(&v, "candidateBefore", &p).is_err());
    }
    #[test]
    fn unbound_or_historical_checkpoint_cannot_feed_combined_runtime_observation() {
        let mut r = crate::signed_release::trust::CheckpointPairReceipt {
            generation: 11,
            head_sha256: "a".repeat(64),
            host_manifest_sha256: "b".repeat(64),
            host_archive_sha256: "c".repeat(64),
            host_archive_bytes: 100,
            trust_archive_sha256: "d".repeat(64),
            trust_archive_bytes: 100,
            source_plan_bound: true,
        };
        current_bound_checkpoint(&r, 11).unwrap();
        r.source_plan_bound = false;
        assert!(current_bound_checkpoint(&r, 11).is_err());
        r.source_plan_bound = true;
        assert!(current_bound_checkpoint(&r, 12).is_err());
    }
}
