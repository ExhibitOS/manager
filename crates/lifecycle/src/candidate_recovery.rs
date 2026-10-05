// SPDX-License-Identifier: Apache-2.0
//! Current candidate DB/blob before and after configuration/images in one retained fence.
use super::*;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRecoveryReceipt {
    pub inventory: CandidateInventoryReceipt,
    pub configuration: CandidateConfigurationReceipt,
    pub repeated_inventory: source_inventory::InventoryProof,
    pub observed_at: u64,
    pub current_trust_unchanged: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EphemeralCandidateRecoveryReceipt {
    pub inventory: EphemeralCandidateInventoryReceipt,
    pub configuration: CandidateConfigurationReceipt,
    pub observed_at: u64,
    pub current_trust_unchanged: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
impl ExecutionSession<'_> {
    pub(super) fn observe_ephemeral_candidate_recovery_at(
        &self,
        ctx: &candidate_inventory::CandidateContext<'_>,
        image: &str,
        export_parent: &Path,
    ) -> crate::Result<EphemeralCandidateRecoveryReceipt> {
        self.validate_candidate_export_parent(export_parent)?;
        // Images2GiB + growth/headroom2GiB + retained disk floor6GiB.
        // Database copies live in bounded tmpfs, not on the host filesystem.
        for parent in [ctx.root, export_parent] {
            if fs2::available_space(parent).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 10 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
        }
        let first = ephemeral_inventory::observe(ctx, image)?;
        let configuration = self.observe_candidate_configuration_at(ctx, image, export_parent)?;
        let repeated = ephemeral_inventory::observe(ctx, image)?;
        if !candidate_inventory::copies_match(&first.physical, &repeated.physical) {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        self.recheck_candidate_configuration_at(ctx, image, &configuration)?;
        Ok(EphemeralCandidateRecoveryReceipt {
            inventory: EphemeralCandidateInventoryReceipt {
                source_instance: ctx.plan.source_instance.clone(),
                candidate_instance: ctx.plan.target_instance.clone(),
                observation: first,
                repeated_observation: repeated,
                storage: "bounded-tmpfs-no-persistent-snapshot",
                preflight_verified: false,
                update_executed: false,
            },
            configuration,
            observed_at: crate::now(),
            current_trust_unchanged: true,
            preflight_verified: false,
            update_executed: false,
        })
    }

    /// Full candidate data/configuration/images in one retained fence, with no persistent DB copies.
    pub fn verify_restored_candidate_recovery_ephemeral(
        &self,
        image: &str,
        export_parent: &Path,
        acknowledged: bool,
    ) -> crate::Result<EphemeralCandidateRecoveryReceipt> {
        let mut observation = self.inspect_restored_candidate(acknowledged, |ctx| {
            self.observe_ephemeral_candidate_recovery_at(ctx, image, export_parent)
        })?;
        // Retire only fresh image exports after the common wrapper rechecks all identities.
        source_image_bytes::retire_verified(
            Path::new(&observation.configuration.export_workspace),
            &observation.configuration.images,
        )?;
        observation.configuration.image_archives_retained = false;
        Ok(observation)
    }
    /// Exactly two fresh physical copies, not two standalone verifications/four copies.
    /// No permit is issued; current release expiry/authority/recovery/execution remain gates.
    pub fn verify_restored_candidate_recovery(
        &self,
        image: &str,
        export_parent: &Path,
        acknowledged: bool,
    ) -> crate::Result<CandidateRecoveryReceipt> {
        let mut observation = self.inspect_restored_candidate(acknowledged, |ctx| {
            self.validate_candidate_export_parent(export_parent)?;
            // 2x2GiB database copies +2GiB image bytes +4GiB growth/headroom +6GiB floor.
            for parent in [ctx.root, export_parent] {
                if fs2::available_space(parent).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                    < 16 * 1024 * 1024 * 1024
                {
                    return Err(crate::err("RESTORE_SPACE_REQUIRED"));
                }
            }
            let (snapshot, physical, logical) = candidate_inventory::observe_copy(ctx, image)?;
            // This helper borrows the existing context; it never takes another operation lock.
            let configuration =
                self.observe_candidate_configuration_at(ctx, image, export_parent)?;
            let (repeated_snapshot, repeated_physical, repeated_logical) =
                candidate_inventory::observe_copy(ctx, image)?;
            if !candidate_inventory::copies_match(&physical, &repeated_physical) {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
            self.recheck_candidate_configuration_at(ctx, image, &configuration)?;
            let inventory = CandidateInventoryReceipt {
                source_instance: ctx.plan.source_instance.clone(),
                candidate_instance: ctx.plan.target_instance.clone(),
                candidate_bundle_id: ctx.manifest.bundle_id.clone(),
                candidate_database_volume: ctx.candidate_before.database_volume.clone(),
                candidate_blob_volume: ctx.candidate_before.blob_volume.clone(),
                snapshot_volume: snapshot,
                repeated_snapshot_volume: repeated_snapshot,
                candidate_content_sha256: physical.content_sha256,
                inventory: logical,
                observed_at: crate::now(),
                configuration_verified: false,
                image_bytes_verified: false,
                preflight_verified: false,
                update_executed: false,
            };
            Ok(CandidateRecoveryReceipt {
                inventory,
                configuration,
                repeated_inventory: repeated_logical,
                observed_at: crate::now(),
                current_trust_unchanged: true,
                preflight_verified: false,
                update_executed: false,
            })
        })?;
        // The common wrapper has now rechecked exact source/target/host/trust/provenance.
        source_image_bytes::retire_verified(
            Path::new(&observation.configuration.export_workspace),
            &observation.configuration.images,
        )?;
        observation.configuration.image_archives_retained = false;
        Ok(observation)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_physical_identity_is_exact_and_never_accepts_unclean_or_changed_copies() {
        let base = serde_json::json!({"cleanShutdown":true,"files":4,"entries":6,"bytes":200,"contentSha256":"a".repeat(64),"postgresMajor":18,"pgdata":"18/docker","systemIdentifier":"123"});
        let proof = |v| serde_json::from_value::<source_database::DatabaseCopyProof>(v).unwrap();
        assert!(candidate_inventory::copies_match(
            &proof(base.clone()),
            &proof(base.clone())
        ));
        for (field, value) in [
            ("cleanShutdown", serde_json::json!(false)),
            ("files", serde_json::json!(5)),
            ("entries", serde_json::json!(7)),
            ("bytes", serde_json::json!(201)),
            ("contentSha256", serde_json::json!("b".repeat(64))),
            ("postgresMajor", serde_json::json!(17)),
            ("pgdata", serde_json::json!("foreign")),
            ("systemIdentifier", serde_json::json!("456")),
        ] {
            let mut changed = base.clone();
            changed[field] = value;
            assert!(!candidate_inventory::copies_match(
                &proof(base.clone()),
                &proof(changed)
            ));
        }
    }
}
