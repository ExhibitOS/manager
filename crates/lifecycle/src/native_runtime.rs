// SPDX-License-Identifier: Apache-2.0
//! Owned PostgreSQL/Node lifecycle for a prepared native installation. No shell,
//! container engine, PATH lookup, PID adoption, or destructive recovery.
use super::*;
use super::{native_installation::Ports, native_payload::{Payload, WorkspaceFurnishingCatalog}};
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
use crate::native_payload::FurnishingTransition;
#[path = "native_runtime_oex_worker.rs"] mod oex_worker;
#[path = "native_runtime_oex_worker_v2.rs"] mod oex_worker_v2;
#[path = "native_runtime_oex_preflight.rs"] mod oex_preflight;
#[path = "native_runtime_oex_format_hint.rs"] mod oex_format_hint;
/// Fixed helper-only read-only ingress; no profile or lifecycle is instantiated.
pub fn selected_oex_format(resources:&Path,input_fd:i32,cancelled:&AtomicBool)->Result<Vec<u8>> {
    oex_format_hint::run(&Payload::open(resources)?,input_fd,cancelled)
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
pub fn selected_oex_format_v5_validation(resources: &Path, input_fd: i32, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    oex_format_hint::run(&Payload::open_workspace_v5_validation(resources, true)?, input_fd, cancelled)
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
pub fn selected_oex_format_v6_validation(resources: &Path, input_fd: i32, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    oex_format_hint::run(&Payload::open_workspace_import_validation(resources, crate::native_payload::WorkspaceImport::V6, true)?, input_fd, cancelled)
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
pub fn selected_oex_format_v7_validation(resources: &Path, input_fd: i32, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    oex_format_hint::run(&Payload::open_workspace_import_validation(resources, crate::native_payload::WorkspaceImport::V7, true)?, input_fd, cancelled)
}
#[path = "native_runtime_oex_preflight_frames.rs"] mod oex_preflight_frames;
#[path = "native_runtime_oex_binding.rs"] mod oex_binding;
#[path = "native_runtime_oex_read_fd.rs"] mod oex_read_fd;
#[path = "native_runtime_oex_stage_root.rs"] mod oex_stage_root;
#[path = "native_runtime_oex_reservation.rs"] mod oex_reservation;
#[path = "native_runtime_oex_ack.rs"] mod oex_ack;
#[path = "native_runtime_oex_ack_v2.rs"] mod oex_ack_v2;
#[path = "native_runtime_oex_control_v2.rs"] mod oex_control_v2;
#[path = "native_runtime_oex_chunks.rs"] mod oex_chunks;
#[path = "native_runtime_oex_materializer_fds.rs"] mod oex_materializer_fds;
#[path = "native_runtime_oex_materializer_protocol.rs"] mod oex_materializer_protocol;
#[path = "native_runtime_oex_materializer.rs"] mod oex_materializer;
#[path = "native_runtime_owner_channel.rs"] mod owner_channel;
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
#[allow(dead_code)] // Inactive pending native containment and owner-channel integration.
#[path = "native_runtime_hayes.rs"] mod hayes;
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
use hayes::operation::{HayesBootstrap, RetainedHayes};
use owner_channel::HayesSessionPin;
#[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
#[path = "native_runtime_hayes_ordinary.rs"]
pub(crate) mod hayes_ordinary;
#[cfg(all(target_os = "macos", feature = "workspace-v5-owner-validation", debug_assertions))]
#[path = "native_runtime_hayes_recovery.rs"]
mod hayes_recovery;
#[path = "native_runtime_furnishing_lifetime.rs"] mod furnishing_lifetime;
pub(crate) use furnishing_lifetime::WriterIdentity as FurnishingWriterIdentity;
#[path = "native_runtime_furnishing_lease.rs"] mod furnishing_lease;
#[path = "native_runtime_furnishing_lifecycle.rs"] mod furnishing_lifecycle;
use furnishing_lifecycle::{Admission, Binding, Owner as FurnishingOwner, Phase, Restore as FurnishingRestore};
#[path = "native_runtime_oex_owner.rs"] mod oex_owner;
#[path = "native_runtime_oex_recovery.rs"] pub(crate) mod oex_recovery;
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[path = "native_runtime_oex_retained_cleanup.rs"] pub(crate) mod oex_retained_cleanup;
#[path = "native_runtime_oex_catalog_frames.rs"] mod oex_catalog_frames;
#[path = "native_runtime_owner_claim_frames.rs"] mod owner_claim_frames;
#[path = "native_runtime_owner_metadata_frames.rs"] mod owner_metadata_frames;
#[path = "native_runtime_owner_binding_encoding.rs"] mod owner_binding_encoding;
#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[path = "native_runtime_hayes_042_probe.rs"]
mod hayes_042_probe;
#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[path = "native_runtime_hayes_probe_source.rs"]
mod hayes_probe_source;

pub(crate) const WORKSPACE_FURNISHING_INITIALIZATION: &str = "native-workspace-furnishing-initialization.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum FurnishingStart { Initialize, Reopen, SelectedImport038, SelectedImport039, SelectedImport040, Upgrade039To040, Upgrade040To044, Upgrade044To045 }

#[derive(Clone, Copy)]
enum StartMode<'a> {
    Services,
    Database,
    RestoreStage,
    RestoreActivation(&'a str),
}

impl FurnishingStart {
    fn selected_import(catalog: WorkspaceFurnishingCatalog) -> Option<Self> {
        match catalog {
            WorkspaceFurnishingCatalog::Publication038 => Some(Self::SelectedImport038),
            WorkspaceFurnishingCatalog::MaterialPublication039 => Some(Self::SelectedImport039),
            WorkspaceFurnishingCatalog::MaterialMultimap040 => Some(Self::SelectedImport040),
            _ => None,
        }
    }

    fn is_selected_import(self) -> bool {
        matches!(self, Self::SelectedImport038 | Self::SelectedImport039 | Self::SelectedImport040)
    }

    fn opens_existing(self) -> bool {
        matches!(self, Self::Reopen | Self::Upgrade039To040 | Self::Upgrade040To044 | Self::Upgrade044To045)
    }

    fn owner_mode(self) -> &'static str {
        match self {
            Self::Initialize => "workspace-furnishing-036-initialize-v1",
            Self::Reopen => "workspace-furnishing-036-reopen-v1",
            Self::SelectedImport038 => "workspace-furnishing-038-selected-import-v7",
            Self::SelectedImport039 => "workspace-furnishing-039-selected-import-v7",
            Self::SelectedImport040 => "workspace-furnishing-040-selected-import-v7",
            Self::Upgrade039To040 => "workspace-furnishing-039-to-040-upgrade-v1",
            Self::Upgrade040To044 => "workspace-furnishing-040-to-044-upgrade-v1",
            Self::Upgrade044To045 => "workspace-furnishing-044-to-045-upgrade-v1",
        }
    }
}
fn native_port_bind_code(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "NATIVE_PORT_IN_USE",
        std::io::ErrorKind::PermissionDenied => "NATIVE_PORT_ACCESS_DENIED",
        _ => "NATIVE_PORT_BIND_FAILED",
    }
}
#[cfg(test)]
#[path = "native_runtime_oex_v6_wire_tests.rs"]
mod oex_v6_wire_tests;
#[cfg(test)]
mod port_bind_diagnostic_tests {
    #[test]
    fn permission_failure_is_not_reported_as_foreign_port_owner() {
        use std::io::{Error, ErrorKind};
        assert_eq!(super::native_port_bind_code(&Error::from(ErrorKind::AddrInUse)), "NATIVE_PORT_IN_USE");
        assert_eq!(super::native_port_bind_code(&Error::from(ErrorKind::PermissionDenied)), "NATIVE_PORT_ACCESS_DENIED");
        assert_eq!(super::native_port_bind_code(&Error::from_raw_os_error(libc::EPERM)), "NATIVE_PORT_ACCESS_DENIED");
        assert_eq!(super::native_port_bind_code(&Error::from(ErrorKind::AddrNotAvailable)), "NATIVE_PORT_BIND_FAILED");
    }
}
use std::{
    collections::BTreeMap,
    net::TcpListener,
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
fn cancelled() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAuthority {
    format: u32,
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
    owner_uid: u32,
    session_token: String,
    csrf_token: String,
}
/// A synchronous borrow used only to construct the owned exporter's private startup frame.
pub(crate) struct StaticDetailAuthority<'a> {
    pub(crate) workspace_id: &'a str,
    pub(crate) tenant_id: &'a str,
    pub(crate) principal_id: &'a str,
    pub(crate) runtime_generation: &'a str,
    pub(crate) port: u16,
    pub(crate) session_token: &'a str,
    pub(crate) csrf_token: &'a str,
}
impl Drop for LocalAuthority {
    fn drop(&mut self) {
        for value in [&mut self.session_token, &mut self.csrf_token] {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalSessionRenewal<'a> {
    format: u32,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    owner_uid: u32,
    previous_session_token: &'a str,
    session_token: &'a str,
    csrf_token: &'a str,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalSessionRenewalReceipt {
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
}
fn local_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(err("NATIVE_RANDOM_UNAVAILABLE"));
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(43);
    for block in bytes.chunks(3) {
        let a = block[0];
        let b = *block.get(1).unwrap_or(&0);
        let c = *block.get(2).unwrap_or(&0);
        output.push(alphabet[(a >> 2) as usize] as char);
        output.push(alphabet[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        if block.len() > 1 {
            output.push(alphabet[(((b & 15) << 2) | (c >> 6)) as usize] as char);
        }
        if block.len() > 2 {
            output.push(alphabet[(c & 63) as usize] as char);
        }
    }
    bytes.fill(0);
    Ok(output)
}

struct Environment(BTreeMap<String, String>);
impl Drop for Environment {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
struct Log {
    file: Mutex<(File, usize)>,
    truncated: AtomicBool,
}
impl Log {
    fn write(&self, bytes: &[u8]) {
        if let Ok(mut state) = self.file.lock() {
            let remaining = (1024 * 1024usize).saturating_sub(state.1);
            let count = bytes.len().min(remaining);
            if state.0.write_all(&bytes[..count]).is_err() || count != bytes.len() {
                self.truncated.store(true, Ordering::Relaxed);
            }
            state.1 += count;
        } else {
            self.truncated.store(true, Ordering::Relaxed);
        }
    }
}
struct Owned {
    child: Child,
    term_sent: bool,
    term_delivered: bool,
    capture: Option<Arc<Mutex<Vec<u8>>>>,
    stdout_thread: Option<thread::JoinHandle<()>>,
}
impl Owned {
    fn signal(&mut self) -> Result<()> {
        if !self.term_sent
            && self
                .child
                .try_wait()
                .map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))?
                .is_none()
        {
            let result = unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                return Err(err("NATIVE_STOP_UNAVAILABLE"));
            }
            self.term_delivered = result == 0;
            self.term_sent = true;
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        use std::os::unix::process::ExitStatusExt;
        let Some(exit) = self
            .child
            .try_wait()
            .map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))?
        else {
            return Ok(None);
        };
        if self.capture.is_some()
            && let Some(reader) = self.stdout_thread.take()
        {
            let _ = reader.join();
        }
        Ok(Some(exit.code().unwrap_or(-exit.signal().unwrap_or(1))))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub format: u32,
    pub job_id: Option<String>,
    pub state: String,
    pub ports: Ports,
    pub readiness: Readiness,
    pub error_code: Option<String>,
    pub failure_code: Option<String>,
    pub postgres_pid: Option<u32>,
    pub node_pid: Option<u32>,
    pub auxiliary_pid: Option<u32>,
    pub exits: BTreeMap<String, i32>,
    pub log_truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    format: u32,
    id: String,
    #[serde(default, deserialize_with = "read_furnishing_binding", skip_serializing_if = "Option::is_none")]
    furnishing: Option<Binding>,
}
fn read_furnishing_binding<'de, D: serde::Deserializer<'de>>(input: D) -> std::result::Result<Option<Binding>, D::Error> {
    Binding::deserialize(input).map(Some)
}

fn current_run(root: &Path) -> Result<Option<Current>> {
    if !exists(&root.join("native-current-run.json"))? {
        return Ok(None);
    }
    let current: Current = serde_json::from_slice(&installation_backup::source_bytes(
        root, "native-current-run.json", 4096, true,
    )?).map_err(|_| err("NATIVE_JOURNAL_INVALID"))?;
    if current.format != 1 || !installations::uuid(&current.id) {
        return Err(err("NATIVE_JOURNAL_INVALID"));
    }
    if let Some(binding) = &current.furnishing {
        binding.check()?;
    }
    Ok(Some(current))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FurnishingCompletion {
    binding_sha256: String,
    consumed_by: Option<String>,
}

fn read_run(root: &Path, generation: &str) -> Result<(Context, Option<FurnishingCompletion>)> {
    let invalid = || err("NATIVE_JOURNAL_INVALID");
    if !installations::uuid(generation) { return Err(invalid()); }
    let relative = format!("native-runs/{generation}/state.json");
    let mut record: Value = serde_json::from_slice(&installation_backup::source_bytes(root, &relative, 8192, true)?)
        .map_err(|_| invalid())?;
    let proof = record.as_object_mut().ok_or_else(invalid)?.remove("furnishingCompletion")
        .map(serde_json::from_value).transpose().map_err(|_| invalid())?;
    let context: Context = serde_json::from_value(record).map_err(|_| invalid())?;
    if context.format != 1 || context.job_id.as_deref() != Some(generation) {
        return Err(invalid());
    }
    Ok((context, proof))
}

fn write_run(root: &Path, context: &Context, completion: Option<FurnishingCompletion>) -> Result<()> {
    if let Some(completion) = completion {
        let mut record = serde_json::to_value(context).map_err(|_| err("NATIVE_JOURNAL_INVALID"))?;
        record["furnishingCompletion"] = serde_json::to_value(completion).map_err(|_| err("NATIVE_JOURNAL_INVALID"))?;
        write_json(root, "state.json", &record)?;
    } else {
        write_json(root, "state.json", context)?;
    }
    File::open(root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))
}

fn unconsumed_furnishing_completion(root: &Path, prior: &Binding) -> Result<(Context, FurnishingCompletion)> {
    let invalid = || err(furnishing_lifecycle::ERROR);
    let (context, completion) = read_run(root, &prior.generation)?;
    let proof = completion.ok_or_else(invalid)?;
    if proof.consumed_by.is_some() || proof.binding_sha256 != prior.sha256()? || context.state != "stopped"
        || context.failure_code.is_some() || context.exits.get("postgres") != Some(&0)
        || context.node_pid.is_some() || context.postgres_pid.is_some() || context.auxiliary_pid.is_some()
        || (prior.phase == Phase::Clean && context.exits.get("node") != Some(&0))
        || (prior.phase == Phase::ActivatedRestore && context.exits.contains_key("node"))
        || prior.phase == Phase::Active
    {
        return Err(invalid());
    }
    Ok((context, proof))
}

fn consume_furnishing_completion(root: &Path, prior: &Binding, next: &str) -> Result<()> {
    if !installations::uuid(next) || next == prior.generation {
        return Err(err(furnishing_lifecycle::ERROR));
    }
    let (context, mut proof) = unconsumed_furnishing_completion(root, prior)?;
    proof.consumed_by = Some(next.into());
    write_run(&root.join("native-runs").join(&prior.generation), &context, Some(proof))
}

fn validate_cold_furnishing_source(root: &Path, prior: Option<&Binding>, live: &Binding) -> Result<()> {
    let prior = prior.filter(|prior| prior.phase == Phase::Clean && prior.same_installation(live))
        .ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
    unconsumed_furnishing_completion(root, prior)?;
    Ok(())
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn catalog45_failed_reopen_shape(failed: &Context, startup_cancelled: bool) -> bool {
    let code = if startup_cancelled { "NATIVE_CANCELLED" } else { furnishing_lifecycle::ERROR };
    let node_exit = if startup_cancelled { -15 } else { 0 };
    failed.state == "failed"
        && failed.postgres_pid.is_none() && failed.node_pid.is_none() && failed.auxiliary_pid.is_none()
        && failed.failure_code.as_deref() == Some(code) && failed.error_code.as_deref() == Some(code)
        && failed.exits.get("postgres") == Some(&0) && failed.exits.get("auxiliary") == Some(&0)
        && failed.exits.get("node") == Some(&node_exit)
        && (!startup_cancelled || (!failed.readiness.ready && failed.exits.len() == 3))
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn check_catalog45_recovery_source(root: &Path, source: &Current, startup_cancelled: bool) -> Result<()> {
    let rejected = || err(furnishing_lifecycle::ERROR);
    let binding = source.furnishing.as_ref().ok_or_else(rejected)?;
    let predecessor = binding.previous_generation.as_deref().ok_or_else(rejected)?;
    if source.id != binding.generation || binding.phase != Phase::Active
        || binding.admission_kind != Admission::CleanReopen || binding.catalog != 45
        || binding.restore.is_some()
    {
        return Err(rejected());
    }
    let (failed, completion) = read_run(root, &source.id)?;
    if completion.is_some() || !catalog45_failed_reopen_shape(&failed, startup_cancelled) {
        return Err(rejected());
    }
    let (prior, prior_completion) = read_run(root, predecessor)?;
    let prior_completion = prior_completion.ok_or_else(rejected)?;
    if prior.state != "stopped" || prior.failure_code.is_some() || prior.ports != failed.ports
        || prior.postgres_pid.is_some() || prior.node_pid.is_some() || prior.auxiliary_pid.is_some()
        || ["postgres", "node"].iter().any(|name| prior.exits.get(*name) != Some(&0))
        || !hash_valid(&prior_completion.binding_sha256)
        || prior_completion.consumed_by.as_deref() != Some(source.id.as_str())
    {
        return Err(rejected());
    }
    Ok(())
}

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[test]
fn catalog45_cancelled_startup_is_separate_from_unqualified_stop() {
    let mut context = Context {
        format: 1, job_id: None, state: "failed".into(),
        ports: Ports { web: 56851, api: 56852, database: 56853 },
        readiness: Readiness::default(), error_code: Some("NATIVE_CANCELLED".into()),
        failure_code: Some("NATIVE_CANCELLED".into()),
        postgres_pid: None, node_pid: None, auxiliary_pid: None,
        exits: BTreeMap::from([("postgres".into(), 0), ("node".into(), -15), ("auxiliary".into(), 0)]),
        log_truncated: false,
    };
    assert!(catalog45_failed_reopen_shape(&context, true));
    assert!(!catalog45_failed_reopen_shape(&context, false));
    context.readiness.ready = true;
    assert!(!catalog45_failed_reopen_shape(&context, true));
    context.readiness.ready = false;
    context.exits.insert("node".into(), -9);
    assert!(!catalog45_failed_reopen_shape(&context, true));
    context.exits.insert("node".into(), 0);
    context.error_code = Some(furnishing_lifecycle::ERROR.into());
    context.failure_code = context.error_code.clone();
    assert!(catalog45_failed_reopen_shape(&context, false));
    assert!(!catalog45_failed_reopen_shape(&context, true));
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn catalog45_recovery_source_generation(root: &Path, startup_cancelled: bool) -> Result<String> {
    let source = current_run(root)?.ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
    check_cold_transition_root(root, Some(&source.id))?;
    check_catalog45_recovery_source(root, &source, startup_cancelled)?;
    Ok(source.id)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FurnishingTransitionTrial {
    source: Current,
    generation: String,
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) use furnishing_lifetime::WriterLifetime as FurnishingTransitionWriterGuard;

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
impl FurnishingTransitionTrial {
    pub(crate) fn capture(service: &LifecycleService, transition: FurnishingTransition) -> Result<Self> {
        let source = current_run(&service.root)?.ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        let trial = Self { source, generation: Uuid::new_v4().to_string() };
        trial.hold_source(service, transition)?;
        Ok(trial)
    }

    pub(crate) fn renew_generation(&mut self) {
        self.generation = Uuid::new_v4().to_string();
    }

    fn source_binding(&self, transition: FurnishingTransition) -> Result<&Binding> {
        let prior = self.source.furnishing.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        prior.check()?;
        let source_phase = if matches!(transition, FurnishingTransition::Recover45 { .. }) {
            Phase::Active
        } else {
            Phase::Clean
        };
        if self.source.format != 1 || !installations::uuid(&self.source.id)
            || !installations::uuid(&self.generation) || self.generation == prior.generation
            || prior.phase != source_phase || prior.restore.is_some()
            || !prior.matches_catalog_tuple(transition.source_catalog().version(), transition.source())
        {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        Ok(prior)
    }

    pub(crate) fn hold_source(&self, service: &LifecycleService, transition: FurnishingTransition) -> Result<FurnishingTransitionWriterGuard> {
        let (guard, observed) = self.hold(service, transition)?;
        if observed != self.source {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        if matches!(transition, FurnishingTransition::Recover45 { .. }) {
            check_cold_transition_root(&service.root, Some(&self.source.id))?;
            check_catalog45_recovery_source(&service.root, &self.source, transition.startup_cancel_recovery())?;
        } else {
            unconsumed_furnishing_completion(&service.root, self.source_binding(transition)?)?;
        }
        Ok(guard)
    }

    fn hold(&self, service: &LifecycleService, transition: FurnishingTransition) -> Result<(FurnishingTransitionWriterGuard, Current)> {
        let guard = FurnishingTransitionWriterGuard::acquire_existing(&service.root)?;
        let prior = self.source_binding(transition)?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        if prior.physical != guard.identity()? || prior.owner != (FurnishingOwner {
            workspace_id, tenant_id, principal_id, owner_uid: unsafe { libc::geteuid() },
        }) {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        let observed = current_run(&service.root)?.ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        if observed != self.source {
            let target = observed.furnishing.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
            if observed.id != self.generation || target.generation != self.generation
                || !matches!(target.phase, Phase::Active | Phase::Clean)
                || target.owner != prior.owner || target.physical != prior.physical
                || target.previous_generation.as_deref() != Some(prior.generation.as_str())
                || !target.matches_catalog_tuple(transition.target_catalog().version(), transition.target())
                || target.admission_kind != Binding::transition_admission(transition) || target.restore.is_some()
            {
                return Err(err(furnishing_lifecycle::ERROR));
            }
        }
        Ok((guard, observed))
    }

    // Called with the operation fence held and after the trial's processes
    // have drained. Never remove the trial run or its diagnostic evidence.
    pub(crate) fn hold_recovery(&self, service: &LifecycleService, transition: FurnishingTransition) -> Result<FurnishingTransitionWriterGuard> {
        let (guard, _) = self.hold(service, transition)?;
        if matches!(transition, FurnishingTransition::Recover45 { .. }) {
            check_catalog45_recovery_source(&service.root, &self.source, transition.startup_cancel_recovery())?;
        } else {
            self.recoverable_source_completion(&service.root, transition)?;
        }
        Ok(guard)
    }

    pub(crate) fn check_recovery_root(
        &self,
        root: &Path,
        guard: &FurnishingTransitionWriterGuard,
        transition: FurnishingTransition,
        retained: Option<&crate::installations::native_maintenance::RetainedProductImage>,
    ) -> Result<()> {
        let cold = || err("NATIVE_PHYSICAL_CHECKPOINT_NOT_COLD");
        if guard.identity()? != self.source_binding(transition)?.physical {
            return Err(cold());
        }
        check_interrupted_root(root)?;
        if exists(&root.join("native-database/postmaster.pid"))? {
            return Err(cold());
        }
        let current = current_run(root)?.ok_or_else(cold)?;
        if current == self.source {
            if matches!(transition, FurnishingTransition::Recover45 { .. }) {
                return check_cold_transition_root(root, Some(&self.source.id));
            }
            return check_cold_checkpoint_root(root);
        }
        if current.id != self.generation {
            return Err(cold());
        }
        let binding = current.furnishing.as_ref().ok_or_else(cold)?;
        let (context, _) = read_run(root, &current.id)?;
        if check_cold_checkpoint_root(root).is_ok() {
            return Ok(());
        }
        // Starting is durable before consuming the source. start_inner records
        // starting_database before its first possible spawn for an existing DB.
        if binding.phase == Phase::Active && context.state == "starting"
            && context.exits.is_empty() && context.failure_code.is_none()
            && context.postgres_pid.is_none() && context.node_pid.is_none() && context.auxiliary_pid.is_none()
        {
            return Ok(());
        }
        // The retained checkpoint can roll back a reaped, read-only PG probe
        // failure before Node started. An authenticated trial and normal
        // PostgreSQL shutdown are still required.
        let pair = (transition.source().2, transition.target().2);
        if (transition.startup_cancel_recovery() || retained.is_some()
            && (crate::native_payload::retained_product_image_transition(pair)
                || crate::native_payload::retained_product_image_recovery_transition(pair)))
            && binding.phase == Phase::Active
            && retained_product_image::drained_database_error(&context)
        {
            return Ok(());
        }
        let marker = format!("native-runs/{}/catalog40-drained.json", self.generation);
        let proof = installation_backup::source_bytes(root, &marker, 128, true).map_err(|_| cold())?;
        if binding.phase == Phase::Active
            && serde_json::from_slice::<String>(&proof).map_err(|_| cold())? == binding.sha256()?
        {
            return Ok(());
        }
        Err(cold())
    }

    fn recoverable_source_completion(&self, root: &Path, transition: FurnishingTransition) -> Result<(Context, FurnishingCompletion)> {
        let prior = self.source_binding(transition)?;
        let (context, proof) = read_run(root, &prior.generation)?;
        let proof = proof.ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        if proof.binding_sha256 != prior.sha256()?
            || proof.consumed_by.as_ref().is_some_and(|generation| generation != &self.generation)
            || context.state != "stopped" || context.failure_code.is_some()
            || context.exits.get("postgres") != Some(&0) || context.exits.get("node") != Some(&0)
            || context.node_pid.is_some() || context.postgres_pid.is_some() || context.auxiliary_pid.is_some()
        {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        Ok((context, proof))
    }

    pub(crate) fn recover_source(&self, service: &LifecycleService, guard: &FurnishingTransitionWriterGuard, transition: FurnishingTransition) -> Result<()> {
        let prior = self.source_binding(transition)?;
        if guard.identity()? != prior.physical {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        if matches!(transition, FurnishingTransition::Recover45 { .. }) {
            check_catalog45_recovery_source(&service.root, &self.source, transition.startup_cancel_recovery())?;
        } else {
            let (context, mut proof) = self.recoverable_source_completion(&service.root, transition)?;
            if proof.consumed_by.is_some() {
                proof.consumed_by = None;
                write_run(&service.root.join("native-runs").join(&prior.generation), &context, Some(proof))?;
            }
            unconsumed_furnishing_completion(&service.root, prior)?;
        }
        write_json(&service.root, "native-current-run.json", &self.source)?;
        File::open(&service.root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))
    }
}

fn furnishing_restore_stage_creates_lock(prior: Option<&Current>, has_database: bool, restore_pending: bool) -> Result<bool> {
    match (prior, has_database, restore_pending) {
        (None, false, false) => Ok(true),
        (prior, true, true) if prior.is_none_or(|current| current.furnishing.is_none()) => Ok(false),
        _ => Err(err(furnishing_lifecycle::ERROR)),
    }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[path = "native_runtime_retained_product_image.rs"]
mod retained_product_image;

pub struct Session {
    service: LifecycleService,
    observation_root: crate::maintenance_source::DirectoryPin,
    payload: Arc<Payload>,
    ports: Ports,
    environment: Environment,
    transition_fence: Option<String>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    furnishing_transition_trial: Option<(FurnishingTransitionTrial, FurnishingTransition)>,
    guard: Option<OperationGuard>,
    postgres: Option<Owned>,
    node: Option<Owned>,
    auxiliary: Option<Owned>,
    log: Option<Arc<Log>>,
    node_started: Arc<AtomicBool>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    oex_retained_cleanup: Option<oex_retained_cleanup::Run>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    selected_recovery_045: Option<oex_retained_cleanup::Selected045>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    retained_product_image: Option<retained_product_image::Trial>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    startup_cancellation_045: Option<crate::installations::native_maintenance::StartupCancellation045>,
    node_drain_requested: bool,
    job_id: Option<String>,
    run: Option<PathBuf>,
    state: String,
    failure: Option<String>,
    exits: BTreeMap<String, i32>,
    readiness: Readiness,
    cancellation: Arc<AtomicBool>,
    local_authority: Option<LocalAuthority>,
    owner_channel: Option<owner_channel::Connection>,
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions)))]
    hayes_owner_connection: Option<owner_channel::Connection>,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    hayes_bootstrap: HayesBootstrap,
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    retained_hayes: Option<RetainedHayes>,
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    hayes_probe_source: Option<hayes_probe_source::ProbeSourceFd>,
    furnishing_writer_lifetime: Option<furnishing_lifetime::WriterLifetime>,
    furnishing_binding: Option<Binding>,
    furnishing_drain_attempted: bool,
    furnishing_drain_ack: bool,
    furnishing_drain_deadline: Option<Instant>,
    furnishing_clean_checked: bool,
    furnishing_keep_fences: bool,
    fresh_oex_test: bool,
    normal_oex_selected: bool,
    fresh_oex_started: bool,
    fresh_ordinary_furnishing: bool,
    furnishing_selected_saved: bool,
    oex_upload_selection_started: bool,
    oex_upload_selected: Option<(String, u64, String, String)>,
    maintenance_space: Vec<PathBuf>,
}
fn directory(root: &Path, name: &str) -> Result<PathBuf> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => installations::new_directory(&path)?,
        Ok(_) => installations::private_directory(&path)?,
        Err(_) => return Err(err("NATIVE_DIRECTORY_UNAVAILABLE")),
    }
    Ok(path)
}
fn alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return true;
    }
    (unsafe { libc::kill(pid as i32, 0) == 0 })
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
// A database-only maintenance session legitimately has no Node exit record.
// If a service ran, disappearance alone does not prove its workers drained.
fn clean_node_quiescence(service_expected: bool, drain_requested: bool, exits: &BTreeMap<String, i32>) -> bool {
    !service_expected || (drain_requested && exits.get("node") == Some(&0))
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(err("NATIVE_JOURNAL_UNAVAILABLE")),
    }
}
/// Read-only orphan guard. Callers must hold the root operation fence; it does
/// not inspect environment credentials, start a process, or confer authority.
pub(crate) fn check_interrupted_root(root: &Path) -> Result<()> {
    if let Some(current) = current_run(root)? {
        let (prior, _) = read_run(root, &current.id)?;
        if [prior.postgres_pid, prior.node_pid, prior.auxiliary_pid]
                .into_iter()
                .flatten()
                .any(alive)
        {
            return Err(err("NATIVE_RECOVERY_REQUIRED"));
        }
    }
    let postmaster = root.join("native-database/postmaster.pid");
    if exists(&postmaster)? {
        let bytes = installation_backup::source_bytes(
            root,
            "native-database/postmaster.pid",
            8192,
            true,
        )?;
        let pid = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.lines().next())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| err("NATIVE_RECOVERY_REQUIRED"))?;
        if alive(pid) {
            return Err(err("NATIVE_RECOVERY_REQUIRED"));
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
pub(crate) fn check_cold_checkpoint_root(root: &Path) -> Result<()> {
    check_cold_transition_root(root, None)
}

#[cfg(debug_assertions)]
pub(crate) fn check_cold_transition_root(root: &Path, drained_failure: Option<&str>) -> Result<()> {
    check_interrupted_root(root)?;
    let cold = || err("NATIVE_PHYSICAL_CHECKPOINT_NOT_COLD");
    if exists(&root.join("native-database/postmaster.pid"))? {
        return Err(cold());
    }
    let current = current_run(root).map_err(|_| cold())?.ok_or_else(cold)?;
    let (prior, _) = read_run(root, &current.id).map_err(|_| cold())?;
    let terminal = if let Some(confirmed) = drained_failure {
        prior.state == "failed" && confirmed == current.id
    } else {
        prior.state == "stopped" && ["postgres", "node", "auxiliary"].iter()
            .all(|name| prior.exits.get(*name) == Some(&0))
    };
    if !terminal || prior.postgres_pid.is_some() || prior.node_pid.is_some()
        || prior.auxiliary_pid.is_some() || prior.exits.values().any(|code| *code != 0) {
        return Err(cold());
    }
    Ok(())
}

/// Positive server/DB drain for an interrupted export whose child was reaped.
/// The detail-admitted Platform must include its scoped handlers in native drain.
pub(crate) struct DetailDrainFence {
    _operation: OperationGuard,
    _writer: furnishing_lifetime::WriterLifetime,
}

pub(crate) fn detail_drained_generation(
    service: &LifecycleService,
    generation: &str,
) -> Result<DetailDrainFence> {
    let unconfirmed = || err("NATIVE_STATIC_DETAIL_CLEANUP_UNCONFIRMED");
    let operation = service.lock()?;
    let writer = furnishing_lifetime::WriterLifetime::acquire_existing(&service.root)?;
    check_interrupted_root(&service.root)?;
    if exists(&service.root.join("native-database/postmaster.pid"))? {
        return Err(unconfirmed());
    }
    let binding = current_run(&service.root)?.and_then(|current| current.furnishing)
        .ok_or_else(unconfirmed)?;
    let owner = service.personal_identity()?;
    if binding.phase != Phase::Clean || binding.generation != generation
        || binding.physical != writer.identity()?
        || (&binding.owner.workspace_id, &binding.owner.tenant_id, &binding.owner.principal_id)
            != (&owner.0, &owner.1, &owner.2)
        || binding.owner.owner_uid != unsafe { libc::geteuid() }
    {
        return Err(unconfirmed());
    }
    unconsumed_furnishing_completion(&service.root, &binding)?;
    Ok(DetailDrainFence { _operation: operation, _writer: writer })
}

#[cfg(test)]
mod interrupted_root_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    #[ignore = "requires the verified isolated native payload and shared heavy admission"]
    fn terminal_record_failure_keeps_fence_and_status_retries() {
        struct RestoreMode(PathBuf);
        impl Drop for RestoreMode { fn drop(&mut self) { let _ = fs::set_permissions(&self.0,fs::Permissions::from_mode(0o700)); } }
        let payload_path=PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PAYLOAD").expect("payload required"));
        let payload=Arc::new(Payload::open(&payload_path).unwrap());
        let base=std::env::temp_dir().join(format!("native-terminal-record-{}",Uuid::new_v4()));
        fs::create_dir(&base).unwrap();fs::set_permissions(&base,fs::Permissions::from_mode(0o700)).unwrap();
        let root=fs::canonicalize(base).unwrap();let service=LifecycleService::new(root.clone()).unwrap();
        service.prepare_native_personal(&payload,Ports::default()).unwrap();
        for via_status in [false,true] {
            let mut session=Session::new(&service,payload.clone(),Ports::default()).unwrap();
            let run=root.join(format!("terminal-test-{}",Uuid::new_v4()));fs::create_dir(&run).unwrap();fs::set_permissions(&run,fs::Permissions::from_mode(0o700)).unwrap();
            let _restore=RestoreMode(run.clone());
            session.run=Some(run.clone());session.job_id=Some(Uuid::new_v4().to_string());
            session.local_authority=Some(LocalAuthority{format:1,workspace_id:Uuid::new_v4().to_string(),tenant_id:Uuid::new_v4().to_string(),principal_id:Uuid::new_v4().to_string(),owner_uid:unsafe{libc::geteuid()},session_token:"synthetic-session".into(),csrf_token:"synthetic-csrf".into()});
            if via_status {session.state="stopping".into();}
            fs::set_permissions(&run,fs::Permissions::from_mode(0o500)).unwrap();
            let result=if via_status {session.status()} else {session.stop()};
            assert!(result.is_err());assert_eq!(session.state,"stopping");assert!(session.guard.is_some());assert!(session.local_authority.is_some());assert_eq!(service.lock().err().unwrap().code,"BUSY");
            fs::set_permissions(&run,fs::Permissions::from_mode(0o700)).unwrap();
            let terminal=session.status().unwrap();assert_eq!(terminal.state,"stopped");assert!(session.guard.is_none());assert!(session.local_authority.is_none());
            let persisted:Context=serde_json::from_slice(&fs::read(run.join("state.json")).unwrap()).unwrap();assert_eq!(persisted.state,"stopped");
            assert!(service.lock().is_ok());drop(session);
        }
        println!("isolated_record_failure_root={} payload_manifest_sha256={} both_paths=PASS",root.display(),payload.context().manifest_sha256);
    }
    #[test]
    fn node_drain_database_only_or_clean_exit_but_not_crash() {
        let mut exits = BTreeMap::new();
        assert!(clean_node_quiescence(false, false, &exits));
        assert!(!clean_node_quiescence(true, true, &exits));
        for (program, expected) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
            let status = Command::new(program).status().unwrap();
            exits.insert("node".into(), status.code().unwrap());
            assert_eq!(clean_node_quiescence(true, true, &exits), expected);
        }
        exits.insert("node".into(), -libc::SIGTERM);
        assert!(!clean_node_quiescence(true, true, &exits));
        exits.insert("node".into(), 0);
        assert!(!clean_node_quiescence(true, false, &exits));
    }

    #[test]
    fn node_drain_delivered_signal_without_graceful_exit_refused() {
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let mut owned = Owned { child, term_sent: false, term_delivered: false, capture: None, stdout_thread: None };
        if let Err(error) = owned.signal() {
            let _ = owned.child.kill(); let _ = owned.child.wait();
            panic!("owned signal failed: {}", error.code);
        }
        let mut code = None;
        for _ in 0..100 {
            code = owned.poll().unwrap();
            if code.is_some() { break; }
            thread::sleep(Duration::from_millis(10));
        }
        if code.is_none() { let _ = owned.child.kill(); let _ = owned.child.wait(); panic!("owned child did not exit"); }
        assert!(owned.term_delivered);
        let mut exits = BTreeMap::new();
        exits.insert("node".into(), code.unwrap());
        assert!(!clean_node_quiescence(true, owned.term_delivered, &exits));
    }

    #[test]
    #[ignore = "Explicit isolated stopped native profile and verified runtime under the heavy guard only"]
    fn node_drain_actual_owned_native_runtime() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_NATIVE_DRAIN_ROOT").expect("isolated test root required"));
        let payload_path = PathBuf::from(std::env::var_os("EXHIBITOS_NATIVE_DRAIN_PAYLOAD").expect("verified test runtime required"));
        assert!(!root.join("native-database/postmaster.pid").exists());
        let service = LifecycleService::bound_existing(root).unwrap();
        let payload = Arc::new(Payload::open(&payload_path).unwrap());
        let prepared = service.native_prepared().unwrap().unwrap();
        assert_eq!(prepared.manifest_sha256, payload.context().manifest_sha256);
        let mut session = Session::new(&service, payload, prepared.ports).unwrap();
        let started = session.start().unwrap();
        assert_eq!(started.state, "ready");
        let drained = session.quiesce_for_backup().unwrap();
        assert_eq!(drained.state, "database_ready");
        assert_eq!(drained.exits.get("node"), Some(&0));
        assert!(drained.node_pid.is_none() && drained.postgres_pid.is_some());
        let stopped = session.stop().unwrap();
        assert_eq!(stopped.state, "stopped");
        assert!(stopped.node_pid.is_none() && stopped.postgres_pid.is_none());
        assert_eq!(stopped.exits.get("postgres"), Some(&0));
    }

    #[test]
    fn offline_guard_refuses_live_and_malformed_postmaster_without_environment() {
        let root = std::env::temp_dir().join(format!("native-offline-orphan-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let db = root.join("native-database");
        fs::create_dir(&db).unwrap();
        fs::set_permissions(&db, fs::Permissions::from_mode(0o700)).unwrap();
        check_interrupted_root(&root).unwrap();
        write_private_new(&db, "postmaster.pid", format!("{}\n", std::process::id()).as_bytes(), 8192).unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_RECOVERY_REQUIRED");
        fs::write(db.join("postmaster.pid"), b"not-a-pid\n").unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_RECOVERY_REQUIRED");
        fs::remove_file(db.join("postmaster.pid")).unwrap();
        write_private_new(&root, "native-current-run.json", b"{}", 4096).unwrap();
        assert_eq!(check_interrupted_root(&root).unwrap_err().code, "NATIVE_JOURNAL_INVALID");
        fs::remove_dir_all(root).unwrap();
    }
}

fn check_selected_empty_root(service: &LifecycleService, guard: &OperationGuard,
                             root_pin: &crate::maintenance_source::DirectoryPin) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    installations::private_directory(&service.root)?;
    root_pin.check_path(&service.root)?;
    let held = guard.metadata().map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
    let named = fs::symlink_metadata(service.root.join("operation.lock"))
        .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
    if !held.is_file() || named.is_symlink() || held.uid() != unsafe { libc::geteuid() }
        || held.mode() & 0o777 != 0o600 || held.nlink() != 1
        || (held.dev(), held.ino()) != (named.dev(), named.ino()) {
        return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
    }
    for entry in fs::read_dir(&service.root).map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))? {
        if entry.map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?.file_name()
            != std::ffi::OsStr::new("operation.lock") {
            return Err(err("NATIVE_OEX_FRESH_ROOT_REQUIRED"));
        }
    }
    root_pin.check_path(&service.root)
}

