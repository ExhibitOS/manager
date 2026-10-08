// SPDX-License-Identifier: Apache-2.0
//! Current full host/trust, source/candidate services and actual runtime in one fence.
use super::*;
use crate::{Result, Value, err};
#[cfg(unix)]
#[path = "owned_preflight.rs"]
mod owned_preflight;
#[cfg(unix)]
pub use owned_preflight::{OwnedPreflight, ReadyCandidate, StartedUpdate};

pub struct RecoveryRuntimeInputs<'a> {
    pub checkpoint: CheckpointInputs<'a>,
    pub export_parent: &'a Path,
    pub python: &'a Path,
    pub source_commit: &'a str,
    pub maintenance_image: &'a str,
    pub external_writers_quiesced: bool,
}
/// Exact private target catalog bound to the signed release. Not a success receipt.
pub struct MigrationRuntimeInputs<'a> {
    pub catalog: &'a Path,
    pub catalog_sha256: &'a str,
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
struct RetainedRecoveryOutcome<T> {
    work: T,
    receipt: Value,
    lease: candidate_inventory::RetainedCandidateLease,
    host: Option<crate::profile_backup::HostReceipt>,
    oci: PreparedOciReceipt,
    catalog: Option<super::migration_catalog::CatalogInput>,
}
fn retained_host_boundary(profile: &Path, authority: &Path, destination: &Path, host: &Path) -> Result<()> {
    if !host.is_absolute() || fs::canonicalize(host).ok().as_deref()!=Some(host)
        || host.starts_with(profile) || profile.starts_with(host)
        || host.starts_with(authority) || authority.starts_with(host)
        || host.starts_with(destination) || destination.starts_with(host) {
        return Err(err("HOST_CHECKPOINT_INVALID"));
    }
    crate::installations::private_directory(host)?;
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
        self.with_recovery_runtime_impl(artifact, inputs, None, work)
            .map(|(result, receipt, _lease, _host, _oci)| (result, receipt))
    }
    /// Full recovery and migrated Runtime observation; no OwnedPreflight or activation.
    pub fn with_migrated_current_recovery_runtime<T>(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        migration: &MigrationRuntimeInputs<'_>,
        work: impl FnOnce(&CurrentRecoveryRuntime) -> Result<T>,
    ) -> Result<(T, Value)> {
        self.with_recovery_runtime_mode(artifact, inputs, None, Some(migration), work)
            .map(|(result, receipt, _lease, _host, _oci)| (result, receipt))
    }
    pub fn qualify_migrated_full_recovery_runtime(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        migration: &MigrationRuntimeInputs<'_>,
        destination: &Path,
        key_file: &Path,
    ) -> Result<Value> {
        self.with_recovery_runtime_mode(
            artifact,
            inputs,
            Some((destination, key_file)),
            Some(migration),
            |_| Ok(()),
        )
        .map(|(_, receipt, _lease, _host, _oci)| receipt)
    }
    fn with_recovery_runtime_impl<T>(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        extraction: Option<(&Path, &Path)>,
        work: impl FnOnce(&CurrentRecoveryRuntime) -> Result<T>,
    ) -> Result<(
        T,
        Value,
        candidate_inventory::RetainedCandidateLease,
        Option<crate::profile_backup::HostReceipt>,
        PreparedOciReceipt,
    )> {
        self.with_recovery_runtime_mode(artifact, inputs, extraction, None, work)
    }
    fn with_recovery_runtime_mode<T>(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        extraction: Option<(&Path, &Path)>,
        migration: Option<&MigrationRuntimeInputs<'_>>,
        work: impl FnOnce(&CurrentRecoveryRuntime) -> Result<T>,
    ) -> Result<(
        T,
        Value,
        candidate_inventory::RetainedCandidateLease,
        Option<crate::profile_backup::HostReceipt>,
        PreparedOciReceipt,
    )> {
        self.with_recovery_runtime_retained(artifact, inputs, extraction, migration, None, work)
            .map(|r| (r.work, r.receipt, r.lease, r.host, r.oci))
    }
    #[allow(clippy::too_many_arguments)]
    fn with_recovery_runtime_retained<T>(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        extraction: Option<(&Path, &Path)>,
        migration: Option<&MigrationRuntimeInputs<'_>>,
        retained_host: Option<&Path>,
        work: impl FnOnce(&CurrentRecoveryRuntime) -> Result<T>,
    ) -> Result<RetainedRecoveryOutcome<T>> {
        self.check()?;
        if !inputs.external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let image = inputs.maintenance_image;
        if !image.strip_prefix("sha256:").is_some_and(crate::hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let cp = &inputs.checkpoint;
        if let Some(host) = retained_host {
            let (destination, _) = extraction.ok_or_else(||err("HOST_CHECKPOINT_INVALID"))?;
            retained_host_boundary(&self.store.profile, &self.store.root, destination, host)?;
        }
        let retained_identity=retained_host.map(fs::symlink_metadata).transpose().map_err(|_|err("HOST_CHECKPOINT_INVALID"))?;
        if let Some((destination, key_file)) = extraction {
            let parent = destination
                .parent()
                .ok_or_else(|| err("HOST_CHECKPOINT_INVALID"))?;
            crate::installations::private_directory(parent)?;
            if !destination.is_absolute()
                || destination.exists()
                || fs::canonicalize(parent).ok().as_deref() != Some(parent)
                || destination.starts_with(&self.store.profile)
                || destination.starts_with(&self.store.root)
                || key_file.starts_with(destination)
                || inputs.checkpoint.host.starts_with(destination)
                || inputs.checkpoint.trust.starts_with(destination)
            {
                return Err(err("HOST_CHECKPOINT_INVALID"));
            }
        }
        // Reject stale, mixed or unbound recovery inputs before OCI/Engine work.
        let checkpoint = self
            .store
            .verify_checkpoint_pair(cp.binding, cp.host, cp.trust, cp.key)?;
        current_bound_checkpoint(&checkpoint.receipt(), artifact.generation)?;
        self.validate_candidate_export_parent(inputs.export_parent)?;
        let host_restore_bytes = if extraction.is_some() && retained_host.is_none() {
            crate::profile_backup::authenticated_host_restore_bytes(
                &self.store.profile, cp.host, cp.key, &checkpoint.receipt().host_manifest_sha256,
            )?
        } else { 0 };

        // Preserve2GiB headroom and6GiB floor before OCI preparation.
        // Exact authenticated three-export growth is reserved inside CandidateContext.
        if fs2::available_space(inputs.export_parent).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < (8u64 * 1024 * 1024 * 1024)
                .checked_add(host_restore_bytes)
                .ok_or_else(|| err("STORAGE_QUOTA"))?
        {
            return Err(err("RESTORE_SPACE_REQUIRED"));
        }
        let (oci, catalog, migration_input) = if let Some(migration) = migration {
            let (oci, catalog, input) = self.qualify_migrated_runtime_artifact(
                artifact,
                inputs.python,
                inputs.source_commit,
                migration.catalog,
                migration.catalog_sha256,
            )?;
            (oci, Some(catalog), Some(input))
        } else {
            (
                self.qualify_runtime_artifact(artifact, inputs.python, inputs.source_commit)?,
                None,
                None,
            )
        };
        let catalog_cell = std::cell::RefCell::new(catalog);
        let check_catalog = || -> Result<()> {
            if let (Some(host),Some(expected))=(retained_host,retained_identity.as_ref())
                && !super::super::identity(expected,&fs::symlink_metadata(host).map_err(|_|err("HOST_CHECKPOINT_INVALID"))?) {return Err(err("HOST_CHECKPOINT_INVALID"));}
            if let Some(catalog) = catalog_cell.borrow_mut().as_mut() {
                catalog.recheck()?;
            }
            Ok(())
        };
        check_catalog()?;
        let held_artifact = std::cell::RefCell::new(&mut *artifact);
        let mut outcome = None;
        let ((mut before, mut candidate, mut after, mut receipt, full_recovery), lease) = self.inspect_restored_candidate_retained(true, |ctx| {
            let full_recovery = if let Some((destination, key_file)) = extraction {
                let parent = destination.parent().ok_or_else(||err("HOST_CHECKPOINT_INVALID"))?;
                recovery_space::check_with_host(&self.source, ctx, &[&self.source.root, ctx.root, inputs.export_parent, parent], 3, host_restore_bytes)?;
                if super::read_record(key_file).map_err(|_|err("PROFILE_KEY_INVALID"))?.as_slice()!=cp.key {
                    return Err(err("PROFILE_KEY_INVALID"));
                }
                let host_path = retained_host.map(Path::to_path_buf).unwrap_or_else(||destination.join("host"));
                let host = if retained_host.is_some() {
                    let current=crate::profile_backup::verify_host_current_borrowed(&self.store.profile,cp.host,cp.key,&self._session,&checkpoint.receipt().host_manifest_sha256)?;
                    crate::profile_backup::recheck_extracted_host_current(&self.store.profile,&host_path,&current)?
                } else {crate::installations::new_directory(destination)?;crate::profile_backup::extract_host(&self.store.profile, key_file, cp.host, &host_path, true)?};
                if retained_host.is_some() {crate::installations::new_directory(destination)?;}
                if host.manifest_sha256!=checkpoint.receipt().host_manifest_sha256 {return Err(err("RECOVERY_PAIR_INVALID"));}
                let trust = self.store.extract_trust_checkpoint(cp.trust, &destination.join("trust"), cp.key).map_err(|e|err(e.code()))?;
                crate::profile_backup::verify_extracted_host(&self.store.profile, &host_path, &host)?;
                Some((destination, key_file, host, trust, host_path))
            } else {
                recovery_space::check(&self.source, ctx, &[&self.source.root, ctx.root, inputs.export_parent], 3)?;
                None
            };
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            let host = crate::profile_backup::verify_host_current_borrowed(
                &self.store.profile, cp.host, cp.key, &self._session, &checkpoint.receipt().host_manifest_sha256)?;
            let before = self.observe_source_recovery_at(ctx, image, inputs.export_parent)?;
            let candidate = self.observe_ephemeral_candidate_recovery_at(ctx, image, inputs.export_parent)?;
            check_catalog()?;
            let runtime = if migration_input.is_some() {
                self.observe_runtime_at_inner(&mut held_artifact.borrow_mut(), ctx, image, &oci, migration_input.as_ref())?
            } else { self.observe_runtime_at(&mut held_artifact.borrow_mut(), ctx, image, &oci)? };
            check_catalog()?;
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
            if let Some((destination,key_file,host,_,host_path))=&full_recovery {
                crate::profile_backup::verify_extracted_host(&self.store.profile, host_path, host)?;
                self.store.verify_trust_checkpoint(&destination.join("trust")).map_err(|e|err(e.code()))?;
                if super::read_record(key_file).map_err(|_|err("PROFILE_KEY_INVALID"))?.as_slice()!=cp.key {return Err(err("PROFILE_KEY_INVALID"));}
            }
            let mut receipt = serde_json::json!({"operationId":ctx.plan.operation_id,"trustGeneration":checkpoint.receipt().generation,"checkpoint":checkpoint.receipt(),"host":host,"sourceBefore":before,"candidate":candidate,"runtime":runtime,"sourceAfter":after,"availableFreeBytes":available,"sameLifetimeRecoveryRuntimeVerified":true,"hostExtractionVerified":false,"lostAuthorityRecoveryVerified":false,"preflightVerified":false,"updateExecuted":false,"imageOnlyRollbackVerified":false});
            if let Some((destination,_,host,trust,host_path))=&full_recovery {
                receipt["hostExtractionVerified"]=serde_json::json!(true);
                receipt["inactiveFullRecovery"]=serde_json::json!({"destination":destination,"hostPath":host_path,"retainedHostReused":retained_host.is_some(),"host":host,"trust":trust,"hostActivated":false,"liveAuthorityRestored":false});
            }
            Ok((before,candidate,after,receipt,full_recovery))
        },|ctx,(_,_,_,receipt,full_recovery)| {
            check_catalog()?;
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            let held = held_artifact.borrow();
            let proof = CurrentRecoveryRuntime {plan:ctx.plan.clone(), generation:checkpoint.receipt().generation,envelope:held.envelope.clone(),receipt:receipt.clone()};
            if !proof.matches(&held) {return Err(err("UPDATE_RECOVERY_RUNTIME_MISMATCH"));}
            drop(held);
            outcome=Some(work(&proof)?);
            check_catalog()?;
            self.reverify_prepared_artifact(&mut held_artifact.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint, cp.binding, cp.host, cp.trust, cp.key)?;
            if let Some((destination,key_file,host,_,host_path))=full_recovery {
                crate::profile_backup::verify_extracted_host(&self.store.profile, host_path, host)?;
                self.store.verify_trust_checkpoint(&destination.join("trust")).map_err(|e|err(e.code()))?;
                if super::read_record(key_file).map_err(|_|err("PROFILE_KEY_INVALID"))?.as_slice()!=cp.key {return Err(err("PROFILE_KEY_INVALID"));}
            }
            // A callback must not alter any protected host bytes.
            crate::profile_backup::verify_host_current_borrowed(&self.store.profile, cp.host, cp.key, &self._session, &checkpoint.receipt().host_manifest_sha256)?;
            Ok(())
        })?;
        check_catalog()?;
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
        check_catalog()?;
        Ok(RetainedRecoveryOutcome {
            work: outcome.ok_or_else(|| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))?,
            receipt, lease,
            host: full_recovery.map(|(_, _, host, _, _)| host),
            oci, catalog: catalog_cell.into_inner(),
        })
    }
    /// Actual inactive full host/trust extraction in the same fence as service/runtime checks.
    /// Retains fresh private output for independent inspection; no original activation.
    pub fn qualify_full_recovery_runtime(
        &self,
        artifact: &mut PreparedArtifact,
        inputs: &RecoveryRuntimeInputs<'_>,
        destination: &Path,
        key_file: &Path,
    ) -> Result<Value> {
        self.with_recovery_runtime_impl(artifact, inputs, Some((destination, key_file)), |_| Ok(()))
            .map(|(_, receipt, _lease, _host, _oci)| receipt)
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
    #[cfg(unix)]
    #[test]
    fn retained_host_scope_refuses_profile_authority_overlap_and_symlink() {
        let root=fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("retained-recovery-scope-{}",uuid::Uuid::new_v4()));
        crate::installations::new_directory(&root).unwrap();
        let profile=root.join("profile");let authority=root.join("authority");let host=root.join("host");let destination=root.join("new-output");
        for p in [&profile,&authority,&host] {crate::installations::new_directory(p).unwrap();}
        retained_host_boundary(&profile,&authority,&destination,&host).unwrap();
        for p in [&profile,&authority,&root] {assert!(retained_host_boundary(&profile,&authority,&destination,p).is_err());}
        assert!(retained_host_boundary(&profile,&authority,&host.join("overlap"),&host).is_err());
        let alias=root.join("alias");std::os::unix::fs::symlink(&host,&alias).unwrap();
        assert!(retained_host_boundary(&profile,&authority,&destination,&alias).is_err());
        assert!(!destination.exists());assert!(host.is_dir());fs::remove_dir_all(root).unwrap();
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
