// SPDX-License-Identifier: Apache-2.0
//! Shared lifetime source/candidate/current checkpoint observations; no execution permit.
use super::*;
pub struct CheckpointInputs<'a> {
    pub binding: &'a Path,
    pub host: &'a Path,
    pub trust: &'a Path,
    pub key: &'a [u8; 32],
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedRecoveryReceipt {
    pub source_before: EphemeralSourceRecoveryReceipt,
    pub candidate: EphemeralCandidateRecoveryReceipt,
    pub source_after: EphemeralSourceRecoveryReceipt,
    pub checkpoint: crate::signed_release::trust::CheckpointPairReceipt,
    pub host: crate::profile_backup::HostCurrentReceipt,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
fn same_source(a: &EphemeralSourceRecoveryReceipt, b: &EphemeralSourceRecoveryReceipt) -> bool {
    candidate_inventory::copies_match(&a.observation.physical, &b.observation.physical)
        && candidate_inventory::copies_match(
            &a.repeated_observation.physical,
            &b.repeated_observation.physical,
        )
        && a.source_instance == b.source_instance
        && a.target_instance == b.target_instance
        && a.configuration.files == b.configuration.files
        && a.configuration.configuration_volume == b.configuration.configuration_volume
        && a.configuration.images.len() == b.configuration.images.len()
        && a.configuration
            .images
            .iter()
            .zip(&b.configuration.images)
            .all(|(x, y)| {
                x.reference == y.reference
                    && x.content_id == y.content_id
                    && x.archive == y.archive
                    && x.bytes == y.bytes
                    && x.sha256 == y.sha256
            })
}
impl ExecutionSession<'_> {
    pub fn verify_combined_recovery_ephemeral(
        &self,
        image: &str,
        export_parent: &Path,
        inputs: &CheckpointInputs<'_>,
        acknowledged: bool,
    ) -> crate::Result<CombinedRecoveryReceipt> {
        let mut result = self.inspect_restored_candidate(acknowledged, |ctx| {
            self.validate_candidate_export_parent(export_parent)?;
            // Three full image observations up to2GiB each +2GiB headroom+6GiB floor.
            for parent in [&self.source.root, ctx.root, export_parent] {
                if fs2::available_space(parent).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                    < 14 * 1024 * 1024 * 1024
                {
                    return Err(crate::err("RESTORE_SPACE_REQUIRED"));
                }
            }
            let proof = self.store.verify_checkpoint_pair(
                inputs.binding,
                inputs.host,
                inputs.trust,
                inputs.key,
            )?;
            let host = crate::profile_backup::verify_host_current_borrowed(
                &self.store.profile,
                inputs.host,
                inputs.key,
                &self._session,
                &proof.receipt().host_manifest_sha256,
            )?;
            let source_before = self.observe_source_recovery_at(ctx, image, export_parent)?;
            let candidate =
                self.observe_ephemeral_candidate_recovery_at(ctx, image, export_parent)?;
            let source_after = self.observe_source_recovery_at(ctx, image, export_parent)?;
            if !same_source(&source_before, &source_after) {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            self.recheck_candidate_configuration_at(ctx, image, &candidate.configuration)?;
            let host_after = crate::profile_backup::verify_host_current_borrowed(
                &self.store.profile,
                inputs.host,
                inputs.key,
                &self._session,
                &proof.receipt().host_manifest_sha256,
            )?;
            if host != host_after {
                return Err(crate::err("HOST_SOURCE_CHANGED"));
            }
            self.store.recheck_checkpoint_pair(
                &proof,
                inputs.binding,
                inputs.host,
                inputs.trust,
                inputs.key,
            )?;
            Ok(CombinedRecoveryReceipt {
                source_before,
                candidate,
                source_after,
                checkpoint: proof.receipt(),
                host,
                preflight_verified: false,
                update_executed: false,
            })
        })?;
        // All common final guards have now passed; retire only these three fresh export scopes.
        for configuration in [
            &mut result.source_before.configuration,
            &mut result.source_after.configuration,
        ] {
            source_image_bytes::retire_verified(
                Path::new(&configuration.export_workspace),
                &configuration.images,
            )?;
            configuration.image_archives_retained = false;
        }
        source_image_bytes::retire_verified(
            Path::new(&result.candidate.configuration.export_workspace),
            &result.candidate.configuration.images,
        )?;
        result.candidate.configuration.image_archives_retained = false;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation() -> ephemeral_inventory::EphemeralObservation {
        serde_json::from_value(serde_json::json!({"physical":{"cleanShutdown":true,"files":4,"entries":6,"bytes":200,"contentSha256":"a".repeat(64),"postgresMajor":18,"pgdata":"18/docker","systemIdentifier":"123"},"inventory":{"operation":"source-inventory-matched","backupId":"backup","authenticatedManifestSha256":"b".repeat(64),"inventorySha256":"c".repeat(64),"schemaSha256":"d".repeat(64),"observedAt":"2026-10-05T00:00:00Z","currentInventoryVerified":true,"configurationVerified":false,"preflightVerified":false,"updateExecuted":false}})).unwrap()
    }
    fn source() -> EphemeralSourceRecoveryReceipt {
        EphemeralSourceRecoveryReceipt {
            source_instance: "source".into(),
            target_instance: "target".into(),
            observation: observation(),
            repeated_observation: observation(),
            storage: "tmpfs",
            preflight_verified: false,
            update_executed: false,
            configuration: ConfigurationInventoryReceipt {
                source_instance: "source".into(),
                target_instance: "target".into(),
                backup_id: "backup".into(),
                authenticated_manifest_sha256: "b".repeat(64),
                configuration_volume: "configuration".into(),
                files: vec![source_configuration::NativeConfigurationProof {
                    name: "freeze-signing-key.json".into(),
                    bytes: 32,
                    sha256: "e".repeat(64),
                }],
                images: vec![source_image_bytes::ImageArchiveProof {
                    reference: "image".into(),
                    content_id: "f".repeat(64),
                    archive: "deployment/images/image-0.tar".into(),
                    bytes: 1000,
                    sha256: "a".repeat(64),
                }],
                export_workspace: "/unused".into(),
                image_archives_retained: true,
                observed_at: 1,
            },
        }
    }
    #[test]
    fn late_source_data_key_and_image_changes_refuse_even_when_logical_inventory_matches() {
        let before = source();
        assert!(same_source(&before, &source()));
        let mut after = source();
        after.observation.physical.system_identifier = "456".into();
        assert!(!same_source(&before, &after));
        let mut after = source();
        after.repeated_observation.physical.content_sha256 = "f".repeat(64);
        assert!(!same_source(&before, &after));
        let mut after = source();
        after.configuration.files[0].sha256 = "f".repeat(64);
        assert!(!same_source(&before, &after));
        let mut after = source();
        after.configuration.images[0].sha256 = "f".repeat(64);
        assert!(!same_source(&before, &after));
        let mut after = source();
        after.configuration.images.clear();
        assert!(!same_source(&before, &after));
        let mut after = source();
        after.target_instance = "other".into();
        assert!(!same_source(&before, &after));
    }
}
