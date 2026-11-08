// SPDX-License-Identifier: Apache-2.0
//! The committed selector only points to a receipt in the current owned DB.

use super::{
    canonical, canonical_uuid, check_reservation, check_revalidation_record, encode, unavailable,
    Attestation, Database, Kind, OperationBinding, OwnedHayesOpen, Phase, ReadyPins,
    ReservedOutput, Result, BODY_BYTES, BODY_SHA, RESERVATION_RECORD,
};
use crate::{
    hash_valid,
    native_runtime::{
        oex_owner::{Owner, DATABASE_IDENTITY_SQL},
        Session,
    },
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[path = "native_runtime_hayes_release.rs"]
mod release;

#[cfg(test)]
#[path = "native_runtime_hayes_disposition_tests.rs"]
mod tests;

const RECEIPT_LIMIT: usize = 6144;
const OBSERVED_RECORD: &str = "operation-disposition-observed.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Challenge<'a> {
    version: u8,
    kind: Kind,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    ready_frame_sha256: &'a str,
    db_stage_id: Uuid,
    challenge: Uuid,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in super::super) struct Notice {
    version: u8,
    kind: Kind,
    binding: OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: String,
    ready_frame_sha256: String,
    db_stage_id: Uuid,
    challenge: Uuid,
    receipt_id: Uuid,
    receipt_sha256: String,
}

fn check_notice(raw: &[u8], expected: &Challenge<'_>) -> Result<Notice> {
    let notice: Notice = canonical(raw)?;
    if notice.version != 1
        || notice.kind != Kind::HayesDispositionCommitted
        || &notice.binding != expected.binding
        || notice.native_owner_id != expected.native_owner_id
        || notice.runtime_manifest_sha256 != expected.runtime_manifest_sha256
        || notice.ready_frame_sha256 != expected.ready_frame_sha256
        || notice.db_stage_id != expected.db_stage_id
        || notice.challenge != expected.challenge
        || !canonical_uuid(&notice.receipt_id)
        || !hash_valid(&notice.receipt_sha256)
    {
        return Err(unavailable());
    }
    Ok(notice)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt<'a> {
    version: u8,
    outcome: &'static str,
    receipt_id: Uuid,
    binding: &'a OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    worker_manifest_sha256: &'a str,
    native_evidence_sha256: &'a str,
    attestation: &'a Attestation,
    db_stage_id: Uuid,
    original_asset_id: Uuid,
    display_asset_id: Uuid,
    derivative_id: Uuid,
    database: Database<'a>,
}

struct Readback<'a> {
    system_id: &'a str,
    database_oid: &'a str,
    original_asset_id: Uuid,
    display_asset_id: Uuid,
    derivative_id: Uuid,
    bytes: Vec<u8>,
}

impl<'a> Readback<'a> {
    fn parse(row: &'a str) -> Result<Self> {
        let fields: Vec<_> = row.split('|').collect();
        let [system_id, database_oid, original, display, derivative, encoded] = fields.as_slice()
        else {
            return Err(unavailable());
        };
        let uuid = |value: &str| {
            Uuid::parse_str(value)
                .ok()
                .filter(|id| canonical_uuid(id) && id.to_string() == value)
                .ok_or_else(unavailable)
        };
        if encoded.len() > 4 * RECEIPT_LIMIT.div_ceil(3) || original == display {
            return Err(unavailable());
        }
        let bytes = STANDARD.decode(encoded).map_err(|_| unavailable())?;
        if !(1..=RECEIPT_LIMIT).contains(&bytes.len()) || STANDARD.encode(&bytes) != *encoded {
            return Err(unavailable());
        }
        Ok(Self {
            system_id,
            database_oid,
            original_asset_id: uuid(original)?,
            display_asset_id: uuid(display)?,
            derivative_id: uuid(derivative)?,
            bytes,
        })
    }

