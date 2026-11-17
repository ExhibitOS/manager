-- SPDX-License-Identifier: Apache-2.0
-- PG-only gate before a 055 transition starts the Platform process.
-- Only lease columns on the matched settled jobs are exempt from the idle test.
WITH terminal_jobs AS (
    SELECT evidence.tenant_id, evidence.job_id
    FROM public.hayes_preopen_terminal_bindings evidence
    JOIN public.hayes_original_attempts attempt
      ON (attempt.tenant_id, attempt.id, attempt.job_id) = (evidence.tenant_id, evidence.attempt_id, evidence.job_id)
    JOIN public.import_jobs job
      ON (job.tenant_id, job.id, job.hayes_attempt_id, job.lease_id) = (attempt.tenant_id, attempt.job_id, attempt.id, attempt.lease_id)
    CROSS JOIN LATERAL (SELECT convert_from(evidence.proof_canonical, 'UTF8')::jsonb AS body) proof
    WHERE (evidence.tenant_id, evidence.workspace_id, evidence.principal_id) = (@TENANT@, @WORKSPACE@, @PRINCIPAL@)
      AND (job.user_id, job.artwork_id) = (attempt.principal_id, attempt.artwork_id)
      AND job.attempts = 1 AND job.progress = 25 AND job.lease_until <= clock_timestamp()
      AND (proof.body #>> '{claim,leaseUntil}')::timestamptz = job.lease_until
      AND proof.body #>> '{claim,systemId}' = (pg_control_system()).system_identifier::text
      AND proof.body #>> '{claim,databaseOid}' = (SELECT oid::text FROM pg_database WHERE datname = current_database())
      AND encode(sha256(evidence.proof_canonical), 'hex') = evidence.proof_sha256
), completed_jobs AS (
    SELECT release.tenant_id, release.job_id
    FROM public.hayes_native_releases release
    JOIN public.hayes_original_dispositions disposition
      ON (disposition.tenant_id, disposition.id, disposition.job_id, disposition.attempt_id, disposition.stage_id) =
         (release.tenant_id, release.disposition_id, release.job_id, release.attempt_id, release.stage_id)
    JOIN public.hayes_original_attempts attempt
      ON (attempt.tenant_id, attempt.id, attempt.job_id) = (release.tenant_id, release.attempt_id, release.job_id)
    JOIN public.hayes_output_stages stage
      ON (stage.tenant_id, stage.id, stage.job_id, stage.attempt_id) = (release.tenant_id, release.stage_id, release.job_id, release.attempt_id)
    JOIN public.import_jobs job
      ON (job.tenant_id, job.id, job.hayes_attempt_id, job.hayes_disposition_id) =
         (release.tenant_id, release.job_id, release.attempt_id, release.disposition_id)
    WHERE (attempt.tenant_id, attempt.workspace_id, attempt.principal_id) = (@TENANT@, @WORKSPACE@, @PRINCIPAL@)
      AND (job.user_id, job.artwork_id, job.lease_id) = (attempt.principal_id, attempt.artwork_id, attempt.lease_id)
      AND job.state = 'approved' AND job.progress = 100 AND job.original_asset_id IS NULL AND job.lease_until IS NOT NULL
      AND cardinality(job.png_worker_tokens) = 0 AND (job.model_preparation ? 'claim') IS NOT TRUE
      AND job.approved_key = stage.object_key AND job.asset_id = disposition.display_asset_id
      AND ROW(disposition.native_owner_id, disposition.runtime_manifest_sha256, disposition.worker_manifest_sha256,
              disposition.native_evidence_sha256, disposition.database_system_identifier, disposition.database_oid, disposition.receipt_sha256) =
          ROW(release.native_owner_id, release.runtime_manifest_sha256, release.worker_manifest_sha256,
              release.native_evidence_sha256, release.database_system_identifier, release.database_oid, release.receipt_sha256)
      AND release.database_system_identifier = (pg_control_system()).system_identifier::text
      AND release.database_oid = (SELECT oid::text FROM pg_database WHERE datname = current_database())
      AND encode(sha256(release.completion_canonical), 'hex') = release.completion_frame_sha256
      AND convert_from(release.completion_canonical, 'UTF8')::jsonb = jsonb_build_object(
          'version', 1, 'kind', 'hayes-release-completed',
          'binding', jsonb_build_object(
              'version', 1, 'tenantId', attempt.tenant_id::text, 'workspaceId', attempt.workspace_id::text,
              'principalId', attempt.principal_id::text, 'jobId', attempt.job_id::text, 'leaseId', attempt.lease_id::text,
              'attemptId', attempt.id::text, 'runtimeGeneration', attempt.runtime_generation::text,
              'artworkId', attempt.artwork_id::text, 'correlationId', attempt.correlation_id::text,
              'artworkRevision', attempt.artwork_revision, 'sourceSha256', attempt.source_sha256, 'sourceBytes', attempt.source_bytes),
          'nativeOwnerId', release.native_owner_id::text, 'runtimeManifestSha256', release.runtime_manifest_sha256,
          'workerManifestSha256', release.worker_manifest_sha256, 'nativeEvidenceSha256', release.native_evidence_sha256,
          'readyFrameSha256', release.ready_frame_sha256, 'dbStageId', release.stage_id::text,
          'database', jsonb_build_object('systemIdentifier', release.database_system_identifier, 'databaseOid', release.database_oid),
          'challenge', release.challenge::text, 'receiptId', release.disposition_id::text, 'receiptSha256', release.receipt_sha256,
          'intentSha256', release.intent_sha256, 'completionSha256', release.completion_sha256)
), settled_jobs AS (
    SELECT tenant_id, job_id FROM terminal_jobs
    UNION ALL SELECT tenant_id, job_id FROM completed_jobs
)
SELECT EXISTS (
    SELECT 1 FROM public.import_jobs job
    WHERE state IN ('uploading', 'queued', 'processing')
       OR ((lease_id IS NOT NULL OR lease_until IS NOT NULL) AND NOT EXISTS (
           SELECT 1 FROM settled_jobs settled WHERE (settled.tenant_id, settled.job_id) = (job.tenant_id, job.id)
       ))
       OR cardinality(png_worker_tokens) > 0 OR model_preparation ? 'claim' OR derived ? 'claim'
) OR EXISTS (
    SELECT 1 FROM public.oex_import_jobs
    WHERE state IN ('uploading', 'queued', 'processing') OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cleanup_pending
) OR EXISTS (
    SELECT 1 FROM public.workspace_material_intakes WHERE phase IN ('uploading', 'validating', 'cancelling') OR worker_token IS NOT NULL
) OR EXISTS (
    SELECT 1 FROM public.workspace_furnishing_intakes WHERE phase IN ('uploading', 'validating', 'cancelling') OR worker_token IS NOT NULL
) OR EXISTS (
    SELECT 1 FROM public.freeze_requests WHERE state = 'creating' OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cleanup_pending
) OR EXISTS (
    SELECT 1 FROM public.workspace_frame_preparations WHERE state = 'staged'
) AS unsettled
