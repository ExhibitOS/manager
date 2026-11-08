// SPDX-License-Identifier: Apache-2.0
//! Inactive authenticated operation admission. The API owns the DB lease
//! and rights transaction. Neither a binding nor a fresh reply proves drain.

use super::{
    err,
    frame::{BODY_BYTES, BODY_SHA},
    generation::{
        validation::{RunningValidation, ValidatedReadback},
        RunningGeneration, WorkerCheckpoint,
    },
    launch::SpawnedWorker,
    transfer::Transfer,
    Attempt, Binding, Lease, Result, SourceStage,
};
use crate::{
    hash_valid,
    native_payload::Payload,
    native_runtime::{
        oex_owner::{Owner, DATABASE_IDENTITY_SQL},
        owner_channel::Connection,
        Session,
    },
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

const OPEN_RECORD: &str = "operation-open.json";
const REVALIDATED_RECORD: &str = "operation-revalidated.json";
const WORKERS_HELD_RECORD: &str = "operation-workers-held.json";
const CONTROL_LIMIT: usize = 8192;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationBinding {
    version: u8,
    tenant_id: Uuid,
    workspace_id: Uuid,
    principal_id: Uuid,
    job_id: Uuid,
    lease_id: Uuid,
    attempt_id: Uuid,
    runtime_generation: Uuid,
    artwork_id: Uuid,
    correlation_id: Uuid,
    artwork_revision: u64,
    source_sha256: String,
    source_bytes: u64,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    HayesOperationOpen,
    HayesOperationOpenAccepted,
    HayesOperationRevalidate,
    HayesOperationRevalidated,
    HayesOperationAbort,
    HayesOutputReservationReady,
    HayesDispositionChallenge,
    HayesDispositionCommitted,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Open {
    version: u8,
    kind: Kind,
    binding: OperationBinding,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Accepted<'a> {
    version: u8,
    kind: Kind,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Revalidation<'a> {
    version: u8,
    kind: Kind,
    binding: &'a OperationBinding,
    challenge: Uuid,
    runtime_manifest_sha256: &'a str,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: u8,
    binding: OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: String,
    runtime_source_commit: String,
    runtime_producer_sha256: String,
    database_system_id: String,
    database_oid: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeldWorkers<'a> {
    version: u8,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    revalidation_challenge: Uuid,
    output_stage_id: Uuid,
    output_bytes: u64,
    output_sha256: &'a str,
    milliseconds_since_open_accepted: WorkerTimings,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerTimings {
    source_held: u64,
    revalidated: u64,
    generate_before_go: u64,
    generate_observed: u64,
    validate_before_go: u64,
    validate_observed: u64,
    checked: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Opened,
    SourceHeld,
    Revalidated,
    OutputHeld,
    ReadySent,
    Reserved,
    OutputAcknowledged,
    DispositionObserved,
    ReleaseRecorded,
    Failed,
}

/// Native-selected identity issued before the API sends OPEN. The caller
/// must convey it through the existing authenticated private bootstrap.
pub(in super::super) struct HayesSessionPin {
    runtime_generation: Uuid,
    host_manifest_sha256: String,
    native_owner_id: Uuid,
}

impl HayesSessionPin {
    pub(in super::super) fn new(
        runtime_generation: Uuid,
        host_manifest_sha256: &str,
    ) -> Result<Self> {
        if !canonical_uuid(&runtime_generation) || !hash_valid(host_manifest_sha256) {
            return Err(unavailable());
        }
        Ok(Self {
            runtime_generation,
            host_manifest_sha256: host_manifest_sha256.into(),
            native_owner_id: Uuid::new_v4(),
        })
    }

    pub(in super::super) fn runtime_generation(&self) -> Uuid {
        self.runtime_generation
    }

    pub(in super::super) fn host_manifest_sha256(&self) -> &str {
        &self.host_manifest_sha256
    }

    pub(in super::super) fn native_owner_id(&self) -> Uuid {
        self.native_owner_id
    }
}

#[cfg(all(
    test,
    feature = "workspace-v5-owner-validation",
    debug_assertions,
    target_os = "macos"
))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionProbeArm<'a> {
    version: u8,
    kind: &'static str,
    runtime_generation: Uuid,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
}

