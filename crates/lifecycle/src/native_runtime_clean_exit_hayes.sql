-- SPDX-License-Identifier: Apache-2.0
-- Read-only comparison for the exact clean Catalog 045 exit handoff.
-- Collect candidates before checking the immutable joins so missing evidence fails closed.
WITH candidates AS (
    SELECT j.tenant_id, j.id
    FROM import_jobs j
    LEFT JOIN hayes_original_attempts a ON (a.tenant_id, a.id) = (j.tenant_id, j.hayes_attempt_id)
    WHERE j.tenant_id = @TENANT@ AND j.user_id = @PRINCIPAL@
      AND (a.workspace_id = @WORKSPACE@ OR a.id IS NULL)
      AND j.state = 'approved' AND j.hayes_attempt_id IS NOT NULL
    UNION
    SELECT r.tenant_id, r.job_id
    FROM hayes_native_releases r
    JOIN hayes_original_attempts a ON (a.tenant_id, a.id) = (r.tenant_id, r.attempt_id)
    WHERE r.tenant_id = @TENANT@ AND a.workspace_id = @WORKSPACE@ AND a.principal_id = @PRINCIPAL@
), confirmed AS (
    SELECT j.id, r.completion_frame_sha256,
           encode(sha256(convert_to(jsonb_build_array(
               j.lease_id, extract(epoch FROM j.lease_until), r.receipt_sha256
           )::text, 'UTF8')), 'hex') AS lease_fingerprint
    FROM candidates c
    JOIN import_jobs j ON (j.tenant_id, j.id) = (c.tenant_id, c.id)
    JOIN hayes_original_attempts a ON (a.tenant_id, a.id, a.job_id, a.artwork_id, a.lease_id)
        = (j.tenant_id, j.hayes_attempt_id, j.id, j.artwork_id, j.lease_id)
    JOIN hayes_original_dispositions d ON (d.tenant_id, d.id, d.attempt_id, d.job_id, d.display_asset_id)
        = (j.tenant_id, j.hayes_disposition_id, a.id, j.id, j.asset_id)
    JOIN hayes_output_stages s ON (s.tenant_id, s.id, s.attempt_id, s.job_id, s.object_key)
        = (d.tenant_id, d.stage_id, a.id, j.id, j.approved_key)
    JOIN hayes_native_releases r ON (r.tenant_id, r.attempt_id, r.job_id, r.disposition_id, r.stage_id)
        = (j.tenant_id, a.id, j.id, d.id, s.id)
    WHERE a.workspace_id = @WORKSPACE@ AND a.principal_id = @PRINCIPAL@ AND j.user_id = a.principal_id
      AND j.state = 'approved' AND j.lease_until IS NOT NULL AND cardinality(j.png_worker_tokens) = 0
      AND (j.model_preparation ? 'claim') IS NOT TRUE
      AND (d.native_owner_id, d.runtime_manifest_sha256, d.worker_manifest_sha256,
           d.native_evidence_sha256, d.database_system_identifier, d.database_oid, d.receipt_sha256)
        = (r.native_owner_id, r.runtime_manifest_sha256, r.worker_manifest_sha256,
           r.native_evidence_sha256, r.database_system_identifier, r.database_oid, r.receipt_sha256)
      AND r.database_system_identifier = (pg_control_system()).system_identifier::text
      AND r.database_oid = (SELECT oid::text FROM pg_database WHERE datname = current_database())
      AND encode(sha256(r.completion_canonical), 'hex') = r.completion_frame_sha256
      AND convert_from(r.completion_canonical, 'UTF8')::jsonb = jsonb_build_object(
          'version', 1, 'kind', 'hayes-release-completed',
          'binding', jsonb_build_object(
              'version', 1, 'tenantId', a.tenant_id::text, 'workspaceId', a.workspace_id::text,
              'principalId', a.principal_id::text, 'jobId', a.job_id::text, 'leaseId', a.lease_id::text,
              'attemptId', a.id::text, 'runtimeGeneration', a.runtime_generation::text,
              'artworkId', a.artwork_id::text, 'correlationId', a.correlation_id::text,
              'artworkRevision', a.artwork_revision, 'sourceSha256', a.source_sha256,
              'sourceBytes', a.source_bytes
          ),
          'nativeOwnerId', r.native_owner_id::text, 'runtimeManifestSha256', r.runtime_manifest_sha256,
          'workerManifestSha256', r.worker_manifest_sha256, 'nativeEvidenceSha256', r.native_evidence_sha256,
          'readyFrameSha256', r.ready_frame_sha256, 'dbStageId', r.stage_id::text,
          'database', jsonb_build_object('systemIdentifier', r.database_system_identifier, 'databaseOid', r.database_oid),
          'challenge', r.challenge::text, 'receiptId', r.disposition_id::text, 'receiptSha256', r.receipt_sha256,
          'intentSha256', r.intent_sha256, 'completionSha256', r.completion_sha256
      )
)
SELECT jsonb_agg(jsonb_build_object(
    'jobId', id, 'completionFrameSha256', completion_frame_sha256, 'leaseFingerprint', lease_fingerprint
) ORDER BY id)
FROM confirmed
HAVING count(*) BETWEEN 1 AND 128 AND count(*) = (SELECT count(*) FROM candidates);