    fn check_receipt(&self, notice: &Notice, pins: &ReadyPins) -> Result<()> {
        let expected = Receipt {
            version: 1,
            outcome: "committed",
            receipt_id: notice.receipt_id,
            binding: &notice.binding,
            native_owner_id: notice.native_owner_id,
            runtime_manifest_sha256: &notice.runtime_manifest_sha256,
            worker_manifest_sha256: pins.worker_manifest_sha256,
            native_evidence_sha256: &pins.evidence_sha256,
            attestation: &pins.attestation,
            db_stage_id: notice.db_stage_id,
            original_asset_id: self.original_asset_id,
            display_asset_id: self.display_asset_id,
            derivative_id: self.derivative_id,
            database: Database {
                system_identifier: self.system_id,
                database_oid: self.database_oid,
            },
        };
        if self.bytes != encode(&expected)? || crate::digest(&self.bytes) != notice.receipt_sha256 {
            return Err(unavailable());
        }
        Ok(())
    }
}

impl OwnedHayesOpen<'_> {
    /// Read the exact committed state while the original live custody remains.
    pub(in crate::native_runtime) fn observe_committed_disposition(
        &mut self,
        deadline: Instant,
    ) -> Result<()> {
        self.start(Phase::OutputAcknowledged)?;
        match self.read_committed_disposition(deadline) {
            Ok(notice) => {
                self.disposition = Some(notice);
                self.phase = Phase::DispositionObserved;
                Ok(())
            }
            Err(error) => {
                self.channel.invalidate();
                Err(error)
            }
        }
    }

    fn read_committed_disposition(&mut self, deadline: Instant) -> Result<Notice> {
        let attempt = self.attempt.as_ref().ok_or_else(unavailable)?;
        let pins = self.ready_pins.as_ref().ok_or_else(unavailable)?;
        let validated = self.validated.as_ref().ok_or_else(unavailable)?;
        let revalidation = self.revalidation_challenge.ok_or_else(unavailable)?;
        let db_stage_id = self.db_stage_id.ok_or_else(unavailable)?;
        let record = &self.record;
        let expected = ReservedOutput::selected(validated)?;
        let reservation = attempt.journal.read_bytes(RESERVATION_RECORD)?;
        if check_reservation(
            &reservation,
            &record.binding,
            &pins.evidence_sha256,
            &expected,
        )?
        .db_stage_id
            != db_stage_id
        {
            return Err(unavailable());
        }
        check_revalidation_record(attempt, record, revalidation)?;
        pins.check(attempt)?;
        validated.check_leaves(attempt)?;
        self.channel.check_hayes_idle()?;

        let challenge = Challenge {
            version: 1,
            kind: Kind::HayesDispositionChallenge,
            binding: &record.binding,
            native_owner_id: record.native_owner_id,
            runtime_manifest_sha256: &record.runtime_manifest_sha256,
            ready_frame_sha256: &pins.frame_sha256,
            db_stage_id,
            challenge: Uuid::new_v4(),
        };
        let cancellation = self.session.cancellation.clone();
        let mut check = || {
            self.owner.check(self.session)?;
            attempt.check_stage_file(validated.selected.id, &validated.selected.observed)
        };
        self.channel
            .send_frame(&encode(&challenge)?, deadline, &cancellation, &mut check)?;
        let raw = self
            .channel
            .read_frame(deadline, &cancellation, &mut check)?;
        let notice = check_notice(&raw, &challenge)?;
        verify_committed_receipt(&self.owner, self.session, &notice, pins, deadline, false)?;
        check_revalidation_record(attempt, record, revalidation)?;
        pins.check(attempt)?;
        validated.check_leaves(attempt)?;
        if Instant::now() >= deadline
            || attempt.journal.read_bytes(RESERVATION_RECORD)? != reservation
        {
            return Err(unavailable());
        }
        self.channel.check_hayes_idle()?;
        attempt.journal.create(OBSERVED_RECORD, &notice)?;
        Ok(notice)
    }
}

