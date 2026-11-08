// SPDX-License-Identifier: Apache-2.0
//! Verify Platform's separate durable recording in the same owned database.

use super::super::super::canonical;
use super::super::{committed_sql, ReadyPins};
use super::{unavailable, CompletedFrame, Notice, OperationBinding, ReleaseScope, Result};
use crate::native_runtime::oex_owner::DATABASE_IDENTITY_SQL;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Recorded {
    version: u8,
    kind: String,
    binding: OperationBinding,
    native_owner_id: Uuid,
    runtime_manifest_sha256: String,
    ready_frame_sha256: String,
    db_stage_id: Uuid,
    challenge: Uuid,
    receipt_id: Uuid,
    completion_frame_sha256: String,
}

pub(super) fn check_ack(
    raw: &[u8],
    expected: &ReleaseScope<'_>,
    frame_sha: &str,
) -> Result<Recorded> {
    let ack: Recorded = canonical(raw)?;
    if ack.version != 1
        || ack.kind != "hayes-release-recorded"
        || &ack.binding != expected.binding
        || ack.native_owner_id != expected.native_owner_id
        || ack.runtime_manifest_sha256 != expected.runtime_manifest_sha256
        || ack.ready_frame_sha256 != expected.ready_frame_sha256
        || ack.db_stage_id != expected.db_stage_id
        || ack.challenge != expected.challenge
        || ack.receipt_id != expected.receipt_id
        || ack.completion_frame_sha256 != frame_sha
    {
        return Err(unavailable());
    }
    Ok(ack)
}

pub(super) fn check_row<'a>(row: &'a str, frame: &[u8]) -> Result<&'a str> {
    let (identity, encoded) = row.rsplit_once('|').ok_or_else(unavailable)?;
    if encoded != STANDARD.encode(frame) {
        return Err(unavailable());
    }
    Ok(identity)
}

pub(super) fn recorded_sql(
    completion: &CompletedFrame<'_>,
    notice: &Notice,
    pins: &ReadyPins,
    frame_sha: &str,
) -> String {
    let scope = completion.scope;
    let b = scope.binding;
    let committed = committed_sql(notice, pins, false);
    format!(
        "SELECT db.system_id,db.database_oid,replace(encode(r.completion_canonical,'base64'),chr(10),'')
           FROM ({DATABASE_IDENTITY_SQL}) db
           JOIN hayes_native_releases r ON (r.database_system_identifier,r.database_oid)=(db.system_id,db.database_oid)
          WHERE (r.tenant_id,r.attempt_id,r.job_id)=('{tenant}','{attempt}','{job}')
            AND (r.disposition_id,r.stage_id,r.native_owner_id)=('{receipt}','{stage}','{owner}')
            AND r.runtime_manifest_sha256='{runtime}' AND r.worker_manifest_sha256='{worker}'
            AND r.native_evidence_sha256='{evidence}' AND r.ready_frame_sha256='{ready}'
            AND r.challenge='{challenge}' AND r.receipt_sha256='{receipt_sha}'
            AND r.intent_sha256='{intent}' AND r.completion_sha256='{completion_sha}'
            AND r.completion_frame_sha256='{frame_sha}' AND EXISTS ({committed})",
        tenant=b.tenant_id, attempt=b.attempt_id, job=b.job_id,
        receipt=scope.receipt_id, stage=scope.db_stage_id, owner=scope.native_owner_id,
        runtime=scope.runtime_manifest_sha256, worker=scope.worker_manifest_sha256,
        evidence=scope.native_evidence_sha256, ready=scope.ready_frame_sha256,
        challenge=scope.challenge, receipt_sha=scope.receipt_sha256,
        intent=completion.intent_sha256, completion_sha=completion.completion_sha256,
    )
}
