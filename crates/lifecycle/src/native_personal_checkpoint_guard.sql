-- SPDX-License-Identifier: Apache-2.0
BEGIN READ ONLY;
SET LOCAL statement_timeout = '8s';
SET LOCAL search_path = pg_catalog;
WITH owner_scope AS (
    SELECT EXISTS (
        SELECT FROM public.local_workspaces w
        JOIN public.memberships m ON m.tenant_id = w.tenant_id AND m.user_id = w.user_id
        JOIN public.users u ON u.id = w.user_id
        JOIN public.tenants t ON t.id = w.tenant_id
        WHERE w.id = :'workspace'::uuid AND w.tenant_id = :'tenant'::uuid
          AND w.user_id = :'principal'::uuid AND w.mac_uid = :'uid'::integer
          AND m.role = 'admin' AND NOT u.disabled AND t.deleted_at IS NULL
    ) AS valid
), reasons AS (
    SELECT 'import' AS category, id, tenant_id, NULL::uuid AS workspace_id, user_id, state,
           artwork_id AS related_artwork_id,
           state IN ('uploading','queued','processing') AS active,
           lease_id IS NOT NULL AS lease_id_set, lease_until IS NOT NULL AS lease_until_set,
           cardinality(png_worker_tokens) > 0 AS png_workers,
           model_preparation ? 'claim' AS model_claim, false AS cleanup_pending, false AS worker_claim
    FROM public.import_jobs
    UNION ALL
    SELECT 'oex', id, tenant_id, NULL, user_id, state, NULL,
           state IN ('uploading','queued','processing'),
           lease_id IS NOT NULL, lease_until IS NOT NULL, false, false, cleanup_pending, false
    FROM public.oex_import_jobs
    UNION ALL
    SELECT 'material', id, tenant_id, workspace_id, user_id, phase, NULL,
           phase IN ('uploading','validating','cancelling'),
           false, false, false, false, false, worker_token IS NOT NULL
    FROM public.workspace_material_intakes
    UNION ALL
    SELECT 'furnishing', id, tenant_id, workspace_id, user_id, phase, NULL,
           phase IN ('uploading','validating','cancelling'),
           false, false, false, false, false, worker_token IS NOT NULL
    FROM public.workspace_furnishing_intakes
), blockers AS (
    SELECT *, (tenant_id = :'tenant'::uuid AND user_id = :'principal'::uuid
            AND (workspace_id IS NULL OR workspace_id = :'workspace'::uuid)
            AND (SELECT valid FROM owner_scope)) IS TRUE AS in_scope
    FROM reasons
    WHERE active OR lease_id_set OR lease_until_set OR png_workers OR model_claim OR cleanup_pending OR worker_claim
), totals AS (
    SELECT category, count(*) AS total, count(*) FILTER (WHERE in_scope) AS in_scope
    FROM blockers GROUP BY category
), scoped AS (
    SELECT * FROM blockers WHERE in_scope ORDER BY category, id LIMIT 24
)
SELECT jsonb_build_object(
    'ownerVerified', (SELECT valid FROM owner_scope),
    'systemIdentifier', (SELECT system_identifier::text FROM pg_control_system()),
    'ledger', (SELECT string_agg(name || '|' || sha256, E'\n' ORDER BY name) FROM public.schema_migrations),
    'hasProtectedWork', EXISTS (SELECT FROM blockers),
    'outsideScopeCount', (SELECT count(*) FROM blockers WHERE NOT in_scope),
    'truncated', (SELECT count(*) FROM blockers WHERE in_scope) > 24,
    'categories', (SELECT coalesce(jsonb_agg(jsonb_build_object(
        'category', category, 'total', total, 'inScope', in_scope) ORDER BY category), '[]'::jsonb) FROM totals),
    'rows', (SELECT coalesce(jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
        'category', category, 'id', id, 'tenantId', tenant_id, 'workspaceId', workspace_id, 'state', state,
        'relatedArtworkId', related_artwork_id, 'active', active IS TRUE, 'leaseIdSet', lease_id_set,
        'leaseUntilSet', lease_until_set, 'pngWorkers', png_workers IS TRUE, 'modelClaim', model_claim IS TRUE,
        'cleanupPending', cleanup_pending, 'workerClaim', worker_claim)) ORDER BY category, id), '[]'::jsonb) FROM scoped)
);
COMMIT;