fn verify_committed_receipt(
    owner: &Owner,
    session: &mut Session,
    notice: &Notice,
    pins: &ReadyPins,
    deadline: Instant,
    require_live_lease: bool,
) -> Result<Instant> {
    owner.check(session)?;
    let started = Instant::now();
    let value = session.query_database_before(
        "exhibitos",
        &committed_sql(notice, pins, require_live_lease),
        deadline,
    )?;
    let (row, deadline) = if require_live_lease {
        let (row, remaining) = value.rsplit_once('|').ok_or_else(unavailable)?;
        (row, live_release_deadline(started, deadline, remaining)?)
    } else {
        (value.as_str(), deadline)
    };
    let readback = Readback::parse(row)?;
    owner.check_database_identity_result(&format!(
        "{}|{}",
        readback.system_id, readback.database_oid
    ))?;
    readback.check_receipt(notice, pins)?;
    owner.check(session)?;
    Ok(deadline)
}

fn live_release_deadline(started: Instant, deadline: Instant, remaining: &str) -> Result<Instant> {
    let milliseconds = remaining
        .parse::<u64>()
        .ok()
        .filter(|ms| (1..=300_000).contains(ms) && ms.to_string() == remaining)
        .ok_or_else(unavailable)?;
    // The DB samples after this local start, so charging the full query
    // duration is conservative and never extends the existing operation.
    let deadline = deadline.min(started + Duration::from_millis(milliseconds));
    if deadline <= Instant::now() {
        return Err(unavailable());
    }
    Ok(deadline)
}

