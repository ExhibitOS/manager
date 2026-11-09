-- Catalog 045 pre-Node mirror of the pinned Platform admission.
WITH completed_hayes AS (
    SELECT j.tenant_id,j.id FROM import_jobs j
    JOIN hayes_original_attempts a ON (a.tenant_id,a.id,a.job_id,a.artwork_id,a.lease_id)=(j.tenant_id,j.hayes_attempt_id,j.id,j.artwork_id,j.lease_id)
    JOIN hayes_original_dispositions d ON (d.tenant_id,d.id,d.attempt_id,d.job_id,d.display_asset_id)=(j.tenant_id,j.hayes_disposition_id,a.id,j.id,j.asset_id)
    JOIN hayes_output_stages s ON (s.tenant_id,s.id,s.attempt_id,s.job_id,s.object_key)=(j.tenant_id,d.stage_id,a.id,j.id,j.approved_key)
    JOIN hayes_native_releases r ON (r.tenant_id,r.attempt_id,r.job_id,r.disposition_id,r.stage_id)=(j.tenant_id,a.id,j.id,d.id,s.id)
    WHERE j.tenant_id=@2@ AND j.user_id=@3@ AND a.workspace_id=@1@ AND a.principal_id=@3@ AND j.id=@4@
      AND j.state='approved' AND j.lease_until IS NOT NULL AND cardinality(j.png_worker_tokens)=0
      AND (j.model_preparation ? 'claim') IS NOT TRUE AND r.completion_frame_sha256=@5@
      AND r.database_system_identifier=(pg_control_system()).system_identifier::text
      AND r.database_oid=(SELECT oid::text FROM pg_database WHERE datname=current_database())
   ), completed_oex AS (
    SELECT j.tenant_id,j.id,j.expected_bytes FROM oex_import_jobs j
    JOIN workspace_oex_import_terminal_proof t ON (t.tenant_id,t.import_job_id,t.principal_id,t.import_request_id,t.source_archive_sha256)
      =(j.tenant_id,j.id,j.user_id,j.request_id,j.payload_sha256)
    JOIN native_oex_exhibition_provenance p ON (p.tenant_id,p.import_job_id)=(j.tenant_id,j.id)
    WHERE t.attempt_id=@7@ AND t.import_request_id=@8@ AND t.source_archive_sha256=@9@
      AND p.claim_sha256=@10@ AND p.source_database_system_id=@11@ AND p.source_database_oid=@12@
      AND (pg_control_system()).system_identifier::text=@11@
      AND (SELECT oid::text FROM pg_database WHERE datname=current_database())=@12@
      AND j.expected_bytes BETWEEN 1 AND 536870912
      AND j.staging_keys=(SELECT jsonb_agg(j.tenant_id::text||'/oex-staging/'||j.id::text||'/chunk-'||i ORDER BY i)
        FROM generate_series(0,(j.expected_bytes-1)/16777216) AS chunks(i))
      AND t.workspace_id=@1@ AND j.tenant_id=@2@ AND j.user_id=@3@ AND j.id=@6@ AND j.state='complete'
      AND j.archive_profile='bounded-file-v1' AND j.oex_format='1.0.0-draft.12' AND j.lease_id IS NULL AND j.lease_until IS NULL
   ), admission AS (SELECT
    (@4@::uuid IS NOT NULL AND (SELECT count(*) FROM completed_hayes)<>1) OR
    (@6@::uuid IS NOT NULL AND (SELECT count(*) FROM completed_oex)<>1) OR
    EXISTS (SELECT 1 FROM import_jobs j
      WHERE (state IN ('uploading','queued','processing') OR lease_id IS NOT NULL OR lease_until IS NOT NULL
      OR cardinality(png_worker_tokens)>0 OR model_preparation ? 'claim')
      AND NOT EXISTS (SELECT 1 FROM completed_hayes h WHERE (h.tenant_id,h.id)=(j.tenant_id,j.id))) OR
    EXISTS (SELECT 1 FROM oex_import_jobs j WHERE (state IN ('uploading','queued','processing')
      OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cleanup_pending)
      AND NOT EXISTS (SELECT 1 FROM completed_oex o WHERE (o.tenant_id,o.id)=(j.tenant_id,j.id))) OR
    EXISTS (SELECT 1 FROM workspace_material_intakes WHERE phase IN ('uploading','validating','cancelling') OR worker_token IS NOT NULL) OR
    EXISTS (SELECT 1 FROM workspace_furnishing_intakes WHERE phase IN ('uploading','validating','cancelling') OR worker_token IS NOT NULL) AS unsettled
) SELECT CASE WHEN NOT unsettled THEN (SELECT expected_bytes FROM completed_oex) END FROM admission;

