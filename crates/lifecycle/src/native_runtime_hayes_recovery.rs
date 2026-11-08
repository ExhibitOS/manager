// SPDX-License-Identifier: Apache-2.0
//! Read-only continuation after durable native release and before CMS approval.

use super::{
    hayes::{operation::{ExecuteBinding, VerifiedRelease}, provision},
    oex_owner::{DATABASE_IDENTITY_SQL, Owner},
    *,
};
use base64::{engine::general_purpose::STANDARD, Engine};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    system_id: String,
    database_oid: String,
    attempt_id: Uuid,
    receipt_id: Uuid,
    receipt_sha256: String,
    intent_sha256: String,
    completion_sha256: String,
    completion_frame_sha256: String,
    completion_frame: String,
}

impl Session {
    pub(crate) fn recover_hayes_release(
        &mut self,
        job_id: &str,
        saved: &crate::installations::native_upload::Transfer,
    ) -> Result<Value> {
        let unavailable = || err("HAYES_RELEASE_UNCONFIRMED");
        if !self.status()?.readiness.ready || !self.payload.ordinary_hayes_release()
            || self.active_furnishing_binding().is_none_or(|binding| binding.catalog != 44)
            || saved.job_id.as_deref() != Some(job_id)
        {
            return Err(unavailable());
        }
        let intent = saved.intent.as_ref().ok_or_else(unavailable)?;
        let hayes = intent.hayes_original.as_ref().ok_or_else(unavailable)?;
        let binding = ExecuteBinding {
            job_id: Uuid::parse_str(job_id).map_err(|_| unavailable())?,
            artwork_id: Uuid::parse_str(&intent.artwork_id).map_err(|_| unavailable())?,
            artwork_revision: hayes.artwork_revision,
        };
        let auth = self.local_authority.as_ref().ok_or_else(unavailable)?;
        if (auth.workspace_id.as_str(), auth.tenant_id.as_str(), auth.principal_id.as_str())
            != (saved.workspace_id.as_str(), saved.tenant_id.as_str(), saved.principal_id.as_str())
            || ![&auth.workspace_id, &auth.tenant_id, &auth.principal_id].iter().all(|id| installations::uuid(id))
            || !hash_valid(&hayes.display_reference_sha256)
        {
            return Err(unavailable());
        }

        let sql = format!(
            "SELECT json_build_object('systemId',db.system_id,'databaseOid',db.database_oid,
                      'attemptId',r.attempt_id,'receiptId',r.disposition_id,'receiptSha256',r.receipt_sha256,
                      'intentSha256',r.intent_sha256,'completionSha256',r.completion_sha256,
                      'completionFrameSha256',r.completion_frame_sha256,'completionFrame',encode(r.completion_canonical,'base64'))
               FROM ({DATABASE_IDENTITY_SQL}) db
               JOIN hayes_native_releases r ON (r.database_system_identifier,r.database_oid)=(db.system_id,db.database_oid)
               JOIN hayes_original_dispositions d ON (d.tenant_id,d.id,d.job_id,d.attempt_id,d.stage_id)=(r.tenant_id,r.disposition_id,r.job_id,r.attempt_id,r.stage_id)
               JOIN hayes_original_attempts a ON (a.tenant_id,a.id,a.job_id)=(r.tenant_id,r.attempt_id,r.job_id)
               JOIN import_jobs j ON (j.tenant_id,j.id,j.hayes_disposition_id,j.hayes_attempt_id)=(r.tenant_id,r.job_id,r.disposition_id,r.attempt_id)
               JOIN artworks w ON (w.tenant_id,w.id)=(j.tenant_id,j.artwork_id)
               JOIN assets o ON (o.tenant_id,o.id,o.artwork_id)=(d.tenant_id,d.original_asset_id,w.id)
               JOIN assets v ON (v.tenant_id,v.id,v.artwork_id)=(d.tenant_id,d.display_asset_id,w.id)
               JOIN rights rights ON (rights.tenant_id,rights.id)=(o.tenant_id,o.rights_id)
              WHERE (r.tenant_id,a.workspace_id,a.principal_id,j.user_id)=('{tenant}','{workspace}','{principal}','{principal}')
                AND (j.id,w.id,w.revision)=('{job}','{artwork}',{revision})
                AND (a.artwork_id,a.artwork_revision)=(w.id,w.revision)
                AND j.state='approved' AND j.asset_id=v.id AND w.deleted_at IS NULL
                AND o.state='quarantine' AND o.deleted_at IS NULL AND (o.sha256,o.bytes)=(a.source_sha256,a.source_bytes)
                AND v.state='approved' AND v.deleted_at IS NULL AND v.sha256='{display}' AND v.rights_id=o.rights_id
                AND rights.deleted_at IS NULL AND rights.metadata=j.rights AND j.rights=w.metadata->'rights'
                AND w.metadata->'displayReference'->>'assetSha256'='{display}'
                AND r.runtime_manifest_sha256='{manifest}' AND r.worker_manifest_sha256='{worker}'
                AND r.receipt_sha256=d.receipt_sha256
                AND encode(sha256(d.receipt_canonical),'hex')=r.receipt_sha256",
            tenant=auth.tenant_id, workspace=auth.workspace_id, principal=auth.principal_id,
            job=binding.job_id, artwork=binding.artwork_id, revision=binding.artwork_revision,
            display=hayes.display_reference_sha256, manifest=self.payload.context().manifest_sha256,
            worker=crate::native_payload::hayes::WORKER_MANIFEST_SHA256,
        );
        let owner = Owner::capture(self)?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let result = self.query_database_before("exhibitos", &sql, deadline)?;
        let row: Row = serde_json::from_str(&result).map_err(|_| unavailable())?;
        owner.check_database_identity_result(&format!("{}|{}", row.system_id, row.database_oid))?;
        let lease = provision::open_user_lease()?;
        let intent = lease.released_journal(row.attempt_id, "operation-release-intent.json")?;
        let completion = lease.released_journal(row.attempt_id, "operation-release-completed.json")?;
        let ack = lease.released_journal(row.attempt_id, "operation-release-recorded.json")?;
        let verified = check_journals(binding, row, &intent, &completion, &ack)?;
        owner.check(self)?;
        serde_json::to_value(verified).map_err(|_| unavailable())
    }
}