#[cfg(all(
    test,
    feature = "workspace-v5-owner-validation",
    debug_assertions,
    target_os = "macos"
))]
pub(in super::super) fn session_probe_arm(pin: &HayesSessionPin) -> Result<Vec<u8>> {
    encode(&SessionProbeArm {
        version: 1,
        kind: "hayes-session-probe-start",
        runtime_generation: pin.runtime_generation(),
        native_owner_id: pin.native_owner_id(),
        runtime_manifest_sha256: pin.host_manifest_sha256(),
    })
}

pub(in super::super) struct AuthenticatedHayes {
    pub(in super::super) pin: HayesSessionPin,
    pub(in super::super) channel: Connection,
}

pub(in super::super) enum HayesBootstrap {
    Disabled,
    Armed(String),
    Issued(AuthenticatedHayes),
    Spent,
}

impl HayesBootstrap {
    pub(in super::super) fn consume(&mut self) -> Result<AuthenticatedHayes> {
        if !matches!(self, Self::Issued(_)) {
            return Err(unavailable());
        }
        let Self::Issued(authenticated) = std::mem::replace(self, Self::Spent) else {
            unreachable!()
        };
        Ok(authenticated)
    }
}

enum RunningWorker {
    Generate(RunningGeneration),
    Validate(RunningValidation),
}

impl RunningWorker {
    fn into_child(self) -> SpawnedWorker {
        match self {
            Self::Generate(run) => run.child,
            Self::Validate(run) => run.child,
        }
    }
}

/// The Session retains the attempt and any direct child after the exchange
/// closes. Reaping only updates direct-child ownership. It cannot retire the
/// reservation or authorize output.
pub(in super::super) struct RetainedHayes {
    _attempt: Attempt,
    _source: Option<SourceStage>,
    _validated: Option<ValidatedReadback>,
    child: Option<SpawnedWorker>,
}

impl RetainedHayes {
    pub(in super::super) fn has_child(&self) -> bool {
        self.child.is_some()
    }

    pub(in super::super) fn poll(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        if child.try_wait().map_err(|_| unavailable())?.is_none() {
            return Ok(false);
        }
        self.child = None;
        Ok(true)
    }
}

pub(in super::super) struct OwnedHayesOpen<'a> {
    session: &'a mut Session,
    owner: Owner,
    channel: Connection,
    attempt: Option<Attempt>,
    record: Record,
    source: Option<SourceStage>,
    revalidation_challenge: Option<Uuid>,
    worker: Option<RunningWorker>,
    validated: Option<ValidatedReadback>,
    ready_pins: Option<ready::ReadyPins>,
    db_stage_id: Option<Uuid>,
    disposition: Option<ready::disposition::Notice>,
    retain_attempt: bool,
    open_accepted_at: Instant,
    timings: WorkerTimings,
    phase: Phase,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::native_runtime) struct ExecuteBinding {
    pub(in crate::native_runtime) job_id: Uuid,
    pub(in crate::native_runtime) artwork_id: Uuid,
    pub(in crate::native_runtime) artwork_revision: u32,
}

