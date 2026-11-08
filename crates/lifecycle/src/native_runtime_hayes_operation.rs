// SPDX-License-Identifier: Apache-2.0
//! Inactive authenticated operation admission. The API owns the DB lease
//! and rights transaction. Neither a binding nor a fresh reply proves drain.

use super::{err, transfer::Transfer, Attempt, Binding, Lease, Result, SourceStage};
use crate::{
    hash_valid,
    native_runtime::{oex_owner::Owner, owner_channel::Connection, Session},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

const OPEN_RECORD: &str = "operation-open.json";
const REVALIDATED_RECORD: &str = "operation-revalidated.json";
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

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Opened,
    SourceHeld,
    Revalidated,
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

pub(in super::super) struct OwnedHayesOpen<'a> {
    session: &'a mut Session,
    owner: Owner,
    channel: Connection,
    attempt: Attempt,
    record: Record,
    source: Option<SourceStage>,
    phase: Phase,
}

impl Session {
    /// The private selected-runtime entry supplies the acquired UID authority.
    /// Only this Session's dedicated inherited channel can supply the pin.
    pub(in super::super) fn begin_owned_hayes_open<'a>(
        &'a mut self,
        lease: Lease,
    ) -> Result<OwnedHayesOpen<'a>> {
        let AuthenticatedHayes { pin, mut channel } = self.hayes_bootstrap.consume()?;
        let result = (|| {
            channel.claim_hayes()?;
            let host = self.payload.context();
            if host.manifest_sha256 != pin.host_manifest_sha256
                || self.job_id.as_deref() != Some(pin.runtime_generation.to_string().as_str())
            {
                return Err(unavailable());
            }
            self.payload.recheck()?;
            let owner = Owner::capture(self)?;
            let cancellation = self.cancellation.clone();
            let deadline = Instant::now() + Duration::from_secs(30);
            let raw = channel.read_frame(deadline, &cancellation, &mut || owner.check(self))?;
            let open = parse_open(&raw)?;
            let auth = self.local_authority.as_ref().ok_or_else(unavailable)?;
            open.check_session(
                &auth.workspace_id,
                &auth.tenant_id,
                &auth.principal_id,
                self.job_id.as_deref().ok_or_else(unavailable)?,
            )?;
            owner.recheck_database_identity(self)?;
            if open.lease_id == pin.native_owner_id {
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
            let attempt = record.persist(lease)?;
            let accepted = record.accepted()?;
            channel.send_frame(&accepted, deadline, &cancellation, &mut || {
                owner.check(self)
            })?;
            Ok((attempt, owner, record))
        })();
        match result {
            Ok((attempt, owner, record)) => Ok(OwnedHayesOpen {
                session: self,
                owner,
                channel,
                attempt,
                record,
                source: None,
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
            check_record(&self.attempt, &self.record)?;
            let cancel = self.session.cancellation.clone();
            self.attempt.receive_source(
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
    /// records revalidation only. There is no worker GO consumer here.
    pub(in super::super) fn revalidate(&mut self) -> Result<()> {
        self.start(Phase::SourceHeld)?;
        let result = (|| {
            check_record(&self.attempt, &self.record)?;
            let source = self.source.as_ref().ok_or_else(unavailable)?;
            self.attempt
                .check_stage_file(source.binding.attempt, &source.observed)?;
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
            check_record(&self.attempt, &self.record)?;
            self.attempt
                .check_stage_file(source.binding.attempt, &source.observed)?;
            self.attempt.journal.create(REVALIDATED_RECORD, &frame)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.phase = Phase::Revalidated;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }
}

impl Drop for OwnedHayesOpen<'_> {
    fn drop(&mut self) {
        self.channel.invalidate();
    }
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

#[cfg(test)]
#[path = "native_runtime_hayes_operation_tests.rs"]
mod tests;
