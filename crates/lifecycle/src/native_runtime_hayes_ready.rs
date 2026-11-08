// SPDX-License-Identifier: Apache-2.0
//! Private readiness for the fixed Darwin worker pair in this live exchange.
//! The sealed inert entry reaches READY before the watch arms, and all work
//! follows GO. The proof covers only those two exact leaves and this custody.

use super::{
    canonical, canonical_uuid, check_live_claim, check_live_claim_stage, check_revalidation_record,
    encode, unavailable, Kind, OperationBinding, OwnedHayesOpen, Phase, Result, BODY_BYTES,
    BODY_SHA,
};
use crate::native_runtime::hayes::{
    generation::validation::ReadyStage, watch::LeafEvidence, Attempt,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

#[cfg(test)]
#[path = "native_runtime_hayes_reservation_tests.rs"]
mod reservation_tests;

const CONTAINMENT_KIND: &str = "darwin-fixed-hayes-leaves-v1";
const CONTAINMENT_RECORD: &str = "operation-leaf-containment.json";
const EVIDENCE_RECORD: &str = "operation-ready-evidence.json";
const RESERVATION_RECORD: &str = "operation-output-reservation.json";

pub(super) struct ReadyPins {
    evidence_sha256: String,
    containment_sha256: String,
}

impl ReadyPins {
    fn check(&self, attempt: &Attempt) -> Result<()> {
        if crate::digest(&attempt.journal.read_bytes(EVIDENCE_RECORD)?) != self.evidence_sha256
            || crate::digest(&attempt.journal.read_bytes(CONTAINMENT_RECORD)?)
                != self.containment_sha256
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LockIdentity {
    device: String,
    inode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Database<'a> {
    system_identifier: &'a str,
    database_oid: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContainmentRecord<'a> {
    version: u8,
    kind: &'static str,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    revalidation_challenge: Uuid,
    workers: &'a [LeafEvidence; 2],
    lock: &'a LockIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Containment<'a> {
    kind: &'static str,
    receipt_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Attestation<'a> {
    source: &'a ReadyStage,
    output: &'a ReadyStage,
    workers: &'a [LeafEvidence; 2],
    lock: &'a LockIdentity,
    containment: Containment<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Evidence<'a> {
    version: u8,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    revalidation_challenge: Uuid,
    attestation: &'a Attestation<'a>,
    database: &'a Database<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ready<'a> {
    version: u8,
    kind: &'static str,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    native_evidence_sha256: &'a str,
    attestation: &'a Attestation<'a>,
    database: &'a Database<'a>,
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReservedOutput {
    stage_id: Uuid,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reservation {
    version: u8,
    kind: Kind,
    binding: OperationBinding,
    native_evidence_sha256: String,
    db_stage_id: Uuid,
    output: ReservedOutput,
}

fn check_reservation(
    raw: &[u8],
    binding: &OperationBinding,
    evidence_sha256: &str,
    output: &ReservedOutput,
) -> Result<Reservation> {
    let frame: Reservation = canonical(raw)?;
    if frame.version != 1
        || frame.kind != Kind::HayesOutputReservationReady
        || &frame.binding != binding
        || frame.native_evidence_sha256 != evidence_sha256
        || !canonical_uuid(&frame.db_stage_id)
        || frame.db_stage_id == binding.attempt_id
        || frame.db_stage_id == output.stage_id
        || &frame.output != output
    {
        return Err(unavailable());
    }
    Ok(frame)
}

impl OwnedHayesOpen<'_> {
    /// Emits once on this operation's private channel, while retaining the
    /// attempt, original open lock description and all pinned stage handles.
    /// An authorized caller must still require the API's durable reservation
    /// before serving any output.
    pub(in crate::native_runtime) fn emit_output_ready(&mut self, deadline: Instant) -> Result<()> {
        self.start(Phase::OutputHeld)?;
        match self.send_output_ready(deadline) {
            Ok(pins) => {
                self.ready_pins = Some(pins);
                self.phase = Phase::ReadySent;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    fn send_output_ready(&mut self, deadline: Instant) -> Result<ReadyPins> {
        if self.worker.is_some() || !self.retain_attempt || Instant::now() >= deadline {
            return Err(unavailable());
        }
        self.channel.check_hayes_idle()?;
        let attempt = self.attempt.as_ref().ok_or_else(unavailable)?;
        let challenge = self.revalidation_challenge.ok_or_else(unavailable)?;
        let record = &self.record;
        let validated = self.validated.as_ref().ok_or_else(unavailable)?;
        if self.session.payload.context().manifest_sha256 != record.runtime_manifest_sha256 {
            return Err(unavailable());
        }
        self.session.payload.recheck()?;
        check_revalidation_record(attempt, record, challenge)?;
        let material = validated.ready_material(attempt)?;
        let database = Database {
            system_identifier: &record.database_system_id,
            database_oid: &record.database_oid,
        };
        let lock = LockIdentity {
            device: attempt.intent.authority.lock.dev.to_string(),
            inode: attempt.intent.authority.lock.ino.to_string(),
        };
        let containment_record = ContainmentRecord {
            version: 1,
            kind: CONTAINMENT_KIND,
            binding: &record.binding,
            native_owner_id: record.native_owner_id,
            runtime_manifest_sha256: &record.runtime_manifest_sha256,
            worker_manifest_sha256: material.worker_manifest_sha256,
            revalidation_challenge: challenge,
            workers: &material.workers,
            lock: &lock,
        };
        attempt
            .journal
            .create(CONTAINMENT_RECORD, &containment_record)?;
        let containment_sha = attempt
            .journal
            .confirmed_digest(CONTAINMENT_RECORD, &containment_record)?;
        let attestation = Attestation {
            source: &material.source,
            output: &material.output,
            workers: &material.workers,
            lock: &lock,
            containment: Containment {
                kind: CONTAINMENT_KIND,
                receipt_sha256: &containment_sha,
            },
        };
        let evidence = Evidence {
            version: 1,
            binding: &record.binding,
            native_owner_id: record.native_owner_id,
            runtime_manifest_sha256: &record.runtime_manifest_sha256,
            worker_manifest_sha256: material.worker_manifest_sha256,
            revalidation_challenge: challenge,
            attestation: &attestation,
            database: &database,
        };
        attempt.journal.create(EVIDENCE_RECORD, &evidence)?;
        let native_evidence_sha256 = attempt
            .journal
            .confirmed_digest(EVIDENCE_RECORD, &evidence)?;
        let ready = encode(&Ready {
            version: 1,
            kind: "hayes-output-ready",
            binding: &record.binding,
            native_owner_id: record.native_owner_id,
            runtime_manifest_sha256: &record.runtime_manifest_sha256,
            worker_manifest_sha256: material.worker_manifest_sha256,
            native_evidence_sha256: &native_evidence_sha256,
            attestation: &attestation,
            database: &database,
        })?;
        check_live_claim(&self.owner, self.session, &record.binding, deadline)?;
        check_revalidation_record(attempt, record, challenge)?;
        self.channel.check_hayes_idle()?;
        let cancellation = self.session.cancellation.clone();
        self.channel
            .send_frame(&ready, deadline, &cancellation, &mut || {
                self.owner.check(self.session)?;
                attempt.check()?;
                validated.check_leaves(attempt)
            })?;
        Ok(ReadyPins {
            evidence_sha256: native_evidence_sha256,
            containment_sha256: containment_sha,
        })
    }

    /// Accept one post-commit reservation on the channel that sent READY.
    /// The response is a selector for the Manager's own DB read, not authority
    /// to serve bytes by itself.
    pub(in crate::native_runtime) fn receive_output_reservation(
        &mut self,
        deadline: Instant,
    ) -> Result<()> {
        self.start(Phase::ReadySent)?;
        match self.read_output_reservation(deadline) {
            Ok(db_stage_id) => {
                self.db_stage_id = Some(db_stage_id);
                self.phase = Phase::Reserved;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    fn read_output_reservation(&mut self, deadline: Instant) -> Result<Uuid> {
        let attempt = self.attempt.as_ref().ok_or_else(unavailable)?;
        let pins = self.ready_pins.as_ref().ok_or_else(unavailable)?;
        let validated = self.validated.as_ref().ok_or_else(unavailable)?;
        let challenge = self.revalidation_challenge.ok_or_else(unavailable)?;
        let record = &self.record;
        let selected = &validated.selected;
        if selected.observed.bytes != u64::from(BODY_BYTES) || selected.observed.sha256 != BODY_SHA
        {
            return Err(unavailable());
        }
        let expected = ReservedOutput {
            stage_id: selected.id,
            bytes: selected.observed.bytes,
            sha256: selected.observed.sha256.clone(),
        };
        check_revalidation_record(attempt, record, challenge)?;
        pins.check(attempt)?;
        let cancellation = self.session.cancellation.clone();
        let raw = self.channel.read_frame(deadline, &cancellation, &mut || {
            self.owner.check(self.session)?;
            attempt.check_stage_file(selected.id, &selected.observed)
        })?;
        let reservation =
            check_reservation(&raw, &record.binding, &pins.evidence_sha256, &expected)?;
        check_live_claim_stage(
            &self.owner,
            self.session,
            &record.binding,
            Some(reservation.db_stage_id),
            deadline,
        )?;
        check_revalidation_record(attempt, record, challenge)?;
        pins.check(attempt)?;
        validated.check_leaves(attempt)?;
        self.channel.check_hayes_idle()?;
        attempt.journal.create(RESERVATION_RECORD, &reservation)?;
        self.owner.check(self.session)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        Ok(reservation.db_stage_id)
    }
}

#[cfg(test)]
#[test]
#[ignore = "emits a synthetic vector for the Platform codec"]
fn api_ready_codec_fixture() {
    use super::super::{
        frame::{BODY_BYTES, BODY_SHA},
        Role,
    };
    use super::tests::{binding, id, record};
    let record = record(binding());
    let source = ReadyStage::fixture(
        record.binding.attempt_id,
        8,
        record.binding.source_bytes,
        &record.binding.source_sha256,
    );
    let output = ReadyStage::fixture(id(11), 9, u64::from(BODY_BYTES), BODY_SHA);
    let workers = [
        LeafEvidence::fixture(Role::Generate, id(12)),
        LeafEvidence::fixture(Role::Validate, id(13)),
    ];
    let lock = LockIdentity {
        device: "7".into(),
        inode: "10".into(),
    };
    let attestation = Attestation {
        source: &source,
        output: &output,
        workers: &workers,
        lock: &lock,
        containment: Containment {
            kind: CONTAINMENT_KIND,
            receipt_sha256: &"d".repeat(64),
        },
    };
    let database = Database {
        system_identifier: &record.database_system_id,
        database_oid: &record.database_oid,
    };
    let frame = encode(&Ready {
        version: 1,
        kind: "hayes-output-ready",
        binding: &record.binding,
        native_owner_id: record.native_owner_id,
        runtime_manifest_sha256: &record.runtime_manifest_sha256,
        worker_manifest_sha256: "94a6551d16af3fb134f94549d9f087ee8ccb9fd2372d914de5f738a78d3505c6",
        native_evidence_sha256: &"e".repeat(64),
        attestation: &attestation,
        database: &database,
    })
    .unwrap();
    println!(
        "HAYES_READY_VECTOR {}",
        std::str::from_utf8(&frame).unwrap()
    );
}