impl Session {
    /// The private selected-runtime entry supplies the acquired UID authority.
    /// Only this Session's dedicated inherited channel can supply the pin.
    pub(in super::super) fn begin_owned_hayes_open<'a>(
        &'a mut self,
        lease: Lease,
    ) -> Result<OwnedHayesOpen<'a>> {
        self.begin_owned_hayes_open_until(lease, Instant::now() + Duration::from_secs(30))
    }

    pub(in super::super) fn begin_owned_hayes_open_until<'a>(
        &'a mut self,
        lease: Lease,
        deadline: Instant,
    ) -> Result<OwnedHayesOpen<'a>> {
        self.begin_owned_hayes_open_inner(lease, deadline, &mut |_| {}, None, None)
    }

    #[cfg(target_os = "macos")]
    pub(in crate::native_runtime) fn begin_owned_hayes_open_for(
        &mut self,
        lease: Lease,
        deadline: Instant,
        expected: ExecuteBinding,
    ) -> Result<OwnedHayesOpen<'_>> {
        self.begin_owned_hayes_open_inner(lease, deadline, &mut |_| {}, None, Some(expected))
    }

    #[cfg(all(
        test,
        feature = "workspace-v5-owner-validation",
        debug_assertions,
        target_os = "macos"
    ))]
    pub(in super::super) fn begin_owned_hayes_probe_open<'a>(
        &'a mut self,
        lease: Lease,
        deadline: Instant,
        observe: &mut dyn FnMut(&'static str),
    ) -> Result<OwnedHayesOpen<'a>> {
        if self.payload.hayes_session_probe_script()?.is_none() {
            return Err(unavailable());
        }
        self.begin_owned_hayes_open_inner(lease, deadline, observe, Some(session_probe_arm), None)
    }

    fn begin_owned_hayes_open_inner<'a>(
        &'a mut self,
        lease: Lease,
        deadline: Instant,
        observe: &mut dyn FnMut(&'static str),
        probe_arm: Option<fn(&HayesSessionPin) -> Result<Vec<u8>>>,
        expected: Option<ExecuteBinding>,
    ) -> Result<OwnedHayesOpen<'a>> {
        if self.retained_hayes.is_some() || Instant::now() >= deadline {
            observe("open-unavailable");
            return Err(unavailable());
        }
        let AuthenticatedHayes { pin, mut channel } = self
            .hayes_bootstrap
            .consume()
            .inspect_err(|_| observe("open-session-pin-unavailable"))?;
        let result = (|| {
            channel
                .claim_hayes()
                .inspect_err(|_| observe("open-channel-claim-failed"))?;
            let host = self.payload.context();
            if host.manifest_sha256 != pin.host_manifest_sha256
                || self.job_id.as_deref() != Some(pin.runtime_generation.to_string().as_str())
            {
                observe("open-session-pin-mismatch");
                return Err(unavailable());
            }
            observe("open-payload-recheck-start");
            self.payload
                .recheck()
                .inspect_err(|_| observe("open-payload-recheck-failed"))?;
            observe("open-payload-recheck-done");
            observe("open-owner-capture-start");
            let owner = Owner::capture_observed(self, observe)?;
            observe("open-owner-capture-done");
            let cancellation = self.cancellation.clone();
            if let Some(probe_arm) = probe_arm {
                let arm = probe_arm(&pin)?;
                observe("probe-arm-send-start");
                channel
                    .send_frame(&arm, deadline, &cancellation, &mut || {
                        owner.check_observed(self, observe)
                    })
                    .inspect_err(|_| observe("probe-arm-send-failed"))?;
                observe("probe-arm-send-done");
            }
            observe("open-frame-read-start");
            let raw = channel
                .read_frame(deadline, &cancellation, &mut || {
                    owner.check_observed(self, observe)
                })
                .inspect_err(|_| observe("open-frame-read-failed"))?;
            observe("open-frame-read-done");
            let open = parse_open(&raw).inspect_err(|_| observe("open-frame-binding-invalid"))?;
            if expected.is_some_and(|expected| open.job_id != expected.job_id
                || open.artwork_id != expected.artwork_id
                || open.artwork_revision != u64::from(expected.artwork_revision))
            {
                return Err(unavailable());
            }
            let auth = self.local_authority.as_ref().ok_or_else(unavailable)?;
            open.check_session(
                &auth.workspace_id,
                &auth.tenant_id,
                &auth.principal_id,
                self.job_id.as_deref().ok_or_else(unavailable)?,
            )
            .inspect_err(|_| observe("open-frame-session-mismatch"))?;
            owner
                .recheck_database_identity(self)
                .inspect_err(|_| observe("open-frame-database-identity-failed"))?;
            if open.lease_id == pin.native_owner_id {
                observe("open-frame-lease-alias");
                return Err(unavailable());
            }
            let record = Record {
                version: 1,
                binding: open,
                native_owner_id: pin.native_owner_id,
                runtime_manifest_sha256: host.manifest_sha256,
                runtime_source_commit: host.source_commit,
                runtime_producer_sha256: self.payload.producer_sha256().to_owned(),
                database_system_id: owner.system_id.clone(),
                database_oid: owner.database_oid.clone(),
            };
            let attempt = record
                .persist(lease)
                .inspect_err(|_| observe("open-intent-persist-failed"))?;
            let accepted = record.accepted()?;
            observe("open-accept-send-start");
            channel
                .send_frame(&accepted, deadline, &cancellation, &mut || {
                    owner.check_observed(self, observe)
                })
                .inspect_err(|_| observe("open-accept-send-failed"))?;
            observe("open-accept-send-done");
            Ok((attempt, owner, record))
        })();
        match result {
            Ok((attempt, owner, record)) => Ok(OwnedHayesOpen {
                session: self,
                owner,
                channel,
                attempt: Some(attempt),
                record,
                source: None,
                revalidation_challenge: None,
                worker: None,
                validated: None,
                ready_pins: None,
                db_stage_id: None,
                disposition: None,
                retain_attempt: false,
                open_accepted_at: Instant::now(),
                timings: WorkerTimings::default(),
                phase: Phase::Opened,
            }),
            Err(error) => {
                channel.invalidate();
                Err(error)
            }
        }
    }
}

