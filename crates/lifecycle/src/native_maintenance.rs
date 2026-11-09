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
    Select { id: String, acknowledged: bool },
    StageRestore { source: PathBuf, key_file: PathBuf },
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
    },
    Activate {
        staging: PathBuf, key_file: PathBuf,
        #[serde(default)]
        retained_product_image: Option<RetainedProductImage>,
    },
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation"))]
    ActivateSelected045 { staging: PathBuf, key_file: PathBuf, selection: SelectedRecovery045 },
    Recover {
        staging: PathBuf, key_file: PathBuf,
        #[serde(default)]
        retained_product_image: Option<RetainedProductImage>,
    },
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
    pub(crate) saved_etag: String,
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
