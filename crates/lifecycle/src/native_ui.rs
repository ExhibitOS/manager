// SPDX-License-Identifier: Apache-2.0
//! Presentation preferences only. Credentials, lifecycle jobs, and publication
//! acceptance stay in their existing stores; visiting a step never completes it.
use super::*;

const FILE: &str = "native-ui.json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Beginner,
    Expert,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    #[default]
    Prepare,
    Create,
    Select,
    Preview,
    Publish,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Presentation {
    pub format: u32,
    pub revision: u64,
    pub mode: Mode,
    pub step: Step,
    pub text_scale: u16,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            format: 1,
            revision: 0,
            mode: Mode::Beginner,
            step: Step::Prepare,
            text_scale: 100,
        }
    }
}

impl InstallationController {
    fn presentation_unlocked(&self, profile: &LifecycleService) -> Result<Presentation> {
        let path = profile.root.join(FILE);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Presentation::default()),
            Ok(m) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if m.uid() != unsafe { libc::geteuid() }
                        || m.mode() & 0o777 != 0o600
                        || m.nlink() != 1
                    {
                        return Err(err("NATIVE_UI_STATE_INVALID"));
                    }
                }
                if !m.is_file() || m.is_symlink() || m.len() > 4096 {
                    return Err(err("NATIVE_UI_STATE_INVALID"));
                }
                let state: Presentation = read_json(&path)?;
                if state.format != 1 || !matches!(state.text_scale, 100 | 125 | 150 | 175 | 200) {
                    return Err(err("NATIVE_UI_STATE_INVALID"));
                }
                Ok(state)
            }
            Err(_) => Err(err("STATE_UNAVAILABLE")),
        }
    }
    pub fn presentation(&self) -> Result<Presentation> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let _lock = profile_lock(profile)?;
        self.presentation_unlocked(profile)
    }
    pub fn save_presentation(&self, mut state: Presentation) -> Result<Presentation> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let _lock = profile_lock(profile)?;
        let current = self.presentation_unlocked(profile)?;
        if state.format != 1 || !matches!(state.text_scale, 100 | 125 | 150 | 175 | 200) {
            return Err(err("NATIVE_UI_STATE_INVALID"));
        }
        if current.revision != state.revision {
            return Err(err("NATIVE_UI_STATE_CHANGED"));
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| err("NATIVE_UI_STATE_INVALID"))?;
        write_json(&profile.root, FILE, &state)?;
        File::open(&profile.root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        Ok(state)
    }
}

#[derive(Deserialize)]
#[serde(try_from = "WireRequest")]
pub struct Request {
    pub version: u32,
    pub id: u64,
    pub command: Command,
}