impl OwnedHayesOpen<'_> {
    fn start(&mut self, expected: Phase) -> Result<()> {
        if self.phase != expected {
            self.channel.invalidate();
            self.phase = Phase::Failed;
            return Err(unavailable());
        }
        self.phase = Phase::Failed;
        Ok(())
    }

    pub(in super::super) fn receive_source(&mut self, deadline: Instant) -> Result<()> {
        self.start(Phase::Opened)?;
        let result = (|| {
            let attempt = self.attempt.as_mut().ok_or_else(unavailable)?;
            check_record(attempt, &self.record)?;
            let cancel = self.session.cancellation.clone();
            attempt.receive_source(
                &mut self.channel,
                self.record.binding.correlation_id,
                deadline,
                &cancel,
                &mut || self.owner.check(self.session),
            )
        })();
        match result {
            Ok(source) => {
                self.source = Some(source);
                self.timings.source_held = self.open_accepted_at.elapsed().as_millis() as u64;
                self.phase = Phase::SourceHeld;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    /// Fresh same-binding API transaction attestation. A successful result
    /// records revalidation. Both worker GO boundaries recheck the claim.
    pub(in super::super) fn revalidate(&mut self) -> Result<()> {
        self.start(Phase::SourceHeld)?;
        let result = (|| {
            let attempt = self.attempt.as_ref().ok_or_else(unavailable)?;
            check_record(attempt, &self.record)?;
            let source = self.source.as_ref().ok_or_else(unavailable)?;
            attempt.check_stage_file(source.binding.attempt, &source.observed)?;
            if self.session.payload.context().manifest_sha256 != self.record.runtime_manifest_sha256
            {
                return Err(unavailable());
            }
            self.session.payload.recheck()?;
            self.owner.recheck_database_identity(self.session)?;
            let mut frame = self.record.challenge();
            let cancel = self.session.cancellation.clone();
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut check = || self.owner.check(self.session);
            self.channel
                .send_frame(&encode(&frame)?, deadline, &cancel, &mut check)?;
            let response = self.channel.read_frame(deadline, &cancel, &mut check)?;
            check_revalidated(&response, &mut frame)?;
            self.owner.recheck_database_identity(self.session)?;
            check_record(attempt, &self.record)?;
            attempt.check_stage_file(source.binding.attempt, &source.observed)?;
            attempt.journal.create(REVALIDATED_RECORD, &frame)?;
            Ok(frame.challenge)
        })();
        match result {
            Ok(challenge) => {
                self.revalidation_challenge = Some(challenge);
                self.timings.revalidated = self.open_accepted_at.elapsed().as_millis() as u64;
                self.phase = Phase::Revalidated;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    /// Drive only the fixed private worker pair after authenticated source and
    /// revalidation. Successful observation leaves the output held locally.
    pub(in super::super) fn observe_revalidated_workers(
        &mut self,
        worker_payload: &Payload,
        deadline: Instant,
    ) -> Result<()> {
        self.start(Phase::Revalidated)?;
        self.retain_attempt = true;
        let result = self.run_revalidated_workers(worker_payload, deadline);
        match result {
            Ok(()) => {
                self.phase = Phase::OutputHeld;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    fn run_revalidated_workers(
        &mut self,
        worker_payload: &Payload,
        deadline: Instant,
    ) -> Result<()> {
        let attempt = self.attempt.as_mut().ok_or_else(unavailable)?;
        let challenge = self.revalidation_challenge.ok_or_else(unavailable)?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(300));
        let cancellation = self.session.cancellation.clone();
        let owner = &self.owner;
        let channel = &self.channel;
        let record = &self.record;
        let session = &mut *self.session;
        let elapsed = || self.open_accepted_at.elapsed().as_millis() as u64;
        if session.payload.context().manifest_sha256 != record.runtime_manifest_sha256 {
            return Err(unavailable());
        }
        session.payload.recheck()?;
        let mut check = |checkpoint: WorkerCheckpoint<'_>, deadline: Instant| {
            if Instant::now() >= deadline {
                return Err(err("HAYES_WORKER_TIMEOUT"));
            }
            match checkpoint {
                WorkerCheckpoint::Observe => owner.check(session)?,
                WorkerCheckpoint::BeforeGo(attempt) => {
                    channel.check_hayes_idle()?;
                    check_revalidation_record(attempt, record, challenge)?;
                    check_live_claim(owner, session, &record.binding, deadline)?;
                }
            }
            if Instant::now() >= deadline {
                return Err(err("HAYES_WORKER_TIMEOUT"));
            }
            channel.check_hayes_idle()
        };
        check(WorkerCheckpoint::BeforeGo(attempt), deadline)?;
        let source = self.source.take().ok_or_else(unavailable)?;
        let pin = worker_payload.hayes_read_only_generate()?;
        check(WorkerCheckpoint::Observe, deadline)?;
        self.worker = Some(RunningWorker::Generate(
            attempt
                .prepare_generation(source)?
                .spawn_fixed_generate(pin)?,
        ));
        let Some(RunningWorker::Generate(run)) = self.worker.as_mut() else {
            return Err(unavailable());
        };
        let generate_deadline = deadline.min(Instant::now() + Duration::from_secs(16));
        let generated = attempt.observe_generation(
            run,
            generate_deadline,
            &cancellation,
            &mut |checkpoint| {
                check(checkpoint, generate_deadline)?;
                if matches!(checkpoint, WorkerCheckpoint::BeforeGo(_)) {
                    self.timings.generate_before_go = elapsed();
                }
                Ok(())
            },
        )?;
        if !run
            .child
            .try_wait()
            .map_err(|_| unavailable())?
            .is_some_and(|status| status.success())
        {
            return Err(unavailable());
        }
        self.worker = None;
        self.timings.generate_observed = elapsed();
        let body = generated.stage_selected_body(attempt, &cancellation, &mut || {
            check(WorkerCheckpoint::Observe, deadline)
        })?;
        self.worker = Some(RunningWorker::Validate(attempt.spawn_retained_validation(
            worker_payload,
            body,
            &cancellation,
            &mut || check(WorkerCheckpoint::Observe, deadline),
        )?));
        let Some(RunningWorker::Validate(run)) = self.worker.as_mut() else {
            return Err(unavailable());
        };
        let validate_deadline = deadline.min(Instant::now() + Duration::from_secs(16));
        self.validated = Some(attempt.observe_retained_validation(
            run,
            validate_deadline,
            &cancellation,
            &mut |checkpoint| {
                check(checkpoint, validate_deadline)?;
                if matches!(checkpoint, WorkerCheckpoint::BeforeGo(_)) {
                    self.timings.validate_before_go = elapsed();
                }
                Ok(())
            },
        )?);
        self.worker = None;
        self.timings.validate_observed = elapsed();
        check(WorkerCheckpoint::BeforeGo(attempt), deadline)?;
        self.timings.checked = elapsed();
        let validated = self.validated.as_ref().ok_or_else(unavailable)?;
        validated.check_leaves(attempt)?;
        let worker = worker_payload.context();
        attempt.journal.create(
            WORKERS_HELD_RECORD,
            &HeldWorkers {
                version: 1,
                binding: &record.binding,
                native_owner_id: record.native_owner_id,
                runtime_manifest_sha256: &record.runtime_manifest_sha256,
                worker_manifest_sha256: &worker.manifest_sha256,
                revalidation_challenge: challenge,
                output_stage_id: validated.selected.id,
                output_bytes: validated.selected.observed.bytes,
                output_sha256: &validated.selected.observed.sha256,
                milliseconds_since_open_accepted: std::mem::take(&mut self.timings),
            },
        )?;
        check(WorkerCheckpoint::Observe, deadline)
    }
}

impl Drop for OwnedHayesOpen<'_> {
    fn drop(&mut self) {
        self.channel.invalidate();
        if self.retain_attempt {
            if let Some(attempt) = self.attempt.take() {
                let child = self.worker.take().map(|worker| {
                    let mut child = worker.into_child();
                    child.owner.take();
                    child
                });
                self.session.retained_hayes = Some(RetainedHayes {
                    _attempt: attempt,
                    _source: self.source.take(),
                    _validated: self.validated.take(),
                    child,
                });
            }
        }
    }
}

fn check_revalidation_record(attempt: &Attempt, record: &Record, challenge: Uuid) -> Result<()> {
    check_record(attempt, record)?;
    let expected = Revalidation {
        version: 1,
        kind: Kind::HayesOperationRevalidated,
        binding: &record.binding,
        challenge,
        runtime_manifest_sha256: &record.runtime_manifest_sha256,
    };
    if attempt.journal.read_bytes(REVALIDATED_RECORD)? != encode(&expected)? {
        return Err(unavailable());
    }
    Ok(())
}

fn check_live_claim(
    owner: &Owner,
    session: &mut Session,
    b: &OperationBinding,
    deadline: Instant,
) -> Result<()> {
    check_live_claim_stage(owner, session, b, None, deadline)
}

fn check_live_claim_stage(
    owner: &Owner,
    session: &mut Session,
    b: &OperationBinding,
    db_stage_id: Option<Uuid>,
    deadline: Instant,
) -> Result<()> {
    owner.check(session)?;
    let sql = live_claim_sql(b, db_stage_id);
    let identity = session.query_database_before("exhibitos", &sql, deadline)?;
    owner.check_database_identity_result(&identity)?;
    owner.check(session)
}

fn live_claim_sql(b: &OperationBinding, db_stage_id: Option<Uuid>) -> String {
    // All interpolated values are typed UUIDs, bounded integers, or the
    // exact lowercase source digest admitted by parse_open. The API already
    // validated the immutable job rights schema and bound it at revalidation.
    // Here equality and clock_timestamp recheck current rights and expiry.
    let reservation = db_stage_id.map_or_else(String::new, |id| {
        format!(
            " AND EXISTS (SELECT 1 FROM hayes_output_stages s
               WHERE (s.tenant_id,s.attempt_id,s.job_id)=(a.tenant_id,a.id,a.job_id)
                 AND s.id='{id}' AND s.output_sha256='{BODY_SHA}' AND s.output_bytes={BODY_BYTES}
                 AND s.object_key=a.tenant_id::text || '/hayes-output/' || a.id::text || '/{BODY_SHA}')"
        )
    });
    format!(
        "SELECT db.system_id,db.database_oid FROM ({DATABASE_IDENTITY_SQL}) db WHERE EXISTS (
          SELECT 1 FROM hayes_original_attempts a
          JOIN import_jobs j ON (j.tenant_id,j.id,j.hayes_attempt_id)=(a.tenant_id,a.job_id,a.id)
          JOIN artworks w ON (w.tenant_id,w.id)=(a.tenant_id,a.artwork_id)
          WHERE (a.tenant_id,a.workspace_id,a.principal_id)=('{tenant}','{workspace}','{principal}')
            AND (a.job_id,a.lease_id,a.id)=('{job}','{lease}','{attempt}')
            AND (a.runtime_generation,a.artwork_id,a.correlation_id)=('{generation}','{artwork}','{correlation}')
            AND (a.artwork_revision,a.source_bytes,a.source_sha256)=({revision},{bytes},'{sha}')
            AND (j.user_id,j.artwork_id,j.lease_id)=(a.principal_id,a.artwork_id,a.lease_id)
            AND j.state='processing' AND j.lease_until>clock_timestamp()
            AND j.expected_hash=a.source_sha256 AND j.expected_bytes=a.source_bytes
            AND j.mime='model/gltf-binary'
            AND w.revision=a.artwork_revision AND w.deleted_at IS NULL
            AND w.metadata #>> '{{displayReference,status}}' = 'provisional-uncalibrated'
            AND w.metadata #>> '{{displayReference,assetSha256}}' = '{BODY_SHA}'
            AND w.metadata #>> '{{provenance,sourceUnits}}' = 'unknown'
            AND w.metadata #> '{{provenance,scaleApplied}}' = 'false'::jsonb
            AND w.metadata->'rights' = j.rights
            AND j.rights #> '{{permissions,display}}' = 'true'::jsonb
            AND j.rights #> '{{permissions,export}}' = 'true'::jsonb
            AND CASE WHEN NOT j.rights ? 'validFrom' THEN true
                     WHEN jsonb_typeof(j.rights->'validFrom')='string'
                     THEN (j.rights->>'validFrom')::timestamptz <= clock_timestamp()
                     ELSE false END
            AND CASE WHEN NOT j.rights ? 'expiresAt' THEN true
                     WHEN jsonb_typeof(j.rights->'expiresAt')='string'
                     THEN (j.rights->>'expiresAt')::timestamptz > clock_timestamp()
                     ELSE false END{reservation})",
        tenant=b.tenant_id, workspace=b.workspace_id, principal=b.principal_id,
        job=b.job_id, lease=b.lease_id, attempt=b.attempt_id, generation=b.runtime_generation,
        artwork=b.artwork_id, correlation=b.correlation_id, revision=b.artwork_revision,
        bytes=b.source_bytes, sha=b.source_sha256,
    )
}

impl Record {
    fn persist(&self, lease: Lease) -> Result<Attempt> {
        let attempt = lease.begin(Binding {
            attempt: self.binding.attempt_id,
            generation: self.binding.runtime_generation,
            owner: self.native_owner_id,
            job: self.binding.job_id,
        })?;
        attempt.journal.create(OPEN_RECORD, self)?;
        check_record(&attempt, self)?;
        Ok(attempt)
    }

    fn accepted(&self) -> Result<Vec<u8>> {
        encode(&Accepted {
            version: 1,
            kind: Kind::HayesOperationOpenAccepted,
            binding: &self.binding,
            native_owner_id: self.native_owner_id,
            runtime_manifest_sha256: &self.runtime_manifest_sha256,
        })
    }

    fn challenge(&self) -> Revalidation<'_> {
        Revalidation {
            version: 1,
            kind: Kind::HayesOperationRevalidate,
            binding: &self.binding,
            challenge: Uuid::new_v4(),
            runtime_manifest_sha256: &self.runtime_manifest_sha256,
        }
    }
}

fn check_record(attempt: &Attempt, record: &Record) -> Result<()> {
    attempt.check()?;
    if attempt.journal.read::<Record>(OPEN_RECORD)? != *record {
        return Err(unavailable());
    }
    Ok(())
}

impl OperationBinding {
    fn check_session(
        &self,
        workspace: &str,
        tenant: &str,
        principal: &str,
        generation: &str,
    ) -> Result<()> {
        if self.workspace_id.to_string() != workspace
            || self.tenant_id.to_string() != tenant
            || self.principal_id.to_string() != principal
            || self.runtime_generation.to_string() != generation
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

fn parse_open(raw: &[u8]) -> Result<OperationBinding> {
    let open: Open = canonical(raw)?;
    if open.version != 1 || open.kind != Kind::HayesOperationOpen || open.binding.version != 1 {
        return Err(unavailable());
    }
    let b = open.binding;
    let ids = [
        b.tenant_id,
        b.workspace_id,
        b.principal_id,
        b.job_id,
        b.lease_id,
        b.attempt_id,
        b.runtime_generation,
        b.artwork_id,
        b.correlation_id,
    ];
    let flow = Transfer::source(b.attempt_id, b.correlation_id)?;
    if ids.iter().any(|id| !canonical_uuid(id))
        || !(1..=MAX_SAFE_INTEGER).contains(&b.artwork_revision)
        || b.source_bytes != flow.bytes()
        || b.source_sha256 != flow.sha256()
    {
        return Err(unavailable());
    }
    Ok(b)
}

fn check_revalidated(raw: &[u8], expected: &mut Revalidation<'_>) -> Result<()> {
    if let Ok(abort) = canonical::<Open>(raw) {
        if abort.version == 1
            && abort.kind == Kind::HayesOperationAbort
            && &abort.binding == expected.binding
        {
            return Err(err("HAYES_OPERATION_ABORTED"));
        }
    }
    expected.kind = Kind::HayesOperationRevalidated;
    if raw != encode(expected)? {
        return Err(unavailable());
    }
    Ok(())
}

fn canonical_uuid(id: &Uuid) -> bool {
    id.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num())
}

fn canonical<T: DeserializeOwned + Serialize>(raw: &[u8]) -> Result<T> {
    if !(1..=CONTROL_LIMIT).contains(&raw.len()) {
        return Err(unavailable());
    }
    let value = serde_json::from_slice(raw).map_err(|_| unavailable())?;
    if encode(&value)? != raw {
        return Err(unavailable());
    }
    Ok(value)
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| unavailable())?;
    if !(1..=CONTROL_LIMIT).contains(&bytes.len()) {
        return Err(unavailable());
    }
    Ok(bytes)
}

fn unavailable() -> crate::LifecycleError {
    err("HAYES_OPERATION_UNAVAILABLE")
}

#[path = "native_runtime_hayes_ready.rs"]
mod ready;
pub(in crate::native_runtime) use ready::disposition::VerifiedRelease;

#[cfg(test)]
#[path = "native_runtime_hayes_operation_tests.rs"]
mod tests;