#[cfg(test)]
mod selected_empty_root_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn empty_root_check_refuses_existing_data_without_changing_it() {
        let root = std::env::temp_dir().join(format!("exhibitos-selected-empty-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let pin = crate::maintenance_source::DirectoryPin::open(&root).unwrap();
        let guard = service.lock().unwrap();
        assert!(check_selected_empty_root(&service, &guard, &pin).is_ok());
        let marker = root.join("existing-data");
        fs::write(&marker, b"preserve").unwrap();
        assert!(check_selected_empty_root(&service, &guard, &pin).is_err());
        assert_eq!(fs::read(marker).unwrap(), b"preserve");
    }
}

impl Session {
    pub fn new(service: &LifecycleService, payload: Arc<Payload>, ports: Ports) -> Result<Self> {
        installations::private_directory(&service.root)?;
        let guard = service.lock()?;
        Self::new_with_guard(service, payload, ports, guard)
    }
    /// The opaque proof is minted by the atomic creation/selection
    /// transaction after owner admission.
    /// An empty path or a reselected existing workspace cannot mint it.
    #[allow(dead_code)]
    pub(crate) fn prepare_selected_empty(proof: installations::NewSelectedRoot, payload: Arc<Payload>, ports: Ports) -> Result<Self> {
        let (service, guard, root_pin) = proof.into_parts();
        if !payload.normal_oex_selected_input()? { return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        check_selected_empty_root(&service, &guard, &root_pin)?;
        service.prepare_native_personal_locked(&payload, ports, &guard)?;
        root_pin.check_path(&service.root)?;
        let mut session = Self::new_with_guard(&service, payload, ports, guard)?;
        session.check_native_observation_fence()?;
        session.normal_oex_selected = true;
        if session.payload.workspace_furnishing_catalog()?.and_then(FurnishingStart::selected_import).is_some()
            && session.payload.selected_workspace_import_version() == Some(crate::native_payload::WorkspaceImport::V7)
        {
            session.authorize_fresh_workspace_furnishing(&root_pin)?;
        }
        Ok(session)
    }
    pub(crate) fn new_with_guard(
        service: &LifecycleService,
        payload: Arc<Payload>,
        ports: Ports,
        guard: OperationGuard,
    ) -> Result<Self> {
        installations::private_directory(&service.root)?;
        signed_release::trust::lifecycle_writer::admit(&service.root, &Action::Start)?;
        let environment = Environment(service.native_start_environment(&payload, ports)?);
        Self::bound_session(service, payload, ports, guard, environment, None)
    }
    #[cfg(debug_assertions)]
    pub(crate) fn new_transition_trial(
        service: &LifecycleService,
        payload: Arc<Payload>,
        ports: Ports,
        guard: OperationGuard,
        binding: crate::native_personal::physical_checkpoint::TrialBinding,
    ) -> Result<Self> {
        installations::private_directory(&service.root)?;
        signed_release::trust::lifecycle_writer::admit(&service.root, &Action::Start)?;
        let (fields, fence) = binding.into_parts();
        Self::bound_session(service, payload, ports, guard, Environment(fields), Some(fence))
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn with_furnishing_transition_trial(mut self, trial: FurnishingTransitionTrial, transition: FurnishingTransition) -> Self {
        self.furnishing_transition_trial = Some((trial, transition));
        self
    }
    fn bound_session(
        service: &LifecycleService,
        payload: Arc<Payload>,
        ports: Ports,
        guard: OperationGuard,
        environment: Environment,
        transition_fence: Option<String>,
    ) -> Result<Self> {
        Ok(Self {
            service: LifecycleService::bound_existing(service.root.clone())?,
            observation_root: crate::maintenance_source::DirectoryPin::open(&service.root)?,
            payload,
            ports,
            environment,
            transition_fence,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            furnishing_transition_trial: None,
            guard: Some(guard),
            postgres: None,
            node: None,
            auxiliary: None,
            log: None,
            node_started: Arc::new(AtomicBool::new(false)),
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            oex_retained_cleanup: None,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            selected_recovery_045: None,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            retained_product_image: None,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            startup_cancellation_045: None,
            node_drain_requested: false,
            job_id: None,
            run: None,
            state: "stopped".into(),
            failure: None,
            exits: BTreeMap::new(),
            readiness: Readiness::default(),
            cancellation: Arc::new(AtomicBool::new(false)),
            local_authority: None,
            owner_channel: None,
            #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions)))]
            hayes_owner_connection: None,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            hayes_bootstrap: HayesBootstrap::Disabled,
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            retained_hayes: None,
            #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            hayes_probe_source: None,
            furnishing_writer_lifetime: None,
            furnishing_binding: None,
            furnishing_drain_attempted: false,
            furnishing_drain_ack: false,
            furnishing_drain_deadline: None,
            furnishing_clean_checked: false,
            furnishing_keep_fences: false,
            fresh_oex_test: false,
            normal_oex_selected: false,
            fresh_oex_started: false,
            fresh_ordinary_furnishing: false,
            furnishing_selected_saved: false,
            oex_upload_selection_started: false,
            oex_upload_selected: None,
            maintenance_space: Vec::new(),
        })
    }
    pub(crate) fn authorize_fresh_workspace_furnishing(&mut self, root: &crate::maintenance_source::DirectoryPin) -> Result<()> {
        root.check_path(&self.service.root)?;
        self.check_native_observation_fence()?;
        self.fresh_ordinary_furnishing = true;
        Ok(())
    }

    fn workspace_furnishing_start(&self) -> Result<Option<FurnishingStart>> {
        use std::os::unix::fs::MetadataExt;
        let Some(catalog) = self.payload.workspace_furnishing_catalog()? else { return Ok(None); };
        let initialize = if self.normal_oex_selected {
            if !self.fresh_ordinary_furnishing
                || self.payload.selected_workspace_import_version() != Some(crate::native_payload::WorkspaceImport::V7)
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
            FurnishingStart::selected_import(catalog).ok_or_else(|| err("NATIVE_RUNTIME_UNQUALIFIED"))?
        } else {
            FurnishingStart::Initialize
        };
        if self.transition_fence.is_some() {
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            if let Some((_, transition)) = self.furnishing_transition_trial.as_ref() {
                if catalog == transition.target_catalog()
                    && !exists(&self.service.root.join(WORKSPACE_FURNISHING_INITIALIZATION))?
                    && exists(&self.service.root.join("native-database"))?
                {
                    self.payload.recheck()?;
                    return Ok(Some(match transition {
                        FurnishingTransition::Upgrade39(_) => FurnishingStart::Upgrade039To040,
                        FurnishingTransition::Upgrade40(_) => FurnishingStart::Upgrade040To044,
                        FurnishingTransition::Upgrade44 { .. } => FurnishingStart::Upgrade044To045,
                        _ => FurnishingStart::Reopen,
                    }));
                }
            }
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        self.check_native_observation_fence()?;
        self.payload.recheck()?;
        self.payload.workspace_furnishing_ledger(catalog)?;
        let (workspace, tenant, principal) = self.service.personal_identity()?;
        let root = &self.service.root;
        if self.payload.retained_furnishing_catalog().is_some() {
            if exists(&root.join(WORKSPACE_FURNISHING_INITIALIZATION))? {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
            return Ok(Some(FurnishingStart::Reopen));
        }
        let metadata = fs::symlink_metadata(root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        let expected = serde_json::to_vec(&serde_json::json!({
            "format": 1,
            "workspaceId": workspace,
            "tenantId": tenant,
            "principalId": principal,
            "manifestSHA256": self.payload.context().manifest_sha256,
            "rootSHA256": digest(root.as_os_str().as_encoded_bytes()),
            "rootDevice": metadata.dev().to_string(),
            "rootInode": metadata.ino().to_string(),
        })).map_err(|_| err("STATE_INVALID"))?;
        let name = WORKSPACE_FURNISHING_INITIALIZATION;
        if exists(&root.join(name))? {
            let saved = installation_backup::source_bytes(root, name, 4096, true)?;
            if saved != expected { return Err(err("NATIVE_CONFIGURATION_CHANGED")); }
            return Ok(Some(initialize));
        }
        // Official 047 provisions the default personal profile through the
        // normal app. Its bound personal record, Session lock and absence of
        // previous runs are required before creating the initialization marker.
        let fresh_release = self.payload.fresh_image_release() && current_run(root)?.is_none();
        if (self.fresh_ordinary_furnishing || fresh_release) && !exists(&root.join("native-database"))? {
            if exists(&root.join("blobs"))? || exists(&root.join("config"))? {
                return Err(err("NATIVE_FRESH_WORKSPACE_REQUIRED"));
            }
            write_private_new(root, name, &expected, 4096)?;
            File::open(root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
            return Ok(Some(initialize));
        }
        if self.normal_oex_selected {
            return Err(err("NATIVE_OEX_FRESH_ROOT_REQUIRED"));
        }
        Ok(Some(FurnishingStart::Reopen))
    }

    fn furnishing_binding_for(&self, generation: String, phase: Phase, admission_kind: Admission) -> Result<Binding> {
        let (workspace_id, tenant_id, principal_id) = self.service.personal_identity()?;
        let payload = self.payload.context();
        let catalog = self.payload.workspace_furnishing_catalog()?
            .filter(|catalog| catalog.uses_writer_lease()).ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        let physical = self.furnishing_writer_lifetime.as_ref()
            .ok_or_else(|| err(furnishing_lifecycle::ERROR))?.identity()?;
        Ok(Binding {
            version: 1, phase, generation, previous_generation: None, admission_kind,
            owner: FurnishingOwner { workspace_id, tenant_id, principal_id, owner_uid: unsafe { libc::geteuid() } },
            source_commit: payload.source_commit, producer_sha256: self.payload.producer_sha256().into(),
            payload_manifest_sha256: payload.manifest_sha256, catalog: catalog.version(), physical, restore: None,
            interrupted_claim_generations: Vec::new(),
        })
    }

    fn publish_current(&self, furnishing: Option<Binding>) -> Result<()> {
        if let Some(binding) = &furnishing {
            binding.check()?;
            self.check_native_observation_fence()?;
            let lifetime = self.furnishing_writer_lifetime.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
            if lifetime.identity()? != binding.physical { return Err(err(furnishing_lifecycle::ERROR)); }
        }
        let current = Current { format: 1, id: self.job_id.clone().ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?, furnishing };
        write_json(&self.service.root, "native-current-run.json", &current)?;
        File::open(&self.service.root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))
    }

    fn active_furnishing_binding(&self) -> Option<&Binding> {
        self.furnishing_binding.as_ref().filter(|binding| binding.phase == Phase::Active
            && self.job_id.as_deref() == Some(binding.generation.as_str()))
    }

    fn verify_workspace_furnishing_ledger(&mut self) -> Result<()> {
        self.verify_furnishing_ledger_before(Instant::now() + Duration::from_secs(60))
    }

    fn verify_furnishing_ledger_before(&mut self, deadline: Instant) -> Result<()> {
        let catalog = self.payload.workspace_furnishing_catalog()?.ok_or_else(|| err("NATIVE_RUNTIME_UNQUALIFIED"))?;
        let expected = self.payload.workspace_furnishing_ledger(catalog)?;
        let observed = self.query_database_before("exhibitos", "SELECT name,sha256 FROM public.schema_migrations ORDER BY name", deadline)?;
        if observed != expected { return Err(err("NATIVE_WORKSPACE_FURNISHING_CATALOG")); }
        Ok(())
    }

    pub(crate) fn verify_workspace_furnishing_writer_quiescence(&mut self) -> Result<()> {
        if !self.payload.workspace_furnishing_catalog()?.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease) {
            return Ok(());
        }
        if self.status()?.state != "database_ready" || self.node.is_some() || self.auxiliary.is_some() {
            return Err(err("NATIVE_QUIESCENCE_UNCONFIRMED"));
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        if self.active_furnishing_binding().is_some() {
            if !self.furnishing_clean_checked || self.failure.is_some() {
                return Err(err(furnishing_lifecycle::ERROR));
            }
        } else {
            let live = self.furnishing_binding_for(Uuid::new_v4().to_string(), Phase::Active, Admission::FreshEmpty)?;
            validate_cold_furnishing_source(&self.service.root, self.furnishing_binding.as_ref(), &live)?;
        }
        self.verify_furnishing_db_quiescence(deadline)
    }

    pub(crate) fn furnishing_restore_target_identity(&self) -> Result<Option<FurnishingWriterIdentity>> {
        if !self.payload.workspace_furnishing_catalog()?.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease) {
            return Ok(None);
        }
        if self.state != "database_ready"
            || self.furnishing_binding.as_ref().is_some_and(|binding| binding.phase != Phase::ActivatedRestore)
        {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        self.check_native_observation_fence()?;
        self.furnishing_writer_lifetime.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?.identity().map(Some)
    }

    pub(crate) fn verify_furnishing_restore_target(&mut self, id: &str) -> Result<()> {
        if let Some(physical) = self.furnishing_restore_target_identity()? {
            let system = crate::installations::native_restore::pending_furnishing_target(&self.service.root, Some(id), physical)?
                .ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
            let deadline = Instant::now() + Duration::from_secs(60);
            self.verify_furnishing_database_identity(&system, deadline)?;
            self.verify_furnishing_db_quiescence(deadline)?;
        }
        Ok(())
    }

    fn verify_furnishing_database_identity(&mut self, expected: &str, deadline: Instant) -> Result<()> {
        if self.query_database_before("exhibitos", "SELECT system_identifier::text FROM pg_control_system()", deadline)? != expected {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        Ok(())
    }

    fn verify_furnishing_db_quiescence(&mut self, deadline: Instant) -> Result<()> {
        if self.postgres.is_none() || self.node.is_some() || self.auxiliary.is_some() {
            return Err(err("NATIVE_QUIESCENCE_UNCONFIRMED"));
        }
        self.check_native_observation_fence()?;
        self.furnishing_writer_lifetime.as_ref().ok_or_else(|| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?.identity()?;
        self.verify_furnishing_ledger_before(deadline)?;
        if self.query_database_before("exhibitos", "SELECT NOT EXISTS (SELECT 1 FROM public.workspace_furnishing_intakes WHERE worker_token IS NOT NULL)", deadline)? != "t" {
            return Err(err("NATIVE_FURNISHING_WRITER_CLAIM_PENDING"));
        }
        Ok(())
    }

    fn check_workspace_furnishing_channel(&mut self) -> Result<()> {
        self.check_native_observation_fence()?;
        self.furnishing_writer_lifetime.as_ref().ok_or_else(|| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?.identity()?;
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions)))]
        if self.payload.fresh_image_release() {
            self.hayes_owner_connection.as_ref().ok_or_else(|| err("HAYES_OPERATION_UNAVAILABLE"))?
                .check_hayes_peer_idle()?;
        }
        self.owner_channel.as_mut().ok_or_else(|| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_furnishing_idle()
    }
    pub(crate) fn static_detail_authority(&mut self) -> Result<StaticDetailAuthority<'_>> {
        if self.oex_upload_selection_started
            || self.state != "ready"
            || !self.readiness.ready
            || self.auxiliary.is_some()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        if !self.payload.workspace_furnishing_validation()?
            || self.active_furnishing_binding().is_none()
            || self.selected_furnishing_import()
        {
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }
        self.check_workspace_furnishing_channel()?;
        self.check_native_observation_fence()?;
        let identity = self.service.personal_identity()?;
        let authority = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (&identity.0, &identity.1, &identity.2)
            != (&authority.workspace_id, &authority.tenant_id, &authority.principal_id)
            || authority.owner_uid != unsafe { libc::geteuid() }
        {
            return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH"));
        }
        Ok(StaticDetailAuthority {
            workspace_id: &authority.workspace_id,
            tenant_id: &authority.tenant_id,
            principal_id: &authority.principal_id,
            runtime_generation: self.job_id.as_deref().ok_or_else(|| err("NATIVE_NOT_READY"))?,
            port: self.ports.web,
            session_token: &authority.session_token,
            csrf_token: &authority.csrf_token,
        })
    }
    /// Explicit private IPC only, never part of status/journal/log output.
    pub fn authoring_session(&mut self) -> Result<Value> {
        if self.oex_upload_selection_started { return Err(err("NATIVE_OEX_SESSION_BOUND")); }
        if self.status()?.state != "ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let furnishing = self.payload.workspace_furnishing_validation()?;
        if furnishing { self.check_workspace_furnishing_channel()?; }
        if self.auxiliary.is_some() { return Err(err("NATIVE_OPERATION_ACTIVE")); }
        self.observation_root.check()?;
        let identity = self.service.personal_identity()?;
        let old = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if identity != (old.workspace_id.clone(), old.tenant_id.clone(), old.principal_id.clone())
            || old.owner_uid != unsafe { libc::geteuid() }
        { return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH")); }
        let script = self.payload.authoring_session_script()?;
        let database = self.environment.0.get("DATABASE_URL").ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?.clone();
        let next = LocalAuthority {
            format: 1, workspace_id: old.workspace_id.clone(), tenant_id: old.tenant_id.clone(),
            principal_id: old.principal_id.clone(), owner_uid: old.owner_uid,
            session_token: local_token()?, csrf_token: local_token()?,
        };
        let input = LocalSessionRenewal {
            format: 1, workspace_id: &old.workspace_id, tenant_id: &old.tenant_id, principal_id: &old.principal_id,
            owner_uid: old.owner_uid, previous_session_token: &old.session_token,
            session_token: &next.session_token, csrf_token: &next.csrf_token,
        };
        let mut bytes = serde_json::to_vec(&input).map_err(|_| err("STATE_INVALID"))?;
        if bytes.len() > 4096 { bytes.fill(0); return Err(err("NATIVE_AUTHORING_RENEWAL_INVALID")); }
        // Once issuance starts, an ambiguous child/commit result must not emit
        // either credential or retry by reviving the old (possibly revoked) row.
        // The server owns revocation/expiry/current membership policy.
        let _old = self.local_authority.take();
        let result = self.auxiliary_timed(self.payload.node(), vec![script.to_string_lossy().into()],
            vec![("DATABASE_URL", database), ("EXHIBITOS_RUNTIME_MODE", "native".into()), ("EXHIBITOS_AUTHORING_MODE", "personal".into()),
                 ("TENANT_ID", next.tenant_id.clone())], Some(&bytes), Duration::from_secs(15));
        bytes.fill(0);
        let (code, receipt) = result.map_err(|_| err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"))?;
        self.observation_root.check()?;
        if self.service.personal_identity()? != identity { return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH")); }
        self.payload.recheck()?;
        let receipt: LocalSessionRenewalReceipt = serde_json::from_slice(&receipt)
            .map_err(|_| err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"))?;
        if code != 0 || receipt.workspace_id != next.workspace_id || receipt.tenant_id != next.tenant_id || receipt.principal_id != next.principal_id {
            return Err(err("NATIVE_AUTHORING_RENEWAL_UNCERTAIN"));
        }
        if furnishing { self.check_workspace_furnishing_channel()?; }
        self.local_authority = Some(next);
        let a = self
            .local_authority
            .as_ref()
            .ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let mut response = serde_json::json!({
            "tenantId": a.tenant_id, "principalId": a.principal_id,
            "origin": format!("http://127.0.0.1:{}", self.ports.web), "token": a.session_token,
        });
        if self.payload.fresh_image_release()
            && self.active_furnishing_binding().is_some_and(|binding| binding.catalog == 47)
        {
            response["freshImageReleaseManifestSHA256"] = self.payload.context().manifest_sha256.into();
        }
        Ok(response)
    }
    #[cfg(debug_assertions)]
    pub(crate) fn verify_transition_studio(
        &mut self,
        authoring: &Value,
        document: &super::installations::native_document::DocumentState,
    ) -> Result<()> {
        let mismatch = || err("NATIVE_PHYSICAL_CHECKPOINT_STUDIO_CHANGED");
        let draft = document.draft.as_ref().ok_or_else(mismatch)?;
        let exhibition = draft["exhibitionId"].as_str().ok_or_else(mismatch)?;
        let mut input = serde_json::to_vec(&serde_json::json!({
            "origin": authoring["origin"],
            "tenant": document.tenant_id,
            "token": authoring["token"],
            "exhibition": exhibition,
            "etag": document.etag,
            "draft": draft,
        })).map_err(|_| mismatch())?;
        // Credentials travel on stdin. The owned child's only output is its
        // decision; even a malformed response cannot enter the runtime log.
        let script = r#"
const { readFileSync } = require('node:fs');
const { isDeepStrictEqual } = require('node:util');
(async () => {
  const { origin, tenant, token, exhibition, etag, draft } = JSON.parse(readFileSync(0, 'utf8'));
  const response = await fetch(origin + '/api/v1/tenants/' + tenant + '/studio/exhibitions/' + exhibition, {
    redirect: 'error',
    signal: AbortSignal.timeout(7000),
    headers: { origin, accept: 'application/json', cookie: 'exhibitos_local_session=' + token },
  });
  if (!response.ok || response.headers.get('etag') !== etag) return false;
  const chunks = [];
  let length = 0;
  for await (const chunk of response.body) {
    length += chunk.length;
    if (length > 4 * 1024 * 1024) return false;
    chunks.push(chunk);
  }
  const value = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  return value.etag === etag && isDeepStrictEqual(value.draft, draft);
})().then(ok => process.stdout.write(ok ? 'ok' : 'changed'), () => process.stdout.write('changed'));
"#;
        let result = self.auxiliary_timed(
            self.payload.node(),
            vec!["-e".into(), script.into()],
            vec![],
            Some(&input),
            Duration::from_secs(15),
        );
        input.fill(0);
        match result {
            Ok((0, reply)) if reply == b"ok" => Ok(()),
            _ => Err(mismatch()),
        }
    }
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancellation.clone()
    }
    pub fn belongs_to(&self, service: &LifecycleService) -> bool {
        self.service.root == service.root
    }
    fn cancelled(&self) -> bool {
        cancelled() || self.cancellation.load(Ordering::SeqCst)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn arm_oex_retained_cleanup(&mut self, selection: oex_retained_cleanup::Selection) -> Result<()> {
        if self.job_id.is_some() || self.oex_retained_cleanup.is_some()
            || !self.payload.retained_oex_cleanup_hook()
        {
            return Err(err(oex_retained_cleanup::ERROR));
        }
        self.oex_retained_cleanup = Some(oex_retained_cleanup::Run::new(selection));
        Ok(())
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn oex_retained_cleanup_receipt(&self) -> Option<oex_retained_cleanup::Receipt> {
        self.oex_retained_cleanup.as_ref().and_then(|run| run.receipt().ok())
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn arm_selected_recovery_045(&mut self, selection: oex_retained_cleanup::Selected045) -> Result<()> {
        if self.job_id.is_some() || self.oex_retained_cleanup.is_some()
            || self.furnishing_transition_trial.is_none() || !self.payload.catalog45_selected_recovery_validation()
        {
            return Err(err(oex_retained_cleanup::ERROR));
        }
        self.oex_retained_cleanup = Some(oex_retained_cleanup::Run::new(selection.oex.clone()));
        self.selected_recovery_045 = Some(selection);
        Ok(())
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn selected_recovery_045_staging_bytes(&self) -> Result<Option<usize>> {
        self.selected_recovery_045.as_ref().map(|selection| {
            selection.completed_staging_bytes(self.oex_retained_cleanup.as_ref()
                .ok_or_else(|| err(oex_retained_cleanup::ERROR))?.receipt()?)
        }).transpose()
    }
    fn snapshot(&self) -> Context {
        let readiness = if self.state == "ready" {
            self.readiness.clone()
        } else {
            Readiness::default()
        };
        Context {
            format: 1,
            job_id: self.job_id.clone(),
            state: self.state.clone(),
            ports: self.ports,
            readiness,
            error_code: if self.state == "stopping" {
                Some("NATIVE_STOP_PENDING".into())
            } else {
                self.failure.clone()
            },
            failure_code: self.failure.clone(),
            postgres_pid: self.postgres.as_ref().map(|p| p.child.id()),
            node_pid: self.node.as_ref().map(|p| p.child.id()),
            auxiliary_pid: self.auxiliary.as_ref().map(|p| p.child.id()),
            exits: self.exits.clone(),
            log_truncated: self
                .log
                .as_ref()
                .is_some_and(|l| l.truncated.load(Ordering::Relaxed)),
        }
    }
    fn record(&self) -> Result<()> {
        if let Some(run) = &self.run {
            write_run(run, &self.snapshot(), None)?;
        }
        Ok(())
    }
    fn seal_furnishing_completion(&self, binding: &Binding) -> Result<()> {
        self.check_native_observation_fence()?;
        if self.job_id.as_deref() != Some(binding.generation.as_str()) {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        let run = self.run.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        write_run(run, &self.snapshot(), Some(FurnishingCompletion {
            binding_sha256: binding.sha256()?, consumed_by: None,
        }))
    }
    fn poll(&mut self) -> Result<bool> {
        let mut changed = false;
        for (name, slot) in [
            ("auxiliary", &mut self.auxiliary),
            ("node", &mut self.node),
            ("postgres", &mut self.postgres),
        ] {
            if let Some(child) = slot
                && let Some(code) = child.poll()?
            {
                self.exits.insert(name.into(), code);
                *slot = None;
                changed = true;
            }
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if let Some(retained) = self.retained_hayes.as_mut() {
            changed |= retained.poll()?;
        }
        if self.node.is_none() {
            self.owner_channel = None;
            #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions)))]
            { self.hayes_owner_connection = None; }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            if matches!(self.hayes_bootstrap, HayesBootstrap::Issued(_) | HayesBootstrap::Spent) {
                self.hayes_bootstrap = if self.payload.ordinary_hayes_release()
                    || self.payload.ordinary_fresh_furnishing()
                {
                    HayesBootstrap::Disabled
                } else {
                    HayesBootstrap::Spent
                };
            }
        }
        Ok(changed)
    }
    fn any_owned(&self) -> bool {
        let owned = self.postgres.is_some() || self.node.is_some() || self.auxiliary.is_some();
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        let owned = owned || self.retained_hayes.as_ref().is_some_and(RetainedHayes::has_child);
        owned
    }
    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        extra: &[(&str, String)],
        input: Option<&[u8]>,
        capture: bool,
        marker: bool,
        owner_setup: Option<&owner_channel::Setup>,
    ) -> Result<Owned> {
        let log = self
            .log
            .as_ref()
            .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?
            .clone();
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(
                self.run
                    .as_ref()
                    .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?,
            )
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            command.env(key, value);
        }
        if let Some(setup) = owner_setup { setup.configure(&mut command)?; }
        let mut child = command
            .spawn()
            .map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?;
        let mut stdin = child.stdin.take();
        let capture = if capture {
            Some(Arc::new(Mutex::new(Vec::new())))
        } else {
            None
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| err("NATIVE_PROCESS_START_FAILED"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| err("NATIVE_PROCESS_START_FAILED"))?;
        let captured = capture.clone();
        let stdout_log = log.clone();
        let started = self.node_started.clone();
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let retained_cleanup = self.oex_retained_cleanup.clone();
        let stdout_thread = thread::spawn(move || {
            let mut stream = stdout;
            let mut block = [0u8; 4096];
            let mut line = Vec::new();
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            let mut line_overflow = false;
            while let Ok(count) = stream.read(&mut block) {
                if count == 0 {
                    break;
                }
                stdout_log.write(&block[..count]);
                if let Some(captured) = &captured
                    && let Ok(mut bytes) = captured.lock()
                {
                    let room = 16384usize.saturating_sub(bytes.len());
                    bytes.extend_from_slice(&block[..count.min(room)]);
                }
                if marker {
                    for byte in &block[..count] {
                        if *byte == b'\n' {
                            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                            {
                                if let Some(cleanup) = &retained_cleanup {
                                    cleanup.observe(&line, line_overflow, started.load(Ordering::SeqCst));
                                }
                                line_overflow = false;
                            }
                            if line == b"ExhibitOS local runtime started." {
                                started.store(true, Ordering::SeqCst);
                            }
                            line.clear();
                        } else if line.len() < 1024 {
                            line.push(*byte);
                        } else {
                            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                            {
                                line_overflow = true;
                            }
                        }
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut stream = stderr;
            let mut block = [0u8; 4096];
            while let Ok(count) = stream.read(&mut block) {
                if count == 0 {
                    break;
                }
                log.write(&block[..count]);
            }
        });
        if let (Some(stdin), Some(input)) = (&mut stdin, input) {
            let _ = stdin.write_all(input);
        }
        drop(stdin);
        Ok(Owned {
            child,
            term_sent: false,
            term_delivered: false,
            capture,
            stdout_thread: Some(stdout_thread),
        })
    }
    fn auxiliary(
        &mut self,
        program: PathBuf,
        args: Vec<String>,
        extra: Vec<(&str, String)>,
        input: Option<&[u8]>,
    ) -> Result<(i32, Vec<u8>)> {
        self.auxiliary_timed(program, args, extra, input, Duration::from_secs(60))
    }
    fn auxiliary_timed(
        &mut self,
        program: PathBuf,
        args: Vec<String>,
        extra: Vec<(&str, String)>,
        input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<(i32, Vec<u8>)> {
        self.check_maintenance_space()?;
        self.auxiliary = Some(self.spawn(&program, &args, &extra, input, true, false, None)?);
        self.record()?;
        let deadline = Instant::now() + timeout;
        loop {
            self.check_maintenance_space()?;
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            let child = self
                .auxiliary
                .as_mut()
                .ok_or_else(|| err("NATIVE_PROCESS_UNAVAILABLE"))?;
            if let Some(code) = child.poll()? {
                let bytes = child
                    .capture
                    .as_ref()
                    .and_then(|p| p.lock().ok().map(|b| b.clone()))
                    .unwrap_or_default();
                self.exits.insert("auxiliary".into(), code);
                self.auxiliary = None;
                self.record()?;
                return Ok((code, bytes));
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_PROCESS_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn check_maintenance_space(&self) -> Result<()> {
        for path in &self.maintenance_space {
            // Restore expansion is not bounded by compressed dump size. Keep
            // 1GiB of cancellation/shutdown headroom above the existing floor,
            // checking the actual source/candidate/output filesystems per poll.
            // This is operational monitoring, not a quota or atomic reservation.
            if fs2::available_space(path).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < 7 * 1024 * 1024 * 1024
            {
                return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
            }
        }
        Ok(())
    }
    fn pg_tool(&self, name: &str) -> Result<PathBuf> {
        Ok(self
            .payload
            .postgres()
            .parent()
            .ok_or_else(|| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?
            .join(name))
    }
    fn query(&mut self, sql: &str) -> Result<String> {
        self.query_database("postgres", sql)
    }
    fn query_database(&mut self, database: &str, sql: &str) -> Result<String> {
        self.query_database_before(database, sql, Instant::now() + Duration::from_secs(60))
    }
    fn query_database_before(&mut self, database: &str, sql: &str, deadline: Instant) -> Result<String> {
        let timeout = deadline.checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero()).ok_or_else(|| err("NATIVE_DATABASE_UNAVAILABLE"))?;
        let password = self
            .environment
            .0
            .get("POSTGRES_PASSWORD")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        let (code, bytes) = self.auxiliary_timed(
            self.pg_tool("psql")?,
            vec![
                "-h".into(),
                "127.0.0.1".into(),
                "-p".into(),
                self.ports.database.to_string(),
                "-U".into(),
                "exhibitos".into(),
                "-d".into(),
                database.into(),
                "--no-password".into(),
                "--no-psqlrc".into(),
                "--tuples-only".into(),
                "--no-align".into(),
                "--command".into(),
                sql.into(),
            ],
            vec![("PGPASSWORD", password)],
            None, timeout,
        )?;
        if code != 0 {
            return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
        }
        String::from_utf8(bytes)
            .map(|s| s.trim().into())
            .map_err(|_| err("NATIVE_DATABASE_UNAVAILABLE"))
    }
    fn start_inner(&mut self, services: bool, maintenance_system: Option<&str>, existing_database: bool,
        furnishing: Option<FurnishingStart>,
    ) -> Result<()> {
        let mut reservations = Vec::new();
        for port in [self.ports.web, self.ports.api, self.ports.database] {
            reservations.push(
                TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
                    .map_err(|error| err(native_port_bind_code(&error)))?,
            );
        }
        if fs2::available_space(&self.service.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < 3 * 1024 * 1024 * 1024
        {
            return Err(err("STORAGE_LOW"));
        }
        let reopen_import = self.payload.workspace_import_version()
            .filter(|_| services && !self.normal_oex_selected);
        let require_existing = existing_database || reopen_import.is_some() || furnishing.is_some_and(FurnishingStart::opens_existing);
        let database = self.service.root.join("native-database");
        if !exists(&database)? {
            if require_existing {
                return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
            }
            self.state = "initializing_database".into();
            self.record()?;
            let mut input = self
                .environment
                .0
                .get("POSTGRES_PASSWORD")
                .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                .as_bytes()
                .to_vec();
            input.push(b'\n');
            let result = self.auxiliary(
                self.payload.initdb(),
                vec![
                    "-D".into(),
                    database.to_string_lossy().into(),
                    "-U".into(),
                    "exhibitos".into(),
                    "--auth-host=scram-sha-256".into(),
                    "--auth-local=scram-sha-256".into(),
                    "--pwfile=/dev/fd/0".into(),
                    "--no-locale".into(),
                    "--encoding=UTF8".into(),
                    "--no-instructions".into(),
                ],
                vec![],
                Some(&input),
            );
            input.fill(0);
            if result?.0 != 0 {
                return Err(err("NATIVE_DATABASE_INITIALIZATION_FAILED"));
            }
        }
        installations::private_directory(&database)?;
        let version = installation_backup::source_bytes(
            &self.service.root,
            "native-database/PG_VERSION",
            32,
            true,
        )?;
        if std::str::from_utf8(&version).ok().map(str::trim) != Some("18") {
            return Err(err("NATIVE_DATABASE_VERSION_UNSUPPORTED"));
        }
        directory(&self.service.root, "blobs")?;
        directory(&self.service.root, "config")?;
        drop(reservations);
        self.state = "starting_database".into();
        self.record()?;
        let nonce = self
            .job_id
            .clone()
            .ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
        self.postgres = Some(self.spawn(
            &self.payload.postgres(),
            &[
                "-D".into(),
                database.to_string_lossy().into(),
                "-h".into(),
                "127.0.0.1".into(),
                "-p".into(),
                self.ports.database.to_string(),
                "-k".into(),
                "".into(),
                "-c".into(),
                "jit=off".into(),
                "-c".into(),
                format!("exhibitos.instance={nonce}"),
            ],
            &[],
            None,
            false,
            false,
            None,
        )?);
        self.record()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            self.poll()?;
            if self.postgres.is_none() {
                return Err(err("NATIVE_DATABASE_EXITED"));
            }
            let (code, _) = self.auxiliary(
                self.pg_tool("pg_isready")?,
                vec![
                    "-h".into(),
                    "127.0.0.1".into(),
                    "-p".into(),
                    self.ports.database.to_string(),
                    "-U".into(),
                    "exhibitos".into(),
                ],
                vec![],
                None,
            )?;
            if code == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_DATABASE_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(100));
        }
        if self.query("SELECT current_setting('exhibitos.instance',true);")? != nonce {
            return Err(err("NATIVE_DATABASE_OWNERSHIP_INVALID"));
        }
        match self
            .query("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname='exhibitos');")?
            .as_str()
        {
            "t" => {}
            "f" => {
                if require_existing {
                    return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
                }
                let password = self
                    .environment
                    .0
                    .get("POSTGRES_PASSWORD")
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                    .clone();
                if self
                    .auxiliary(
                        self.pg_tool("createdb")?,
                        vec![
                            "-h".into(),
                            "127.0.0.1".into(),
                            "-p".into(),
                            self.ports.database.to_string(),
                            "-U".into(),
                            "exhibitos".into(),
                            "--no-password".into(),
                            "exhibitos".into(),
                        ],
                        vec![("PGPASSWORD", password)],
                        None,
                    )?
                    .0
                    != 0
                {
                    return Err(err("NATIVE_DATABASE_CREATION_FAILED"));
                }
            }
            _ => return Err(err("NATIVE_DATABASE_UNAVAILABLE")),
        }
        // Restore must reach an empty database before any application migration
        // or owner bootstrap runs. Keep the same Session and operation fence.
        if !services {
            if let Some(expected) = maintenance_system {
                self.verify_furnishing_database_identity(expected, Instant::now() + Duration::from_secs(60))?;
            }
            self.state = "database_ready".into();
            self.record()?;
            return Ok(());
        }
        if let Some(version) = reopen_import {
            // Read the settled profile before Platform can apply any migration.
            // Ordinary reopen has no selected owner channel or claim.
            self.check_oex_catalog_ledger(version.into())?;
        }
        if furnishing == Some(FurnishingStart::Reopen) {
            self.verify_workspace_furnishing_ledger()?;
            if let Some(mut binding) = self.active_furnishing_binding()
                .filter(|binding| binding.admission_kind == Admission::InterruptedReopen).cloned()
            {
                // The writer and operation fences are held. The owned PG
                // process is the only child and no Node worker has started.
                let owner = &binding.owner;
                let observed = match binding.catalog {
                    // The 045 recovery leaves every job in place. Mirror the
                    // Platform bootstrap check before starting its process.
                    45 if self.payload.catalog45_recovery_validation() => {
                        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                        let selected_checked = if let Some(selected) = self.selected_recovery_045.as_ref() {
                            let sql = selected.database_query(owner)?;
                            let value = self.query_database("exhibitos", &sql)?;
                            self.selected_recovery_045.as_mut().ok_or_else(|| err(oex_retained_cleanup::ERROR))?
                                .admit_staging(&value)?;
                            true
                        } else { false };
                        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
                        let selected_checked = false;
                        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                        let retained_checked = if let Some((job, frame)) = self.catalog45_hayes_only_recovery() {
                            let sql = oex_retained_cleanup::hayes_only_database_query(
                                owner, job, frame,
                            )?;
                            if selected_checked || self.query_database("exhibitos", &sql)? != "true" {
                                return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
                            }
                            true
                        } else { false };
                        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
                        let retained_checked = false;
                        if !selected_checked && !retained_checked {
                            let unsettled = self.query_database("exhibitos",
                                "SELECT EXISTS (
                                   SELECT 1 FROM import_jobs WHERE state IN ('uploading','queued','processing')
                                   OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cardinality(png_worker_tokens)>0
                                   OR model_preparation ? 'claim'
                                 ) OR EXISTS (
                                   SELECT 1 FROM oex_import_jobs WHERE state IN ('uploading','queued','processing')
                                   OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cleanup_pending
                                 ) OR EXISTS (
                                   SELECT 1 FROM workspace_material_intakes WHERE phase IN ('uploading','validating','cancelling')
                                   OR worker_token IS NOT NULL
                                 ) OR EXISTS (
                                   SELECT 1 FROM workspace_furnishing_intakes WHERE phase IN ('uploading','validating','cancelling')
                                   OR worker_token IS NOT NULL
                                 )")?;
                            if unsettled != "f" {
                                return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
                            }
                        }
                        String::new()
                    },
                    46 if self.payload.large_model_recovery_validation() => self.query_database("exhibitos", &format!(
                        "SELECT DISTINCT COALESCE(model_preparation#>>'{{claim,runtimeGeneration}}','unbound')
                         FROM public.import_jobs WHERE tenant_id='{}'::uuid AND user_id='{}'::uuid
                         AND model_preparation#>>'{{request,profileId}}'='embedded-model-compact-large-20k-512-v2'
                         AND (cardinality(png_worker_tokens)>0 OR model_preparation ? 'claim') ORDER BY 1 LIMIT 2",
                        owner.tenant_id, owner.principal_id,
                    ))?,
                    47 if self.payload.fresh_image_recovery_enabled() => self.query_database("exhibitos", &format!(
                        "SELECT DISTINCT COALESCE(derived#>>'{{claim,runtimeGeneration}}','unbound')
                         FROM public.import_jobs WHERE tenant_id='{}'::uuid AND user_id='{}'::uuid
                         AND derived->>'kind'='fresh-product-image' AND derived->>'workspaceId'='{}'
                         AND (cardinality(png_worker_tokens)>0 OR derived ? 'claim') ORDER BY 1 LIMIT 2",
                        owner.tenant_id, owner.principal_id, owner.workspace_id,
                    ))?,
                    _ => return Err(err(furnishing_lifecycle::ERROR)),
                };
                binding.select_interrupted_claim_generation(&observed)?;
                self.publish_current(Some(binding.clone()))?;
                self.furnishing_binding = Some(binding);
            }
            let restore_system = self.active_furnishing_binding().and_then(|binding| binding.restore.as_ref())
                .map(|restore| restore.target_system_identifier.clone());
            if let Some(expected) = restore_system {
                self.verify_furnishing_database_identity(&expected, Instant::now() + Duration::from_secs(60))?;
            }
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if matches!(furnishing, Some(FurnishingStart::Upgrade039To040 | FurnishingStart::Upgrade040To044 | FurnishingStart::Upgrade044To045)) {
            let expected = match furnishing {
                Some(FurnishingStart::Upgrade040To044) => self.payload.catalog40_upgrade_source_ledger()?,
                Some(FurnishingStart::Upgrade044To045) => self.payload.catalog44_upgrade_source_ledger()?,
                _ => self.payload.catalog39_upgrade_source_ledger()?,
            };
            let observed = self.query_database_before("exhibitos",
                "SELECT name,sha256 FROM public.schema_migrations ORDER BY name",
                Instant::now() + Duration::from_secs(60))?;
            if observed != expected { return Err(err("NATIVE_WORKSPACE_FURNISHING_CATALOG")); }
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        self.check_retained_product_image_before_node()?;
        self.state = "starting_services".into();
        self.record()?;
        let mut environment = vec![
            ("NODE_ENV", "production".into()),
            (
                "BLOB_ROOT",
                self.service.root.join("blobs").to_string_lossy().into(),
            ),
            (
                "CONFIG_ROOT",
                self.service.root.join("config").to_string_lossy().into(),
            ),
        ];
        let personal = self
            .environment
            .0
            .get("EXHIBITOS_AUTHORING_MODE")
            .is_some_and(|v| v == "personal");
        let mut keys = vec![
            "EXHIBITOS_RUNTIME_MODE",
            "EXHIBITOS_PORT",
            "EXHIBITOS_API_PORT",
            "DATABASE_URL",
            "TENANT_ID",
        ];
        if personal {
            keys.push("EXHIBITOS_AUTHORING_MODE");
        } else {
            keys.extend(["ADMIN_SUBJECT", "ADMIN_PASSWORD"]);
        }
        for key in keys {
            environment.push((
                key,
                self.environment
                    .0
                    .get(key)
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
                    .clone(),
            ));
        }
        self.local_authority = if personal {
            let value = |key: &str| {
                self.environment
                    .0
                    .get(key)
                    .cloned()
                    .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))
            };
            Some(LocalAuthority {
                format: 1,
                workspace_id: value("WORKSPACE_ID")?,
                tenant_id: value("TENANT_ID")?,
                principal_id: value("PRINCIPAL_ID")?,
                owner_uid: value("OWNER_UID")?
                    .parse()
                    .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?,
                session_token: local_token()?,
                csrf_token: local_token()?,
            })
        } else {
            None
        };
        let generation = self.job_id.as_deref().ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
        let mut furnishing_lease = None;
        let mut owner_setup = if let Some(start) = furnishing {
            let authority = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let lifetime = self.furnishing_writer_lifetime.as_ref().ok_or_else(|| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?;
            let writer = lifetime.duplicate_for_writer()?;
            furnishing_lease = Some(furnishing_lease::Lease::new(authority, generation, start,
                self.payload.context().manifest_sha256, writer.identity));
            Some(owner_channel::Setup::new()?.with_furnishing_writer(writer))
        } else if self.normal_oex_selected {
            if self.local_authority.is_none() || !self.payload.normal_oex_selected_input()? {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Some(owner_channel::Setup::new()?)
        } else { None };
        if (self.payload.retained_furnishing_catalog().is_some()
            && matches!(furnishing, Some(FurnishingStart::Reopen | FurnishingStart::Upgrade040To044 | FurnishingStart::Upgrade044To045)))
            || (self.payload.ordinary_fresh_furnishing()
                && matches!(furnishing, Some(FurnishingStart::Initialize | FurnishingStart::Reopen)))
        {
            self.payload.recheck()?;
            let generation_id = Uuid::parse_str(generation).map_err(|_| err("HAYES_OPERATION_UNAVAILABLE"))?;
            let pin = HayesSessionPin::new(generation_id, &self.payload.context().manifest_sha256)?;
            owner_setup = Some(owner_setup.take().ok_or_else(|| err("HAYES_OPERATION_UNAVAILABLE"))?.with_hayes(pin)?);
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if matches!(self.hayes_bootstrap, HayesBootstrap::Armed(_)) {
            let HayesBootstrap::Armed(expected) =
                std::mem::replace(&mut self.hayes_bootstrap, HayesBootstrap::Spent)
            else {
                unreachable!()
            };
            if !matches!(furnishing, Some(FurnishingStart::Initialize | FurnishingStart::Reopen))
                || self.payload.context().manifest_sha256 != expected
            {
                return Err(err("HAYES_OPERATION_UNAVAILABLE"));
            }
            self.payload.recheck()?;
            let generation_id = Uuid::parse_str(generation).map_err(|_| err("HAYES_OPERATION_UNAVAILABLE"))?;
            let pin = HayesSessionPin::new(generation_id, &expected)?;
            owner_setup = Some(owner_setup.take().ok_or_else(|| err("HAYES_OPERATION_UNAVAILABLE"))?.with_hayes(pin)?);
        }
        let script = self.payload.script();
        #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let script = match (self.payload.hayes_session_probe_script()?, self.hayes_probe_source.take()) {
            (Some(entry), Some(source)) if furnishing == Some(FurnishingStart::Initialize) => {
                owner_setup = Some(owner_setup.take().ok_or_else(|| err("HAYES_OPERATION_UNAVAILABLE"))?
                    .with_hayes_probe_source(source)?);
                entry
            }
            (None, None) => script,
            _ => return Err(err("HAYES_OPERATION_UNAVAILABLE")),
        };
        let mut private_input = match (&self.local_authority, &owner_setup, self.payload.workspace_import_version(), furnishing) {
            (Some(a), Some(setup), _, Some(start)) => Some(setup.workspace_furnishing_bootstrap(a, generation, start)?),
            (Some(a), Some(setup), Some(version), _) => Some(setup.workspace_import_bootstrap(a, generation, version)?),
            (Some(a), Some(setup), _, _) if self.payload.workspace_v5_selected_input() => Some(setup.workspace_v5_bootstrap(a, generation)?),
            (Some(a), Some(setup), _, _) if self.payload.normal_oex_028_selected_input()? => Some(setup.normal_028_bootstrap(a, generation)?),
            (Some(a), Some(setup), _, _) if self.payload.normal_oex_027_selected_input()? => Some(setup.normal_027_bootstrap(a, generation)?),
            (Some(a), Some(setup), _, _) => Some(setup.normal_bootstrap(a, generation)?),
            (Some(a), None, _, _) => Some(serde_json::to_vec(a).map_err(|_| err("STATE_INVALID"))?),
            (None, _, _, _) => None,
        };
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if let Some(cleanup) = &self.oex_retained_cleanup {
            let selected = self.selected_recovery_045.as_ref();
            let expected = if selected.is_some() { Admission::InterruptedReopen } else { Admission::CleanReopen };
            if !personal || furnishing != Some(FurnishingStart::Reopen) || furnishing_lease.is_none()
                || !self.active_furnishing_binding().is_some_and(|binding|
                    binding.admission_kind == expected && binding.restore.is_none())
            {
                return Err(err(oex_retained_cleanup::ERROR));
            }
            environment.push((oex_retained_cleanup::ENV, cleanup.environment()?));
            if let Some(selected) = selected {
                environment.push((oex_retained_cleanup::HAYES_COMPLETION_ENV, selected.hayes_environment()));
            }
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if let Some((job, frame)) = self.catalog45_hayes_only_recovery() {
            if !personal || furnishing != Some(FurnishingStart::Reopen) || furnishing_lease.is_none()
                || self.oex_retained_cleanup.is_some() || !self.active_furnishing_binding().is_some_and(|binding|
                    binding.admission_kind == Admission::InterruptedReopen && binding.restore.is_none())
            {
                return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
            }
            environment.push((oex_retained_cleanup::HAYES_COMPLETION_ENV, serde_json::json!({
                "jobId": job, "completionFrameSha256": frame,
            }).to_string()));
        }
        let spawned = self.spawn(
            &self.payload.node(),
            &[script.to_string_lossy().into()],
            &environment,
            private_input.as_deref(),
            false,
            true,
            owner_setup.as_ref(),
        );
        if let Some(bytes) = private_input.as_mut() {
            bytes.fill(0);
        }
        self.node = Some(spawned?);
        if let Some(setup) = owner_setup.as_mut() { setup.close_child(); }
        self.record()?;
        if let Some(mut setup) = owner_setup {
            let dedicated = setup.take_hayes();
            let authority = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let generation = self.job_id.as_deref().ok_or_else(|| err("NATIVE_JOURNAL_INVALID"))?;
            let mut channel = setup.handshake(authority, generation, &self.cancellation)?;
            let hayes = dedicated.map(|setup| setup.handshake(authority, generation, &self.cancellation)).transpose()?;
            if let Some(lease) = furnishing_lease {
                let cancellation = self.cancellation.clone();
                let binding = self.active_furnishing_binding().ok_or_else(|| err(furnishing_lifecycle::ERROR))?.clone();
                channel.bind_ordinary_furnishing(&lease, &binding, &cancellation, &mut || {
                    self.check_native_observation_fence()?;
                    self.poll()?;
                    if self.node.is_none() || self.postgres.is_none() {
                        return Err(err("NATIVE_PROCESS_EXITED"));
                    }
                    let current = self.furnishing_writer_lifetime.as_ref()
                        .ok_or_else(|| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?.identity()?;
                    if current != lease.identity { return Err(err("NATIVE_FURNISHING_WRITER_LOCK_INVALID")); }
                    Ok(())
                })?;
            }
            self.owner_channel = Some(channel);
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            if let Some(hayes) = hayes {
                self.hayes_bootstrap = HayesBootstrap::Issued(hayes);
            }
            #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions)))]
            { self.hayes_owner_connection = hayes.map(|authenticated| authenticated.channel); }
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            self.poll()?;
            if self.node.is_none() || self.postgres.is_none() {
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            if self.oex_retained_cleanup.as_ref().is_some_and(oex_retained_cleanup::Run::unresolved) {
                return Err(err(oex_retained_cleanup::ERROR));
            }
            if self.node_started.load(Ordering::SeqCst) {
                #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                if let Some(cleanup) = &self.oex_retained_cleanup { cleanup.receipt()?; }
                let ready = readiness_target(
                    &format!("http://127.0.0.1:{}/api/v1/readiness", self.ports.web),
                    &[self.ports.web],
                    "1.0.0-draft.1",
                    "1",
                    "0.1.0",
                );
                if ready.ready {
                    if let Some(start) = furnishing {
                        if start != FurnishingStart::Reopen { self.verify_workspace_furnishing_ledger()?; }
                        if !start.is_selected_import() { self.check_workspace_furnishing_channel()?; }
                        if !start.opens_existing() {
                            fs::remove_file(self.service.root.join(WORKSPACE_FURNISHING_INITIALIZATION))
                                .map_err(|_| err("STATE_UNAVAILABLE"))?;
                            File::open(&self.service.root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
                        }
                    }
                    self.readiness = ready;
                    self.state = "ready".into();
                    if self.normal_oex_selected {
                        if let Some(version) = self.payload.workspace_import_version() {
                            self.accept_oex_catalog(version.into())?;
                        } else if self.payload.workspace_v5_selected_input() {
                            self.accept_oex_catalog(oex_catalog_frames::Protocol::WorkspaceV5)?;
                        } else if self.payload.normal_oex_027_selected_input()? {
                            self.accept_oex_catalog(oex_catalog_frames::Protocol::V2)?;
                        }
                        self.await_oex_owner_ready()?;
                    }
                    // The selected owner is not ready for use until its private
                    // operation-ready frame has also been verified.  Do not
                    // publish a ready journal entry while that check is pending.
                    self.record()?;
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_READINESS_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn start(&mut self) -> Result<Context> {
        self.cancellation.store(false, Ordering::SeqCst);
        self.start_cancellable()
    }
    /// The host resets cancellation before publishing its handle, so a stop
    /// arriving between publication and startup cannot be overwritten.
    pub(crate) fn start_cancellable(&mut self) -> Result<Context> {
        self.start_owned(StartMode::Services)
    }
    /// Maintenance preparation only: owned PG18 and database, without Node,
    /// API/workers, schema migrations, authentication bootstrap or readiness.
    pub fn start_database_only(&mut self) -> Result<Context> {
        self.cancellation.store(false, Ordering::SeqCst);
        self.start_database_only_cancellable()
    }
    pub(crate) fn start_database_only_cancellable(&mut self) -> Result<Context> {
        self.start_owned(StartMode::Database)
    }
    pub(crate) fn start_restore_stage_cancellable(&mut self) -> Result<Context> {
        self.start_owned(StartMode::RestoreStage)
    }
    pub(crate) fn start_restore_activation(&mut self, id: &str) -> Result<Context> {
        self.start_owned(StartMode::RestoreActivation(id))
    }
    fn start_owned(&mut self, mode: StartMode<'_>) -> Result<Context> {
        let services = matches!(mode, StartMode::Services);
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if !matches!(self.hayes_bootstrap, HayesBootstrap::Disabled)
            && !(services && matches!(self.hayes_bootstrap, HayesBootstrap::Armed(_)))
        {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        if !services && exists(&self.service.root.join(WORKSPACE_FURNISHING_INITIALIZATION))? {
            return Err(err("NATIVE_WORKSPACE_FURNISHING_INITIALIZATION_PENDING"));
        }
        if self.normal_oex_selected {
            if self.fresh_oex_started || !services || self.guard.is_none()
                || exists(&self.service.root.join("native-database"))?
                || exists(&self.service.root.join("blobs"))?
                || exists(&self.service.root.join("config"))?
                || !self.payload.normal_oex_selected_input()? {
                return Err(err("NATIVE_OEX_FRESH_ROOT_REQUIRED"));
            }
            self.check_native_observation_fence()?;
            self.fresh_oex_started = true;
        }
        if services && (exists(&self.service.root.join("native-restore-current.json"))?
            || exists(&self.service.root.join("native-target-preparation-current.json"))?
            || exists(&self.service.root.join("native-legacy-migration-current.json"))?) {
            return Err(err("NATIVE_RESTORE_ACTIVATION_REQUIRED"));
        }
        if self.any_owned() {
            return Err(err("BUSY"));
        }
        if self.cancelled() {
            return Err(err("NATIVE_CANCELLED"));
        }
        self.readiness = Readiness::default();
        if self.guard.is_none() {
            self.guard = Some(self.service.lock()?);
        }
        signed_release::trust::lifecycle_writer::admit(&self.service.root, &Action::Start)?;
        #[cfg(debug_assertions)]
        if let Some(fence) = &self.transition_fence {
            self.check_native_observation_fence()?;
            crate::native_personal::physical_checkpoint::check_trial_fence(&self.service.root, fence)?;
        }
        if self.transition_fence.is_none() {
            self.environment = Environment(
                self.service.native_start_environment(&self.payload, self.ports)?,
            );
        }
        // Keep the last writer binding when a DB-only job rotates the diagnostic pointer.
        check_interrupted_root(&self.service.root)?;
        let prior = current_run(&self.service.root)?;
        let id = Uuid::new_v4().to_string();
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let id = if let Some((trial, _)) = &self.furnishing_transition_trial {
            if self.transition_fence.is_none() || !services || prior.as_ref() != Some(&trial.source) {
                return Err(err(furnishing_lifecycle::ERROR));
            }
            trial.generation.clone()
        } else { id };
        let prior_binding = prior.as_ref().and_then(|current| current.furnishing.clone());
        let catalog = self.payload.workspace_furnishing_catalog()?;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if self.oex_retained_cleanup.is_some() {
            let admissible_source = if self.selected_recovery_045.is_some() {
                self.transition_fence.is_some() && self.furnishing_transition_trial.as_ref()
                    .is_some_and(|(_, trial)| matches!(trial, FurnishingTransition::Recover45 { .. }))
                    && prior_binding.as_ref().is_some_and(|binding| binding.phase == Phase::Active && binding.restore.is_none())
            } else {
                self.transition_fence.is_none() && prior_binding.as_ref()
                    .is_some_and(|binding| binding.phase == Phase::Clean && binding.restore.is_none())
            };
            if !services || self.normal_oex_selected || !admissible_source
                || !catalog.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease)
                || !self.environment.0.get("EXHIBITOS_AUTHORING_MODE").is_some_and(|mode| mode == "personal")
            {
                return Err(err(oex_retained_cleanup::ERROR));
            }
        }
        #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if matches!(catalog, Some(WorkspaceFurnishingCatalog::HayesOriginal042 | WorkspaceFurnishingCatalog::HayesDisposition043 | WorkspaceFurnishingCatalog::HayesRelease044))
            && !self.payload.ordinary_hayes_release()
            && (!matches!(self.hayes_bootstrap, HayesBootstrap::Armed(_)) || prior.is_some()
                || self.payload.hayes_session_probe_script()?.is_some() != self.hayes_probe_source.is_some())
        {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        let mut binding = prior_binding.clone();
        let mut maintenance_system = None;
        let mut existing_database = false;
        let mut furnishing = None;
        if catalog.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease) {
            self.check_native_observation_fence()?;
            let has_database = exists(&self.service.root.join("native-database"))?;
            if services {
                furnishing = self.workspace_furnishing_start()?;
            }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            if self.oex_retained_cleanup.is_some() && furnishing != Some(FurnishingStart::Reopen) {
                return Err(err(oex_retained_cleanup::ERROR));
            }
            let restore_pending = exists(&self.service.root.join("native-restore-current.json"))?;
            let create_lock = match (mode, furnishing, has_database, restore_pending) {
                (StartMode::Services, Some(FurnishingStart::Initialize | FurnishingStart::SelectedImport038 | FurnishingStart::SelectedImport039 | FurnishingStart::SelectedImport040), false, false)
                    if prior.is_none() => true,
                (StartMode::Services, Some(start), true, false) if start.opens_existing() && prior_binding.is_some() => false,
                (StartMode::Database, None, true, false) if prior_binding.is_some() => false,
                (StartMode::RestoreStage, None, _, _) => furnishing_restore_stage_creates_lock(
                    prior.as_ref(), has_database, restore_pending,
                )?,
                (StartMode::RestoreActivation(_), None, true, true) => false,
                _ => return Err(err(furnishing_lifecycle::ERROR)),
            };
            existing_database = has_database;
            self.furnishing_writer_lifetime = None;
            self.furnishing_writer_lifetime = Some(if create_lock {
                furnishing_lifetime::WriterLifetime::acquire(&self.service.root)?
            } else {
                furnishing_lifetime::WriterLifetime::acquire_existing(&self.service.root)?
            });
            let mut active = self.furnishing_binding_for(id.clone(), Phase::Active, Admission::FreshEmpty)?;
            if services {
                match furnishing {
                    Some(FurnishingStart::Initialize | FurnishingStart::SelectedImport038 | FurnishingStart::SelectedImport039 | FurnishingStart::SelectedImport040) => (),
                    Some(FurnishingStart::Reopen | FurnishingStart::Upgrade039To040 | FurnishingStart::Upgrade040To044 | FurnishingStart::Upgrade044To045) => {
                        let source = prior_binding.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
                        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                        let transition = self.furnishing_transition_trial.as_ref();
                        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
                        let transition: Option<()> = None;
                        if source.phase == Phase::Active && transition.is_none()
                            && (self.payload.fresh_image_recovery_enabled() || self.payload.large_model_recovery_validation())
                            && furnishing == Some(FurnishingStart::Reopen)
                        {
                            if prior.as_ref().is_none_or(|run| run.id != source.generation) {
                                return Err(err(furnishing_lifecycle::ERROR));
                            }
                            check_interrupted_root(&self.service.root)?;
                            let (record, completion) = read_run(&self.service.root, &source.generation)?;
                            if completion.is_some() || record.ports != self.ports
                                || (record.state == "starting_database" && record.postgres_pid.is_none()
                                    && !record.exits.contains_key("postgres"))
                                || (source.admission_kind != Admission::InterruptedReopen && !record.readiness.ready
                                    && record.node_pid.is_none() && !record.exits.contains_key("node"))
                            {
                                return Err(err(furnishing_lifecycle::ERROR));
                            }
                            active.consume_interrupted(source, record.readiness.ready)?;
                        } else {
                            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
                            if let Some((_, transition)) = transition {
                                active.consume_furnishing_transition(source, *transition)?;
                            } else {
                                active.consume(source)?;
                            }
                            #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
                            active.consume(source)?;
                        }
                        if active.admission_kind == Admission::VerifiedRestore
                            || active.restore.as_ref().is_some_and(|restore| restore.provenance.is_some())
                        {
                            let proof = active.restore.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
                            crate::installations::native_restore::verify_ordinary_activation(
                                &self.service.root, &proof.id, &proof.backup_id, &proof.archive_manifest_sha256,
                                &proof.target_system_identifier, &active.payload_manifest_sha256, proof.provenance.as_ref(),
                            )?;
                        }
                    }
                    _ => return Err(err(furnishing_lifecycle::ERROR)),
                }
                binding = Some(active);
            } else {
                match mode {
                    StartMode::Database => {
                        validate_cold_furnishing_source(&self.service.root, prior_binding.as_ref(), &active)?;
                        maintenance_system = prior_binding.as_ref().and_then(|prior| prior.restore.as_ref())
                            .map(|restore| restore.target_system_identifier.clone());
                    }
                    StartMode::RestoreStage | StartMode::RestoreActivation(_) if restore_pending => {
                        let id = match mode { StartMode::RestoreActivation(id) => Some(id), _ => None };
                        if let Some(prior) = &prior_binding {
                            if prior.phase != Phase::ActivatedRestore || !active.same_installation(prior)
                                || prior.restore.as_ref().map(|restore| restore.id.as_str()) != id {
                                return Err(err(furnishing_lifecycle::ERROR));
                            }
                            unconsumed_furnishing_completion(&self.service.root, prior)?;
                        }
                        maintenance_system = crate::installations::native_restore::pending_furnishing_target(
                            &self.service.root, id, active.physical,
                        )?;
                        if maintenance_system.is_none() { return Err(err(furnishing_lifecycle::ERROR)); }
                    }
                    StartMode::RestoreStage => (),
                    _ => return Err(err(furnishing_lifecycle::ERROR)),
                }
            }
        } else if catalog.is_some() || prior_binding.is_some() {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        let parent = directory(&self.service.root, "native-runs")?;
        let run = directory(&parent, &id)?;
        let file = private_options()
            .open(run.join("process.log"))
            .map_err(|_| err("NATIVE_LOG_UNAVAILABLE"))?;
        self.log = Some(Arc::new(Log {
            file: Mutex::new((file, 0)),
            truncated: AtomicBool::new(false),
        }));
        self.job_id = Some(id.clone());
        self.run = Some(run);
        self.failure = None;
        self.exits.clear();
        self.node_started.store(false, Ordering::SeqCst);
        self.node_drain_requested = false;
        self.furnishing_drain_attempted = false;
        self.furnishing_drain_ack = false;
        self.furnishing_drain_deadline = None;
        self.furnishing_clean_checked = false;
        self.furnishing_selected_saved = false;
        self.furnishing_keep_fences = false;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        if self.furnishing_transition_trial.is_some() {
            self.furnishing_keep_fences = true;
        }
        self.state = "starting".into();
        self.record()?;
        // No process or mutable bootstrap may start before Active is durable.
        if services {
            if let Some(prior) = &prior_binding
                && !binding.as_ref().is_some_and(|next| next.admission_kind == Admission::InterruptedReopen)
            {
                let generation = self.job_id.as_deref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
                consume_furnishing_completion(&self.service.root, prior, generation)?;
            }
        }
        self.publish_current(binding.clone())?;
        self.furnishing_binding = binding;
        // The current pointer now describes this owned attempt; it has no PIDs yet.
        let result = self.start_inner(services, maintenance_system.as_deref(), existing_database, furnishing);
        if let Err(error) = result {
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            if error.code == oex_retained_cleanup::ERROR
                && self.oex_retained_cleanup.as_ref().is_some_and(oex_retained_cleanup::Run::unresolved)
                && !self.node_started.load(Ordering::SeqCst)
                && !self.cancelled()
                && self.check_workspace_furnishing_channel().is_ok()
            {
                // The pre-ready child is available only for the ordinary owner
                // drain. stop publishes Clean only after the drain ACK, both
                // zero exits, and database quiescence.
                if self.stop().is_ok_and(|context| context.state == "stopped")
                    && self.furnishing_binding.as_ref().is_some_and(|binding| binding.phase == Phase::Clean)
                {
                    return Err(error);
                }
            }
            self.failure.get_or_insert_with(|| error.code.clone());
            let _ = self.stop();
            return Err(error);
        }
        Ok(self.snapshot())
    }
    pub(crate) fn selected_furnishing_import(&self) -> bool {
        self.normal_oex_selected && self.furnishing_binding.as_ref().is_some_and(|binding| matches!(binding.catalog, 38 | 39))
    }

    /// The host calls this after the selected operation completes and its
    /// terminal receipt, document, and cleared transfer are durable.
    pub(crate) fn note_selected_oex_saved(&mut self) {
        self.furnishing_selected_saved = self.selected_furnishing_import();
    }

    fn request_node_drain(&mut self, deadline: Instant) -> Result<()> {
        if self.active_furnishing_binding().is_some() && !self.furnishing_drain_attempted {
            self.furnishing_drain_attempted = true;
            if self.selected_furnishing_import() && !self.furnishing_selected_saved {
                if let Some(mut channel) = self.owner_channel.take() {
                    channel.invalidate();
                }
                self.failure.get_or_insert_with(|| "NATIVE_OEX_WORKER_UNCERTAIN".into());
            }
            if let Some(mut channel) = self.owner_channel.take() {
                let cancellation = self.cancellation.clone();
                let physical = self.active_furnishing_binding().ok_or_else(|| err(furnishing_lifecycle::ERROR))?.physical;
                self.furnishing_drain_ack = channel.drain_ordinary_furnishing(
                    deadline.min(Instant::now() + Duration::from_secs(20)), &cancellation,
                    &mut || {
                        self.check_native_observation_fence()?;
                        let lifetime = self.furnishing_writer_lifetime.as_ref()
                            .ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
                        if lifetime.identity()? != physical { return Err(err(furnishing_lifecycle::ERROR)); }
                        Ok(())
                    },
                ).is_ok();
                if self.furnishing_drain_ack {
                    self.owner_channel = Some(channel);
                    self.node_drain_requested = true;
                    self.furnishing_drain_deadline = Some(deadline);
                }
            }
        }
        if self.furnishing_drain_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            self.furnishing_drain_ack = false;
        }
        if !self.furnishing_drain_ack {
            if let Some(node) = self.node.as_mut() {
                node.signal()?;
                self.node_drain_requested |= node.term_delivered;
            }
        }
        Ok(())
    }

    fn verify_furnishing_clean_stop(&mut self, deadline: Instant) -> Result<()> {
        if self.active_furnishing_binding().is_none() || self.furnishing_clean_checked {
            return Ok(());
        }
        if !self.furnishing_drain_ack || self.exits.get("node") != Some(&0) || self.failure.is_some() {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        let deadline = self.furnishing_drain_deadline.map_or(deadline, |drain| drain.min(deadline));
        self.verify_furnishing_db_quiescence(deadline)?;
        self.furnishing_clean_checked = true;
        Ok(())
    }

    pub(crate) fn stop_with_furnishing_activation_fences(&mut self, id: &str) -> Result<Context> {
        if self.payload.workspace_furnishing_catalog()?.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease) {
            if self.furnishing_binding.as_ref().is_some_and(|binding| binding.phase != Phase::ActivatedRestore)
                || self.state != "database_ready" {
                return Err(err(furnishing_lifecycle::ERROR));
            }
            self.verify_furnishing_restore_target(id)?;
            self.furnishing_keep_fences = true;
        }
        self.stop()
    }

    pub(crate) fn record_furnishing_activation(
        &mut self, id: &str, backup_id: &str, archive_manifest_sha256: &str, target_system_identifier: &str,
        provenance: Option<crate::installations::native_restore::RestoreProof>,
    ) -> Result<()> {
        if !self.payload.workspace_furnishing_catalog()?.is_some_and(WorkspaceFurnishingCatalog::uses_writer_lease) {
            return Ok(());
        }
        if !self.furnishing_keep_fences || self.any_owned() || self.state != "stopped"
            || self.exits.get("postgres") != Some(&0)
            || self.exits.contains_key("node")
        {
            return Err(err(furnishing_lifecycle::ERROR));
        }
        let generation = self.job_id.clone().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
        let mut restored = self.furnishing_binding_for(generation, Phase::ActivatedRestore, Admission::VerifiedRestore)?;
        restored.restore = Some(FurnishingRestore {
            id: id.into(), backup_id: backup_id.into(), archive_manifest_sha256: archive_manifest_sha256.into(),
            target_system_identifier: target_system_identifier.into(),
            provenance,
        });
        if let Some(prior) = &self.furnishing_binding {
            if prior.phase != Phase::ActivatedRestore || prior.restore != restored.restore || !restored.same_installation(prior) {
                return Err(err(furnishing_lifecycle::ERROR));
            }
            // An interrupted activation may resume the same pending plan only.
            restored = prior.clone();
        }
        if self.furnishing_binding.is_none() {
            self.seal_furnishing_completion(&restored)?;
        }
        self.publish_current(Some(restored.clone()))?;
        self.furnishing_binding = Some(restored);
        Ok(())
    }

    /// Stop and reap the owned API/worker process while retaining PostgreSQL
    /// and the root operation fence for the authenticated backup transaction.
    /// No successful quiescence is reported while an owned writer remains.
    pub fn quiesce_for_backup(&mut self) -> Result<Context> {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if self.retained_hayes.is_some() {
            return Err(err("NATIVE_QUIESCENCE_UNCONFIRMED"));
        }
        self.poll()?;
        if !matches!(
            self.state.as_str(),
            "ready" | "unavailable" | "quiescing" | "database_ready"
        ) || self.postgres.is_none()
            || self.auxiliary.is_some()
            || self.guard.is_none()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        let service_expected = matches!(self.state.as_str(), "ready" | "unavailable")
            || self.node.is_some()
            || self.exits.contains_key("node");
        self.state = "quiescing".into();
        self.readiness = Readiness::default();
        self.local_authority = None;
        self.record()?;
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            self.poll()?;
            if self.postgres.is_none() {
                self.failure = Some("NATIVE_PROCESS_EXITED".into());
                let _ = self.stop();
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            if self.cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            if self.node.is_some() {
                self.request_node_drain(deadline)?;
            } else {
                if !clean_node_quiescence(service_expected, self.node_drain_requested, &self.exits) {
                    return Err(err("NATIVE_QUIESCENCE_UNCONFIRMED"));
                }
                self.verify_furnishing_clean_stop(deadline)?;
                self.state = "database_ready".into();
                self.record()?;
                return Ok(self.snapshot());
            }
            if Instant::now() >= deadline {
                return Err(err("NATIVE_QUIESCENCE_PENDING"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn backup_available(&self) -> Result<()> {
        if exists(&self.service.root.join("native-restore-current.json"))?
            || exists(&self.service.root.join("native-target-preparation-current.json"))?
            || exists(&self.service.root.join("native-legacy-migration-current.json"))?
            || exists(&self.service.root.join(WORKSPACE_FURNISHING_INITIALIZATION))? {
            return Err(err("NATIVE_RESTORE_PENDING"));
        }
        self.maintenance_available()
    }
    pub(crate) fn maintenance_available(&self) -> Result<()> {
        self.payload.backup_resources()?;
        if self.payload.backup_runtime_bytes()? > 32 * 1024 * 1024 {
            return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
        }
        Ok(())
    }
    pub(crate) fn backup_source_current(&self) -> Result<()> {
        let current = Environment(
            self.service
                .native_start_environment(&self.payload, self.ports)?,
        );
        if current.0 != self.environment.0 {
            return Err(err("NATIVE_CONFIGURATION_CHANGED"));
        }
        Ok(())
    }
    pub(crate) fn maintenance_system_identifier(&mut self) -> Result<String> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let value = self.query("SELECT system_identifier::text FROM pg_control_system()")?;
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err("NATIVE_DATABASE_UNAVAILABLE"));
        }
        Ok(value)
    }
    pub(crate) fn maintenance_empty_target(&mut self) -> Result<bool> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let empty = self.query_database("exhibitos", "SELECT NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public') AND NOT EXISTS(SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace WHERE n.nspname='public')")? == "t";
        Ok(empty
            && fs::read_dir(self.service.root.join("blobs"))
                .map_err(|_| err("NATIVE_DIRECTORY_UNAVAILABLE"))?
                .next()
                .is_none())
    }
    pub(crate) fn maintenance_owner(
        &mut self,
        kind: &str,
        fields: &BTreeMap<String, String>,
    ) -> Result<()> {
        if self.status()?.state != "database_ready" {
            return Err(err("NATIVE_NOT_READY"));
        }
        let get = |name: &str| {
            fields
                .get(name)
                .ok_or_else(|| err("NATIVE_RESTORE_OWNER_MISMATCH"))
        };
        let tenant = get("TENANT_ID")?;
        if !installations::uuid(tenant) {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        }
        let condition = if kind == "personal" {
            let workspace = get("WORKSPACE_ID")?;
            let principal = get("PRINCIPAL_ID")?;
            let uid = get("OWNER_UID")?
                .parse::<u32>()
                .map_err(|_| err("NATIVE_RESTORE_OWNER_MISMATCH"))?;
            if !installations::uuid(workspace)
                || !installations::uuid(principal)
                || uid == 0
                || uid != unsafe { libc::geteuid() }
            {
                return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
            }
            format!(
                "EXISTS(SELECT 1 FROM local_workspaces w JOIN memberships m ON m.tenant_id=w.tenant_id AND m.user_id=w.user_id JOIN users u ON u.id=w.user_id JOIN tenants t ON t.id=w.tenant_id WHERE w.id='{workspace}'::uuid AND w.tenant_id='{tenant}'::uuid AND w.user_id='{principal}'::uuid AND w.mac_uid={uid} AND m.role='admin' AND NOT u.disabled AND t.deleted_at IS NULL)"
            )
        } else if kind == "legacy_account" {
            let subject = get("ADMIN_SUBJECT")?;
            if subject.is_empty() || subject.len() > 256 || subject.chars().any(char::is_control) {
                return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
            }
            let subject = subject.replace('\'', "''");
            format!(
                "EXISTS(SELECT 1 FROM memberships m JOIN users u ON u.id=m.user_id JOIN tenants t ON t.id=m.tenant_id WHERE m.tenant_id='{tenant}'::uuid AND u.subject='{subject}' AND m.role='admin' AND NOT u.disabled AND t.deleted_at IS NULL)"
            )
        } else {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        };
        if self.query_database("exhibitos", &format!("SELECT {condition}"))? != "t" {
            return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
        }
        Ok(())
    }
    pub(crate) fn matches_native_source(&self, root: &Path, manifest_sha256: &str) -> bool {
        self.service.root == root && self.payload.context().manifest_sha256 == manifest_sha256
    }
    pub(crate) fn maintenance_payload(&self) -> Arc<Payload> {
        self.payload.clone()
    }
    /// Fixed rollback-only legacy schema inspector, never an arbitrary script.
    pub(crate) fn inspect_legacy_schema(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 0)
    }
    pub(crate) fn apply_legacy_schema(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 1)
    }
    pub(crate) fn check_legacy_activation(&mut self, input: &[u8]) -> Result<Value> {
        self.legacy_schema_tool(input, 2)
    }
    fn legacy_schema_tool(&mut self, input: &[u8], kind: u8) -> Result<Value> {
        if self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
            || input.len() > 16384
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        self.payload.recheck()?;
        let (script, pins) = match kind {
            0 => crate::native_legacy_migration::inspection_resources(&self.payload)?,
            1 => crate::native_legacy_migration::schema_resources(&self.payload)?,
            2 => crate::native_legacy_migration::activation_resources(&self.payload)?,
            _ => return Err(err("NATIVE_LEGACY_OPERATION_INVALID")),
        };
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        self.maintenance_space = vec![self.service.root.clone()];
        let result = self.auxiliary_timed(
            self.payload.node(),
            vec![script.to_string_lossy().into_owned()],
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            Some(input),
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        for pin in pins {
            pin.check()?;
        }
        self.payload.recheck()?;
        match result {
            Ok((0, mut bytes)) => {
                let value = serde_json::from_slice(&bytes)
                    .map_err(|_| err("NATIVE_LEGACY_SCHEMA_RESULT_INVALID"));
                bytes.fill(0);
                value
            }
            Err(error) => Err(error),
            _ => Err(err("NATIVE_LEGACY_INSPECTION_FAILED")),
        }
    }
    /// Closed maintenance entry, callable only while this Session owns the PG
    /// and root fence and has reaped every application writer. Credentials are
    /// supplied through the cleared child's private environment, never argv.
    pub(crate) fn backup_tool(
        &mut self,
        args: Vec<String>,
        configuration: String,
        deployment: String,
        budget_bytes: u64,
        scratch: &Path,
    ) -> Result<Value> {
        if self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        let tools = self.payload.backup_resources()?;
        let resume_identifier = if args.first().is_some_and(|v| v == "reconcile") {
            Some(self.maintenance_system_identifier()?)
        } else {
            None
        };
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        let runtime = self
            .payload
            .script()
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| err("NATIVE_RUNTIME_PAYLOAD_INVALID"))?
            .join("apps/web/dist");
        self.maintenance_space = vec![self.service.root.clone()];
        if let Some(index) = args.iter().position(|v| v == "--destination") {
            let path = args
                .get(index + 1)
                .map(Path::new)
                .and_then(Path::parent)
                .ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?;
            if !self.maintenance_space.iter().any(|p| p == path) {
                self.maintenance_space.push(path.into());
            }
        }
        let mut command = vec![tools.script.to_string_lossy().into_owned()];
        command.extend(args);
        let result = self.auxiliary_timed(
            self.payload.node(),
            command,
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ("PG_DUMP_BIN", tools.pg_dump.to_string_lossy().into_owned()),
                (
                    "PG_RESTORE_BIN",
                    tools.pg_restore.to_string_lossy().into_owned(),
                ),
                (
                    "FREEZE_RUNTIME_ROOT",
                    runtime.to_string_lossy().into_owned(),
                ),
                ("BACKUP_CONFIGURATION_FILES", configuration),
                ("BACKUP_DEPLOYMENT_FILES", deployment),
                (
                    "EXHIBITOS_NATIVE_BACKUP_JOB_BUDGET_BYTES",
                    budget_bytes.to_string(),
                ),
                (
                    "BACKUP_MAX_DUMP_BYTES",
                    (32u64 * 1024 * 1024 * 1024).to_string(),
                ),
                ("TMPDIR", scratch.to_string_lossy().into_owned()),
                (
                    "EXHIBITOS_NATIVE_RESTORE_SYSTEM_IDENTIFIER",
                    resume_identifier.unwrap_or_default(),
                ),
            ],
            None,
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        match result {
            Ok((0, mut bytes)) => {
                let value = serde_json::from_slice(&bytes).map_err(|_| err("NATIVE_BACKUP_FAILED"));
                bytes.fill(0);
                value
            }
            other => {
                // Keep the caller's profile fence until children actually exit.
                while self.any_owned() {
                    self.stop()?;
                    if self.any_owned() {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                match other {
                    Err(error) => Err(error),
                    _ => Err(err("NATIVE_BACKUP_FAILED")),
                }
            }
        }
    }
    fn advance_stop(&mut self, deadline: Instant) -> Result<bool> {
        let changed = self.poll()?;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
        if self.retained_hayes.as_ref().is_some_and(RetainedHayes::has_child) {
            return Ok(false);
        }
        if let Some(child) = self.auxiliary.as_mut() {
            child.signal()?;
        } else if self.node.is_some() {
            self.request_node_drain(deadline)?;
        } else if self.postgres.is_some() {
            if let Err(error) = self.verify_furnishing_clean_stop(deadline) {
                self.failure.get_or_insert(error.code);
            }
            if self.auxiliary.is_none() {
                if let Some(child) = self.postgres.as_mut() { child.signal()?; }
            }
        } else {
            if self.active_furnishing_binding().is_some()
                && (!self.furnishing_clean_checked || self.exits.get("postgres") != Some(&0))
            {
                self.failure.get_or_insert_with(|| furnishing_lifecycle::ERROR.into());
            }
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            if self.furnishing_transition_trial.is_some() && self.failure.is_some()
                && (!self.exits.contains_key("node") || self.furnishing_drain_ack)
                && self.exits.values().all(|code| *code == 0)
            {
                if let Some(binding) = self.active_furnishing_binding() {
                    // Record the owned drain before publishing Failed, so recovery
                    // can distinguish it from an interrupted or unverified stop.
                    let run = self.run.as_ref().ok_or_else(|| err(furnishing_lifecycle::ERROR))?;
                    write_json(run, "catalog40-drained.json", &binding.sha256()?)?;
                    File::open(run).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
                }
            }
            self.state = if self.failure.is_some() { "failed" } else { "stopped" }.into();
            if let Err(error) = self.record() {
                self.state = "stopping".into();
                return Err(error);
            }
            if self.furnishing_clean_checked && self.failure.is_none() && self.exits.get("postgres") == Some(&0) {
                if let Some(active) = self.active_furnishing_binding() {
                    let mut clean = active.clone();
                    clean.phase = Phase::Clean;
                    if let Err(error) = self.seal_furnishing_completion(&clean)
                        .and_then(|()| self.publish_current(Some(clean.clone()))) {
                        self.state = "stopping".into();
                        return Err(error);
                    }
                    self.furnishing_binding = Some(clean);
                }
            }
            if !self.furnishing_keep_fences {
                self.furnishing_writer_lifetime = None;
                if self.transition_fence.is_none() { self.guard = None; }
            }
            self.local_authority = None;
            return Ok(true);
        }
        if changed { let _ = self.record(); }
        Ok(false)
    }

    pub fn stop(&mut self) -> Result<Context> {
        if self.furnishing_binding.is_some() && !self.any_owned()
            && matches!(self.state.as_str(), "stopped" | "failed")
        {
            return Ok(self.snapshot());
        }
        self.state = "stopping".into();
        self.readiness = Readiness::default();
        let _ = self.record();
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            if self.advance_stop(deadline)? || Instant::now() >= deadline {
                return Ok(self.snapshot());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(debug_assertions)]
    pub(crate) fn drained_transition_guard(&self) -> Result<&OperationGuard> {
        if self.transition_fence.is_none() || self.any_owned()
            || !matches!(self.state.as_str(), "stopped" | "failed")
            || !clean_node_quiescence(self.exits.contains_key("node"), self.node_drain_requested, &self.exits)
            || self.exits.values().any(|code| *code != 0) {
            return Err(err("NATIVE_STOP_PENDING"));
        }
        self.check_native_observation_fence()?;
        self.guard.as_ref().ok_or_else(|| err("NATIVE_STOP_PENDING"))
    }
    pub fn status(&mut self) -> Result<Context> {
        self.poll()?;
        if self.state == "stopping" {
            self.advance_stop(Instant::now() + Duration::from_secs(20))?;
            return Ok(self.snapshot());
        }
        if (matches!(self.state.as_str(), "ready" | "unavailable")
            && (self.node.is_none() || self.postgres.is_none()))
            || (matches!(self.state.as_str(), "database_ready" | "quiescing")
                && self.postgres.is_none())
        {
            self.failure = Some("NATIVE_PROCESS_EXITED".into());
            return self.stop();
        }
        if matches!(self.state.as_str(), "ready" | "unavailable") {
            let previous = self.state.clone();
            self.readiness = readiness_target(
                &format!("http://127.0.0.1:{}/api/v1/readiness", self.ports.web),
                &[self.ports.web],
                "1.0.0-draft.1",
                "1",
                "0.1.0",
            );
            self.state = if self.readiness.ready {
                "ready"
            } else {
                "unavailable"
            }
            .into();
            if previous != self.state {
                self.record()?;
            }
        }
        Ok(self.snapshot())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Keep the ownership fence until children really stop. A slow stop is
        // never converted to success or a force-kill of an arbitrary PID.
        while self.any_owned() {
            let _ = self.stop();
            if self.any_owned() {
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

impl Session {
    fn check_oex_catalog_ledger(&mut self, protocol: oex_catalog_frames::Protocol) -> Result<()> {
        let rows = self.query_database("exhibitos", &protocol.ledger_query())?;
        protocol.check_ledger(&rows)
    }

    /// Verify the catalog against our owned DB, pinned store and payload before
    /// acknowledging the peer. The channel remains tied to this Session.
    fn accept_oex_catalog(&mut self, protocol: oex_catalog_frames::Protocol) -> Result<()> {
        let owner = oex_owner::Owner::capture(self)?;
        let authority = self.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity = (authority.workspace_id.clone(), authority.tenant_id.clone(), authority.principal_id.clone());
        let generation = self.job_id.clone().ok_or_else(|| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let manifest = self.payload.context().manifest_sha256;
        let cancel = self.cancellation.clone();
        let mut channel = self.owner_channel.take().ok_or_else(|| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let outcome = (|| {
            let mut live = || owner.check(self);
            let raw = channel.read_frame(Instant::now() + Duration::from_secs(120), &cancel, &mut live)?;
            drop(live);
            owner.recheck_database_identity(self)?;
            self.payload.recheck()?;
            owner.check(self)?;
            self.check_oex_catalog_ledger(protocol)?;
            if self.active_furnishing_binding().is_some() {
                self.verify_workspace_furnishing_ledger()?;
            }
            owner.check(self)?;
            if protocol == oex_catalog_frames::Protocol::V2 && self.payload.normal_oex_028_selected_input()? {
                let row=self.query_database("exhibitos","SELECT sha256 FROM public.schema_migrations WHERE name='028_oex_material_catalogue.sql'")?;
                if row!=super::native_payload::NORMAL_OEX_028_SCHEMA_SHA256{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
                owner.check(self)?;
            }
            if protocol == oex_catalog_frames::Protocol::V2 && self.payload.normal_oex_029_floor_selected_input()? {
                let row=self.query_database("exhibitos","SELECT sha256 FROM public.schema_migrations WHERE name='029_oex_quiet_oak_floor.sql'")?;
                if row!=super::native_payload::NORMAL_OEX_029_FLOOR_SCHEMA_SHA256{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
                owner.check(self)?;
                if self.payload.normal_oex_030_concrete_tuple() {
                    let row = self.query_database("exhibitos", "SELECT sha256 FROM public.schema_migrations WHERE name='030_oex_concrete_surface.sql'")?;
                    if row != super::native_payload::NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256 {
                        return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
                    }
                    owner.check(self)?;
                }
            }
            owner.recheck_database_identity(self)?;
            self.payload.recheck()?;
            let (_, device, inode) = owner.store_identity();
            let observed = oex_catalog_frames::Observation { workspace:&identity.0, tenant:&identity.1, principal:&identity.2,
                generation:&generation, manifest:&manifest, system:&owner.system_id, oid:&owner.database_oid, device, inode };
            let ack = oex_catalog_frames::acknowledge(&raw, &observed, protocol)?;
            let mut live = || owner.check(self);
            channel.send_frame(&ack, Instant::now() + Duration::from_secs(30), &cancel, &mut live)?;
            live()?;
            Ok(())
        })();
        match outcome { Ok(()) => { self.owner_channel=Some(channel); Ok(()) }, Err(e) => { channel.invalidate(); Err(e) } }
    }
    /// Inactive one-shot selected-path readiness. Repeating it closes the
    /// channel. Success is correlation, not claim or reservation authority.
    #[allow(dead_code)]
    fn await_oex_owner_ready(&mut self) -> Result<()> {
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=self.job_id.clone().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.poll()?;
            self.check_native_observation_fence()?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.service.personal_identity()?;
            if self.state!="ready" || self.node.is_none() || self.postgres.is_none()
                || current.owner_uid!=unsafe {libc::geteuid()} || actual!=identity
                || self.job_id.as_deref()!=Some(generation.as_str())
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())
                    != (identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let result=channel.receive_ready(&cancel,&mut check,&identity.0,&identity.1,&identity.2,&generation);
        drop(check);
        match result {
            Ok(())=>{self.owner_channel=Some(channel);Ok(())},
            Err(error)=>{channel.invalidate();Err(error)},
        }
    }
    /// The host supplies only the currently selected, persisted intent. The
    /// Swift caller must retain and match the selected private snapshot too.
    pub(crate) fn select_oex_upload_from_intent(&mut self,state:&crate::installations::native_oex_import::Transfer)->Result<()> {
        crate::installations::native_oex_import::validate(state)?;
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (state.workspace_id.as_str(),state.tenant_id.as_str(),state.principal_id.as_str())!=(auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())
            || state.job_id.is_some() {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let intent=state.intent.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if intent.profile.as_deref() != Some("bounded-file-v1")
            || !self.payload.admits_selected_oex_format(intent.oex_format.as_deref().unwrap_or_default())? {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        self.select_oex_upload_format(&intent.request_id,intent.bytes,&intent.sha256,intent.oex_format.as_deref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?)
    }
    /// Inactive until the host binds this to the exact durably saved intent
    /// and retained private snapshot. It never selects a path or grants HTTP
    /// authorization by itself. Selection attempts and renewal are one-shot.
    #[allow(dead_code)]
    fn select_oex_upload(&mut self,request:&str,bytes:u64,sha256:&str)->Result<()> { self.select_oex_upload_format(request,bytes,sha256,"1.0.0-draft.9") }
    fn select_oex_upload_format(&mut self,request:&str,bytes:u64,sha256:&str,format:&str)->Result<()> {
        let v4=self.payload.workspace_import_selected_input() || self.payload.normal_oex_028_selected_input()?;
        if !self.payload.admits_selected_oex_format(format)? { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
        if !(self.fresh_oex_test || self.normal_oex_selected)||self.oex_upload_selection_started||self.state!="ready"||self.node.is_none()||self.postgres.is_none(){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        self.oex_upload_selection_started=true;
        let auth=match self.local_authority.as_ref(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_PERSONAL_SESSION_REQUIRED"));
        }};
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=match self.job_id.clone(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }};
        let frame=owner_claim_frames::UploadSelection{version:1,kind:"upload-selection".into(),correlation_id:Uuid::new_v4().to_string(),runtime_generation:generation.clone(),workspace_id:identity.0.clone(),tenant_id:identity.1.clone(),principal_id:identity.2.clone(),request_id:request.into(),archive_bytes:bytes,archive_sha256:sha256.into(),archive_format:if v4{Some(format.into())}else{None}};
        let encoded=match frame.encode(){Ok(value)=>value,Err(error)=>{
            if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.poll()?;self.check_native_observation_fence()?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            if self.cancelled()||self.state!="ready"||self.node.is_none()||self.postgres.is_none()||self.job_id.as_deref()!=Some(generation.as_str())
                || current.owner_uid!=unsafe{libc::geteuid()}||self.service.personal_identity()?!=identity
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())!=(identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}Ok(())
        };
        let response=channel.exchange(&encoded,&cancel,&mut check).and_then(|reply|frame.check_selected(&reply)).and_then(|_|check());drop(check);
        match response {Ok(())=>{self.oex_upload_selected=Some((request.into(),bytes,sha256.into(),format.into()));self.owner_channel=Some(channel);Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Inactive one-shot claim. The borrow keeps this exact Session alive; on
    /// error/abandonment the socket closes and the API must retain uncertainty.
    #[allow(dead_code)]
    fn begin_owned_oex_claim<'a>(&'a mut self,stage_root:&'a oex_stage_root::StageRoot,job:&str,request:&str,bytes:u64,sha256:&str) -> Result<OwnedOexClaim<'a>> {
        if !(self.fresh_oex_test || self.normal_oex_selected) || self.oex_upload_selected.as_ref().is_none_or(|v|v.0!=request||v.1!=bytes||v.2!=sha256){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=self.job_id.clone().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let open=owner_claim_frames::ClaimOpen{version:1,kind:"claim-open".into(),correlation_id:Uuid::new_v4().to_string(),
            runtime_generation:generation.clone(),workspace_id:identity.0.clone(),tenant_id:identity.1.clone(),job_id:job.to_string()};
        let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            stage_root.check_empty(self)?;
            let current=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.service.personal_identity()?;
            if self.state!="ready" || self.node.is_none() || self.postgres.is_none() || actual!=identity
                || current.owner_uid!=unsafe {libc::geteuid()} || self.job_id.as_deref()!=Some(generation.as_str())
                || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())
                    != (identity.0.as_str(),identity.1.as_str(),identity.2.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let result=channel.begin_claim(&open,&identity.2,request,bytes,sha256,stage_root.path(),stage_root.device(),0,&cancel,&mut check);
        drop(check);
        match result {
            Ok((challenge,ready,capacity))=>{self.owner_channel=Some(channel);Ok(OwnedOexClaim{session:self,stage_root,challenge,ready,capacity,stage_started:false,retained_packed:None,save_started:false,save_ready:None,settled:false})},
            Err(error)=>{channel.invalidate();Err(error)},
        }
    }
    /// One two-phase claim under the admitted new workspace Session and its
    /// existing operation fence. Legacy frames cannot enter this exchange.
    #[allow(dead_code)]
    fn begin_owned_oex_claim_v3<'a>(&'a mut self,stage_root:&'a oex_stage_root::StageRoot,job:&str,request:&str,bytes:u64,sha256:&str) -> Result<OwnedOexV3Claim<'a>> {
        if !(self.fresh_oex_test || self.normal_oex_selected) || self.oex_upload_selected.as_ref().is_none_or(|v|v.0!=request||v.1!=bytes||v.2!=sha256) {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let owner=oex_owner::Owner::capture(self)?;
        owner.recheck_database_identity(self)?;
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=(auth.workspace_id.clone(),auth.tenant_id.clone(),auth.principal_id.clone());
        let generation=self.job_id.clone().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let open=owner_claim_frames::ClaimOpen{version:1,kind:"claim-open-v3".into(),correlation_id:Uuid::new_v4().to_string(),
            runtime_generation:generation,workspace_id:identity.0,tenant_id:identity.1,job_id:job.into()};
        let cancel=self.cancellation.clone();
        let mut channel=self.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let format=self.oex_upload_selected.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?.3.clone();
        let v4=self.payload.workspace_import_selected_input() || self.payload.normal_oex_028_selected_input()?;
        let archive=owner_channel::SelectedArchive{request_id:request,bytes,sha256,format:if v4{Some(&format)}else{None}};
        let scope=oex_reservation::V3PlanScope::from_owner(&owner,stage_root.path(),stage_root.device());
        let mut check=|| {owner.check(self)?;stage_root.check_empty(self)};
        let result=channel.begin_claim_v3(&open,&identity.2,archive,scope,&cancel,&mut check);
        drop(check);
        let result=result.and_then(|value|{owner.recheck_database_identity(self)?;stage_root.check_empty(self)?;Ok(value)});
        match result {
            Ok((challenge,ready,input_receipt))=>{self.owner_channel=Some(channel);Ok(OwnedOexV3Claim{session:self,stage_root,owner,challenge,ready,input_receipt,preflight_started:false,retained:None,report_started:false,bound:None,packed_started:false,packed:None,save_started:false,save_ready:None,peer_save_verified:false,retirement_started:false,retired:None,uncertain_retirement:None,uncertain_save:None,settlement_started:false,completed:false})},
            Err(error)=>{channel.invalidate();Err(error)},
        }
    }
    /// One fresh-selected operation using the exact durable queued intent.
    /// The callback must persist the announced save request and complete the
    /// real same-session GET/one PUT/GET before returning its new ETag. It may
    /// not rearm on error. This routine holds the Session and claim through
    /// selected-flow settlement and completion; failure preserves uncertainty.
    #[allow(dead_code)]
    pub(crate) fn run_fresh_oex_once<F>(&mut self,state:&crate::installations::native_oex_import::Transfer,
        cancelled:&AtomicBool,mut save:F)->Result<()>
    where F:FnMut(&str,&str,&str,&str)->Result<String> {
        if self.normal_oex_selected && self.payload.normal_oex_027_selected_input()? {
            return self.run_selected_oex_v3_once(state,cancelled,save);
        }
        crate::installations::native_oex_import::validate(state)?;
        if !(self.fresh_oex_test || self.normal_oex_selected){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (state.workspace_id.as_str(),state.tenant_id.as_str(),state.principal_id.as_str())!=(auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str()) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let intent=state.intent.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if intent.profile.as_deref()!=Some("bounded-file-v1")||intent.oex_format.as_deref()!=Some("1.0.0-draft.9"){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let job=state.job_id.as_deref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        let result=(|| {
        let root=oex_stage_root::StageRoot::create(self)?;
        let mut claim=self.begin_owned_oex_claim(&root,job,&intent.request_id,intent.bytes,&intent.sha256)?;
        let input_root=claim.session.observation_root.try_clone_file()?;
        let chunks=oex_chunks::open_local(&input_root,&claim.ready.tenant_id,&claim.ready.job_id,claim.ready.archive_bytes)?;
        let stage=claim.stage_root.create_input(claim.session,&claim.ready.stage_id)?;
        let (device,inode)=stage.identity()?;let r=&claim.ready;
        let request=oex_materializer_protocol::Request{version:1,kind:"oex-chunks-v1".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),stage_device:device.to_string(),stage_inode:inode.to_string(),chunks:chunks.iter().map(|c|oex_materializer_protocol::Chunk{device:c.device.to_string(),inode:c.inode.to_string(),bytes:c.bytes}).collect(),reservation_digest:r.reservation_digest.clone(),source_digest:r.source_digest.clone(),worker_digest:claim.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
        let correlation=claim.challenge.correlation_id.clone();
        claim.materialize_pack_and_serve(stage,&chunks,&request,&correlation,cancelled)?;
        let ready=claim.wait_save_ready(cancelled)?;
        let etag=save(&ready.exhibition_id,&ready.etag,&ready.request_id,&ready.attempt_id)?;
        claim.confirm_save(&etag,cancelled)
        })();
        if result.is_err(){if let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}}
        result
    }
    pub(crate) fn run_selected_oex_v3_once<F>(&mut self,state:&crate::installations::native_oex_import::Transfer,
        cancelled:&AtomicBool,mut save:F)->Result<()>
    where F:FnMut(&str,&str,&str,&str)->Result<String> {
        let result=(|| {
        crate::installations::native_oex_import::validate(state)?;
        if !self.normal_oex_selected || !self.payload.normal_oex_027_selected_input()?{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let auth=self.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        if (state.workspace_id.as_str(),state.tenant_id.as_str(),state.principal_id.as_str())!=(auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str()) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let intent=state.intent.as_ref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if intent.profile.as_deref() != Some("bounded-file-v1")
            || !self.payload.admits_selected_oex_format(intent.oex_format.as_deref().unwrap_or_default())? {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let job=state.job_id.as_deref().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if self.cancelled()||cancelled.load(Ordering::SeqCst)||self.oex_upload_selected.as_ref().is_none_or(|v|v.0!=intent.request_id||v.1!=intent.bytes||v.2!=intent.sha256||Some(v.3.as_str())!=intent.oex_format.as_deref()) {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
            let root=oex_stage_root::StageRoot::create(self)?;
            let mut claim=self.begin_owned_oex_claim_v3(&root,job,&intent.request_id,intent.bytes,&intent.sha256)?;
            claim.materialize_and_preflight(cancelled)?;
            claim.bind_preflight(cancelled)?;
            claim.pack_and_serve(cancelled)?;
            claim.save_and_retire(cancelled,&mut save)?;
            claim.settle(cancelled)
        })();
        if result.is_err() && let Some(mut channel)=self.owner_channel.take(){channel.invalidate();}
        result
    }
    pub(crate) fn target_observation_root(&self) -> &Path {
        &self.service.root
    }
    pub(crate) fn check_native_observation_fence(&self) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        self.observation_root.check()?;
        let held = self
            .guard
            .as_ref()
            .ok_or_else(|| err("NATIVE_NOT_READY"))?
            .metadata()
            .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        let named = fs::symlink_metadata(self.service.root.join("operation.lock"))
            .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        if !held.is_file()
            || named.is_symlink()
            || held.dev() != named.dev()
            || held.ino() != named.ino()
            || held.nlink() != 1
            || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o777 != 0o600
        {
            return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        Ok(())
    }
    /// A fixed read-only worker, under the existing native session/root fence.
    /// It never runs application bootstrap, migrations, restore or a caller script.
    pub(crate) fn observe_target_inventory(
        &mut self,
        manifest: &[u8],
        manifest_sha256: &str,
        target_schema: &str,
    ) -> Result<Value> {
        if manifest.is_empty()
            || manifest.len() > 16 * 1024 * 1024
            || digest(manifest) != manifest_sha256
            || !hash_valid(target_schema)
            || self.status()?.state != "database_ready"
            || self.guard.is_none()
            || self.node.is_some()
            || self.auxiliary.is_some()
        {
            return Err(err("NATIVE_NOT_READY"));
        }
        self.check_native_observation_fence()?;
        self.backup_source_current()?;
        let resources = self.payload.target_inventory_resources()?;
        let system = self.maintenance_system_identifier()?;
        let database = self
            .environment
            .0
            .get("DATABASE_URL")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?
            .clone();
        self.maintenance_space = vec![self.service.root.clone()];
        let result = self.auxiliary_timed(
            self.payload.node(),
            vec![
                "--max-old-space-size=256".into(),
                "--input-type=module".into(),
                "-e".into(),
                include_str!("native_target_inventory.mjs").into(),
                resources.platform_root.to_string_lossy().into_owned(),
                manifest_sha256.into(),
                target_schema.into(),
                system,
                String::from_utf8(resources.migrations.clone())
                    .map_err(|_| err("NATIVE_TARGET_INPUT_INVALID"))?,
                resources.migrations_sha256.clone(),
            ],
            vec![
                ("DATABASE_URL", database),
                (
                    "BLOB_ROOT",
                    self.service
                        .root
                        .join("blobs")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            Some(manifest),
            Duration::from_secs(3600),
        );
        self.maintenance_space.clear();
        match result {
            Ok((0, mut bytes)) => {
                resources.recheck()?;
                self.payload.recheck()?;
                self.check_native_observation_fence()?;
                self.backup_source_current()?;
                if bytes.len() > 16384 {
                    bytes.fill(0);
                    return Err(err("NATIVE_TARGET_RESULT_INVALID"));
                }
                let value =
                    serde_json::from_slice(&bytes).map_err(|_| err("NATIVE_TARGET_RESULT_INVALID"));
                bytes.fill(0);
                value
            }
            other => {
                // A cancellation/timeout is not permission to release a running
                // maintenance child. Reuse the existing normal owned drain.
                while self.any_owned() {
                    self.stop()?;
                    if self.any_owned() {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                resources.recheck()?;
                self.payload.recheck()?;
                match other {
                    Err(e) => Err(e),
                    _ => Err(err("NATIVE_TARGET_OBSERVATION_FAILED")),
                }
            }
        }
    }
}

/// A borrowed Session plus a correlated live-bridge claim. Its selected-flow
/// settlement is not general publication or crash-recovery authority.
#[allow(dead_code)]
struct OwnedOexClaim<'a> {
    session:&'a mut Session,
    stage_root:&'a oex_stage_root::StageRoot,
    challenge:owner_claim_frames::ClaimChallenge,
    ready:owner_claim_frames::ClaimReady,
    capacity:u64,
    stage_started:bool,
    retained_packed:Option<RetainedOexPacked>,
    save_started:bool,
    save_ready:Option<owner_claim_frames::SaveFrame>,
    settled:bool,
}
/// The v3 claim holds the exact Session, owner observation and input receipt.
/// It is intentionally not a provider release or completion token.
#[allow(dead_code)]
struct OwnedOexV3Claim<'a> {
    session:&'a mut Session,
    stage_root:&'a oex_stage_root::StageRoot,
    owner:oex_owner::Owner,
    challenge:owner_claim_frames::ClaimChallenge,
    ready:owner_claim_frames::ClaimReady,
    input_receipt:Vec<u8>,
    preflight_started:bool,
    retained:Option<RetainedV3Preflight>,
    report_started:bool,
    bound:Option<RetainedV3Reservation>,
    packed_started:bool,
    packed:Option<RetainedV3Packed>,
    save_started:bool,
    save_ready:Option<owner_claim_frames::SaveFrameV3>,
    peer_save_verified:bool,
    retirement_started:bool,
    retired:Option<RetiredV3>,
    uncertain_retirement:Option<RetiredV3>,
    uncertain_save:Option<(owner_claim_frames::SaveFrameV3,String)>,
    settlement_started:bool,
    completed:bool,
}
#[allow(dead_code)]
struct RetainedV3Preflight {
    input:oex_read_fd::ObservedStage,
    materializer:oex_materializer::ReapedMaterializer,
    materializer_request:oex_materializer_protocol::Request,
    preflight:oex_preflight::Prepared,
    preflight_request:oex_preflight::Control,
}
#[allow(dead_code)]
struct RetainedV3Reservation { bound:oex_preflight_frames::Bound, output_receipt:Option<Vec<u8>> }
#[allow(dead_code)]
struct RetainedV3Packed {output:File,prepared:oex_worker_v2::Prepared,control:oex_control_v2::Control,journal:crate::maintenance_source::DirectoryPin}
#[allow(dead_code)]
struct RetiredV3 {
    materializer:oex_materializer::ReapedMaterializer,materializer_request:oex_materializer_protocol::Request,
    preflight:oex_preflight::Prepared,preflight_request:oex_preflight::Control,
    prepared:oex_worker_v2::Prepared,control:oex_control_v2::Control,journal:crate::maintenance_source::DirectoryPin,
    save:owner_claim_frames::SaveFrameV3,etag:String,
}
fn check_v3_held(session:&mut Session,owner:&oex_owner::Owner,root:&oex_stage_root::StageRoot,r:&owner_claim_frames::ClaimReady,
    retained:&RetainedV3Preflight,packed:&RetainedV3Packed,cancelled:&AtomicBool)->Result<()> {
    use std::os::unix::fs::MetadataExt;
    owner.check(session)?;retained.input.check_unchanged()?;packed.journal.check()?;
    let m=packed.output.metadata().map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
    let stamp=(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec());
    root.check_pair(session,&r.stage_id,(retained.input.device,retained.input.inode),&r.attempt_id,(m.dev(),m.ino()))?;
    if cancelled.load(Ordering::SeqCst)||stamp!=packed.prepared.verified.output_stamp
        || packed.control.encode()?!=packed.prepared.control || !retained.preflight.matches(&retained.preflight_request)
        || !retained.materializer.matches(&retained.materializer_request,&retained.materializer_request.encode()?) {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    Ok(())
}
fn check_v3_retired(session:&mut Session,owner:&oex_owner::Owner,root:&oex_stage_root::StageRoot,retired:&RetiredV3,cancelled:&AtomicBool)->Result<()> {
    owner.check(session)?;root.check_empty(session)?;retired.journal.check()?;
    if cancelled.load(Ordering::SeqCst)||retired.control.encode()?!=retired.prepared.control
        || !retired.preflight.matches(&retired.preflight_request)
        || !retired.materializer.matches(&retired.materializer_request,&retired.materializer_request.encode()?) {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    Ok(())
}
#[allow(dead_code)]
impl OwnedOexV3Claim<'_> {
    /// Accept one exact two-receipt settlement only after the owned temporary
    /// files were retired and the trusted host save completed. Lost frames or
    /// partial provider release remain UNKNOWN; no rearm or retry occurs here.
    fn settle(&mut self,cancelled:&AtomicBool)->Result<()> {
        if self.settlement_started {self.completed=false;if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.settlement_started=true;
        let result=(|| {
            let retired=self.retired.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let bound=&self.bound.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?.bound;
            if !self.peer_save_verified||self.uncertain_save.is_some()||self.uncertain_retirement.is_some(){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            self.owner.recheck_database_identity(self.session)?;
            let cancel=self.session.cancellation.clone();let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let exchange=(|| {
                let mut check=||check_v3_retired(self.session,&self.owner,self.stage_root,retired,cancelled);
                let bytes=channel.read_frame(Instant::now()+Duration::from_secs(120),&cancel,&mut check)?;drop(check);
                let settlement=owner_claim_frames::SettlementV3::parse_check(&bytes,&self.challenge,bound,&retired.save,&retired.etag)?;
                self.owner.recheck_database_identity(self.session)?;
                let mut check=||check_v3_retired(self.session,&self.owner,self.stage_root,retired,cancelled);
                let deadline=Instant::now()+Duration::from_secs(120);
                channel.send_frame(&settlement.ready()?,deadline,&cancel,&mut check)?;
                let receipt=channel.read_frame(deadline,&cancel,&mut check)?;drop(check);
                let ack=settlement.receipt_ack(&receipt)?;
                self.owner.recheck_database_identity(self.session)?;
                if self.ready.format == oex_recovery::FORMAT {
                    check_v3_retired(self.session,&self.owner,self.stage_root,retired,cancelled)?;
                    oex_recovery::persist(self.session,&self.owner,&self.challenge,&self.ready,&retired.save,&retired.etag,&receipt)?;
                }
                let mut check=||check_v3_retired(self.session,&self.owner,self.stage_root,retired,cancelled);
                channel.send_frame(&ack,Instant::now()+Duration::from_secs(120),&cancel,&mut check)?;
                let completed=channel.read_frame(Instant::now()+Duration::from_secs(120),&cancel,&mut check)?;
                retired.save.check_completed(&retired.etag,&completed)?;check()
            })();
            match exchange {Ok(())=>{self.session.owner_channel=Some(channel);Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
        })();
        match result {Ok(())=>{self.completed=true;Ok(())},Err(error)=>{self.completed=false;if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)}}
    }
    /// Only the host's existing durable selected-save path may supply `save`:
    /// it must record the exact offer and complete same-session GET/PUT/GET.
    /// No provider release is implied by retirement.
    fn save_and_retire<F>(&mut self,cancelled:&AtomicBool,mut save:F)->Result<()>
    where F:FnMut(&str,&str,&str,&str)->Result<String> {
        use std::os::fd::IntoRawFd;
        if self.retirement_started {if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.retirement_started=true;
        let result=(|| {
            let ready=self.wait_save_ready(cancelled)?;
            let etag=save(&ready.exhibition_id,&ready.etag,&ready.request_id,&ready.attempt_id)?;
            self.uncertain_save=Some((ready.clone(),etag.clone()));
            self.confirm_save(&etag,cancelled)?;
            let retained=self.retained.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let packed=self.packed.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            self.owner.recheck_database_identity(self.session)?;
            check_v3_held(self.session,&self.owner,self.stage_root,&self.ready,retained,packed,cancelled)?;
            if !self.peer_save_verified{return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            self.stage_root.retire_pair(self.session,&self.owner,&self.ready,retained.input.file(),&packed.output,cancelled)?;
            let RetainedV3Preflight{input,materializer,materializer_request,preflight,preflight_request}=self.retained.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let RetainedV3Packed{output,prepared,control,journal}=self.packed.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            self.uncertain_retirement=Some(RetiredV3{materializer,materializer_request,preflight,preflight_request,prepared,control,journal,save:ready,etag});
            let input_closed=input.close_owned();let output_fd=output.into_raw_fd();let output_closed=unsafe{libc::close(output_fd)}==0;
            if input_closed.is_err()||!output_closed{return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            self.owner.recheck_database_identity(self.session)?;self.stage_root.check_empty(self.session)?;
            self.uncertain_save=None;
            self.uncertain_retirement.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))
        })();
        match result {Ok(retired)=>{self.retired=Some(retired);Ok(())},Err(error)=>{if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)}}
    }
    /// Receive a post-import offer while retaining the input, output, and all
    /// direct-child results. The host must durably perform GET/PUT/GET.
    fn wait_save_ready(&mut self,cancelled:&AtomicBool)->Result<owner_claim_frames::SaveFrameV3> {
        if self.save_started {if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.save_started=true;
        let result=(|| {
            let retained=self.retained.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let packed=self.packed.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let bound=&self.bound.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?.bound;
            self.owner.recheck_database_identity(self.session)?;
            let cancel=self.session.cancellation.clone();let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=||check_v3_held(self.session,&self.owner,self.stage_root,&self.ready,retained,packed,cancelled);
            let response=channel.receive_save_ready_v3(bound,&cancel,&mut check);drop(check);
            match response {Ok(ready)=>{self.session.owner_channel=Some(channel);self.owner.recheck_database_identity(self.session)?;Ok(ready)},Err(error)=>{channel.invalidate();Err(error)}}
        })();
        match result {Ok(ready)=>{self.save_ready=Some(ready.clone());Ok(ready)},Err(error)=>{if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)}}
    }
    /// Call only after the host persisted and verified the exact same-session
    /// GET/PUT/GET. Until that host result is wired, this inactive method
    /// establishes only the peer's echo, not Manager save evidence or release.
    fn confirm_save(&mut self,etag:&str,cancelled:&AtomicBool)->Result<()> {
        let result=(|| {
            let ready=self.save_ready.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let retained=self.retained.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let packed=self.packed.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            self.owner.recheck_database_identity(self.session)?;
            let cancel=self.session.cancellation.clone();let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=||check_v3_held(self.session,&self.owner,self.stage_root,&self.ready,retained,packed,cancelled);
            let response=channel.confirm_save_v3(&ready,etag,&cancel,&mut check);drop(check);
            match response {Ok(())=>{self.session.owner_channel=Some(channel);Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
        })();
        match result {Ok(())=>{self.peer_save_verified=true;Ok(())},Err(error)=>{self.peer_save_verified=false;if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)}}
    }
    /// Reuse the owned packed worker and bounded streaming on the confirmed
    /// reservation. The result is not yet a commit or settlement grant.
    fn pack_and_serve(&mut self,cancelled:&AtomicBool)->Result<()> {
        use std::os::unix::fs::MetadataExt;
        if self.packed_started {if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.packed_started=true;
        let result=(|| {
            let retained=self.retained.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let reservation=self.bound.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let r=&self.ready;let input=&retained.input;let bound=&reservation.bound;
            self.owner.recheck_database_identity(self.session)?;input.check_unchanged()?;
            self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;
            match reservation.output_receipt.as_deref(){Some(receipt)=>{bound.confirm_output(receipt)?;},None=>{bound.confirm_empty()?;}}
            let (capacity,manifest,output_plan)=bound.output_plan();
            let (reservation_id,reservation_generation,reservation_digest)=output_plan.unwrap_or((&r.reservation_id,&r.reservation_generation,&r.reservation_digest));
            let output=self.stage_root.create_output(self.session,&r.attempt_id)?;
            let m=output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;let output_id=(m.dev(),m.ino());let input_id=(input.device,input.inode);
            self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;
            let ready=bound.packed_ready(&output,reservation.output_receipt.as_deref())?;let bytes=ready.encode()?;let cancel=self.session.cancellation.clone();
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;input.check_unchanged()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let accepted=channel.exchange(&bytes,&cancel,&mut check).and_then(|reply|ready.check_accepted(&reply)).and_then(|_|check());drop(check);
            if let Err(error)=accepted{channel.invalidate();return Err(error);}self.session.owner_channel=Some(channel);
            self.owner.recheck_database_identity(self.session)?;
            let control=oex_control_v2::Control{version:2,kind:"oex-packed-v2".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:reservation_id.into(),reservation_generation:reservation_generation.into(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),capacity,input_device:input.device.to_string(),input_inode:input.inode.to_string(),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),reservation_digest:reservation_digest.into(),source_digest:r.source_digest.clone(),worker_digest:self.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
            let journal_path=self.session.run.as_ref().ok_or_else(||err("NATIVE_JOURNAL_INVALID"))?.clone();
            let journal=crate::maintenance_source::DirectoryPin::open(&journal_path)?;let journal_file=journal.try_clone_file()?;let payload=self.session.payload.clone();
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;input.check_unchanged()?;journal.check()?;
                self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let prepared=oex_worker_v2::run_direct(&payload,&journal_file,input,&output,&control,cancelled,&mut check)?;drop(check);
            if prepared.verified.manifest_sha256!=manifest||prepared.verified.output_stamp.2!=capacity {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            self.owner.recheck_database_identity(self.session)?;
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;input.check_unchanged()?;journal.check()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let served=channel.serve_packed_v3(owner_channel::PackedServe{control:&control,prepared:&prepared,output:&output},&r.attempt_id,&self.challenge.correlation_id,bound,&cancel,&mut check);drop(check);
            if let Err(error)=served{channel.invalidate();return Err(error);}self.session.owner_channel=Some(channel);
            Ok(RetainedV3Packed{output,prepared,control,journal})
        })();
        match result {Ok(packed)=>{self.packed=Some(packed);Ok(())},Err(error)=>{if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)}}
    }
    /// Report the exact direct-child result and correlate an optional E lease
    /// against the same independently pinned owner. Viewer verifies liveness.
    fn bind_preflight(&mut self,cancelled:&AtomicBool)->Result<()> {
        if self.report_started {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        self.report_started=true;
        let result=(|| {
            let retained=self.retained.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            let r=&self.ready;let input=&retained.input;
            self.owner.recheck_database_identity(self.session)?;
            self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;
            input.check_unchanged()?;
            let plan=oex_reservation::V3PlanScope::from_owner(&self.owner,self.stage_root.path(),self.stage_root.device());
            let scope=oex_preflight_frames::InputScope::from_receipt(&self.input_receipt,&retained.preflight_request,plan)?;
            if !retained.materializer.matches(&retained.materializer_request,&retained.materializer_request.encode()?) {
                return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
            }
            let report=retained.preflight.report(&retained.preflight_request,&self.challenge)?;
            let cancel=self.session.cancellation.clone();
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;input.check_unchanged()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let exchange=(|| {
                let bytes=channel.exchange(&report,&cancel,&mut check)?;
                let bound=oex_preflight_frames::Bound::parse(&bytes,&retained.preflight,&retained.preflight_request,&self.challenge,&scope)?;
                let output_receipt=match bound.output_receipt_digest() {
                    Some(sha)=>Some(channel.receive_reservation_receipt(&self.challenge,sha,&cancel,&mut check)?),None=>None,
                };
                let confirm=match output_receipt.as_deref() {Some(receipt)=>bound.confirm_output(receipt)?,None=>bound.confirm_empty()?};
                Ok((RetainedV3Reservation{bound,output_receipt},confirm))
            })();
            drop(check);
            let (bound,confirm)=match exchange {Ok(value)=>value,Err(error)=>{channel.invalidate();return Err(error)}};
            if let Err(error)=self.owner.recheck_database_identity(self.session) {channel.invalidate();return Err(error);}
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;input.check_unchanged()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let sent=channel.send_frame(&confirm,Instant::now()+Duration::from_secs(30),&cancel,&mut check).and_then(|_|check());
            drop(check);
            match sent {Ok(())=>{self.session.owner_channel=Some(channel);Ok(bound)},Err(error)=>{channel.invalidate();Err(error)}}
        })();
        match result {Ok(bound)=>{self.bound=Some(bound);Ok(())},Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)
        }}
    }
    /// Sequentially materialize and preflight the same input under this claim.
    /// A failed attempt cannot be rearmed; its named stage and durable records
    /// remain for reconciliation. On success the input FD and both direct-child
    /// results are retained, without implying a provider grant.
    fn materialize_and_preflight(&mut self,cancelled:&AtomicBool)->Result<()> {
        if self.preflight_started {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        self.preflight_started=true;
        let r=&self.ready;
        let outcome=(|| {
            self.owner.recheck_database_identity(self.session)?;
            self.stage_root.check_empty(self.session)?;
            let input_root=self.session.observation_root.try_clone_file()?;
            let chunks=oex_chunks::open_local(&input_root,&r.tenant_id,&r.job_id,r.archive_bytes)?;
            let stage=self.stage_root.create_input(self.session,&r.stage_id)?;
            let (device,inode)=stage.identity()?;
            self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
            let request=oex_materializer_protocol::Request{version:1,kind:"oex-chunks-v1".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),stage_device:device.to_string(),stage_inode:inode.to_string(),chunks:chunks.iter().map(|c|oex_materializer_protocol::Chunk{device:c.device.to_string(),inode:c.inode.to_string(),bytes:c.bytes}).collect(),reservation_digest:r.reservation_digest.clone(),source_digest:r.source_digest.clone(),worker_digest:self.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
            let bound=owner_claim_frames::StageBound{version:1,kind:"stage-bound-v3".into(),correlation_id:self.challenge.correlation_id.clone(),runtime_generation:r.runtime_generation.clone(),workspace_id:r.workspace_id.clone(),tenant_id:r.tenant_id.clone(),job_id:r.job_id.clone(),attempt_id:r.attempt_id.clone(),generation:r.generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),stage_device:device.to_string(),stage_inode:inode.to_string()};
            let bytes=bound.encode_v3()?;let cancel=self.session.cancellation.clone();
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let accepted=channel.exchange(&bytes,&cancel,&mut check).and_then(|reply|bound.check_accepted_v3(&bytes,&reply)).and_then(|_|check());
            drop(check);
            if let Err(error)=accepted {channel.invalidate();return Err(error);}
            self.session.owner_channel=Some(channel);
            let payload=self.session.payload.clone();
            let v4=payload.workspace_import_selected_input() || payload.normal_oex_028_selected_input()?;
            let mut check=|| {self.owner.check(self.session)?;self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
                self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()?;
                if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())};
            let materializer=oex_materializer::run_direct(&payload,&stage,&chunks,&request,cancelled,&mut check)?;
            if !materializer.matches(&request,&request.encode()?) {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            let input=oex_read_fd::observe_stage(stage,r.archive_bytes,&r.archive_sha256,cancelled,&mut check)?;
            let preflight_request=oex_preflight::Control{version:1,kind:"oex-preflight-v1".into(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),reservation_digest:r.reservation_digest.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),input_device:device.to_string(),input_inode:inode.to_string()};
            let preflight=oex_preflight::run_direct(&payload,&input,&preflight_request,if v4{Some(r.format.as_str())}else{None},cancelled,&mut check)?;
            drop(check);
            if !v4 && r.format!="1.0.0-draft.9"{return Err(err("NATIVE_OEX_PREFLIGHT_INVALID"));}
            input.check_unchanged()?;
            Ok(RetainedV3Preflight{input,materializer,materializer_request:request,preflight,preflight_request})
        })();
        match outcome {Ok(retained)=>{self.retained=Some(retained);Ok(())},Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)
        }}
    }
}
impl Drop for OwnedOexV3Claim<'_> {
    fn drop(&mut self) {
        // An unverified or abandoned operation must close the channel. A
        // checked completion retains the existing Session's connection.
        owner_channel::finish_v3_claim(&mut self.session.owner_channel, self.completed);
    }
}
// Kept with the borrowed Session/claim until settlement or conservative drop.
// These handles are not authority to release on their own.
#[allow(dead_code)]
struct RetainedOexPacked {
    _input:oex_read_fd::ObservedStage,
    _output:File,
    _prepared:oex_worker_v2::Prepared,
    _journal:crate::maintenance_source::DirectoryPin,
    _materializer:oex_materializer::ReapedMaterializer,
    _materializer_request:oex_materializer_protocol::Request,
}
#[allow(dead_code)]
impl OwnedOexClaim<'_> {
    /// One sequential attempt, under this same borrowed Session and claim.
    /// Retains descriptors after drain response; neither response nor return
    /// establishes Node decoder drain, DB commit, or reservation release.
    fn materialize_pack_and_serve(&mut self,stage:oex_read_fd::Stage,chunks:&[oex_chunks::Chunk],
        request:&oex_materializer_protocol::Request,correlation:&str,cancelled:&AtomicBool)->Result<()> {
        use std::os::unix::fs::MetadataExt;
        if self.stage_started||self.retained_packed.is_some()||correlation!=self.challenge.correlation_id {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let (input,materializer)=self.materialize(stage,chunks,request,cancelled)?;
        let r=&self.ready;
        let result=(|| {
            self.stage_root.check_input(self.session,&r.stage_id,input.device,input.inode)?;
            let output=self.stage_root.create_output(self.session,&r.attempt_id)?;
            let m=output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            let output_id=(m.dev(),m.ino());let input_id=(input.device,input.inode);
            self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;
            let journal_path=self.session.run.as_ref().ok_or_else(||err("NATIVE_JOURNAL_INVALID"))?.clone();
            let journal=crate::maintenance_source::DirectoryPin::open(&journal_path)?;
            let journal_file=journal.try_clone_file()?;
            let control=oex_control_v2::Control{version:2,kind:"oex-packed-v2".into(),tenant_id:r.tenant_id.clone(),resource_id:r.workspace_id.clone(),job_id:r.job_id.clone(),user_id:r.user_id.clone(),request_id:r.request_id.clone(),lease_id:r.lease_id.clone(),attempt_id:r.attempt_id.clone(),runtime_generation:r.runtime_generation.clone(),stage_id:r.stage_id.clone(),reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),archive_bytes:r.archive_bytes,archive_sha256:r.archive_sha256.clone(),capacity:self.capacity,input_device:input.device.to_string(),input_inode:input.inode.to_string(),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),reservation_digest:r.reservation_digest.clone(),source_digest:r.source_digest.clone(),worker_digest:self.session.payload.context().manifest_sha256,profile:r.profile.clone(),format:r.format.clone()};
            let payload=self.session.payload.clone();
            let mut check=|| {
                self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;journal.check()?;
                if self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
                self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()
            };
            let prepared=oex_worker_v2::run_direct(&payload,&journal_file,&input,&output,&control,cancelled,&mut check)?;
            drop(check);
            let cancel=self.session.cancellation.clone();
            let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            let mut check=|| {
                self.stage_root.check_pair(self.session,&r.stage_id,input_id,&r.attempt_id,output_id)?;journal.check()?;
                if self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}Ok(())
            };
            let served=channel.serve_packed(&control,&prepared,&output,&r.attempt_id,correlation,&cancel,&mut check);
            drop(check);
            if let Err(error)=served{channel.invalidate();return Err(error);}
            self.session.owner_channel=Some(channel);
            Ok(RetainedOexPacked{_input:input,_output:output,_prepared:prepared,_journal:journal,
                _materializer:materializer,_materializer_request:request.clone()})
        })();
        match result {Ok(retained)=>{self.retained_packed=Some(retained);Ok(())},Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}Err(error)
        }}
    }
    /// Receive one post-import save offer without releasing this claim/output.
    /// This is not itself permission to mutate a draft or infer COMMIT.
    fn wait_save_ready(&mut self,cancelled:&AtomicBool)->Result<owner_claim_frames::SaveFrame> {
        use std::os::unix::fs::MetadataExt;
        if self.save_started||self.retained_packed.is_none(){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        self.save_started=true;let retained=self.retained_packed.as_ref().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let input=(retained._input.device,retained._input.inode);let m=match retained._output.metadata(){Ok(value)=>value,Err(_)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }};let output=(m.dev(),m.ino());
        let stamp=retained._prepared.verified.output_stamp;let r=&self.ready;let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.stage_root.check_pair(self.session,&r.stage_id,input,&r.attempt_id,output)?;retained._journal.check()?;
            let m=retained._output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            if (m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())!=stamp
                || self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}Ok(())
        };
        let result=channel.receive_save_ready(&r.attempt_id,&cancel,&mut check);drop(check);
        match result {Ok(ready)=>{self.save_ready=Some(ready.clone());self.session.owner_channel=Some(channel);Ok(ready)},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Caller invokes only after the real same-session GET/one PUT/GET has
    /// returned and the exact pending request/draft is durably retained.
    fn confirm_save(&mut self,etag:&str,cancelled:&AtomicBool)->Result<()> {
        use std::os::unix::fs::MetadataExt;
        let ready=self.save_ready.take().ok_or_else(||err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let retained=match self.retained_packed.as_ref(){Some(value)=>value,None=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }};
        let input=(retained._input.device,retained._input.inode);let m=match retained._output.metadata(){Ok(value)=>value,Err(_)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }};let output=(m.dev(),m.ino());
        let stamp=retained._prepared.verified.output_stamp;let r=&self.ready;let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let trace_log=self.session.log.clone();
        let mut trace=|message: &'static [u8]| { if let Some(log)=&trace_log { log.write(message); } };
        let materializer_request=&retained._materializer_request;
        let materializer_bytes=materializer_request.encode()?;
        let packed:oex_control_v2::Control=serde_json::from_slice(&retained._prepared.control).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        let started=self.stage_started;
        let mut check=|| {
            if !started || !retained._materializer.matches(materializer_request,&materializer_bytes)
                || packed.encode()?!=retained._prepared.control || packed.attempt_id!=r.attempt_id
                || packed.runtime_generation!=r.runtime_generation || packed.reservation_id!=r.reservation_id
                || packed.reservation_generation!=r.reservation_generation || packed.reservation_digest!=r.reservation_digest
                || packed.stage_id!=r.stage_id || packed.archive_sha256!=r.archive_sha256 {
                return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
            }
            self.stage_root.check_pair(self.session,&r.stage_id,input,&r.attempt_id,output)?;retained._journal.check()?;
            let m=retained._output.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
            if (m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())!=stamp
                || self.session.cancelled()||cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}Ok(())
        };
        let result=channel.confirm_save(&ready,&self.challenge,etag,&cancel,&mut check,&mut trace);drop(check);
        match result {Ok(())=>{self.session.owner_channel=Some(channel);self.settled=true;Ok(())},Err(error)=>{channel.invalidate();Err(error)}}
    }
    /// Inactive connection of the same claimed Session to the directly owned
    /// materializer. The caller must create the stage in the independently
    /// pinned live provider reservation and open source chunks from the exact
    /// authenticated job root. A correlated result is still not a stage seal,
    /// DB commit, claim release, or proof of absent noncooperating writers.
    fn materialize(&mut self,stage:oex_read_fd::Stage,chunks:&[oex_chunks::Chunk],
        request:&oex_materializer_protocol::Request,cancelled:&AtomicBool)->Result<(oex_read_fd::ObservedStage,oex_materializer::ReapedMaterializer)> {
        let r=&self.ready;
        if self.stage_started || !stage.matches_stage(&r.stage_id)
            || request.tenant_id!=r.tenant_id || request.resource_id!=r.workspace_id || request.job_id!=r.job_id
            || request.user_id!=r.user_id || request.request_id!=r.request_id || request.lease_id!=r.lease_id
            || request.attempt_id!=r.attempt_id || request.runtime_generation!=r.runtime_generation
            || request.stage_id!=r.stage_id || request.reservation_id!=r.reservation_id
            || request.reservation_generation!=r.reservation_generation || request.archive_bytes!=r.archive_bytes
            || request.archive_sha256!=r.archive_sha256 || request.source_digest!=r.source_digest
            || request.reservation_digest!=r.reservation_digest || request.worker_digest!=self.session.payload.context().manifest_sha256 {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let (device,inode)=match stage.identity(){Ok(id)=>id,Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};
        if let Err(error)=self.stage_root.check_input(self.session,&r.stage_id,device,inode) {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }
        if request.stage_device!=device.to_string() || request.stage_inode!=inode.to_string() {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        self.stage_started=true;
        let stage_bound=owner_claim_frames::StageBound {version:1,kind:"stage-bound".into(),correlation_id:self.challenge.correlation_id.clone(),
            runtime_generation:r.runtime_generation.clone(),workspace_id:r.workspace_id.clone(),tenant_id:r.tenant_id.clone(),
            job_id:r.job_id.clone(),attempt_id:r.attempt_id.clone(),generation:r.generation.clone(),stage_id:r.stage_id.clone(),
            reservation_id:r.reservation_id.clone(),reservation_generation:r.reservation_generation.clone(),
            stage_device:device.to_string(),stage_inode:inode.to_string()};
        let bytes=match stage_bound.encode(){Ok(bytes)=>bytes,Err(error)=>{
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
            return Err(error);
        }};let cancel=self.session.cancellation.clone();
        let mut channel=self.session.owner_channel.take().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut check=|| {
            self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
            let auth=self.session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.session.service.personal_identity()?;
            if self.session.cancelled() || cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
            if self.session.state!="ready" || self.session.node.is_none() || self.session.postgres.is_none()
                || self.session.job_id.as_deref()!=Some(r.runtime_generation.as_str()) || auth.owner_uid!=unsafe {libc::geteuid()}
                || actual!=(r.workspace_id.clone(),r.tenant_id.clone(),r.user_id.clone())
                || (auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())!=(r.workspace_id.as_str(),r.tenant_id.as_str(),r.user_id.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let response=channel.exchange(&bytes,&cancel,&mut check)
            .and_then(|reply|stage_bound.check_accepted(&bytes,&reply)).and_then(|_|check());
        drop(check);
        if let Err(error)=response {channel.invalidate();return Err(error);}
        self.session.owner_channel=Some(channel);
        let payload=self.session.payload.clone();
        let mut check=|| {
            self.stage_root.check_input(self.session,&r.stage_id,device,inode)?;
            self.session.owner_channel.as_mut().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.check_idle()?;
            let auth=self.session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
            let actual=self.session.service.personal_identity()?;
            if self.session.cancelled() || cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
            if self.session.state!="ready" || self.session.node.is_none() || self.session.postgres.is_none()
                || self.session.job_id.as_deref()!=Some(r.runtime_generation.as_str()) || auth.owner_uid!=unsafe {libc::geteuid()}
                || actual!=(r.workspace_id.clone(),r.tenant_id.clone(),r.user_id.clone())
                || (auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())!=(r.workspace_id.as_str(),r.tenant_id.as_str(),r.user_id.as_str()) {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
            Ok(())
        };
        let observed=(|| {
            let reaped=oex_materializer::run_direct(&payload,&stage,chunks,request,cancelled,&mut check)?;
            if !reaped.matches(request,&request.encode()?) {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
            let observed=oex_read_fd::observe_stage(stage,r.archive_bytes,&r.archive_sha256,cancelled,&mut check)?;
            Ok((observed,reaped))
        })();
        drop(check);
        if observed.is_err() {
            if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}
        }
        observed
    }
}
impl Drop for OwnedOexClaim<'_> {
    fn drop(&mut self) {
        // Any path without both settlement and operation-complete remains
        // uncertain: close the channel and preserve the durable records.
        if !self.settled {if let Some(mut channel)=self.session.owner_channel.take(){channel.invalidate();}}
    }
}
