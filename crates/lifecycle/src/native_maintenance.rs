// SPDX-License-Identifier: Apache-2.0
//! Closed native presentation intents. Existing controllers retain authority
//! over key creation, installation selection and authenticated restoration.
use super::*;

#[derive(Clone, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Input {
    CreateKey { path: PathBuf },
    CreateRecovery { acknowledged: bool },
    CreateOexWorkspace { acknowledged: bool },
    CheckOexWorkspace,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    RetainAbandonedOrdinaryPublicationStage { proof: super::native_publication_custody::Input },
    Select { id: String, acknowledged: bool },
    StageRestore {
        source: PathBuf,
        key_file: PathBuf,
        carried_restore: Option<super::native_restore::CarriedInput>,
    },
    ActivateRestore { id: String },
    #[cfg(debug_assertions)]
    LocalPersonalTransition {
        transition: LocalPersonalTransition,
        acknowledged: bool,
    },
}

#[cfg(debug_assertions)]
#[derive(Clone, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum LocalPersonalTransition {
    Protect {
        source_payload: PathBuf,
        archive: PathBuf,
        key_file: PathBuf,
        recovered: PathBuf,
        upload_job_id: String,
    },
    Stage { source_payload: PathBuf, staging: PathBuf, key_file: PathBuf },
    #[cfg(feature = "workspace-v5-owner-validation")]
    StageSaved {
        source_payload: PathBuf, staging: PathBuf, key_file: PathBuf,
        #[serde(default)]
        retained_product_image: Option<RetainedProductImage>,
        #[serde(default)]
        retained_gpu_variant: Option<RetainedGpuVariant>,
        #[serde(default)]
        retained_original_model: Option<RetainedOriginalModel>,
        #[serde(default)]
        startup_cancellation_045: Option<StartupCancellation045>,
    },
    Activate {
        staging: PathBuf, key_file: PathBuf,
        #[serde(default)]
        retained_product_image: Option<RetainedProductImage>,
        #[serde(default)]
        retained_gpu_variant: Option<RetainedGpuVariant>,
        #[serde(default)]
        retained_original_model: Option<RetainedOriginalModel>,
        #[serde(default)]
        startup_cancellation_045: Option<StartupCancellation045>,
    },
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation"))]
    ActivateSelected045 { staging: PathBuf, key_file: PathBuf, selection: SelectedRecovery045 },
    Recover {
        staging: PathBuf, key_file: PathBuf,
        #[serde(default)]
        retained_product_image: Option<RetainedProductImage>,
        #[serde(default)]
        retained_gpu_variant: Option<RetainedGpuVariant>,
        #[serde(default)]
        retained_original_model: Option<RetainedOriginalModel>,
    },
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation"))]
    InspectPreReadyGuard {
        staging: PathBuf,
        key_file: PathBuf,
        failed_generation: String,
        recovery_receipt_sha256: String,
    },
}

/// Preserves an already-bound pending request, never business completion or
/// retry authority. Only the exact corrective GPU transition admits this proof.
#[cfg(debug_assertions)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedGpuVariant {
    pub(crate) job_id: String,
    pub(crate) transfer_sha256: String,
}

/// Exact journal/source receipt binding for an already terminal required model import.
/// Read-only owned-PG and normal import readback prove readiness; this wire value
/// never supplies completion, retry, CMS approval or placement authority.
#[cfg(debug_assertions)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedOriginalModel {
    pub(crate) job_id: String,
    pub(crate) artwork_id: String,
    pub(crate) artwork_revision: u32,
    pub(crate) upload_idempotency_key: String,
    pub(crate) generation: String,
    pub(crate) source_sha256: String,
    pub(crate) source_bytes: u64,
    pub(crate) scale_meters: f64,
    pub(crate) receipt_sha256: String,
    pub(crate) rights_sha256: String,
    pub(crate) transfer_sha256: String,
}

