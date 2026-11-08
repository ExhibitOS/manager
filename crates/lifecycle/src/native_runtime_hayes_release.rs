// SPDX-License-Identifier: Apache-2.0
//! Authenticated notification of actual native custody return.

use super::super::{
    check_reservation, check_revalidation_record, encode, unavailable, Database, OperationBinding,
    OwnedHayesOpen, Phase, ReservedOutput, Result, RESERVATION_RECORD,
};
use super::{verify_committed_receipt, Notice, OBSERVED_RECORD};
use crate::native_runtime::hayes::{
    stage_return::{ClosedCustody, StageIdentity},
    Attempt,
};
use serde::Serialize;
use std::time::Instant;
use uuid::Uuid;

#[cfg(test)]
#[path = "native_runtime_hayes_release_tests.rs"]
mod tests;

const INTENT_RECORD: &str = "operation-release-intent.json";
const COMPLETION_RECORD: &str = "operation-release-completed.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseScope<'a> {
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    native_evidence_sha256: &'a str,
    ready_frame_sha256: &'a str,
    db_stage_id: Uuid,
    database: Database<'a>,
    challenge: Uuid,
    receipt_id: Uuid,
    receipt_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Intent<'a> {
    version: u8,
    scope: &'a ReleaseScope<'a>,
    stages: &'a [StageIdentity; 4],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Completion<'a> {
    version: u8,
    scope: &'a ReleaseScope<'a>,
    intent_sha256: &'a str,
    stages: &'a [StageIdentity; 4],
    active_reservation_retired: bool,
    original_custody_lock_closed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletedFrame<'a> {
    version: u8,
    kind: &'static str,
    #[serde(flatten)]
    scope: &'a ReleaseScope<'a>,
    intent_sha256: &'a str,
    completion_sha256: &'a str,
}

impl OwnedHayesOpen<'_> {
    /// Only the current exchange can return its native custody. The local
    /// completion is durable before sending; socket success alone cannot
    /// establish the API's separate durable completion.
    pub(in crate::native_runtime) fn return_committed_custody(
        &mut self,
        deadline: Instant,
    ) -> Result<()> {
        self.start(Phase::DispositionObserved)?;
        let result = self.release_and_notify(deadline);
        if result.is_ok() {
            self.phase = Phase::ReleaseNotified;
        } else {
            self.channel.invalidate();
        }
        result
    }

    fn release_and_notify(&mut self, deadline: Instant) -> Result<()> {
        if !self.retain_attempt || self.worker.is_some() || self.source.is_some() {
            return Err(unavailable());
        }
        let attempt = self.attempt.as_ref().ok_or_else(unavailable)?;
        let pins = self.ready_pins.as_ref().ok_or_else(unavailable)?;
        let notice = self.disposition.as_ref().ok_or_else(unavailable)?;
        let record = &self.record;
        let revalidation = self.revalidation_challenge.ok_or_else(unavailable)?;
        let validated = self.validated.as_ref().ok_or_else(unavailable)?;
        let expected = ReservedOutput::selected(validated)?;
        let reservation = attempt.journal.read_bytes(RESERVATION_RECORD)?;
        if &notice.binding != &record.binding
            || notice.native_owner_id != record.native_owner_id
            || notice.runtime_manifest_sha256 != record.runtime_manifest_sha256
            || notice.ready_frame_sha256 != pins.frame_sha256
            || self.db_stage_id != Some(notice.db_stage_id)
            || check_reservation(
                &reservation,
                &record.binding,
                &pins.evidence_sha256,
                &expected,
            )?
            .db_stage_id
                != notice.db_stage_id
        {
            return Err(unavailable());
        }
        check_revalidation_record(attempt, record, revalidation)?;
        pins.check(attempt)?;
        validated.check_leaves(attempt)?;
        attempt.journal.confirmed_digest(OBSERVED_RECORD, notice)?;
        self.channel.check_hayes_idle()?;
        let deadline =
            verify_committed_receipt(&self.owner, self.session, notice, pins, deadline, true)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }

        let scope = ReleaseScope {
            binding: &record.binding,
            native_owner_id: record.native_owner_id,
            runtime_manifest_sha256: &record.runtime_manifest_sha256,
            worker_manifest_sha256: pins.worker_manifest_sha256,
            native_evidence_sha256: &pins.evidence_sha256,
            ready_frame_sha256: &pins.frame_sha256,
            db_stage_id: notice.db_stage_id,
            database: Database {
                system_identifier: &record.database_system_id,
                database_oid: &record.database_oid,
            },
            challenge: notice.challenge,
            receipt_id: notice.receipt_id,
            receipt_sha256: &notice.receipt_sha256,
        };
        let (stages, worker_pin) = self
            .validated
            .take()
            .ok_or_else(unavailable)?
            .into_retained_stages(attempt)?;
        let inventory = stages.inventory(attempt)?;
        let intent = Intent {
            version: 1,
            scope: &scope,
            stages: &inventory,
        };
        attempt.journal.create(INTENT_RECORD, &intent)?;
        let intent_sha256 = attempt.journal.confirmed_digest(INTENT_RECORD, &intent)?;
        let mut check = |attempt: &Attempt| {
            if Instant::now() >= deadline {
                return Err(unavailable());
            }
            self.owner.check(self.session)?;
            self.channel.check_hayes_idle()?;
            worker_pin.check()?;
            if attempt.journal.confirmed_digest(INTENT_RECORD, &intent)? != intent_sha256
                || Instant::now() >= deadline
            {
                return Err(unavailable());
            }
            Ok(())
        };
        let stages_returned = stages.return_owned(attempt, &mut check)?;
        let closed = stages_returned
            .close_custody(self.attempt.take().ok_or_else(unavailable)?, &mut check)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        self.owner.check(self.session)?;
        self.channel.check_hayes_idle()?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let completion_sha256 = persist_completion(&closed, &intent, &intent_sha256)?;
        let frame = encode(&CompletedFrame {
            version: 1,
            kind: "hayes-release-completed",
            scope: &scope,
            intent_sha256: &intent_sha256,
            completion_sha256: &completion_sha256,
        })?;
        let cancellation = self.session.cancellation.clone();
        self.channel
            .send_frame(&frame, deadline, &cancellation, &mut || {
                self.owner.check(self.session)?;
                if crate::digest(&closed.journal().read_bytes(COMPLETION_RECORD)?)
                    != completion_sha256
                {
                    return Err(unavailable());
                }
                Ok(())
            })
    }
}

fn persist_completion(
    closed: &ClosedCustody,
    intent: &Intent<'_>,
    intent_sha256: &str,
) -> Result<String> {
    if closed.inventory() != intent.stages
        || closed.journal().confirmed_digest(INTENT_RECORD, intent)? != intent_sha256
    {
        return Err(unavailable());
    }
    let completion = Completion {
        version: 1,
        scope: intent.scope,
        intent_sha256,
        stages: closed.inventory(),
        active_reservation_retired: true,
        original_custody_lock_closed: true,
    };
    closed.journal().create(COMPLETION_RECORD, &completion)?;
    closed
        .journal()
        .confirmed_digest(COMPLETION_RECORD, &completion)
}
