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
    backup: Option<super::installations::native_backup::Receipt>,
    restoration: Option<super::installations::native_maintenance::Restoration>,
}
pub struct Host {
    controller: Arc<InstallationController>,
    resources: PathBuf,
    state: Arc<Mutex<State>>,
    worker: Option<thread::JoinHandle<()>>,
    exporter: super::installations::native_static_export::Exporter,
}
enum Work {
    Prepare(Option<InitialAccountInput>),
    Start,
    Stop,
    Backup(super::installations::native_backup::Input),
    Restore(super::installations::native_restore::Input),
    Activate(String),
}
impl Host {
    pub fn new(controller: Arc<InstallationController>, resources: PathBuf) -> Self {
        Self {
            exporter: super::installations::native_static_export::Exporter::new(controller.clone()),
            controller,
            resources,
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
                backup: None,
                restoration: None,
            })),
            worker: None,
        }
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
            if state.busy || self.exporter.active() {
                return Err(err("BUSY"));
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
        Ok(())
    }
    fn snapshot(&self) -> Result<Status> {
        let state = self
            .state
            .lock()
            .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
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
                if state.operation == "activating_restore" {
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
            state.operation = match &work {
                Work::Prepare(_) => "preparing",
                Work::Start => "starting",
                Work::Stop => "stopping",
                Work::Backup(_) => "backing_up",
                Work::Restore(_) => "restoring",
                Work::Activate(_) => "activating_restore",
            }
            .into();
        }
        let shared = self.state.clone();
        let controller = self.controller.clone();
        let resources = self.resources.clone();
        self.worker = Some(thread::spawn(move || {
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
                                session.start_database_only_cancellable()?;
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
                            let result = (|| {
                                if session.status()?.postgres_pid.is_none() {
                                    session.start_database_only_cancellable()?;
                                }
                                controller.create_native_backup(&token, &mut session, input)
                            })();
                            let stopped = session.stop();
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
                            let context = controller.with_current_mutation(&token, |service| {
                                if account.is_none() {
                                    service.prepare_native_personal(&payload, Ports::default())
                                } else {
                                    service.prepare_native_installation(
                                        &payload,
                                        Ports::default(),
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
                            let (cached, existing) = {
                                let mut s =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                (s.payload.clone(), s.session.take())
                            };
                            let payload = match cached {
                                Some(p) => p,
                                None => Arc::new(Payload::open(&resources)?),
                            };
                            let mut session = match existing {
                                Some(s) => s,
                                None => controller.with_current_mutation(&token, |service| {
                                    let configured = service
                                        .native_prepared()?
                                        .ok_or_else(|| err("INITIAL_ACCOUNT_REQUIRED"))?;
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
                            let started = session.start_cancellable();
                            let requested = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .stop_requested;
                            if requested {
                                session.cancellation().store(true, Ordering::SeqCst);
                                let _ = session.stop();
                            }
                            let context = session.status()?;
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            state.runtime = Some(context);
                            state.payload = Some(payload);
                            state.session = Some(session);
                            state.cancel = None;
                            started?;
                        }
                        Work::Stop => {
                            let session = shared
                                .lock()
                                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?
                                .session
                                .take();
                            if let Some(mut session) = session {
                                let stopped = session.stop();
                                let context = session.status()?;
                                let mut state =
                                    shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                                state.runtime = Some(context);
                                state.session = Some(session);
                                stopped?;
                            }
                        }
                    }
                    Ok(())
                }));
            if let Ok(mut state) = shared.lock() {
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
                state.runtime = Some(session.status()?);
            }
        }
        drop(state);
        self.snapshot()
    }
    pub fn handle(&mut self, request: Request) -> Result<Value> {
        if request.version != 1 || request.id > 9_007_199_254_740_991 {
            return Err(err("NATIVE_PROTOCOL_INVALID"));
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
        if self.exporter.active()
            && matches!(
                &request.command,
                UiCommand::NativePrepare { .. }
                    | UiCommand::NativeStart { .. }
                    | UiCommand::NativeBackup { .. }
                    | UiCommand::NativeMaintenance { .. }
            )
        {
            return Err(err("BUSY"));
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
            self.selected(&selection_token)?;
            self.detach_stopped()?;
            return match maintenance {
                Input::CreateKey { path } => {
                    self.controller.create_backup_key(&selection_token, &path)?;
                    Ok(Value::Null)
                }
                Input::CreateRecovery { acknowledged } => {
                    let context = self.controller.create(&selection_token, acknowledged)?;
                    self.reset_selection_state()?;
                    serde_json::to_value(context).map_err(|_| err("STATE_INVALID"))
                }
                Input::Select { id, acknowledged } => {
                    let context = self
                        .controller
                        .select(&selection_token, &id, acknowledged)?;
                    self.reset_selection_state()?;
                    serde_json::to_value(context).map_err(|_| err("STATE_INVALID"))
                }
                Input::StageRestore { source, key_file } => serde_json::to_value(self.launch(
                    selection_token,
                    Work::Restore(super::installations::native_restore::Input { source, key_file }),
                )?)
                .map_err(|_| err("STATE_INVALID")),
                Input::ActivateRestore { id } => {
                    serde_json::to_value(self.launch(selection_token, Work::Activate(id))?)
                        .map_err(|_| err("STATE_INVALID"))
                }
            };
        }
        match &request.command {
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
                if state.root.as_ref() != Some(&root) || state.busy {
                    return Err(err("NATIVE_NOT_READY"));
                }
                let current = state
                    .session
                    .as_mut()
                    .ok_or_else(|| err("NATIVE_NOT_READY"))?
                    .status()?;
                if !current.readiness.ready {
                    return Err(err("NATIVE_NOT_READY"));
                }
                let payload = state
                    .payload
                    .clone()
                    .ok_or_else(|| err("NATIVE_NOT_READY"))?;
                return serde_json::to_value(self.exporter.start(
                    selection_token.clone(),
                    export.clone(),
                    payload,
                    current.ports.web,
                )?)
                .map_err(|_| err("STATE_INVALID"));
            }
            UiCommand::NativeStop { .. } => self.exporter.cancel(),
            _ => {}
        }
        if let UiCommand::NativeAuthoringSession { selection_token } = &request.command {
            let root = self.selected(selection_token)?;
            let mut state = self
                .state
                .lock()
                .map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            if state.root.as_ref() != Some(&root) || state.busy {
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
            } => self.launch(selection_token, Work::Backup(backup))?,
            UiCommand::NativePrepare {
                selection_token,
                account,
            } => self.launch(selection_token, Work::Prepare(account))?,
            UiCommand::NativeStart { selection_token } => {
                self.launch(selection_token, Work::Start)?
            }
            UiCommand::NativeStatus { selection_token } => self.status(&selection_token)?,
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
        self.exporter.cancel();
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
            drop(session);
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.shutdown();
    }
}