#[cfg(debug_assertions)]
impl RetainedOriginalModel {
    #[cfg(test)]
    pub(crate) fn synthetic() -> Self {
        Self { job_id:"10000000-0000-4000-8000-000000000004".into(),
            artwork_id:"10000000-0000-4000-8000-000000000005".into(),artwork_revision:1,
            upload_idempotency_key:"10000000-0000-4000-8000-000000000006".into(),generation:"synthetic-required-model".into(),
            source_sha256:"a".repeat(64),source_bytes:100,scale_meters:1.0,receipt_sha256:"c".repeat(64),
            rights_sha256:"e".repeat(64),transfer_sha256:"d".repeat(64) }
    }
    pub(crate) fn valid(&self) -> bool {
        [&self.job_id, &self.artwork_id, &self.upload_idempotency_key].into_iter().all(|id| uuid(id))
            && (1..=i32::MAX as u32).contains(&self.artwork_revision)
            && self.generation.len() <= 128 && self.generation.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && self.generation.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && [&self.source_sha256, &self.receipt_sha256, &self.rights_sha256, &self.transfer_sha256].into_iter().all(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            && (28..=32 * 1024 * 1024).contains(&self.source_bytes)
            && self.scale_meters.is_finite() && self.scale_meters > 0.0 && self.scale_meters <= 1_000_000.0
    }
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Default)]
pub(crate) enum TransitionRetention<'a> {
    #[default]
    None,
    ProductImage(&'a RetainedProductImage),
    GpuVariant(&'a RetainedGpuVariant),
    OriginalModel(&'a RetainedOriginalModel),
}

#[cfg(debug_assertions)]
impl<'a> TransitionRetention<'a> {
    pub(crate) fn from_proofs(image: Option<&'a RetainedProductImage>, gpu: Option<&'a RetainedGpuVariant>, original: Option<&'a RetainedOriginalModel>) -> Result<Self> {
        match (image, gpu, original) {
            (None, None, None) => Ok(Self::None),
            (Some(image), None, None) => Ok(Self::ProductImage(image)),
            (None, Some(gpu), None) => Ok(Self::GpuVariant(gpu)),
            (None, None, Some(original)) if original.valid() => Ok(Self::OriginalModel(original)),
            _ => Err(err("NATIVE_PERSONAL_TRANSITION_WRITER_PENDING")),
        }
    }
    pub(crate) fn image(self) -> Option<&'a RetainedProductImage> {
        if let Self::ProductImage(proof) = self { Some(proof) } else { None }
    }
    pub(crate) fn original(self) -> Option<&'a RetainedOriginalModel> {
        if let Self::OriginalModel(proof) = self { Some(proof) } else { None }
    }
    pub(crate) fn gpu(self) -> Option<&'a RetainedGpuVariant> {
        if let Self::GpuVariant(proof) = self { Some(proof) } else { None }
    }
}

