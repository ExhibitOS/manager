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
    UploadState {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    SaveUploadState {
        selection_token: String,
        transfer: Box<super::native_upload::Transfer>,
    },
    #[cfg(target_os = "macos")]
    NativePrepare {
        selection_token: String,
        account: Option<crate::initial_account::InitialAccountInput>,
    },
    #[cfg(target_os = "macos")]
    NativeAuthoringSession {
        selection_token: String,
    },
    #[cfg(target_os = "macos")]
    NativeStart {
        selection_token: String,
    },
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
    version: u32,
    id: u64,
    command: String,
    #[cfg(target_os = "macos")]
    maintenance: Option<super::native_maintenance::Input>,
    state: Option<Presentation>,
    selection_token: Option<String>,
    #[cfg(target_os = "macos")]
    account: Option<crate::initial_account::InitialAccountInput>,
    #[cfg(target_os = "macos")]
    transfer: Option<super::native_upload::Transfer>,
    #[cfg(target_os = "macos")]
    document: Option<super::native_document::DocumentState>,
    #[cfg(target_os = "macos")]
    publication: Option<super::native_publication::PublicationState>,
    #[cfg(target_os = "macos")]
    export: Option<super::native_static_export::Input>,
    #[cfg(target_os = "macos")]
    backup: Option<super::native_backup::Input>,
}
impl TryFrom<WireRequest> for Request {
    type Error = &'static str;
    fn try_from(wire: WireRequest) -> std::result::Result<Self, Self::Error> {
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
                | "native_status"
                | "native_stop"
                | "native_authoring_session"
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
                ("native_authoring_session", None) => {
                    Command::NativeAuthoringSession { selection_token }
                }
                ("native_start", None) => Command::NativeStart { selection_token },
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
            Command::UploadState { selection_token } => {
                serde_json::to_value(controller.native_upload(&selection_token)?)
            }
            #[cfg(target_os = "macos")]
            Command::SaveUploadState {
                selection_token,
                transfer,
            } => serde_json::to_value(controller.save_native_upload(&selection_token, *transfer)?),
            #[cfg(target_os = "macos")]
            Command::NativeAuthoringSession { .. }
            | Command::NativeMaintenance { .. }
            | Command::NativeBackup { .. }
            | Command::StaticExport { .. }
            | Command::StaticExportStatus { .. }
            | Command::StaticExportCancel { .. }
            | Command::NativePrepare { .. }
            | Command::NativeStart { .. }
            | Command::NativeStatus { .. }
            | Command::NativeStop { .. } => return Err(err("NATIVE_HOST_REQUIRED")),
        };
        result.map_err(|_| err("STATE_INVALID"))
    }
}
