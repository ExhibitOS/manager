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
    pub oex_save_offer: Option<OexSaveOffer>,
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
    selected_oex: bool,
    oex_save_offer: Option<OexSaveOffer>,
    oex_save_sender: Option<std::sync::mpsc::Sender<String>>,
    oex_terminal: Option<(OexSaveOffer, String, super::installations::native_oex_import::Transfer)>,
    oex_put_authorized: bool,
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
    state: Arc<Mutex<State>>,
    worker: Option<thread::JoinHandle<()>>,
    exporter: super::installations::native_static_export::Exporter,
    hoster: super::installations::native_hosting::Hoster,
}
enum Work {
    Prepare(Option<InitialAccountInput>),
    Start,
    RunSelectedOex,
    Stop,
    Backup(super::installations::native_backup::Input),
    Restore(super::installations::native_restore::Input),
    Activate(String),
}
impl Host {
    pub fn new(controller: Arc<InstallationController>, resources: PathBuf) -> Self {
        Self {
            exporter: super::installations::native_static_export::Exporter::new(controller.clone()),
            hoster: super::installations::native_hosting::Hoster::new(controller.clone()),
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
                selected_oex: false,
                oex_save_offer: None,
                oex_save_sender: None,
                oex_terminal: None,
                oex_put_authorized: false,
            })),
            worker: None,
        }
    }
    /// Explicit selected entry for a profile that must not already exist.
    /// The controller and its exclusively locked Session remain owned by the
    /// same Host; no existing installation is stopped or selected here.
    pub fn new_fresh_selected(profile: PathBuf, resources: PathBuf) -> Result<Self> {
        let payload = Arc::new(Payload::open(&resources)?);
        let (controller, session) = InstallationController::with_fresh_selected(profile, payload.clone(), Ports::default())?;
        let token = controller.context()?.selection_token;
        let root = controller.with_current(&token, |s| Ok(s.root.clone()))?;
        let host = Self::new(Arc::new(controller), resources);
        {
            let mut state = host.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            state.root = Some(root);
            state.payload = Some(payload);
            state.session = Some(session);
            state.selected_oex = true;
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
        state.oex_save_offer = None;
        state.oex_save_sender = None;
        state.oex_terminal = None;
        state.oex_put_authorized = false;
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
            selected_oex: state.selected_oex,
            oex_save_offer: state.oex_save_offer.clone(),
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
                Work::RunSelectedOex => "importing_oex",
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
                            let cached = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.payload.clone();
                            let payload = match cached {
                                Some(p) => p,
                                None => Arc::new(Payload::open(&resources)?),
                            };
                            let existing = shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.session.take();
                            let session = match existing {
                                Some(s) => s,
                                None => controller.with_current_mutation(&token, |service| {
                                    let configured = service
                                        .native_prepared()?
                                        .ok_or_else(|| err("INITIAL_ACCOUNT_REQUIRED"))?;
                                    Session::new(service, payload.clone(), configured.ports)
                                })?,
                            };
                            let mut lease = SessionLease { shared: shared.clone(), session: Some(session) };
                            let session = lease.session.as_mut().ok_or_else(|| err("NATIVE_HOST_UNAVAILABLE"))?;
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
                            let context = session.status();
                            let mut state =
                                shared.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
                            let status_error = match context { Ok(value) => { state.runtime = Some(value); None }, Err(error) => { state.runtime = None; Some(error) } };
                            state.payload = Some(payload);
                            if state.session.is_some() { return Err(err("NATIVE_HOST_OWNER_CONFLICT")); }
                            state.session = lease.session.take();
                            state.cancel = None;
                            started?;
                            if let Some(error) = status_error { return Err(error); }
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
                                state.session = lease.session.take();
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
        let (selected_busy, selected_import_busy) = {
            let state = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?;
            (state.selected_oex && state.busy, state.selected_oex && state.busy && state.operation == "importing_oex")
        };
        if selected_busy && !matches!(&request.command, UiCommand::NativeStatus { .. } | UiCommand::NativeStop { .. })
            && !(selected_import_busy && matches!(&request.command,
                UiCommand::DocumentState { .. } | UiCommand::SaveDocumentState { .. }
                | UiCommand::NativeOexSaveAuthorize { .. } | UiCommand::NativeOexSaveObserved { .. })) {
            return Err(err("BUSY"));
        }
        if let UiCommand::SaveOEXImportState { selection_token, transfer } = &request.command {
            let selected = self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.selected_oex;
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
                    || document.revision != current.revision || pending.request_id != offer.request_id
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
                self.state.lock().map_err(|_| err("NATIVE_HOST_UNAVAILABLE"))?.oex_terminal = None;
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
                || pending.request_id != offer.request_id || pending.etag.as_deref() != Some(offer.etag.as_str())
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
            UiCommand::NativeStop { .. } => { self.exporter.cancel(); self.hoster.cancel(); },
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
                self.launch(selection_token, Work::Start)?
            }
            UiCommand::NativeOexRun { selection_token } => self.launch(selection_token, Work::RunSelectedOex)?,
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
            drop(session);
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.shutdown();
    }
}