/// Exact approved image preparation retained while the saved workspace changes payloads.
#[cfg(debug_assertions)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedProductImage {
    pub(crate) job_id: String,
    pub(crate) artwork_id: String,
    pub(crate) artwork_revision: u32,
    pub(crate) upload_idempotency_key: String,
    pub(crate) source_sha256: String,
    pub(crate) source_bytes: u64,
    pub(crate) receipt_sha256: String,
    pub(crate) transfer_sha256: String,
    pub(crate) replacement_request_id: String,
    pub(crate) replacement_attempt_path: PathBuf,
    pub(crate) replacement_attempt_sha256: String,
    pub(crate) saved_etag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recovery: Option<RetainedProductImageRecovery>,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RetainedProductImageRecovery {
    pub(crate) failed_generation: String,
    pub(crate) hayes_job_id: String,
    pub(crate) hayes_completion_frame_sha256: String,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupCancellation045 {
    pub(crate) failed_generation: String,
    pub(crate) hayes_job_id: String,
    pub(crate) hayes_completion_frame_sha256: String,
    pub(crate) saved_etag: String,
    pub(crate) transfer_sha256: String,
}

#[cfg(all(target_os = "macos", debug_assertions))]
impl RetainedProductImage {
    pub(crate) fn check_replacement_attempt(&self, document: &super::native_document::DocumentState) -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        const NAME: &str = ".automation-artwork-replacement.json";
        let invalid = || err("NATIVE_RETAINED_PRODUCT_IMAGE_ATTEMPT_CHANGED");
        let path = &self.replacement_attempt_path;
        let directory = path.parent().filter(|_| path.is_absolute() && path.ends_with(NAME)).ok_or_else(invalid)?;
        let metadata = fs::symlink_metadata(directory).map_err(|_| invalid())?;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(invalid());
        }
        let bytes = super::installation_backup::source_bytes(directory, NAME, 8192, true).map_err(|_| invalid())?;
        if digest(&bytes) != self.replacement_attempt_sha256 {
            return Err(invalid());
        }
        let attempt: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let exhibition = document.draft.as_ref().and_then(super::native_document::draft_id).ok_or_else(invalid)?;
        let studio_revision: u32 = self.saved_etag.strip_prefix("\"studio-r")
            .and_then(|value| value.split_once('-'))
            .and_then(|(revision, _)| revision.parse().ok()).ok_or_else(invalid)?;
        let expected = serde_json::json!({
            "version": 1, "operation": "artwork-replace-reviewed", "requestId": self.replacement_request_id,
            "exhibitionId": exhibition, "workspaceId": document.workspace_id, "tenantId": document.tenant_id,
            "principalId": document.principal_id, "expectedRevision": document.revision,
            "expectedEditVersion": document.saved_edit_version.ok_or_else(invalid)?,
            "command": {
                "kind": "product-image-pair-2048-512-v1", "artworkId": self.artwork_id,
                "artworkRevision": self.artwork_revision, "importJobId": self.job_id,
                "studioRevision": studio_revision, "etag": self.saved_etag, "receiptSha256": self.receipt_sha256,
            },
        });
        if attempt != expected {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedRecovery045 {
    pub(crate) hayes_job_id: String,
    pub(crate) hayes_completion_frame_sha256: String,
    pub(crate) oex_job_id: String,
    pub(crate) oex_import_runtime_generation: String,
    pub(crate) oex_retirement_sha256: String,
    pub(crate) rollback_lineage: Vec<RecoveryRollback045>,
}

#[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecoveryRollback045 {
    pub(crate) receipt_sha256: String,
    pub(crate) key_file: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restoration {
    pub id: String,
    pub state: String,
    pub backup_id: Option<String>,
}

impl InstallationController {
    /// Presentation only: the marker and optional plan are not an activation
    /// permit. Stage/activate independently revalidate their complete inputs.
    pub fn native_pending_restoration(&self, token: &str) -> Result<Option<Restoration>> {
        self.with_current(token, |service| {
            let path = service.root.join("native-restore-current.json");
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(err("NATIVE_RESTORE_INVALID")),
                Ok(_) => {}
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Marker {
                format: u32,
                id: String,
            }
            let bytes = super::installation_backup::source_bytes(
                &service.root,
                "native-restore-current.json",
                4096,
                true,
            )?;
            let marker: Marker =
                serde_json::from_slice(&bytes).map_err(|_| err("NATIVE_RESTORE_INVALID"))?;
            if marker.format != 1 || !uuid(&marker.id) {
                return Err(err("NATIVE_RESTORE_INVALID"));
            }
            let directory = service.root.join(format!("native-restore-{}", marker.id));
            private_directory(&directory)?;
            let staged = match fs::symlink_metadata(directory.join("stage.json")) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(err("NATIVE_RESTORE_INVALID")),
                Ok(_) => {
                    let bytes = super::installation_backup::source_bytes(
                        &directory,
                        "stage.json",
                        65536,
                        true,
                    )?;
                    let plan: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| err("NATIVE_RESTORE_INVALID"))?;
                    let backup = plan["backupId"]
                        .as_str()
                        .ok_or_else(|| err("NATIVE_RESTORE_INVALID"))?;
                    if plan["id"].as_str() != Some(marker.id.as_str()) || !uuid(backup) {
                        return Err(err("NATIVE_RESTORE_INVALID"));
                    }
                    Some(backup.to_owned())
                }
            };
            Ok(Some(Restoration {
                id: marker.id,
                state: if staged.is_some() {
                    "staged"
                } else {
                    "interrupted"
                }
                .into(),
                backup_id: staged,
            }))
        })
    }
}