fn committed_sql(notice: &Notice, pins: &ReadyPins, require_live_lease: bool) -> String {
    let b = &notice.binding;
    let lease_budget = if require_live_lease {
        ",floor(EXTRACT(EPOCH FROM (j.lease_until-clock_timestamp()))*1000)::bigint"
    } else {
        ""
    };
    // Interpolated values come from typed IDs, validated digests and the fixed
    // current native pin. A committed claim keeps its lease ID after approval.
    format!(
        "SELECT db.system_id,db.database_oid,o.id,dsp.id,x.id,replace(encode(d.receipt_canonical,'base64'),chr(10),''){lease_budget}
         FROM ({DATABASE_IDENTITY_SQL}) db
         JOIN hayes_original_dispositions d ON (d.database_system_identifier,d.database_oid)=(db.system_id,db.database_oid)
         JOIN hayes_original_attempts a ON (a.tenant_id,a.id,a.job_id)=(d.tenant_id,d.attempt_id,d.job_id)
         JOIN import_jobs j ON (j.tenant_id,j.id,j.hayes_attempt_id,j.hayes_disposition_id)=(a.tenant_id,a.job_id,a.id,d.id)
         JOIN hayes_output_stages s ON (s.tenant_id,s.id,s.attempt_id,s.job_id)=(d.tenant_id,d.stage_id,d.attempt_id,d.job_id)
         JOIN artworks w ON (w.tenant_id,w.id)=(a.tenant_id,a.artwork_id)
         JOIN assets o ON (o.tenant_id,o.id)=(d.tenant_id,d.original_asset_id)
         JOIN assets dsp ON (dsp.tenant_id,dsp.id)=(d.tenant_id,d.display_asset_id)
         JOIN asset_derivatives x ON (x.tenant_id,x.id,x.asset_id)=(d.tenant_id,d.derivative_id,o.id)
         JOIN rights r ON (r.tenant_id,r.id)=(o.tenant_id,o.rights_id)
         WHERE (a.tenant_id,a.workspace_id,a.principal_id)=('{tenant}','{workspace}','{principal}')
           AND (a.job_id,a.lease_id,a.id)=('{job}','{lease}','{attempt}')
           AND (a.runtime_generation,a.artwork_id,a.correlation_id)=('{generation}','{artwork}','{correlation}')
           AND (a.artwork_revision,a.source_bytes,a.source_sha256)=({revision},{source_bytes},'{source_sha}')
           AND d.id='{receipt}' AND d.receipt_sha256='{receipt_sha}' AND d.stage_id='{stage}'
           AND d.native_owner_id='{native_owner}' AND d.runtime_manifest_sha256='{host_manifest}'
           AND d.worker_manifest_sha256='{worker_manifest}' AND d.native_evidence_sha256='{evidence}'
           AND d.attestation=(convert_from(d.receipt_canonical,'UTF8')::jsonb->'attestation')
           AND (j.user_id,j.artwork_id,j.lease_id)=(a.principal_id,a.artwork_id,a.lease_id)
           AND j.state='approved' AND j.progress=100 AND j.asset_id=dsp.id AND j.original_asset_id IS NULL
           AND j.approved_key=s.object_key AND j.expected_hash=a.source_sha256 AND j.expected_bytes=a.source_bytes
           AND j.mime='model/gltf-binary'
           AND s.output_sha256='{BODY_SHA}' AND s.output_bytes={BODY_BYTES}
           AND s.object_key=a.tenant_id::text || '/hayes-output/' || a.id::text || '/{BODY_SHA}'
           AND w.revision=a.artwork_revision AND w.deleted_at IS NULL
           AND w.metadata #>> '{{displayReference,status}}'='provisional-uncalibrated'
           AND w.metadata #>> '{{displayReference,assetSha256}}'=s.output_sha256
           AND w.metadata #>> '{{provenance,sourceUnits}}'='unknown'
           AND w.metadata #> '{{provenance,scaleApplied}}'='false'::jsonb
           AND r.deleted_at IS NULL AND r.metadata=j.rights AND w.metadata->'rights'=j.rights
           AND j.rights #> '{{permissions,display}}'='true'::jsonb
           AND j.rights #> '{{permissions,export}}'='true'::jsonb
           AND CASE WHEN NOT j.rights ? 'validFrom' THEN true
                    WHEN jsonb_typeof(j.rights->'validFrom')='string'
                    THEN (j.rights->>'validFrom')::timestamptz <= clock_timestamp() ELSE false END
           AND CASE WHEN NOT j.rights ? 'expiresAt' THEN true
                    WHEN jsonb_typeof(j.rights->'expiresAt')='string'
                    THEN (j.rights->>'expiresAt')::timestamptz > clock_timestamp() ELSE false END
           AND o.id<>dsp.id AND o.artwork_id=a.artwork_id AND dsp.artwork_id=a.artwork_id AND o.rights_id=dsp.rights_id
           AND o.state='quarantine' AND o.deleted_at IS NULL AND o.object_key=j.object_key AND o.target_key=j.object_key
           AND o.sha256=a.source_sha256 AND o.bytes=a.source_bytes AND o.mime='model/gltf-binary' AND o.scale_meters=1
           AND dsp.state='approved' AND dsp.deleted_at IS NULL AND dsp.object_key=s.object_key AND dsp.target_key=s.object_key
           AND dsp.sha256=s.output_sha256 AND dsp.bytes=s.output_bytes AND dsp.mime='model/gltf-binary' AND dsp.scale_meters=1
           AND x.deleted_at IS NULL AND x.metadata=jsonb_build_object(
                'version',1,'kind','hayes-original-display-v1','originalAssetId',o.id::text,'displayAssetId',dsp.id::text,
                'attemptId',a.id::text,'sourceSha256',a.source_sha256,'sourceBytes',a.source_bytes,
                'outputSha256',s.output_sha256,'outputBytes',s.output_bytes,
                'recipe',jsonb_build_object('kind','native-hayes-generate-validate-v1','workerManifestSha256',d.worker_manifest_sha256),
                'receiptSha256',d.receipt_sha256)",
        tenant=b.tenant_id, workspace=b.workspace_id, principal=b.principal_id,
        job=b.job_id, lease=b.lease_id, attempt=b.attempt_id, generation=b.runtime_generation,
        artwork=b.artwork_id, correlation=b.correlation_id, revision=b.artwork_revision,
        source_bytes=b.source_bytes, source_sha=b.source_sha256, stage=notice.db_stage_id,
        native_owner=notice.native_owner_id, host_manifest=notice.runtime_manifest_sha256,
        worker_manifest=pins.worker_manifest_sha256, evidence=pins.evidence_sha256,
        receipt=notice.receipt_id, receipt_sha=notice.receipt_sha256,
    )
}