pub enum Command {
    #[cfg(target_os = "macos")]
    NativeModelAdoption {
        selection_token: String,
        adoption: Box<crate::native_runtime::model_adoption::Input>,
    },
    #[cfg(target_os = "macos")]
    NativeHosting { selection_token: String, hosting: super::native_hosting::Input },
    #[cfg(target_os = "macos")]
    NativeHostingStatus { selection_token: String },
    #[cfg(target_os = "macos")]
    NativeHostingReview { selection_token: String },
    #[cfg(target_os = "macos")]
    NativeHostingCancel { selection_token: String },
    #[cfg(target_os = "macos")]
    NativeMaintenance {
        selection_token: String,
        maintenance: super::native_maintenance::Input,
    },
    #[cfg(target_os = "macos")]
    NativeBackup {
        selection_token: String,
        backup: super::native_backup::Input,
    },
    #[cfg(target_os = "macos")]
    StaticExport {
        selection_token: String,
        export: super::native_static_export::Input,
    },
    #[cfg(target_os = "macos")]
    StaticExportStatus {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    StaticExportCancel {
        selection_token: String,
    },
    Context,
    Presentation,
    SavePresentation {
        state: Presentation,
    },
    Status {
        selection_token: String,
    },
    Jobs {
        selection_token: String,
    },
    Logs {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    RuntimePayload,
    #[cfg(target_os = "macos")]
    PublicationState {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    SavePublicationState {
        selection_token: String,
        publication: Box<super::native_publication::PublicationState>,
    },
    #[cfg(target_os = "macos")]
    DocumentState {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    SaveDocumentState {
        selection_token: String,
        document: Box<super::native_document::DocumentState>,
    },
    #[cfg(target_os = "macos")]
    FurnishingThumbnailState { selection_token: String },
    #[cfg(target_os = "macos")]
    SaveFurnishingThumbnailState { selection_token: String, thumbnail: Box<super::native_furnishing_thumbnail::State> },
    #[cfg(target_os = "macos")]
    UploadState {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    SaveUploadState {
        selection_token: String,
        transfer: Box<super::native_upload::Transfer>,
    },
    #[cfg(target_os = "macos")]
    OEXImportState { selection_token: String },
    #[cfg(target_os = "macos")]
    SaveOEXImportState { selection_token: String, transfer: Box<super::native_oex_import::Transfer> },
    #[cfg(target_os = "macos")]
    NativePrepare {
        selection_token: String,
        account: Option<crate::initial_account::InitialAccountInput>,
    },
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    NativeHayesExecute {
        selection_token: String,
        input: crate::native_runtime::hayes_ordinary::Input,
    },
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    NativeHayesRelease {
        selection_token: String,
        job_id: String,
        current_artwork_revision: Option<u32>,
        finished_artwork: Option<(String, String)>,
    },
    #[cfg(target_os = "macos")]
    NativeAuthoringSession {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    NativeGpuPreviewResources {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    NativeStart {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    NativeOexSelect { selection_token: String },
    #[cfg(target_os = "macos")]
    NativeOexRun { selection_token: String },
    #[cfg(target_os = "macos")]
    NativeOexSaveAuthorize { selection_token: String, observation: crate::native_host::OexSaveObservation },
    #[cfg(target_os = "macos")]
    NativeOexSaveObserved { selection_token: String, observation: crate::native_host::OexSaveObservation },
    #[cfg(target_os = "macos")]
    NativeStatus {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    NativeStop {
        selection_token: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    #[cfg(target_os = "macos")]
    adoption: Option<crate::native_runtime::model_adoption::Input>,
    version: u32,
    id: u64,
    command: String,
    #[cfg(target_os = "macos")]
    hosting: Option<super::native_hosting::Input>,
    #[cfg(target_os = "macos")]
    maintenance: Option<super::native_maintenance::Input>,
    state: Option<Presentation>,
    selection_token: Option<String>,
    #[cfg(target_os = "macos")]
    account: Option<crate::initial_account::InitialAccountInput>,
    #[cfg(target_os = "macos")]
    transfer: Option<super::native_upload::Transfer>,
    #[cfg(target_os = "macos")]
    thumbnail: Option<super::native_furnishing_thumbnail::State>,
    #[cfg(target_os = "macos")]
    oex_import: Option<super::native_oex_import::Transfer>,
    #[cfg(target_os = "macos")]
    document: Option<super::native_document::DocumentState>,
    #[cfg(target_os = "macos")]
    publication: Option<super::native_publication::PublicationState>,
    #[cfg(target_os = "macos")]
    export: Option<super::native_static_export::Input>,
    #[cfg(target_os = "macos")]
    backup: Option<super::native_backup::Input>,
    #[cfg(target_os = "macos")]
    oex_save: Option<crate::native_host::OexSaveObservation>,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_execute: Option<crate::native_runtime::hayes_ordinary::Input>,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_job_id: Option<String>,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_current_artwork_revision: Option<u32>,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_finished_artwork_id: Option<String>,
    #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_finished_display_sha256: Option<String>,
}
impl TryFrom<WireRequest> for Request {
    type Error = &'static str;
    fn try_from(wire: WireRequest) -> std::result::Result<Self, Self::Error> {
        #[cfg(target_os = "macos")]
        if matches!(wire.command.as_str(), "furnishing_thumbnail_state" | "save_furnishing_thumbnail_state") {
            if wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.oex_import.is_some()
                || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some()
                || wire.hosting.is_some() || wire.maintenance.is_some() || wire.oex_save.is_some() || wire.adoption.is_some()
            { return Err("NATIVE_PROTOCOL_INVALID"); }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            if wire.hayes_execute.is_some() || wire.hayes_job_id.is_some() || wire.hayes_current_artwork_revision.is_some()
                || wire.hayes_finished_artwork_id.is_some() || wire.hayes_finished_display_sha256.is_some()
            { return Err("NATIVE_PROTOCOL_INVALID"); }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.thumbnail) {
                ("furnishing_thumbnail_state", None) => Command::FurnishingThumbnailState { selection_token },
                ("save_furnishing_thumbnail_state", Some(thumbnail)) => Command::SaveFurnishingThumbnailState { selection_token, thumbnail: Box::new(thumbnail) },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self { version: wire.version, id: wire.id, command });
        }
        #[cfg(target_os = "macos")]
        if wire.thumbnail.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }

        #[cfg(target_os = "macos")]
        if wire.command == "native_model_adoption" {
            if wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.oex_import.is_some()
                || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some()
                || wire.hosting.is_some() || wire.maintenance.is_some() || wire.oex_save.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            if wire.hayes_execute.is_some() || wire.hayes_job_id.is_some() || wire.hayes_current_artwork_revision.is_some()
                || wire.hayes_finished_artwork_id.is_some() || wire.hayes_finished_display_sha256.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let adoption = Box::new(wire.adoption.ok_or("NATIVE_PROTOCOL_INVALID")?);
            return Ok(Self { version: wire.version, id: wire.id, command: Command::NativeModelAdoption { selection_token, adoption } });
        }
        #[cfg(target_os = "macos")]
        if wire.adoption.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }

        #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
        if matches!(wire.command.as_str(), "native_hayes_execute" | "native_hayes_release") {
            if wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.oex_import.is_some()
                || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some()
                || wire.hosting.is_some() || wire.maintenance.is_some() || wire.oex_save.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let finished_artwork = match (wire.hayes_finished_artwork_id, wire.hayes_finished_display_sha256) {
                (None, None) => None,
                (Some(artwork), Some(display)) if wire.hayes_current_artwork_revision.is_some()
                    && crate::installations::uuid(&artwork) && crate::hash_valid(&display) => Some((artwork, display)),
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            let command = match (wire.command.as_str(), wire.hayes_execute, wire.hayes_job_id, wire.hayes_current_artwork_revision) {
                ("native_hayes_execute", Some(input), None, None) if finished_artwork.is_none() =>
                    Command::NativeHayesExecute { selection_token, input },
                ("native_hayes_release", None, Some(job_id), current_artwork_revision)
                    if crate::installations::uuid(&job_id)
                        && current_artwork_revision.is_none_or(|revision| (1..=i32::MAX as u32).contains(&revision)) =>
                    Command::NativeHayesRelease { selection_token, job_id, current_artwork_revision, finished_artwork },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self { version: wire.version, id: wire.id, command });
        }
        #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
        if wire.hayes_execute.is_some() || wire.hayes_job_id.is_some() || wire.hayes_current_artwork_revision.is_some()
            || wire.hayes_finished_artwork_id.is_some() || wire.hayes_finished_display_sha256.is_some()
        { return Err("NATIVE_PROTOCOL_INVALID"); }
        #[cfg(target_os = "macos")]
        if matches!(wire.command.as_str(), "native_oex_save_observed" | "native_oex_save_authorize") {
            if wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.oex_import.is_some()
                || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some()
                || wire.hosting.is_some() || wire.maintenance.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let observation = wire.oex_save.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = if wire.command == "native_oex_save_authorize" {
                Command::NativeOexSaveAuthorize { selection_token, observation }
            } else { Command::NativeOexSaveObserved { selection_token, observation } };
            return Ok(Self { version: wire.version, id: wire.id, command });
        }
        #[cfg(target_os = "macos")]
        if wire.oex_save.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
        #[cfg(target_os = "macos")]
        if matches!(wire.command.as_str(), "oex_import_state" | "save_oex_import_state") {
            if wire.hosting.is_some() || wire.maintenance.is_some() || wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.oex_import) {
                ("oex_import_state", None) => Command::OEXImportState { selection_token },
                ("save_oex_import_state", Some(transfer)) => Command::SaveOEXImportState { selection_token, transfer: Box::new(transfer) },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self { version: wire.version, id: wire.id, command });
        }
        #[cfg(target_os = "macos")]
        if wire.oex_import.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
        #[cfg(target_os = "macos")]
        if matches!(wire.command.as_str(), "native_hosting" | "native_hosting_status" | "native_hosting_review" | "native_hosting_cancel") {
            if wire.maintenance.is_some() || wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() || wire.document.is_some() || wire.publication.is_some() || wire.export.is_some() || wire.backup.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.hosting) {
                ("native_hosting", Some(hosting)) => Command::NativeHosting { selection_token, hosting },
                ("native_hosting_status", None) => Command::NativeHostingStatus { selection_token },
                ("native_hosting_review", None) => Command::NativeHostingReview { selection_token },
                ("native_hosting_cancel", None) => Command::NativeHostingCancel { selection_token },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self { version: wire.version, id: wire.id, command });
        }
        #[cfg(target_os = "macos")]
        if wire.hosting.is_some() { return Err("NATIVE_PROTOCOL_INVALID"); }
        #[cfg(target_os = "macos")]
        if wire.command == "native_maintenance" {
            if wire.state.is_some()
                || wire.account.is_some()
                || wire.transfer.is_some()
                || wire.document.is_some()
                || wire.publication.is_some()
                || wire.export.is_some()
                || wire.backup.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command: Command::NativeMaintenance {
                    selection_token: wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?,
                    maintenance: wire.maintenance.ok_or("NATIVE_PROTOCOL_INVALID")?,
                },
            });
        }
        #[cfg(target_os = "macos")]
        if wire.maintenance.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if wire.command == "native_backup" {
            if wire.state.is_some()
                || wire.account.is_some()
                || wire.transfer.is_some()
                || wire.document.is_some()
                || wire.publication.is_some()
                || wire.export.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command: Command::NativeBackup {
                    selection_token: wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?,
                    backup: wire.backup.ok_or("NATIVE_PROTOCOL_INVALID")?,
                },
            });
        }
        #[cfg(target_os = "macos")]
        if wire.backup.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if matches!(
            wire.command.as_str(),
            "static_export" | "static_export_status" | "static_export_cancel"
        ) {
            if wire.state.is_some()
                || wire.account.is_some()
                || wire.transfer.is_some()
                || wire.document.is_some()
                || wire.publication.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.export) {
                ("static_export", Some(export)) => Command::StaticExport {
                    selection_token,
                    export,
                },
                ("static_export_status", None) => Command::StaticExportStatus { selection_token },
                ("static_export_cancel", None) => Command::StaticExportCancel { selection_token },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command,
            });
        }
        #[cfg(target_os = "macos")]
        if wire.export.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if matches!(
            wire.command.as_str(),
            "publication_state" | "save_publication_state"
        ) {
            if wire.state.is_some()
                || wire.account.is_some()
                || wire.transfer.is_some()
                || wire.document.is_some()
            {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.publication) {
                ("publication_state", None) => Command::PublicationState { selection_token },
                ("save_publication_state", Some(publication)) => Command::SavePublicationState {
                    selection_token,
                    publication: Box::new(publication),
                },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command,
            });
        }
        #[cfg(target_os = "macos")]
        if wire.publication.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if matches!(
            wire.command.as_str(),
            "document_state" | "save_document_state"
        ) {
            if wire.state.is_some() || wire.account.is_some() || wire.transfer.is_some() {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.document) {
                ("document_state", None) => Command::DocumentState { selection_token },
                ("save_document_state", Some(document)) => Command::SaveDocumentState {
                    selection_token,
                    document: Box::new(document),
                },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command,
            });
        }
        #[cfg(target_os = "macos")]
        if wire.document.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if matches!(wire.command.as_str(), "upload_state" | "save_upload_state") {
            if wire.state.is_some() || wire.account.is_some() {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.transfer) {
                ("upload_state", None) => Command::UploadState { selection_token },
                ("save_upload_state", Some(transfer)) => Command::SaveUploadState {
                    selection_token,
                    transfer: Box::new(transfer),
                },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command,
            });
        }
        #[cfg(target_os = "macos")]
        if wire.transfer.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        #[cfg(target_os = "macos")]
        if matches!(
            wire.command.as_str(),
            "native_prepare"
                | "native_start"
                | "native_oex_select"
                | "native_oex_run"
                | "native_status"
                | "native_stop"
                | "native_authoring_session"
                | "native_gpu_preview_resources"
        ) {
            if wire.state.is_some() {
                return Err("NATIVE_PROTOCOL_INVALID");
            }
            let selection_token = wire.selection_token.ok_or("NATIVE_PROTOCOL_INVALID")?;
            let command = match (wire.command.as_str(), wire.account) {
                ("native_prepare", account) => Command::NativePrepare {
                    selection_token,
                    account,
                },
                ("native_gpu_preview_resources", None) => Command::NativeGpuPreviewResources { selection_token },
                ("native_authoring_session", None) => {
                    Command::NativeAuthoringSession { selection_token }
                }
                ("native_start", None) => Command::NativeStart { selection_token },
                ("native_oex_select", None) => Command::NativeOexSelect { selection_token },
                ("native_oex_run", None) => Command::NativeOexRun { selection_token },
                ("native_status", None) => Command::NativeStatus { selection_token },
                ("native_stop", None) => Command::NativeStop { selection_token },
                _ => return Err("NATIVE_PROTOCOL_INVALID"),
            };
            return Ok(Self {
                version: wire.version,
                id: wire.id,
                command,
            });
        }
        #[cfg(target_os = "macos")]
        if wire.account.is_some() {
            return Err("NATIVE_PROTOCOL_INVALID");
        }
        let command = match (wire.command.as_str(), wire.state, wire.selection_token) {
            ("context", None, None) => Command::Context,
            ("presentation", None, None) => Command::Presentation,
            ("save_presentation", Some(state), None) => Command::SavePresentation { state },
            ("status", None, Some(selection_token)) => Command::Status { selection_token },
            ("jobs", None, Some(selection_token)) => Command::Jobs { selection_token },
            ("logs", None, Some(selection_token)) => Command::Logs { selection_token },
            #[cfg(target_os = "macos")]
            ("runtime_payload", None, None) => Command::RuntimePayload,
            _ => return Err("NATIVE_PROTOCOL_INVALID"),
        };
        Ok(Self {
            version: wire.version,
            id: wire.id,
            command,
        })
    }
}