fn check_journals(binding: ExecuteBinding, row: Row, intent: &[u8], completion: &[u8], ack: &[u8]) -> Result<VerifiedRelease> {
    let unavailable = || err("HAYES_RELEASE_UNCONFIRMED");
    let raw = STANDARD.decode(row.completion_frame.replace('\n', "")).map_err(|_| unavailable())?;
    if digest(intent) != row.intent_sha256 || digest(completion) != row.completion_sha256
        || digest(&raw) != row.completion_frame_sha256
    {
        return Err(unavailable());
    }
    let mut frame: serde_json::Map<String, Value> = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
    let intent: Value = serde_json::from_slice(intent).map_err(|_| unavailable())?;
    let completion: Value = serde_json::from_slice(completion).map_err(|_| unavailable())?;
    let ack: Value = serde_json::from_slice(ack).map_err(|_| unavailable())?;
    if frame.remove("version") != Some(Value::from(1))
        || frame.remove("kind") != Some(Value::from("hayes-release-completed"))
        || frame.remove("intentSha256") != Some(Value::from(row.intent_sha256.as_str()))
        || frame.remove("completionSha256") != Some(Value::from(row.completion_sha256.as_str()))
        || frame.get("receiptId") != Some(&Value::from(row.receipt_id.to_string()))
        || frame.get("receiptSha256") != Some(&Value::from(row.receipt_sha256.as_str()))
    {
        return Err(unavailable());
    }
    let scope = Value::Object(frame);
    if scope["binding"]["jobId"] != binding.job_id.to_string()
        || scope["binding"]["artworkId"] != binding.artwork_id.to_string()
        || scope["binding"]["artworkRevision"] != binding.artwork_revision
        || scope["binding"]["attemptId"] != row.attempt_id.to_string()
        || intent["version"] != 1 || intent["scope"] != scope
        || completion["version"] != 1 || completion["scope"] != scope
        || completion["intentSha256"] != row.intent_sha256
        || completion["stages"] != intent["stages"]
        || !completion["stages"].as_array().is_some_and(|stages| stages.len() == 4)
        || completion["activeReservationRetired"] != true
        || completion["originalCustodyLockClosed"] != true
        || ack["version"] != 1 || ack["kind"] != "hayes-release-recorded"
        || ack["completionFrameSha256"] != row.completion_frame_sha256
        || ["binding", "nativeOwnerId", "runtimeManifestSha256", "readyFrameSha256", "dbStageId", "challenge", "receiptId"]
            .iter().any(|key| ack[key] != scope[key])
    {
        return Err(unavailable());
    }
    Ok(VerifiedRelease {
        job_id: binding.job_id, receipt_id: row.receipt_id,
        receipt_sha256: row.receipt_sha256, completion_frame_sha256: row.completion_frame_sha256,
    })
}
