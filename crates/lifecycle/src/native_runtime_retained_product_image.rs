// SPDX-License-Identifier: Apache-2.0
//! Pin the one prepared image across the fenced saved-workspace trial.
use super::*;
use crate::installations::native_maintenance::{RetainedProductImage, RetainedProductImageRecovery};

const ERROR: &str = "NATIVE_RETAINED_PRODUCT_IMAGE_CHANGED";

pub(super) struct Trial {
    proof: RetainedProductImage,
    before: Option<Value>,
}

pub(super) fn drained_database_error(context: &Context) -> bool {
    context.state == "failed"
        && context.failure_code.as_deref() == Some("NATIVE_DATABASE_UNAVAILABLE")
        && !context.readiness.ready
        && context.postgres_pid.is_none() && context.node_pid.is_none() && context.auxiliary_pid.is_none()
        && context.exits.len() == 2
        && context.exits.get("postgres") == Some(&0)
        && context.exits.get("auxiliary") == Some(&1)
}

fn custody_query(owner: &FurnishingOwner, proof: &RetainedProductImage) -> Result<String> {
    let tenant = &owner.tenant_id;
    let workspace = &owner.workspace_id;
    let principal = &owner.principal_id;
    let job = &proof.job_id;
    let artwork = &proof.artwork_id;
    let request = &proof.replacement_request_id;
    let upload = &proof.upload_idempotency_key;
    let source = &proof.source_sha256;
    let receipt = &proof.receipt_sha256;
    let revision = proof.artwork_revision;
    let bytes = proof.source_bytes;
    if ![tenant, workspace, principal, job, artwork, request, upload]
        .into_iter().all(|id| oex_preflight_frames::uuid(id))
        || !hash_valid(source) || !hash_valid(receipt) || revision == 0 || bytes == 0
    {
        return Err(err(ERROR));
    }
    Ok(format!(r#"
SELECT jsonb_build_object(
  'receipt', j.derived->'result',
  'custodySha256', encode(sha256(convert_to(jsonb_build_array(
    to_jsonb(j), to_jsonb(a), to_jsonb(approval), to_jsonb(revision_row),
    (SELECT jsonb_agg(jsonb_build_array(
       to_jsonb(asset), to_jsonb(rights),
       (SELECT jsonb_agg(to_jsonb(derivative) ORDER BY derivative.id)
        FROM public.asset_derivatives derivative
        WHERE (derivative.tenant_id, derivative.asset_id) = (asset.tenant_id, asset.id))
     ) ORDER BY asset.id)
     FROM public.assets asset JOIN public.rights rights
       ON (rights.tenant_id, rights.id) = (asset.tenant_id, asset.rights_id)
     WHERE (asset.tenant_id, asset.artwork_id) = (a.tenant_id, a.id))
  )::text, 'UTF8')), 'hex'))
FROM public.import_jobs j
JOIN public.artworks a ON (a.tenant_id, a.id) = (j.tenant_id, j.artwork_id)
JOIN public.artwork_approvals approval
  ON (approval.tenant_id, approval.artwork_id, approval.revision, approval.asset_id)
     = (a.tenant_id, a.id, a.approved_revision, a.approved_asset_id)
JOIN public.artwork_revisions revision_row
  ON (revision_row.tenant_id, revision_row.artwork_id, revision_row.revision) = (a.tenant_id, a.id, a.revision)
WHERE j.tenant_id = '{tenant}' AND j.user_id = '{principal}' AND j.id = '{job}'
  AND j.artwork_id = '{artwork}' AND a.revision = {revision}
  AND a.approved_revision = a.revision AND a.deleted_at IS NULL
  AND j.state = 'approved' AND j.lease_id IS NULL AND j.lease_until IS NULL
  AND j.png_drain_version = 1 AND cardinality(j.png_worker_tokens) = 0
  AND j.idempotency_key = '{upload}' AND j.mime = 'image/jpeg'
  AND j.expected_hash = '{source}' AND j.expected_bytes = {bytes}
  AND j.derived->>'kind' = 'product-image-pair'
  AND j.derived->>'workspaceId' = '{workspace}' AND j.derived->>'principalId' = '{principal}'
  AND j.derived#>>'{{request,profile}}' = 'product-image-pair-2048-512-v1'
  AND j.derived#>>'{{request,artworkRevision}}' = '{revision}'
  AND j.derived#>>'{{result,sha256}}' = '{receipt}'
  AND NOT EXISTS (
    SELECT 1 FROM public.studio_requests r
    WHERE r.tenant_id = j.tenant_id AND r.user_id = j.user_id AND r.request_id = '{request}'
  )
"#))
}

impl Session {
    pub(crate) fn arm_retained_product_image(&mut self, proof: RetainedProductImage) -> Result<()> {
        let (trial, transition) = self.furnishing_transition_trial.as_ref().ok_or_else(|| err(ERROR))?;
        let pair = (transition.source().2, transition.target().2);
        if self.job_id.is_some() || self.retained_product_image.is_some() || self.transition_fence.is_none()
            || match proof.recovery.as_ref() {
                Some(recovery) => !crate::native_payload::retained_product_image_recovery_transition(pair)
                    || recovery.failed_generation != trial.source.id || self.oex_retained_cleanup.is_some(),
                None => !crate::native_payload::retained_product_image_transition(pair),
            }
        {
            return Err(err(ERROR));
        }
        self.retained_product_image = Some(Trial { proof, before: None });
        Ok(())
    }

    pub(super) fn retained_product_image_recovery(&self) -> Option<&RetainedProductImageRecovery> {
        self.retained_product_image.as_ref()?.proof.recovery.as_ref()
    }

    pub(crate) fn arm_startup_cancellation_045(
        &mut self, proof: crate::installations::native_maintenance::StartupCancellation045,
    ) -> Result<()> {
        let (trial, transition) = self.furnishing_transition_trial.as_ref().ok_or_else(|| err(ERROR))?;
        if self.job_id.is_some() || self.startup_cancellation_045.is_some() || self.transition_fence.is_none()
            || !transition.startup_recovery().is_some_and(crate::native_payload::Catalog45StartupRecovery::requires_hayes_proof)
            || proof.failed_generation != trial.source.id
            || self.retained_product_image.is_some() || self.oex_retained_cleanup.is_some()
        {
            return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
        }
        self.startup_cancellation_045 = Some(proof);
        Ok(())
    }

    pub(super) fn catalog45_hayes_only_recovery(&self) -> Option<(&str, &str)> {
        if let Some(proof) = &self.startup_cancellation_045 {
            return Some((&proof.hayes_job_id, &proof.hayes_completion_frame_sha256));
        }
        let proof = self.retained_product_image_recovery()?;
        Some((&proof.hayes_job_id, &proof.hayes_completion_frame_sha256))
    }

    pub(crate) fn verify_startup_cancellation_045(&mut self) -> Result<()> {
        let Some(proof) = &self.startup_cancellation_045 else { return Ok(()); };
        let binding = self.active_furnishing_binding().ok_or_else(|| err("FURNISHING_RECOVERY_UNAVAILABLE"))?;
        let sql = oex_retained_cleanup::hayes_only_database_query(
            &binding.owner, &proof.hayes_job_id, &proof.hayes_completion_frame_sha256,
        )?;
        if self.query_database("exhibitos", &sql)? != "true" {
            return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
        }
        Ok(())
    }

    fn retained_product_image_custody(&mut self) -> Result<Value> {
        let trial = self.retained_product_image.as_ref().ok_or_else(|| err(ERROR))?;
        let binding = self.active_furnishing_binding().ok_or_else(|| err(ERROR))?;
        let sql = custody_query(&binding.owner, &trial.proof)?;
        let row = self.query_database("exhibitos", &sql)?;
        let custody: Value = serde_json::from_str(&row).map_err(|_| err(ERROR))?;
        if !custody["custodySha256"].as_str().is_some_and(hash_valid)
            || !custody["receipt"].is_object()
        {
            return Err(err(ERROR));
        }
        Ok(custody)
    }

    pub(super) fn check_retained_product_image_before_node(&mut self) -> Result<()> {
        if self.retained_product_image.is_some() {
            let custody = self.retained_product_image_custody()?;
            self.retained_product_image.as_mut().ok_or_else(|| err(ERROR))?.before = Some(custody);
        }
        Ok(())
    }

    pub(crate) fn verify_retained_product_image(&mut self, authoring: &Value) -> Result<Value> {
        let custody = self.retained_product_image_custody()?;
        if let Some(recovery) = self.retained_product_image_recovery() {
            let binding = self.active_furnishing_binding().ok_or_else(|| err(ERROR))?;
            let sql = oex_retained_cleanup::hayes_only_database_query(
                &binding.owner, &recovery.hayes_job_id, &recovery.hayes_completion_frame_sha256,
            )?;
            if self.query_database("exhibitos", &sql)? != "true" {
                return Err(err("FURNISHING_RECOVERY_UNAVAILABLE"));
            }
        }
        let trial = self.retained_product_image.as_ref().ok_or_else(|| err(ERROR))?;
        if trial.before.as_ref() != Some(&custody) {
            return Err(err(ERROR));
        }
        let mut input = serde_json::to_vec(&serde_json::json!({
            "origin": authoring["origin"], "tenant": authoring["tenantId"],
            "token": authoring["token"], "job": trial.proof.job_id, "receipt": custody["receipt"],
        })).map_err(|_| err(ERROR))?;
        // The existing import readback validates the canonical receipt, its
        // asset/rights rows and immutable derivative lineage.
        let script = r#"
const { readFileSync } = require('node:fs');
const { isDeepStrictEqual } = require('node:util');
(async () => {
  const { origin, tenant, token, job, receipt } = JSON.parse(readFileSync(0, 'utf8'));
  const response = await fetch(origin + '/api/v1/tenants/' + tenant + '/imports/' + job, {
    redirect: 'error', signal: AbortSignal.timeout(7000),
    headers: { origin, accept: 'application/json', cookie: 'exhibitos_local_session=' + token },
  });
  if (!response.ok) return false;
  const chunks = [];
  let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.length;
    if (bytes > 16384) return false;
    chunks.push(chunk);
  }
  const value = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  const image = value.imagePreparation;
  return value.id === job && value.state === 'approved' && image?.status === 'ready'
    && image.workerDrained === true && isDeepStrictEqual(image.serverReceipt, receipt);
})().then(ok => process.stdout.write(ok ? 'ok' : 'changed'), () => process.stdout.write('changed'));
"#;
        let result = self.auxiliary_timed(
            self.payload.node(), vec!["-e".into(), script.into()], vec![],
            Some(&input), Duration::from_secs(15),
        );
        input.fill(0);
        match result {
            Ok((0, reply)) if reply == b"ok" => Ok(custody["receipt"].clone()),
            _ => Err(err(ERROR)),
        }
    }
}