#[cfg(all(test, target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
mod finished_hayes_wire_tests {
    use super::*;

    #[test]
    fn finished_hayes_release_requires_complete_explicit_scope() {
        let mut wire = serde_json::json!({
            "version":1, "id":1, "command":"native_hayes_release", "selection_token":"scope",
            "hayes_job_id":"2d26a5d9-12d0-4fa2-a25b-4d322e042dd8"
        });
        let parse = |value| Request::try_from(serde_json::from_value::<WireRequest>(value).unwrap());
        assert!(matches!(parse(wire.clone()).unwrap().command, Command::NativeHayesRelease { finished_artwork: None, .. }));
        wire["hayes_finished_artwork_id"] = serde_json::json!("996d69ed-c26b-4dfc-a8ee-1d44b8514cdb");
        assert!(parse(wire.clone()).is_err());
        wire["hayes_finished_display_sha256"] = serde_json::json!("a".repeat(64));
        assert!(parse(wire.clone()).is_err());
        wire["hayes_current_artwork_revision"] = serde_json::json!(2);
        assert!(matches!(parse(wire.clone()).unwrap().command, Command::NativeHayesRelease {
            finished_artwork: Some(_), current_artwork_revision: Some(2), ..
        }));
        wire["command"] = serde_json::json!("native_status");
        assert!(parse(wire).is_err());
    }
}

