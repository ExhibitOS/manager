// SPDX-License-Identifier: Apache-2.0
//! One asynchronous owner for the fixed native runtime. Presentation requests
//! remain responsive while preparation/start/stop runs on its owned worker.
use super::*;
use crate::{
    initial_account::InitialAccountInput,
    installations::{
        InstallationController,
        native_ui::{Command as UiCommand, Request},
    },
    native_installation::{Ports, Prepared},
    native_payload::Payload,
    native_runtime::{Context, Session},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OexSaveOffer { pub attempt_id: String, pub exhibition_id: String, pub etag: String, pub request_id: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OexSaveObservation { pub attempt_id: String, pub exhibition_id: String, pub etag: String, pub request_id: String }

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub busy: bool,
    pub operation: String,
    pub configuration: Option<Prepared>,
    pub account_required: bool,
    pub runtime: Option<Context>,
    pub error_code: Option<String>,
    pub backup: Option<super::installations::native_backup::Receipt>,
    pub restoration: Option<super::installations::native_maintenance::Restoration>,
    pub selected_oex: bool,
    pub oex_027_qualified: bool,
    pub oex_028_qualified: bool,
    pub oex_workspace_v6_qualified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oex_workspace_v7_qualified: Option<bool>,
    pub oex_save_offer: Option<OexSaveOffer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_image_release_configured: Option<bool>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retained_oex_cleanup: Option<crate::native_runtime::oex_retained_cleanup::Receipt>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub hayes_original_ready: bool,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hayes_receipt: Option<Value>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_ready_guard_diagnostic: Option<Value>,
}
struct State {
    root: Option<PathBuf>,
    busy: bool,
    operation: String,
    configuration: Option<Prepared>,
    account_required: bool,
    runtime: Option<Context>,
    error: Option<String>,
    payload: Option<Arc<Payload>>,
    session: Option<Session>,
    cancel: Option<Arc<AtomicBool>>,
    stop_requested: bool,
    detail_closing: bool,
    backup: Option<super::installations::native_backup::Receipt>,
    restoration: Option<super::installations::native_maintenance::Restoration>,
    selected_oex: bool,
    selected_oex_completed: bool,
    selected_oex_stop_accepted: bool,
    oex_027_qualified: bool,
    oex_028_qualified: bool,
    oex_save_offer: Option<OexSaveOffer>,
    oex_save_sender: Option<std::sync::mpsc::Sender<String>>,
    oex_terminal: Option<(OexSaveOffer, String, super::installations::native_oex_import::Transfer)>,
    oex_put_authorized: bool,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_receipt: Option<Value>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pre_ready_guard_diagnostic: Option<Value>,
}
impl State {
    fn retire_completed_oex_after_drain(&mut self) -> Option<Session> {
        if self.selected_oex && self.selected_oex_completed && self.selected_oex_stop_accepted
            && self.oex_terminal.is_none() && self.oex_save_offer.is_none()
            && self.oex_save_sender.is_none()
            && self.runtime.as_ref().is_some_and(is_drained)
        {
            let session = self.session.take()?;
            if session.selected_furnishing_import() {
                self.payload = None;
                self.oex_027_qualified = false;
                self.oex_028_qualified = false;
            }
            self.selected_oex = false;
            self.selected_oex_completed = false;
            self.selected_oex_stop_accepted = false;
            Some(session)
        } else {
            None
        }
    }
}
fn is_drained(context: &Context) -> bool {
    context.state == "stopped"
        && context.postgres_pid.is_none()
        && context.node_pid.is_none()
        && context.auxiliary_pid.is_none()
}
/// Return the unique runtime owner to Host on every early return or unwind.
/// The worker owns this lease only while Host is marked busy.
struct SessionLease {
    shared: Arc<Mutex<State>>,
    session: Option<Session>,
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else { return };
        let mut state = match self.shared.lock() { Ok(state) => state, Err(error) => error.into_inner() };
        if state.session.is_none() {
            state.session = Some(session);
        } else {
            // An impossible double owner must retain its fence, not stop a
            // potentially live process or overwrite the first owner.
            state.error = Some("NATIVE_HOST_OWNER_CONFLICT".into());
            std::mem::forget(session);
        }
    }
}
pub struct Host {
    controller: Arc<InstallationController>,
    resources: PathBuf,
    prepare_ports: Ports,
    fresh_ordinary_root: Option<(PathBuf, Arc<crate::maintenance_source::DirectoryPin>)>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    workspace_v5_maintenance_only: bool,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    retained_oex_cleanup: Option<(crate::native_runtime::oex_retained_cleanup::Selection, bool)>,
    state: Arc<Mutex<State>>,
    worker: Option<thread::JoinHandle<()>>,
    exporter: super::installations::native_static_export::Exporter,
    hoster: super::installations::native_hosting::Hoster,
}
enum Work {
    Prepare(Option<InitialAccountInput>),
    Start,
    RunSelectedOex,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    RunHayes(crate::native_runtime::hayes_ordinary::Input),
    Stop,
    Backup(super::installations::native_backup::Input),
    Restore(super::installations::native_restore::Input),
    Activate(String),
    #[cfg(debug_assertions)]
    PersonalTransition(super::installations::native_maintenance::LocalPersonalTransition),
}
impl Host {
    pub fn new(controller: Arc<InstallationController>, resources: PathBuf) -> Self {
        Self {
            exporter: super::installations::native_static_export::Exporter::new(controller.clone()),
            hoster: super::installations::native_hosting::Hoster::new(controller.clone()),
            controller,
            resources,
            prepare_ports: Ports::default(),
            fresh_ordinary_root: None,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            workspace_v5_maintenance_only: false,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            retained_oex_cleanup: None,
            state: Arc::new(Mutex::new(State {
                root: None,
                busy: false,
                operation: "idle".into(),
                configuration: None,
                account_required: true,
                runtime: None,
                error: None,
                payload: None,
                session: None,
                cancel: None,
                stop_requested: false,
                detail_closing: false,
                backup: None,
                restoration: None,
                selected_oex: false,
                selected_oex_completed: false,
                selected_oex_stop_accepted: false,
                oex_027_qualified: false,
                oex_028_qualified: false,
                oex_save_offer: None,
                oex_save_sender: None,
                oex_terminal: None,
                oex_put_authorized: false,
                #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
                hayes_receipt: None,
                #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                pre_ready_guard_diagnostic: None,
            })),
            worker: None,
        }
    }
    /// Fresh ordinary automation workspaces use their own validated ports.
    /// The CLI checks nonexistence before constructing the controller; the
    /// profile lock still supplies exclusive ownership during initialization.
    pub fn new_with_ports(controller: Arc<InstallationController>, resources: PathBuf, ports: Ports) -> Result<Self> {
        ports.validate()?;
        let mut host = Self::new(controller, resources);
        host.prepare_ports = ports;
        let token = host.controller.context()?.selection_token;
        let root = host.selected(&token)?;
        let pin = Arc::new(crate::maintenance_source::DirectoryPin::open(&root)?);
        host.fresh_ordinary_root = Some((root, pin));
        Ok(host)
    }
    /// Explicit selected entry for a profile that must not already exist.
    /// The controller and its exclusively locked Session remain owned by the
    /// same Host; no existing installation is stopped or selected here.
    pub fn new_fresh_selected(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        Self::new_fresh_selected_with_ports(profile, resources, Ports::default())
    }
    pub fn new_fresh_selected_with_ports(profile: PathBuf, resources: PathBuf, ports: Ports) -> Result<Self> {
        ports.validate()?;
        let payload = Arc::new(Payload::open(&resources)?);
        Self::from_fresh_selected_payload(profile, resources, ports, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn new_fresh_workspace_v5_validation(profile: PathBuf, resources: PathBuf, ports: Ports) -> Result<Self> {
        ports.validate()?;
        let payload = Arc::new(Payload::open_workspace_v5_validation(&resources, true)?);
        Self::from_fresh_selected_payload(profile, resources, ports, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn new_fresh_workspace_v6_validation(profile: PathBuf, resources: PathBuf, ports: Ports) -> Result<Self> {
        ports.validate()?;
        let payload = Arc::new(Payload::open_workspace_import_validation(&resources, crate::native_payload::WorkspaceImport::V6, true)?);
        Self::from_fresh_selected_payload(profile, resources, ports, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn new_fresh_workspace_v7_validation(profile: PathBuf, resources: PathBuf, ports: Ports) -> Result<Self> {
        ports.validate()?;
        let payload = Arc::new(Payload::open_workspace_import_validation(&resources, crate::native_payload::WorkspaceImport::V7, true)?);
        Self::from_fresh_selected_payload(profile, resources, ports, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn reopen_workspace_v5_validation(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        let payload = Payload::open_workspace_v5_validation(&resources, false)?;
        Self::reopen_workspace_validation(profile, resources, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn reopen_workspace_v6_validation(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        let payload = Payload::open_workspace_import_validation(&resources, crate::native_payload::WorkspaceImport::V6, false)?;
        Self::reopen_workspace_validation(profile, resources, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn reopen_workspace_v7_validation(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        let payload = Payload::open_workspace_import_validation(&resources, crate::native_payload::WorkspaceImport::V7, false)?;
        Self::reopen_workspace_validation(profile, resources, payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn reopen_workspace_v7_retained_oex_cleanup(
        profile: PathBuf, resources: PathBuf, job_id: &str, import_runtime_generation: &str,
    ) -> Result<Self> {
        let selection = crate::native_runtime::oex_retained_cleanup::Selection::new(job_id, import_runtime_generation)?;
        let payload = Payload::open_workspace_import_validation(&resources, crate::native_payload::WorkspaceImport::V7, false)?;
        if !payload.retained_oex_cleanup_hook() {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        let mut host = Self::reopen_workspace_validation(profile, resources, payload)?;
        host.retained_oex_cleanup = Some((selection, false));
        Ok(host)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    fn reopen_workspace_validation(profile: PathBuf, resources: PathBuf, payload: Payload) -> Result<Self> {
        if !profile.is_absolute() || !fs::symlink_metadata(&profile).is_ok_and(|m| m.is_dir()) {
            return Err(err("NATIVE_PROFILE_REQUIRED"));
        }
        let payload = Arc::new(payload);
        let controller = InstallationController::open_existing_managed(profile)?;
        let token = controller.context()?.selection_token;
        let prepared = controller.with_current(&token, |service| service.native_prepared())?
            .ok_or_else(|| err("NATIVE_NOT_READY"))?;
        if prepared.manifest_sha256 != payload.context().manifest_sha256 {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        let host = Self::new(Arc::new(controller), resources);
        host.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.payload = Some(payload);
        Ok(host)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub fn maintain_workspace_v5_transition(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        let target = Payload::open_saved_workspace_transition(&resources)?;
        let target_hash = target.context().manifest_sha256;
        if !crate::native_payload::saved_workspace_transition_target(&target_hash)
        {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        let controller = InstallationController::open_existing_managed(profile)?;
        let mut host = Self::new(Arc::new(controller), resources);
        host.workspace_v5_maintenance_only = true;
        Ok(host)
    }
    fn from_fresh_selected_payload(profile: PathBuf, resources: PathBuf, ports: Ports, payload: Arc<Payload>) -> Result<Self> {
        let (controller, session) = InstallationController::with_fresh_selected(profile, payload.clone(), ports)?;
        let token = controller.context()?.selection_token;
        let root = controller.with_current(&token, |s| Ok(s.root.clone()))?;
        let configuration = controller.with_current(&token, |service| service.native_prepared())?;
        let host = Self::new(Arc::new(controller), resources);
        {
            let mut state = host.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            state.root = Some(root);
            state.payload = Some(payload);
            state.session = Some(session);
            state.selected_oex = true;
            state.configuration = configuration;
            state.account_required = false;
        }
        Ok(host)
    }
    fn selected(&self, token: &str) -> Result<PathBuf> {
        self.controller.with_current(token, |s| Ok(s.root.clone()))
    }
    /// A stopped Session still owns its root lock. Explicit maintenance releases
    /// only that empty owner; it never terminates a live source implicitly.
    fn detach_stopped(&mut self) -> Result<()> {
        let session = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.busy || self.exporter.active() || self.hoster.active() {
                return Err(err("BUSY"));
            }
            if state.selected_oex && state.session.is_some() {
                return Err(err("NATIVE_OEX_SESSION_BOUND"));
            }
            if let Some(session) = state.session.as_mut() {
                let status = session.status()?;
                if status.postgres_pid.is_some()
                    || status.node_pid.is_some()
                    || status.auxiliary_pid.is_some()
                {
                    return Err(err("NATIVE_STOP_REQUIRED"));
                }
            }
            state.session.take()
        };
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
        }
        drop(session);
        Ok(())
    }
    fn reset_selection_state(&mut self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
        state.root = None;
        state.configuration = None;
        state.account_required = true;
        state.runtime = None;
        state.error = None;
        state.backup = None;
        state.restoration = None;
        state.cancel = None;
        state.stop_requested = false;
        state.selected_oex = false;
        state.selected_oex_completed = false;
        state.selected_oex_stop_accepted = false;
        state.oex_027_qualified = false;
        state.oex_028_qualified = false;
        state.oex_save_offer = None;
        state.oex_save_sender = None;
        state.oex_terminal = None;
        state.oex_put_authorized = false;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        { state.hayes_receipt = None; }
        Ok(())
    }
    fn snapshot(&self) -> Result<Status> {
        let state = self
            .state
            .lock()
            .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
        let selected_workspace_import = if state.selected_oex
            && state.runtime.as_ref().is_some_and(|runtime| runtime.state == "ready")
        {
            state.payload.as_ref().and_then(|payload| payload.selected_workspace_import_version())
        } else {
            None
        };
        Ok(Status {
            busy: state.busy,
            operation: state.operation.clone(),
            configuration: state.configuration.clone(),
            account_required: state.account_required,
            runtime: if state.busy {
                None
            } else {
                state.runtime.clone()
            },
            error_code: state.error.clone(),
            backup: state.backup.clone(),
            restoration: state.restoration.clone(),
            selected_oex: state.selected_oex,
            oex_027_qualified: state.oex_027_qualified,
            oex_028_qualified: state.oex_028_qualified,
            oex_workspace_v6_qualified: selected_workspace_import == Some(crate::native_payload::WorkspaceImport::V6),
            oex_workspace_v7_qualified: (selected_workspace_import == Some(crate::native_payload::WorkspaceImport::V7)).then_some(true),
            oex_save_offer: state.oex_save_offer.clone(),
            fresh_image_release_configured: state.configuration.as_ref().and_then(|configured| {
                crate::native_payload::fresh_image_release_manifest_matches(&configured.manifest_sha256)
                    .then_some(true)
            }),
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            retained_oex_cleanup: (!state.busy && state.error.is_none())
                .then(|| state.session.as_ref().and_then(Session::oex_retained_cleanup_receipt)).flatten(),
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            hayes_original_ready: !state.busy && state.session.as_ref().is_some_and(Session::ordinary_hayes_available),
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            hayes_receipt: state.hayes_receipt.clone(),
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            pre_ready_guard_diagnostic: state.pre_ready_guard_diagnostic.clone(),
        })
    }
    fn launch(&mut self, token: String, work: Work) -> Result<Status> {
        let root = self.selected(&token)?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.root.as_ref().is_some_and(|r| r != &root)
                && (state.busy || state.session.is_some())
            {
                return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
            }
            if matches!(work, Work::Stop) && state.busy {
                state.stop_requested = true;
                if matches!(state.operation.as_str(), "activating_restore" | "transitioning_personal_runtime") {
                    // Activation owns a final verification/commit sequence.
                    // A quit request waits for ordered completion; it is not a
                    // cancellation acknowledgement and must retain this phase.
                    drop(state);
                    return self.snapshot();
                }
                state.operation = "stopping".into();
                if let Some(cancel) = &state.cancel {
                    cancel.store(true, Ordering::SeqCst);
                }
                drop(state);
                return self.snapshot();
            }
            if state.busy {
                return Err(err("BUSY"));
            }
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        let retained_cleanup = match &mut self.retained_oex_cleanup {
            Some((selection, used)) if matches!(&work, Work::Start) => {
                if *used { return Err(err("NATIVE_PROTOCOL_INVALID")); }
                *used = true;
                Some(selection.clone())
            }
            _ => None,
        };
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            state.root = Some(root);
            state.busy = true;
            state.error = None;
            state.stop_requested = matches!(work, Work::Stop);
            state.runtime = None;
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            { state.pre_ready_guard_diagnostic = None; }
            state.operation = match &work {
                Work::Prepare(_) => "preparing",
                Work::Start => "starting",
                Work::RunSelectedOex => "importing_oex",
                #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
                Work::RunHayes(_) => {
                    state.hayes_receipt = None;
                    "importing_hayes"
                },
                Work::Stop => "stopping",
                Work::Backup(_) => "backing_up",
                Work::Restore(_) => "restoring",
                Work::Activate(_) => "activating_restore",
                #[cfg(debug_assertions)]
                Work::PersonalTransition(_) => "transitioning_personal_runtime",
            }
            .into();
        }
        let shared = self.state.clone();
        let controller = self.controller.clone();
        #[cfg(debug_assertions)]
        let saved_workspace_host = {
            #[cfg(feature = "workspace-v5-owner-validation")]
            { self.workspace_v5_maintenance_only }
            #[cfg(not(feature = "workspace-v5-owner-validation"))]
            { false }
        };
        let resources = self.resources.clone();
        let prepare_ports = self.prepare_ports;
        let fresh_ordinary_root = self.fresh_ordinary_root.clone();
        self.worker = Some(thread::spawn(move || {
            let mut preserved_backup = false;
            let mut oex_owned_session = false;
            // A panic still drops the owned Session (which contains its children)
            // before surfacing a static host error; no child handle is abandoned.
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                    match work {
                        Work::Restore(input) => {
                            let payload = Arc::new(Payload::open(&resources)?);
                            payload.backup_resources()?;
                            let mut session =
                                controller.with_current_mutation(&token, |service| {
                                    let configured = service
                                        .native_prepared()?
                                        .ok_or_else(|| err("NATIVE_NOT_READY"))?;
                                    Session::new(service, payload.clone(), configured.ports)
                                })?;
                            {
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                let cancel = session.cancellation();
                                cancel.store(state.stop_requested, Ordering::SeqCst);
                                state.cancel = Some(cancel);
                            }
                            let result = (|| {
                                session.start_restore_stage_cancellable()?;
                                controller.stage_native_restore(&token, &mut session, input)
                            })();
                            let stopped = session.stop();
                            let context = session.status()?;
                            let pending = controller.native_pending_restoration(&token)?;
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.runtime = Some(context);
                            state.payload = Some(payload);
                            // Preserve the owner if stop needs another attempt.
                            // Activation/selection explicitly detach only after
                            // status proves every owned child has terminated.
                            state.session = Some(session);
                            state.restoration = pending;
                            state.cancel = None;
                            result?;
                            stopped?;
                        }
                        Work::Activate(id) => {
                            let payload = Arc::new(Payload::open(&resources)?);
                            let result =
                                controller.activate_native_restore(&token, &id, payload.clone());
                            let pending = controller.native_pending_restoration(&token)?;
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.payload = Some(payload);
                            state.restoration = match result {
                                Ok(receipt) => {
                                    Some(super::installations::native_maintenance::Restoration {
                                        id: receipt.id,
                                        state: receipt.state,
                                        backup_id: Some(receipt.backup_id),
                                    })
                                }
                                Err(error) => {
                                    state.restoration = pending;
                                    return Err(error);
                                }
                            };
                        }
                        #[cfg(debug_assertions)]
                        Work::PersonalTransition(transition) => {
                            use super::installations::native_maintenance::LocalPersonalTransition;
                            match transition {
                                LocalPersonalTransition::Protect { source_payload, archive, key_file, recovered, upload_job_id } => {
                                    let source = Payload::open(&source_payload)?;
                                    controller.protect_selected_personal_transition(
                                        &token, &source, &archive, &key_file, &recovered, &upload_job_id,
                                    )?;
                                }
                                LocalPersonalTransition::Stage { source_payload, staging, key_file } => {
                                    let source = Payload::open(&source_payload)?;
                                    let target = Payload::open(&resources)?;
                                    controller.prepare_local_personal_runtime_transition(
                                        &token, &source, &target, &staging, &key_file,
                                    )?;
                                }
                                #[cfg(feature = "workspace-v5-owner-validation")]
                                LocalPersonalTransition::StageSaved { source_payload, staging, key_file, retained_product_image, startup_cancellation_045 } => {
                                    let source = Payload::open_saved_workspace_transition(&source_payload)?;
                                    let target = Payload::open_saved_workspace_transition(&resources)?;
                                    controller.prepare_saved_workspace_v5_transition(
                                        &token, &source, &target, &staging, &key_file, retained_product_image.as_ref(), startup_cancellation_045.as_ref(),
                                    )?;
                                }
                                LocalPersonalTransition::Activate { staging, key_file, retained_product_image, startup_cancellation_045 } => {
                                    let target = Arc::new(Payload::open(&resources)?);
                                    controller.activate_local_personal_runtime_transition(
                                        &token, target, &staging, &key_file, saved_workspace_host, retained_product_image.as_ref(), startup_cancellation_045.as_ref(),
                                    )?;
                                }
                                #[cfg(feature = "workspace-v5-owner-validation")]
                                LocalPersonalTransition::ActivateSelected045 { staging, key_file, selection } => {
                                    let target = Arc::new(Payload::open_saved_workspace_transition(&resources)?);
                                    controller.activate_selected_recovery_045(&token, target, &staging, &key_file, selection)?;
                                }
                                LocalPersonalTransition::Recover { staging, key_file, retained_product_image } => {
                                    controller.recover_local_personal_runtime_transition(
                                        &token, &staging, &key_file, saved_workspace_host, retained_product_image.as_ref(),
                                    )?;
                                }
                                #[cfg(all(feature = "workspace-v5-owner-validation", target_os = "macos"))]
                                LocalPersonalTransition::InspectPreReadyGuard { staging, key_file, failed_generation, recovery_receipt_sha256 } => {
                                    let target = Payload::open_saved_workspace_transition(&resources)?;
                                    let receipt = controller.inspect_pre_ready_guard(
                                        &token, &target, &staging, &key_file, &failed_generation, &recovery_receipt_sha256,
                                    )?;
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.pre_ready_guard_diagnostic = Some(receipt);
                                }
                            }
                        }
                        Work::Backup(input) => {
                            let cached = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .payload
                                .clone();
                            let payload = match cached {
                                Some(p) => p,
                                None => Arc::new(Payload::open(&resources)?),
                            };
                            payload.backup_resources()?;
                            let existing = {
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                state.backup = None;
                                state.session.take()
                            };
                            let had_existing_session = existing.is_some();
                            let mut session = match existing {
                                Some(s) => s,
                                None => controller.with_current_mutation(&token, |service| {
                                    // Backup never initializes an empty source as a side effect.
                                    let version = super::installation_backup::source_bytes(
                                        &service.root,
                                        "native-database/PG_VERSION",
                                        32,
                                        true,
                                    )?;
                                    if std::str::from_utf8(&version).ok().map(str::trim)
                                        != Some("18")
                                    {
                                        return Err(err("NATIVE_DATABASE_VERSION_UNSUPPORTED"));
                                    }
                                    let configured = service
                                        .native_prepared()?
                                        .ok_or_else(|| err("NATIVE_NOT_READY"))?;
                                    Session::new(service, payload.clone(), configured.ports)
                                })?,
                            };
                            {
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                let cancel = session.cancellation();
                                cancel.store(state.stop_requested, Ordering::SeqCst);
                                state.cancel = Some(cancel);
                            }
                            let mut started_database = false;
                            let result = (|| {
                                if session.status()?.postgres_pid.is_none() {
                                    started_database = true;
                                    session.start_database_only_cancellable()?;
                                }
                                controller.create_native_backup(&token, &mut session, input)
                            })();
                            // Another controller may have saved an OEX intent
                            // after host preflight. The core checks under the
                            // profile lock before any quiesce. Preserve a
                            // previously running session on this refusal.
                            let preserve_existing = had_existing_session && !started_database && result.as_ref().err().is_some_and(|e| matches!(e.code.as_str(), "NATIVE_OEX_IMPORT_PENDING" | "NATIVE_OEX_IMPORT_INVALID" | "NATIVE_OEX_IMPORT_SCOPE_CHANGED"));
                            preserved_backup = preserve_existing;
                            let stopped = if preserve_existing { Ok(()) } else { session.stop().map(|_| ()) };
                            let context = session.status()?;
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.runtime = Some(context);
                            state.payload = Some(payload);
                            state.session = Some(session);
                            state.cancel = None;
                            state.backup = Some(result?);
                            stopped?;
                        }
                        Work::Prepare(account) => {
                            let cached = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .payload
                                .clone();
                            let payload = match cached {
                                Some(p) => p,
                                None => Arc::new(Payload::open(&resources)?),
                            };
                            if payload.workspace_furnishing_validation()? && account.is_some() {
                                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
                            }
                            let context = controller.with_current_mutation(&token, |service| {
                                if account.is_none() {
                                    service.prepare_native_personal(&payload, prepare_ports)
                                } else {
                                    service.prepare_native_installation(
                                        &payload,
                                        prepare_ports,
                                        account,
                                    )
                                }
                            })?;
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.configuration = Some(Prepared {
                                kind: context.kind,
                                tenant_id: context.tenant_id,
                                subject: context.subject,
                                ports: context.ports,
                                manifest_sha256: context.manifest_sha256,
                            });
                            state.account_required = false;
                            state.payload = Some(payload);
                        }
                        Work::Start => {
                            let cached = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.payload.clone();
                            let payload = match cached {
                                Some(p) => p,
                                None => Arc::new(Payload::open(&resources)?),
                            };
                            let oex_027_qualified = payload.normal_oex_027_selected_input()?;
                            let oex_028_qualified = payload.normal_oex_028_selected_input()?;
                            let existing = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.session.take();
                            let session = match existing {
                                Some(s) => s,
                                None => controller.with_current_mutation(&token, |service| {
                                    let configured = service
                                        .native_prepared()?
                                        .ok_or_else(|| err("INITIAL_ACCOUNT_REQUIRED"))?;
                                    let mut session = Session::new(service, payload.clone(), configured.ports)?;
                                    if let Some((path, pin)) = &fresh_ordinary_root
                                        && path == &service.root && payload.workspace_furnishing_validation()?
                                    {
                                        session.authorize_fresh_workspace_furnishing(pin)?;
                                    }
                                    Ok(session)
                                })?,
                            };
                            let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                            let session = lease.session.as_mut().ok_or_else(|| err("NATIVE_HOST_UNAVAILABLE"))?;
                            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
                            if let Some(selection) = retained_cleanup { session.arm_oex_retained_cleanup(selection)?; }
                            {
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                let cancel = session.cancellation();
                                cancel.store(state.stop_requested, Ordering::SeqCst);
                                state.cancel = Some(cancel);
                            }
                            let selected = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.selected_oex;
                            let started = if selected { session.start_cancellable() } else {
                                controller.with_current_mutation(&token, |service| {
                                    controller.ensure_no_bounded_import_transition_unlocked(service)?;
                                    session.start_cancellable()
                                })
                            };
                            let requested = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .stop_requested;
                            if requested {
                                session.cancellation().store(true, Ordering::SeqCst);
                                let _ = session.stop();
                            }
                            let context = session.status();
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            let status_error = match context { Ok(value) => { state.runtime = Some(value); None }, Err(error) => { state.runtime = None; Some(error) } };
                            state.payload = Some(payload);
                            state.oex_027_qualified = oex_027_qualified;
                            state.oex_028_qualified = oex_028_qualified;
                            if state.session.is_some() { return Err(err("NATIVE_HOST_OWNER_CONFLICT")); }
                            state.session = lease.session.take();
                            state.cancel = None;
                            started?;
                            if let Some(error) = status_error { return Err(error); }
                        }
                        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
                        Work::RunHayes(input) => {
                            if !input.matches(&controller.native_upload(&token)?) {
                                return Err(err("HAYES_OPERATION_UNAVAILABLE"));
                            }
                            let session = {
                                let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                if state.selected_oex { return Err(err("HAYES_OPERATION_UNAVAILABLE")); }
                                state.session.take().ok_or_else(|| err("NATIVE_NOT_READY"))?
                            };
                            let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                            let session = lease.session.as_mut().ok_or_else(|| err("NATIVE_HOST_UNAVAILABLE"))?;
                            let cancel = session.cancellation();
                            {
                                let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                cancel.store(state.stop_requested, Ordering::SeqCst);
                                state.cancel = Some(cancel);
                            }
                            let result = session.execute_ordinary_hayes(input);
                            let requested = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.stop_requested;
                            if requested { session.stop()?; }
                            let context = session.status();
                            let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.cancel = None;
                            state.hayes_receipt = result.as_ref().ok().cloned();
                            state.runtime = Some(context?);
                            result?;
                        }
                        Work::RunSelectedOex => {
                            oex_owned_session = true;
                            let persisted = controller.native_oex_import(&token)?;
                            let session = {
                                let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                if !state.selected_oex { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
                                state.session.take().ok_or_else(|| err("NATIVE_NOT_READY"))?
                            };
                            let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                            let session = lease.session.as_mut().ok_or_else(|| err("NATIVE_HOST_UNAVAILABLE"))?;
                            let cancel = session.cancellation();
                            { let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                              cancel.store(state.stop_requested, Ordering::SeqCst); state.cancel = Some(cancel.clone()); }
                            let mut accepted_offer = None;
                            let result = session.run_fresh_oex_once(&persisted, &cancel, |exhibition, etag, request, attempt| {
                                let (sender, receiver) = std::sync::mpsc::channel();
                                let offer = OexSaveOffer { attempt_id: attempt.into(), exhibition_id: exhibition.into(), etag: etag.into(), request_id: request.into() };
                                { let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                  if state.stop_requested || state.oex_save_offer.is_some() || state.oex_save_sender.is_some() { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
                                  state.oex_save_offer = Some(offer.clone()); state.oex_save_sender = Some(sender); }
                                let deadline = Instant::now() + Duration::from_secs(110);
                                loop {
                                    if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
                                    match receiver.recv_timeout(Duration::from_millis(200)) {
                                        Ok(value) => { accepted_offer = Some((offer.clone(), value.clone())); return Ok(value); },
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
                                        Err(_) => return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")),
                                    }
                                }
                            });
                            let context = session.status();
                            let mut state = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.oex_save_offer = None; state.oex_save_sender = None; state.cancel = None;
                            let status_error = match context { Ok(value) => { state.runtime = Some(value); None }, Err(error) => { state.runtime = None; Some(error) } };
                            if state.session.is_some() { return Err(err("NATIVE_HOST_OWNER_CONFLICT")); }
                            state.session = lease.session.take();
                            if result.is_ok() && status_error.is_none() {
                                state.oex_terminal = accepted_offer.map(|(offer, etag)| (offer, etag, persisted));
                            }
                            result?;
                            if let Some(error) = status_error { return Err(error); }
                        }
                        Work::Stop => {
                            let session = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .session
                                .take();
                            if let Some(session) = session {
                                let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                                let session = lease.session.as_mut().ok_or_else(|| err("NATIVE_HOST_UNAVAILABLE"))?;
                                let stopped = session.stop();
                                let context = session.status();
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                let status_error = match context { Ok(value) => { state.runtime = Some(value); None }, Err(error) => { state.runtime = None; Some(error) } };
                                if state.session.is_some() { return Err(err("NATIVE_HOST_OWNER_CONFLICT")); }
                                // The selected Session may start only in an empty root. After
                                // durable completion and normal drain, the next Start must use
                                // the ordinary Session and its persisted transition guard.
                                state.selected_oex_stop_accepted = state.selected_oex && state.selected_oex_completed
                                    && stopped.as_ref().is_ok_and(|context| is_drained(context) || context.state == "stopping")
                                    && status_error.is_none();
                                state.session = lease.session.take();
                                let retired = state.retire_completed_oex_after_drain();
                                drop(state);
                                drop(retired);
                                stopped?;
                                if let Some(error) = status_error { return Err(error); }
                            }
                        }
                    }
                    Ok(())
                }));
            {
                let mut state = match shared.lock() { Ok(state) => state, Err(error) => error.into_inner() };
                if oex_owned_session {
                    state.oex_save_offer = None;
                    state.oex_save_sender = None;
                    state.cancel = None;
                    if !matches!(&result, Ok(Ok(()))) { state.oex_terminal = None; }
                }
                // Atomically decide whether a Stop accepted during backup must
                // stop a session preserved after an OEX refusal. Once idle is
                // published, a later Stop launches its own worker instead.
                if (preserved_backup || oex_owned_session) && state.stop_requested
                    && let Some(session) = state.session.take()
                {
                    drop(state);
                    let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let session = lease.session.as_mut().expect("owned session");
                        (session.stop(), session.status())
                    }));
                    let mut state = match shared.lock() { Ok(state) => state, Err(error) => error.into_inner() };
                    if state.session.is_some() { state.error = Some("NATIVE_HOST_OWNER_CONFLICT".into()); return; }
                    let stop_error = match stopped {
                        Ok((stopped, context)) => {
                            let status_error = match context {
                                Ok(context) => { state.runtime = Some(context); None }
                                Err(error) => { state.runtime = None; Some(error.code) }
                            };
                            stopped.err().map(|e| e.code).or(status_error)
                        }
                        Err(_) => { state.runtime = None; Some("NATIVE_HOST_UNAVAILABLE".into()) }
                    };
                    state.session = lease.session.take();
                    state.busy = false;
                    state.operation = "idle".into();
                    state.error = stop_error.or_else(|| match result {
                        Ok(Ok(())) => None,
                        Ok(Err(error)) => Some(error.code),
                        Err(_) => Some("NATIVE_HOST_UNAVAILABLE".into()),
                    });
                    return;
                }
                state.busy = false;
                state.operation = "idle".into();
                state.error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error.code),
                    Err(_) => Some("NATIVE_HOST_UNAVAILABLE".into()),
                };
            }
        }));
        self.snapshot()
    }
    pub fn status(&mut self, token: &str) -> Result<Status> {
        let root = self.selected(token)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
        if state.root.as_ref().is_some_and(|r| r != &root)
            && (state.busy || state.session.is_some())
        {
            return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
        }
        if !state.busy {
            let pending = self.controller.native_pending_restoration(token)?;
            let prepared = self.controller.with_current(token, |s| {
                match (s.native_prepared(), s.native_account_required()) {
                    (Ok(prepared), Ok(required)) => Ok((prepared, required)),
                    _ if pending.is_some() => Ok((None, false)),
                    (Err(error), _) | (_, Err(error)) => Err(error),
                }
            })?;
            if pending.is_some() {
                state.restoration = pending;
            }
            state.configuration = prepared.0;
            state.account_required = prepared.1;
            if let Some(session) = state.session.as_mut() {
                match session.status() {
                    Ok(runtime) => state.runtime = Some(runtime),
                    Err(error) => {
                        state.selected_oex_stop_accepted = false;
                        return Err(error);
                    }
                }
            }
            // A normal stop may finish after the worker returns "stopping".
            let retired = state.retire_completed_oex_after_drain();
            drop(state);
            drop(retired);
        } else {
            drop(state);
        }
        self.snapshot()
    }
    pub fn handle(&mut self, request: Request) -> Result<Value> {
        if request.version != 1 || request.id > 9_007_199_254_740_991 {
            return Err(err("NATIVE_PROTOCOL_INVALID"));
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        {
            use super::installations::native_maintenance::{Input, LocalPersonalTransition};
            let transition_entry = matches!(
                &request.command,
                UiCommand::NativeMaintenance { maintenance: Input::LocalPersonalTransition {
                    transition: LocalPersonalTransition::StageSaved { .. }
                        | LocalPersonalTransition::ActivateSelected045 { .. }, ..
                }, .. }
            );
            #[cfg(target_os = "macos")]
            let guard_diagnostic = matches!(&request.command, UiCommand::NativeMaintenance {
                maintenance: Input::LocalPersonalTransition {
                    transition: LocalPersonalTransition::InspectPreReadyGuard { .. }, ..
                }, ..
            });
            #[cfg(not(target_os = "macos"))]
            let guard_diagnostic = false;
            if (transition_entry || guard_diagnostic) && !self.workspace_v5_maintenance_only {
                return Err(err("NATIVE_PROTOCOL_INVALID"));
            }
            if self.workspace_v5_maintenance_only && !guard_diagnostic && !matches!(
                &request.command,
                UiCommand::Context | UiCommand::NativeStatus { .. } |
                UiCommand::NativeMaintenance { maintenance: Input::CreateKey { .. }, .. } |
                UiCommand::NativeMaintenance { maintenance: Input::LocalPersonalTransition {
                    transition: LocalPersonalTransition::StageSaved { .. }
                        | LocalPersonalTransition::Activate { .. }
                        | LocalPersonalTransition::ActivateSelected045 { .. }
                        | LocalPersonalTransition::Recover { .. }, ..
                }, .. }
            ) {
                return Err(err("NATIVE_PROTOCOL_INVALID"));
            }
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if self.retained_oex_cleanup.is_some() && !matches!(
            &request.command,
            UiCommand::Context | UiCommand::NativeStart { .. } |
            UiCommand::NativeStatus { .. } | UiCommand::NativeStop { .. }
        ) {
            return Err(err("NATIVE_PROTOCOL_INVALID"));
        }
        let detail_fenced = match &request.command {
            UiCommand::NativePrepare { selection_token, .. }
            | UiCommand::NativeStart { selection_token }
            | UiCommand::NativeBackup { selection_token, .. }
            | UiCommand::NativeMaintenance { selection_token, .. }
            | UiCommand::NativeOexSelect { selection_token }
            | UiCommand::NativeOexRun { selection_token }
            | UiCommand::NativeHosting { selection_token, .. }
            | UiCommand::SavePublicationState { selection_token, .. }
            | UiCommand::NativeAuthoringSession { selection_token }
            | UiCommand::StaticExport { selection_token, .. } =>
                self.exporter.detail_held(selection_token)?,
            _ => false,
        };
        if detail_fenced { return Err(err("NATIVE_STATIC_DETAIL_BUSY")); }
        let (selected_busy, selected_import_busy) = {
            let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            (state.selected_oex && state.busy, state.selected_oex && state.busy && state.operation == "importing_oex")
        };
        #[cfg(debug_assertions)]
        {
            let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.busy && state.operation == "transitioning_personal_runtime"
                && !matches!(&request.command, UiCommand::NativeStatus { .. } | UiCommand::NativeStop { .. }) {
                return Err(err("BUSY"));
            }
        }
        if selected_busy && !matches!(&request.command, UiCommand::NativeStatus { .. } | UiCommand::NativeStop { .. })
            && !(selected_import_busy && matches!(&request.command,
                UiCommand::DocumentState { .. } | UiCommand::SaveDocumentState { .. }
                | UiCommand::NativeOexSaveAuthorize { .. } | UiCommand::NativeOexSaveObserved { .. })) {
            return Err(err("BUSY"));
        }
        if let UiCommand::SaveOEXImportState { selection_token, transfer } = &request.command {
            let selected = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.selected_oex;
            if !selected && transfer.intent.as_ref().is_some_and(|i| i.profile.as_deref() == Some("bounded-file-v1")) {
                return Err(err("NATIVE_OEX_IMPORT_PENDING"));
            }
            if selected {
                let current = self.controller.native_oex_import(selection_token)?;
                if let Some(intent) = &current.intent {
                    if serde_json::to_value(&transfer.intent).map_err(|_| err("STATE_INVALID"))?
                        != serde_json::to_value(Some(intent)).map_err(|_| err("STATE_INVALID"))?
                        || transfer.file_name != current.file_name
                        || (current.job_id.is_some() && transfer.job_id != current.job_id) {
                        return Err(err("NATIVE_OEX_IMPORT_PENDING"));
                    }
                }
            }
        }
        if selected_import_busy {
            if let UiCommand::SaveDocumentState { selection_token, document } = &request.command {
                let root = self.selected(selection_token)?;
                let offer = {
                    let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                    if state.root.as_ref() != Some(&root) || state.stop_requested || state.oex_put_authorized { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
                    state.oex_save_offer.clone().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?
                };
                let current = self.controller.native_document(selection_token)?;
                let pending = document.pending.as_ref().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
                if current.draft.is_some() || current.pending.is_some() || current.etag.is_some() || current.saved_edit_version.is_some()
                    || document.revision != current.revision || pending.origin.as_deref() != Some("bounded-import-v1") || pending.request_id != offer.request_id
                    || pending.etag.as_deref() != Some(offer.etag.as_str()) || document.etag != pending.etag
                    || document.draft.as_ref() != Some(&pending.draft)
                    || pending.draft.get("exhibitionId").and_then(Value::as_str) != Some(offer.exhibition_id.as_str())
                    || pending.draft.get("editVersion").and_then(Value::as_u64) != document.saved_edit_version {
                    return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
                }
                let body = serde_json::json!({"draft": pending.draft, "requestId": pending.request_id});
                if serde_json::to_vec(&body).map_err(|_| err("STATE_INVALID"))?.len() > 1_048_576 { return Err(err("EXHIBITION_SIZE_LIMIT")); }
                let stored = self.controller.save_native_document(selection_token, (**document).clone())?;
                let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                if state.stop_requested || !state.busy || state.oex_save_offer.as_ref() != Some(&offer) { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
                return serde_json::to_value(stored).map_err(|_| err("STATE_INVALID"));
            }
        }
        if let UiCommand::SaveDocumentState { selection_token, document } = &request.command {
            let terminal = {
                let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                if state.selected_oex { state.oex_terminal.clone() } else { None }
            };
            if let Some((offer, saved_etag, transfer)) = terminal {
                let root = self.selected(selection_token)?;
                { let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                  if state.busy || state.stop_requested || state.root.as_ref() != Some(&root) || state.session.is_none() {
                      return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
                  } }
                let stored = self.controller.finish_selected_oex(selection_token, &transfer, &offer, &saved_etag, document)?;
                let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                state.session.as_mut().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?.note_selected_oex_saved();
                state.oex_terminal = None;
                state.selected_oex_completed = true;
                return serde_json::to_value(stored).map_err(|_| err("STATE_INVALID"));
            }
            let selected = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.selected_oex;
            if selected && self.controller.native_oex_import(selection_token)?.intent.is_some() {
                return Err(err("NATIVE_OEX_IMPORT_PENDING"));
            }
        }
        if let UiCommand::NativeOexSelect { selection_token } = &request.command {
            let root = self.selected(selection_token)?;
            let persisted = self.controller.native_oex_import(selection_token)?;
            let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if !state.selected_oex || state.root.as_ref() != Some(&root) || state.busy { return Err(err("NATIVE_NOT_READY")); }
            state.session.as_mut().ok_or_else(|| err("NATIVE_NOT_READY"))?.select_oex_upload_from_intent(&persisted)?;
            return Ok(serde_json::json!({"selected":true}));
        }
        if let UiCommand::NativeOexSaveAuthorize { selection_token, observation }
            | UiCommand::NativeOexSaveObserved { selection_token, observation } = &request.command {
            let authorize = matches!(&request.command, UiCommand::NativeOexSaveAuthorize { .. });
            let root = self.selected(selection_token)?;
            let document = self.controller.native_document(selection_token)?;
            let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            let offer = state.oex_save_offer.as_ref().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let pending = document.pending.as_ref().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            if !state.selected_oex || state.root.as_ref() != Some(&root) || !state.busy || state.stop_requested
                || state.operation != "importing_oex" || observation.attempt_id != offer.attempt_id
                || observation.exhibition_id != offer.exhibition_id || observation.request_id != offer.request_id
                || pending.origin.as_deref() != Some("bounded-import-v1") || pending.request_id != offer.request_id || pending.etag.as_deref() != Some(offer.etag.as_str())
                || pending.draft.get("exhibitionId").and_then(Value::as_str) != Some(offer.exhibition_id.as_str())
                || document.draft.as_ref() != Some(&pending.draft)
                || (authorize && (state.oex_put_authorized || observation.etag != offer.etag))
                || (!authorize && (!state.oex_put_authorized || observation.etag == offer.etag)) {
                return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
            }
            if authorize {
                state.oex_put_authorized = true;
            } else {
                let sender = state.oex_save_sender.take().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
                state.oex_save_offer = None;
                sender.send(observation.etag.clone()).map_err(|_| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            }
            return Ok(serde_json::json!({"accepted":true}));
        }
        let maintenance_busy = {
            let state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            state.busy
                && matches!(
                    state.operation.as_str(),
                    "backing_up" | "restoring" | "activating_restore"
                )
        };
        if maintenance_busy
            && !matches!(
                &request.command,
                UiCommand::Context
                    | UiCommand::Presentation
                    | UiCommand::NativeStatus { .. }
                    | UiCommand::NativeStop { .. }
            )
        {
            return Err(err("BUSY"));
        }
        if (self.exporter.active() || self.hoster.active())
            && matches!(
                &request.command,
                UiCommand::NativePrepare { .. }
                    | UiCommand::NativeStart { .. }
                    | UiCommand::NativeBackup { .. }
                    | UiCommand::NativeMaintenance { .. }
                    | UiCommand::StaticExport { .. }
            )
        {
            return Err(err("BUSY"));
        }
        if let UiCommand::NativeHosting { selection_token, hosting } = request.command {
            if self.exporter.active() { return Err(err("BUSY")); }
            let payload = Arc::new(Payload::open(&self.resources)?);
            return serde_json::to_value(self.hoster.start(selection_token, hosting, payload)?)
                .map_err(|_| err("STATE_INVALID"));
        }
        if let UiCommand::NativePrepare {
            selection_token, ..
        } = &request.command
            && self
                .controller
                .native_pending_restoration(selection_token)?
                .is_some()
        {
            return Err(err("NATIVE_RESTORE_PENDING"));
        }
        if let UiCommand::NativeMaintenance {
            selection_token,
            maintenance,
        } = request.command
        {
            use super::installations::native_maintenance::Input;
            let selected = self.selected(&selection_token)?;
            if matches!(&maintenance, super::installations::native_maintenance::Input::CheckOexWorkspace) {
                let payload=Payload::open(&self.resources)?;
                return serde_json::to_value(payload.normal_oex_027_selected_input()?).map_err(|_|err("STATE_INVALID"));
            }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            if let Input::RetainAbandonedOrdinaryPublicationStage { proof } = &maintenance {
                if self.exporter.detail_held(&selection_token)? { return Err(err("BUSY")); }
                let plan = self.controller.ordinary_publication_custody_plan(&selection_token, proof)?;
                let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                if state.root.as_ref() != Some(&selected) || state.busy || state.detail_closing || state.selected_oex {
                    return Err(err("NATIVE_NOT_READY"));
                }
                let session = state.session.as_mut().ok_or_else(|| err("NATIVE_NOT_READY"))?;
                return session.retain_abandoned_ordinary_publication_stage(&plan, || {
                    if self.controller.ordinary_publication_custody_plan(&selection_token, proof)? != plan {
                        return Err(err("NATIVE_PUBLICATION_STAGE_CUSTODY_UNCONFIRMED"));
                    }
                    Ok(())
                });
            }
            let _ = selected;
            self.exporter.stop_detail_child(Some(&selection_token))?;
            self.detach_stopped()?;
            return match maintenance {
                Input::CheckOexWorkspace => Err(err("NATIVE_PROTOCOL_INVALID")),
                #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
                Input::RetainAbandonedOrdinaryPublicationStage { .. } => Err(err("NATIVE_PROTOCOL_INVALID")),
                Input::CreateKey { path } => {
                    self.controller.create_backup_key(&selection_token, &path)?;
                    Ok(Value::Null)
                }
                Input::CreateRecovery { acknowledged } => {
                    let context = self.controller.create(&selection_token, acknowledged)?;
                    self.reset_selection_state()?;
                    serde_json::to_value(context).map_err(|_| err("STATE_INVALID"))
                }
                Input::CreateOexWorkspace { acknowledged } => {
                    let payload=Arc::new(Payload::open(&self.resources)?);
                    let created=self.controller.create_selected_oex(&selection_token,acknowledged,payload.clone(),Ports::default());
                    let (context,session)=match created {
                        Ok(value)=>value,
                        Err(error)=>{self.reset_selection_state()?;return Err(error);}
                    };
                    self.reset_selection_state()?;
                    let root=context.installations.iter().find(|v|v.id==context.active_id).map(|v|PathBuf::from(&v.path))
                        .ok_or_else(||err("NATIVE_OEX_CREATION_PENDING"))?;
                    { let mut state=self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                      state.oex_028_qualified=payload.normal_oex_028_selected_input()?;state.root=Some(root);state.payload=Some(payload);state.session=Some(session);state.selected_oex=true;state.oex_027_qualified=true; }
                    // Journal clearing occurs only after Host retains the exact Session.
                    self.controller.finish_selected_oex_creation(&context.selection_token)?;
                    serde_json::to_value(context).map_err(|_| err("STATE_INVALID"))
                }
                Input::Select { id, acknowledged } => {
                    let context = self
                        .controller
                        .select(&selection_token, &id, acknowledged)?;
                    self.reset_selection_state()?;
                    serde_json::to_value(context).map_err(|_| err("STATE_INVALID"))
                }
                Input::StageRestore { source, key_file, carried_restore } => serde_json::to_value(self.launch(
                    selection_token,
                    Work::Restore(super::installations::native_restore::Input { source, key_file, carried_restore }),
                )?)
                .map_err(|_| err("STATE_INVALID")),
                #[cfg(debug_assertions)]
                Input::LocalPersonalTransition { transition, acknowledged } => {
                    if !acknowledged { return Err(err("INSTALLATION_SELECTION_ACK_REQUIRED")); }
                    serde_json::to_value(self.launch(selection_token, Work::PersonalTransition(transition))?)
                        .map_err(|_| err("STATE_INVALID"))
                }
                Input::ActivateRestore { id } => {
                    serde_json::to_value(self.launch(selection_token, Work::Activate(id))?)
                        .map_err(|_| err("STATE_INVALID"))
                }
            };
        }
        match &request.command {
            UiCommand::NativeHostingReview { selection_token } => {
                return serde_json::to_value(self.hoster.review(selection_token)?)
                    .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::NativeHostingStatus { selection_token } => {
                return serde_json::to_value(self.hoster.status(selection_token)?)
                    .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::NativeHostingCancel { selection_token } => {
                self.hoster.status(selection_token)?;
                self.hoster.cancel();
                return serde_json::to_value(self.hoster.status(selection_token)?)
                    .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::StaticExportStatus { selection_token } => {
                return serde_json::to_value(self.exporter.status(selection_token)?)
                    .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::StaticExportCancel { selection_token } => {
                self.exporter.status(selection_token)?;
                self.exporter.cancel();
                return serde_json::to_value(self.exporter.status(selection_token)?)
                    .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::StaticExport {
                selection_token,
                export,
            } => {
                let root = self.selected(selection_token)?;
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                if state.root.as_ref() != Some(&root) || state.busy || state.detail_closing {
                    return Err(err("NATIVE_NOT_READY"));
                }
                let payload = state.payload.clone().ok_or_else(|| err("NATIVE_NOT_READY"))?;
                let session = state.session.as_mut().ok_or_else(|| err("NATIVE_NOT_READY"))?;
                let current = session.status()?;
                if !current.readiness.ready {
                    return Err(err("NATIVE_NOT_READY"));
                }
                let mut export = export.clone();
                export.verify_saved_head_receipt(session)?;
                let authority = if export.detail_mode.is_some() {
                    Some(session.static_detail_authority()?)
                } else {
                    None
                };
                return serde_json::to_value(self.exporter.start(
                    selection_token.clone(),
                    export,
                    payload,
                    current.ports.web,
                    authority,
                )?)
                .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::NativeStop { selection_token } => {
                let root = self.selected(selection_token)?;
                if self.exporter.detail_held(selection_token)? {
                    let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                    if state.root.as_ref().is_some_and(|selected| selected != &root) {
                        return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
                    }
                    state.detail_closing = true;
                    drop(state);
                    self.exporter.stop_detail_child(Some(selection_token))?;
                } else {
                    self.exporter.cancel();
                }
                self.hoster.cancel();
            },
            _ => {}
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if let UiCommand::NativeHayesRelease { selection_token, job_id, current_artwork_revision, finished_artwork } = &request.command {
            let root = self.selected(selection_token)?;
            let transfer = self.controller.native_upload(selection_token)?;
            let mut state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.root.as_ref() != Some(&root) || state.busy || state.detail_closing || state.selected_oex {
                return Err(err("NATIVE_NOT_READY"));
            }
            return state.session.as_mut().ok_or_else(|| err("NATIVE_NOT_READY"))?
                .recover_hayes_release(job_id, &transfer, *current_artwork_revision,
                    finished_artwork.as_ref().map(|(artwork, display)| (artwork.as_str(), display.as_str())));
        }
        if let UiCommand::NativeAuthoringSession { selection_token } = &request.command {
            let root = self.selected(selection_token)?;
            let mut state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.root.as_ref() != Some(&root) || state.busy || state.detail_closing {
                return Err(err("NATIVE_NOT_READY"));
            }
            return state
                .session
                .as_mut()
                .ok_or_else(|| err("NATIVE_NOT_READY"))?
                .authoring_session();
        }
        let status = match request.command {
            UiCommand::NativeBackup {
                selection_token,
                backup,
            } => {
                // Reject an unfinished OEX request before the worker can take
                // or stop the runtime. handle() is serialized; launch marks
                // maintenance busy before any later journal write is accepted.
                self.controller.ensure_no_pending_oex_import(&selection_token)?;
                self.launch(selection_token, Work::Backup(backup))?
            },
            UiCommand::NativePrepare {
                selection_token,
                account,
            } => self.launch(selection_token, Work::Prepare(account))?,
            UiCommand::NativeStart { selection_token } => {
                let selected = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.selected_oex;
                if !selected { self.controller.ensure_no_bounded_import_transition(&selection_token)?; }
                self.launch(selection_token, Work::Start)?
            }
            UiCommand::NativeOexRun { selection_token } => self.launch(selection_token, Work::RunSelectedOex)?,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            UiCommand::NativeHayesExecute { selection_token, input } =>
                self.launch(selection_token, Work::RunHayes(input))?,
            UiCommand::NativeStatus { selection_token } => {
                let status = self.status(&selection_token)?;
                if !status.busy && status.runtime.as_ref().is_none_or(is_drained)
                    && self.exporter.detail_held(&selection_token)?
                {
                    self.exporter.reconcile_detail_stop(&selection_token)?;
                }
                if !status.busy && !self.exporter.detail_held(&selection_token)? {
                    self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.detail_closing = false;
                }
                status
            },
            UiCommand::NativeStop { selection_token } => {
                self.launch(selection_token, Work::Stop)?
            }
            command => {
                return Request {
                    version: request.version,
                    id: request.id,
                    command,
                }
                .execute(&self.controller);
            }
        };
        serde_json::to_value(status).map_err(|_| err("STATE_INVALID"))
    }
    pub fn shutdown(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.detail_closing = true;
        }
        self.exporter.cancel();
        let detail_child_returned = self.exporter.stop_detail_child(None).is_ok();
        self.hoster.shutdown();
        if let Ok(mut state) = self.state.lock() {
            state.stop_requested = true;
            if let Some(cancel) = &state.cancel {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut state) = self.state.lock()
            && let Some(session) = state.session.take()
        {
            drop(state);
            if detail_child_returned {
                drop(session);
            } else {
                // The durable detail record still holds the reservation.
                // Do not stop the server under a child whose exit is unknown.
                std::mem::forget(session);
            }
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod selected_oex_ipc_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn request(id: u64, command: UiCommand) -> Request { Request { version: 1, id, command } }
    #[test]
    fn selected_pending_authorize_observe_and_stop_are_scoped() {
        let parent = std::env::temp_dir().join(format!("exhibitos-selected-ipc-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let controller = Arc::new(InstallationController::new(parent.join("profile"), None).unwrap());
        let token = controller.context().unwrap().selection_token;
        let root = controller.with_current(&token, |service| {
            let _guard = service.lock()?;
            let personal = serde_json::json!({"format":1,"workspaceId":Uuid::new_v4().to_string(),
                "tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
                "ownerUid":unsafe { libc::geteuid() },"ports":{"web":13200,"api":13201,"database":15432},
                "manifestSha256":"a".repeat(64),"environmentSha256":"b".repeat(64)});
            write_private_new(&service.root, "native-personal.json", &serde_json::to_vec(&personal).unwrap(), 4096)?;
            Ok(service.root.clone())
        }).unwrap();
        let mut transfer = controller.native_oex_import(&token).unwrap();
        transfer.intent = Some(crate::installations::native_oex_import::Intent {
            request_id: Uuid::new_v4().to_string(), sha256: "a".repeat(64), bytes: 67_108_865,
            profile: Some("bounded-file-v1".into()), oex_format: Some("1.0.0-draft.9".into()),
        });
        transfer.file_name = Some("synthetic.oex".into());
        transfer = controller.save_native_oex_import(&token, transfer).unwrap();
        transfer.job_id = Some(Uuid::new_v4().to_string());
        transfer = controller.save_native_oex_import(&token, transfer).unwrap();
        let exhibition = Uuid::new_v4().to_string();
        let draft = serde_json::json!({"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.1",
            "exhibitionId":exhibition,"id":Uuid::new_v4().to_string(),"candidate":{"id":exhibition},"editVersion":1});
        let old_etag = format!("\"studio-r1-{}\"", "b".repeat(64));
        let new_etag = format!("\"studio-r2-{}\"", "c".repeat(64));
        let offer = OexSaveOffer { attempt_id: Uuid::new_v4().to_string(), exhibition_id: exhibition, etag: old_etag.clone(), request_id: Uuid::new_v4().to_string() };
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut host = Host::new(controller.clone(), parent.join("unused-resources"));
        { let mut state = host.state.lock().unwrap(); state.root = Some(root); state.busy = true;
          state.operation = "importing_oex".into(); state.selected_oex = true; state.oex_save_offer = Some(offer.clone()); state.oex_save_sender = Some(sender); }
        let mut document = controller.native_document(&token).unwrap();
        document.draft = Some(draft.clone()); document.etag = Some(old_etag.clone()); document.saved_edit_version = Some(1);
        document.pending = Some(crate::installations::native_document::Pending { request_id: offer.request_id.clone(), etag: Some(old_etag.clone()), draft, origin: Some("bounded-import-v1".into()), model_candidate: None, product_image: None });
        let mut stale = document.clone(); stale.revision += 1;
        assert!(host.handle(request(1, UiCommand::SaveDocumentState { selection_token: token.clone(), document: Box::new(stale) })).is_err());
        let mut wrong_scope = document.clone(); wrong_scope.workspace_id = Uuid::new_v4().to_string();
        assert!(host.handle(request(2, UiCommand::SaveDocumentState { selection_token: token.clone(), document: Box::new(wrong_scope) })).is_err());
        assert!(controller.native_document(&token).unwrap().draft.is_none());
        let save = UiCommand::SaveDocumentState { selection_token: token.clone(), document: Box::new(document) };
        host.handle(request(3, save)).unwrap();
        let observation = |attempt: String, etag: String| OexSaveObservation { attempt_id: attempt, exhibition_id: offer.exhibition_id.clone(), etag, request_id: offer.request_id.clone() };
        { host.state.lock().unwrap().stop_requested = true; }
        assert!(host.handle(request(4, UiCommand::NativeOexSaveAuthorize { selection_token: token.clone(), observation: observation(offer.attempt_id.clone(), old_etag.clone()) })).is_err());
        { host.state.lock().unwrap().stop_requested = false; }
        assert!(host.handle(request(5, UiCommand::NativeOexSaveAuthorize { selection_token: token.clone(), observation: observation(Uuid::new_v4().to_string(), old_etag.clone()) })).is_err());
        host.handle(request(6, UiCommand::NativeOexSaveAuthorize { selection_token: token.clone(), observation: observation(offer.attempt_id.clone(), old_etag.clone()) })).unwrap();
        assert!(host.handle(request(7, UiCommand::NativeOexSaveAuthorize { selection_token: token.clone(), observation: observation(offer.attempt_id.clone(), old_etag) })).is_err());
        let mut cleared = transfer.clone(); cleared.intent = None; cleared.job_id = None; cleared.file_name = None;
        assert!(host.handle(request(8, UiCommand::SaveOEXImportState { selection_token: token.clone(), transfer: Box::new(cleared.clone()) })).is_err());
        host.handle(request(9, UiCommand::NativeOexSaveObserved { selection_token: token.clone(), observation: observation(offer.attempt_id.clone(), new_etag.clone()) })).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), new_etag);
        host.handle(request(10, UiCommand::NativeStop { selection_token: token.clone() })).unwrap();
        let doc = controller.native_document(&token).unwrap();
        assert!(doc.pending.is_some());
        assert!(host.handle(request(11, UiCommand::SaveDocumentState { selection_token: token.clone(), document: Box::new(doc) })).is_err());
        { let mut state = host.state.lock().unwrap(); state.busy = false; state.operation = "idle".into(); }
        assert!(host.handle(request(12, UiCommand::SaveOEXImportState { selection_token: token.clone(), transfer: Box::new(cleared) })).is_err());
        assert!(controller.native_oex_import(&token).unwrap().intent.is_some());
    }
}
