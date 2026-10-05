// SPDX-License-Identifier: Apache-2.0
//! Source data before/after complete configuration/images under retained recovery fences.
use super::*;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EphemeralSourceRecoveryReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub observation: ephemeral_inventory::EphemeralObservation,
    pub configuration: ConfigurationInventoryReceipt,
    pub repeated_observation: ephemeral_inventory::EphemeralObservation,
    pub storage: &'static str,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
impl ExecutionSession<'_> {
    pub(super) fn observe_source_recovery_at(
        &self,
        ctx: &candidate_inventory::CandidateContext<'_>,
        image: &str,
        export_parent: &Path,
    ) -> crate::Result<EphemeralSourceRecoveryReceipt> {
        self.validate_candidate_export_parent(export_parent)?;
        for parent in [&self.source.root, export_parent] {
            if fs2::available_space(parent).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 10 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
        }
        let original_files = source_deployment::authenticated_files(
            &self.source.root,
            ctx.workspace,
            ctx.receipt,
            ctx.plan,
        )?;
        let first = ephemeral_inventory::observe_source(ctx, image)?;
        let configuration =
            self.verify_configuration_inventory_at(image, true, true, Some(export_parent), true)?;
        let repeated = ephemeral_inventory::observe_source(ctx, image)?;
        if !candidate_inventory::copies_match(&first.physical, &repeated.physical) {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        // Re-observe configuration after the second real PostgreSQL observation.
        let late_files = source_deployment::authenticated_files(
            &self.source.root,
            ctx.workspace,
            ctx.receipt,
            ctx.plan,
        )?;
        let manifest = source_full_configuration::scope(ctx.raw)?;
        let late_key = source_configuration::observe(
            image,
            &ctx.source_before.configuration_volume,
            &source_configuration::expected(ctx.raw)?,
        )?;
        let generated =
            source_full_configuration::generated_inventory(&configuration.images, &manifest)?;
        let late_full = source_full_configuration::files(&late_files, late_key, generated)?;
        source_images::observe(&self.source, ctx.workspace, ctx.raw)?;
        if original_files != late_files
            || late_full != configuration.files
            || configuration.source_instance != ctx.plan.source_instance
            || configuration.target_instance != ctx.plan.target_instance
            || configuration.backup_id != ctx.plan.backup_id
            || configuration.authenticated_manifest_sha256 != ctx.plan.backup_manifest
            || configuration.configuration_volume != ctx.source_before.configuration_volume
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        for proof in &configuration.images {
            if crate::backup_creation::local_image("docker", &proof.reference)? != proof.content_id
            {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_MISMATCH"));
            }
        }
        Ok(EphemeralSourceRecoveryReceipt {
            source_instance: ctx.plan.source_instance.clone(),
            target_instance: ctx.plan.target_instance.clone(),
            observation: first,
            configuration,
            repeated_observation: repeated,
            storage: "bounded-tmpfs-no-persistent-snapshot",
            preflight_verified: false,
            update_executed: false,
        })
    }

    pub fn verify_source_recovery_ephemeral(
        &self,
        image: &str,
        export_parent: &Path,
        acknowledged: bool,
    ) -> crate::Result<EphemeralSourceRecoveryReceipt> {
        let mut result = self.inspect_restored_candidate(acknowledged, |ctx| {
            self.observe_source_recovery_at(ctx, image, export_parent)
        })?;
        source_image_bytes::retire_verified(
            Path::new(&result.configuration.export_workspace),
            &result.configuration.images,
        )?;
        result.configuration.image_archives_retained = false;
        Ok(result)
    }
}