impl Request {
    pub fn execute(self, controller: &InstallationController) -> Result<Value> {
        if self.version != 1 || self.id > 9_007_199_254_740_991 {
            return Err(err("NATIVE_PROTOCOL_INVALID"));
        }
        let result = match self.command {
            Command::Context => serde_json::to_value(controller.context()?),
            Command::Presentation => serde_json::to_value(controller.presentation()?),
            Command::SavePresentation { state } => {
                serde_json::to_value(controller.save_presentation(state)?)
            }
            Command::Status { selection_token } => {
                serde_json::to_value(controller.with_current(&selection_token, |s| s.status())?)
            }
            Command::Jobs { selection_token } => {
                serde_json::to_value(controller.with_current(&selection_token, |s| s.jobs())?)
            }
            Command::Logs { selection_token } => {
                serde_json::to_value(controller.with_current(&selection_token, |s| s.logs())?)
            }
            #[cfg(target_os = "macos")]
            Command::RuntimePayload => {
                let executable =
                    std::env::current_exe().map_err(|_| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?;
                let contents = executable
                    .parent()
                    .and_then(Path::parent)
                    .ok_or_else(|| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?;
                let payload = crate::native_payload::Payload::open(
                    &contents.join("Resources/native-runtime"),
                )?;
                serde_json::to_value(payload.context())
            }
            #[cfg(target_os = "macos")]
            Command::PublicationState { selection_token } => {
                serde_json::to_value(controller.native_publication(&selection_token)?)
            }
            #[cfg(target_os = "macos")]
            Command::SavePublicationState {
                selection_token,
                publication,
            } => serde_json::to_value(
                controller.save_native_publication(&selection_token, *publication)?,
            ),
            #[cfg(target_os = "macos")]
            Command::DocumentState { selection_token } => {
                serde_json::to_value(controller.native_document(&selection_token)?)
            }
            #[cfg(target_os = "macos")]
            Command::SaveDocumentState {
                selection_token,
                document,
            } => {
                serde_json::to_value(controller.save_native_document(&selection_token, *document)?)
            }
            #[cfg(target_os = "macos")]
            Command::FurnishingThumbnailState { selection_token } => serde_json::to_value(controller.native_furnishing_thumbnail(&selection_token)?),
            #[cfg(target_os = "macos")]
            Command::SaveFurnishingThumbnailState { selection_token, thumbnail } => serde_json::to_value(controller.save_native_furnishing_thumbnail(&selection_token, *thumbnail)?),
            #[cfg(target_os = "macos")]
            Command::UploadState { selection_token } => {
                serde_json::to_value(controller.native_upload(&selection_token)?)
            }
            #[cfg(target_os = "macos")]
            Command::SaveUploadState {
                selection_token,
                transfer,
            } => serde_json::to_value(controller.save_native_upload(&selection_token, *transfer)?),
            #[cfg(target_os = "macos")]
            Command::OEXImportState { selection_token } => serde_json::to_value(controller.native_oex_import(&selection_token)?),
            #[cfg(target_os = "macos")]
            Command::SaveOEXImportState { selection_token, transfer } => serde_json::to_value(controller.save_native_oex_import(&selection_token, *transfer)?),
            #[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
            Command::NativeHayesExecute { .. } | Command::NativeHayesRelease { .. } => return Err(err("NATIVE_HOST_REQUIRED")),
            #[cfg(target_os = "macos")]
            Command::NativeAuthoringSession { .. }
            | Command::NativeGpuPreviewResources { .. }
            | Command::NativeModelAdoption { .. }
            | Command::NativeHosting { .. }
            | Command::NativeHostingStatus { .. }
            | Command::NativeHostingReview { .. }
            | Command::NativeHostingCancel { .. }
            | Command::NativeMaintenance { .. }
            | Command::NativeBackup { .. }
            | Command::StaticExport { .. }
            | Command::StaticExportStatus { .. }
            | Command::StaticExportCancel { .. }
            | Command::NativePrepare { .. }
            | Command::NativeStart { .. }
            | Command::NativeOexSelect { .. }
            | Command::NativeOexRun { .. }
            | Command::NativeOexSaveAuthorize { .. }
            | Command::NativeOexSaveObserved { .. }
            | Command::NativeStatus { .. }
            | Command::NativeStop { .. } => return Err(err("NATIVE_HOST_REQUIRED")),
        };
        result.map_err(|_| err("STATE_INVALID"))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod furnishing_thumbnail_wire_tests {
    use super::*;
    fn state() -> Value {
        serde_json::json!({"format":1,"revision":0,"workspaceId":"10000000-0000-4000-8000-000000000001",
            "tenantId":"10000000-0000-4000-8000-000000000002","principalId":"10000000-0000-4000-8000-000000000003","pending":null})
    }
    #[test]
    fn furnishing_thumbnail_wire_is_closed_and_rejects_payload_on_every_other_command() {
        let token=Uuid::new_v4().to_string();
        for command in ["context","document_state","save_document_state","upload_state","save_upload_state","native_stop","native_prepare","presentation"] {
            let value=serde_json::json!({"version":1,"id":1,"command":command,"selection_token":token,"thumbnail":state()});
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        let read=serde_json::json!({"version":1,"id":1,"command":"furnishing_thumbnail_state","selection_token":token});
        assert!(serde_json::from_value::<Request>(read.clone()).is_ok());
        let mut forbidden=read;forbidden["thumbnail"]=state();assert!(serde_json::from_value::<Request>(forbidden).is_err());
    }
    #[test]
    fn furnishing_thumbnail_save_requires_exact_named_state_and_selection_token() {
        let token=Uuid::new_v4().to_string();
        let save=serde_json::json!({"version":1,"id":1,"command":"save_furnishing_thumbnail_state","selection_token":token,"thumbnail":state()});
        assert!(serde_json::from_value::<Request>(save.clone()).is_ok());
        let mut absent=save.clone();absent.as_object_mut().unwrap().remove("selection_token");assert!(serde_json::from_value::<Request>(absent).is_err());
        let mut other=save;other["document"]=serde_json::json!({"format":1,"revision":0,"workspaceId":Uuid::new_v4().to_string(),"tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),"draft":null,"etag":null,"savedEditVersion":null,"pending":null});
        assert!(serde_json::from_value::<Request>(other).is_err());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod gpu_preview_resource_wire_tests {
    use super::*;
    #[test]
    fn preview_getter_accepts_only_existing_selection_token_and_no_caller_paths() {
        let good=serde_json::json!({"version":1,"id":1,"command":"native_gpu_preview_resources","selection_token":"selected"});
        let parsed:WireRequest=serde_json::from_value(good.clone()).unwrap();
        assert!(matches!(Request::try_from(parsed).unwrap().command,Command::NativeGpuPreviewResources{..}));
        let mut missing=good.clone();missing.as_object_mut().unwrap().remove("selection_token");
        assert!(Request::try_from(serde_json::from_value::<WireRequest>(missing).unwrap()).is_err());
        for (field,value) in [("path",serde_json::json!("/caller")),("paths",serde_json::json!(["assets/a.js"])),
            ("url",serde_json::json!("https://caller.invalid/")),("payloadManifestSha256",serde_json::json!("a".repeat(64))),
            ("state",serde_json::json!({})),("account",serde_json::json!({}))] {
            let mut wrong=good.clone();wrong[field]=value;
            let result=serde_json::from_value::<WireRequest>(wrong).map_err(|_|()).and_then(|wire|Request::try_from(wire).map_err(|_|()));
            assert!(result.is_err(),"{field}");
        }
    }
}
