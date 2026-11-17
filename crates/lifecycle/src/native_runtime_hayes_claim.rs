// SPDX-License-Identifier: Apache-2.0
//! Owner-fenced observation of a retained, uncompleted Catalog 052 Hayes claim.

use super::{
    hayes::provision,
    oex_owner::{DATABASE_IDENTITY_SQL, Owner},
    *,
};
use crate::installations::{native_document::DocumentState, native_maintenance::RetainedHayesPreopen, native_upload::Transfer};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Input {
    pub(crate) job_id: Uuid,
    failed_generation: Uuid,
    failed_record_sha256: String,
    saved_etag: String,
    saved_revision: u64,
    transfer_sha256: String,
    custody_authority_sha256: String,
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claim {
    system_id: String,
    database_oid: String,
    attempt_id: Uuid,
    lease_id: Uuid,
    correlation_id: Uuid,
    generation: Uuid,
    lease_until: String,
    lease_expired: bool,
}

pub(super) struct Trial {
    database_identity: String,
    claim: Claim,
    query: String,
    input: Input,
    custody: Option<super::hayes::Lease>,
}

fn denied() -> crate::LifecycleError {
    err("HAYES_PREOPEN_CLAIM_UNCONFIRMED")
}

pub(crate) fn transfer_digest(transfer: &Transfer) -> Result<String> {
    Ok(digest(&serde_json::to_vec(transfer).map_err(|_| denied())?))
}

impl RetainedHayesPreopen {
    fn input(&self) -> Result<Input> {
        super::hayes_terminal_wire::Proof::checked(&self.observation, &self.proof_sha256)?;
        let proof = &self.observation;
        serde_json::from_value(serde_json::json!({
            "jobId":proof["jobId"], "failedGeneration":proof["claim"]["generation"],
            "failedRecordSha256":proof["failedRecordSha256"], "savedEtag":proof["savedEtag"],
            "savedRevision":proof["savedRevision"], "transferSha256":proof["transferSha256"],
            "custodyAuthoritySha256":proof["custodyAuthoritySha256"],
        })).map_err(|_| denied())
    }

    pub(crate) fn check_saved(&self, draft: &DocumentState, saved: &Transfer, raw: &[u8]) -> Result<()> {
        let input = self.input()?;
        let proof = &self.observation;
        let intent = saved.intent.as_ref().ok_or_else(denied)?;
        let hayes = intent.hayes_original.as_ref().ok_or_else(denied)?;
        if ![&saved.workspace_id, &saved.tenant_id, &saved.principal_id, &intent.artwork_id].into_iter().all(|id| installations::uuid(id))
            || ![&input.failed_record_sha256, &input.custody_authority_sha256, &hayes.display_reference_sha256].into_iter().all(|value| hash_valid(value))
            || draft.pending.is_some() || draft.revision != input.saved_revision
            || draft.etag.as_deref() != Some(input.saved_etag.as_str())
            || draft.saved_edit_version.is_none()
            || draft.draft.as_ref().and_then(|draft| draft["editVersion"].as_u64()) != draft.saved_edit_version
            || (saved.workspace_id.as_str(), saved.tenant_id.as_str(), saved.principal_id.as_str())
                != (draft.workspace_id.as_str(), draft.tenant_id.as_str(), draft.principal_id.as_str())
            || proof["workspaceId"] != saved.workspace_id || proof["tenantId"] != saved.tenant_id
            || proof["principalId"] != saved.principal_id || proof["artworkId"] != intent.artwork_id
            || proof["artworkRevision"] != hayes.artwork_revision || hayes.artwork_revision != 1
            || intent.mime != "model/gltf-binary" || intent.bytes != 8383096
            || intent.sha256 != "e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6"
            || saved.job_id.as_deref() != Some(input.job_id.to_string().as_str())
            || saved.compact_variant.is_some() || saved.gpu_variant.is_some()
            || digest(raw) != input.transfer_sha256 || transfer_digest(saved)? != input.transfer_sha256
        {
            return Err(denied());
        }
        Ok(())
    }
}

impl Session {
    pub(crate) fn arm_retained_hayes_preopen(&mut self, proof: &RetainedHayesPreopen, saved: &Transfer) -> Result<()> {
        let (_, transition) = self.furnishing_transition_trial.as_ref().ok_or_else(denied)?;
        if self.job_id.is_some() || self.transition_fence.is_none()
            || self.retained_hayes_trial.is_some()
            || !crate::native_payload::r285_hayes_preopen_terminal_transition((transition.source().2, transition.target().2))
        {
            return Err(denied());
        }
        let intent = saved.intent.as_ref().ok_or_else(denied)?;
        let input = proof.input()?;
        let query = claim_query(&input, intent, (&saved.workspace_id, &saved.tenant_id, &saved.principal_id))?;
        self.retained_hayes_trial = Some(Trial { database_identity: proof.database_identity()?, claim: proof.claim()?,
            query, input, custody: None });
        Ok(())
    }

    pub(super) fn check_hayes_preopen_parent(&mut self) -> Result<()> {
        let trial = self.retained_hayes_trial.as_ref().ok_or_else(denied)?;
        check_failed_run(&self.service.root, &trial.input)?;
        let expected_identity = trial.database_identity.clone();
        let query = trial.query.clone();
        let deadline = Instant::now() + Duration::from_secs(60);
        if self.query_database_before("exhibitos", DATABASE_IDENTITY_SQL, deadline)? != expected_identity {
            return Err(denied());
        }
        let ledger = self.payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::CombinedThumbnails052)?;
        let (parent, terminal) = ledger.rsplit_once('\n').ok_or_else(denied)?;
        if terminal != format!("054_hayes_preopen_terminal.sql|{}", crate::native_payload::HAYES_PREOPEN_TERMINAL_SCHEMA.2)
            || self.query_database_before("exhibitos", "SELECT name,sha256 FROM public.schema_migrations ORDER BY name", deadline)? != parent
        {
            return Err(denied());
        }
        let row = self.query_database_before("exhibitos", &query, deadline)?;
        let observed: Claim = serde_json::from_str(&row).map_err(|_| denied())?;
        if observed != self.retained_hayes_trial.as_ref().ok_or_else(denied)?.claim {
            return Err(denied());
        }
        Ok(())
    }

    pub(crate) fn observe_hayes_preopen(&mut self, input: &Input, saved: &Transfer, draft: &DocumentState) -> Result<Value> {
        if !self.closing_hayes_binding_available() { return Err(denied()); }
        self.observe_hayes_preopen_claim(input, saved, draft, None)
    }

    pub(crate) fn verify_retained_hayes_preopen(&mut self, proof: &RetainedHayesPreopen, saved: &Transfer, draft: &DocumentState) -> Result<()> {
        let custody = self.retained_hayes_trial.as_mut().and_then(|trial| trial.custody.take()).ok_or_else(denied)?;
        let result = self.verify_retained_hayes_preopen_with_lease(proof, saved, draft, &custody);
        self.retained_hayes_trial.as_mut().ok_or_else(denied)?.custody = Some(custody);
        result
    }

    pub(super) fn verify_retained_hayes_preopen_with_lease(&mut self, proof: &RetainedHayesPreopen, saved: &Transfer, draft: &DocumentState, lease: &super::hayes::Lease) -> Result<()> {
        self.verify_workspace_furnishing_ledger()?;
        let observed = self.observe_hayes_preopen_claim(&proof.input()?, saved, draft, Some(lease))?;
        if observed != proof.observation { return Err(denied()); }
        Ok(())
    }

    pub(super) fn pin_hayes_preopen_custody(&self, expected: &str) -> Result<super::hayes::Lease> {
        let auth = self.local_authority.as_ref().ok_or_else(denied)?;
        let lease = provision::open_user_lease(Some(auth))?;
        lease.check_preopen_custody(expected)?;
        Ok(lease)
    }

    pub(super) fn hold_hayes_preopen_trial_custody(&mut self) -> Result<()> {
        if let Some(trial) = &self.retained_hayes_trial {
            check_failed_run(&self.service.root, &trial.input)?;
            let custody = self.pin_hayes_preopen_custody(&trial.input.custody_authority_sha256)?;
            self.retained_hayes_trial.as_mut().ok_or_else(denied)?.custody = Some(custody);
        }
        Ok(())
    }

    fn observe_hayes_preopen_claim(&mut self, input: &Input, saved: &Transfer, draft: &DocumentState, lease: Option<&super::hayes::Lease>) -> Result<Value> {
        if !self.status()?.readiness.ready || self.retained_hayes.is_some()
            || input.job_id.is_nil() || input.failed_generation.is_nil()
            || self.job_id.as_deref() == Some(input.failed_generation.to_string().as_str())
            || ![&input.failed_record_sha256, &input.transfer_sha256, &input.custody_authority_sha256].iter().all(|s| hash_valid(s))
            || draft.revision != input.saved_revision || draft.etag.as_deref() != Some(input.saved_etag.as_str())
            || draft.draft.is_none() || saved.job_id.as_deref() != Some(input.job_id.to_string().as_str())
            || transfer_digest(saved)? != input.transfer_sha256 {
            return Err(denied());
        }
        installations::check_personal_transition_document(draft, false, || Err(denied()))?;
        let intent = saved.intent.as_ref().ok_or_else(denied)?;
        let hayes = intent.hayes_original.as_ref().ok_or_else(denied)?;
        let auth = self.local_authority.as_ref().ok_or_else(denied)?;
        let owner = [&auth.workspace_id, &auth.tenant_id, &auth.principal_id];
        if owner != [&saved.workspace_id, &saved.tenant_id, &saved.principal_id]
            || owner != [&draft.workspace_id, &draft.tenant_id, &draft.principal_id]
            || !owner.iter().all(|id| installations::uuid(id))
            || !installations::uuid(&intent.artwork_id) || !hash_valid(&hayes.display_reference_sha256)
            || hayes.artwork_revision != 1 {
            return Err(denied());
        }
        check_failed_run(&self.service.root, input)?;
        let acquired;
        let lease = if let Some(lease) = lease { lease } else {
            acquired = self.pin_hayes_preopen_custody(&input.custody_authority_sha256)?;
            &acquired
        };
        lease.check_preopen_custody(&input.custody_authority_sha256)?;
        let sql = claim_query(input, intent, (&auth.workspace_id, &auth.tenant_id, &auth.principal_id))?;
        let physical = Owner::capture(self)?;
        let snapshot = self.query_database_before("exhibitos", &sql, Instant::now() + Duration::from_secs(12))?;
        let claim: Claim = serde_json::from_str(&snapshot).map_err(|_| denied())?;
        physical.check_database_identity_result(&format!("{}|{}", claim.system_id, claim.database_oid))?;
        if claim.generation != input.failed_generation || claim.attempt_id.is_nil() || claim.lease_id.is_nil()
            || claim.correlation_id.is_nil() || !claim.lease_expired {
            return Err(denied());
        }
        lease.check_preopen_custody(&input.custody_authority_sha256)?;
        check_failed_run(&self.service.root, input)?;
        physical.check(self)?;
        Ok(serde_json::json!({
            "kind":"hayes-preopen-claim-observation", "version":1, "jobId":input.job_id,
            "artworkId":intent.artwork_id, "artworkRevision":hayes.artwork_revision,
            "workspaceId":saved.workspace_id, "tenantId":saved.tenant_id, "principalId":saved.principal_id,
            "claim":claim, "state":"processing", "attempts":1, "progress":25,
            "outputStagePresent":false, "dispositionPresent":false, "releasePresent":false, "openJournalPresent":false,
            "custodyAuthoritySha256":input.custody_authority_sha256,"failedRecordSha256":input.failed_record_sha256,
            "transferSha256":input.transfer_sha256,"savedEtag":input.saved_etag,"savedRevision":input.saved_revision,
        }))
    }
}

fn check_failed_run(root: &Path, input: &Input) -> Result<()> {
    let relative = format!("native-runs/{}/state.json", input.failed_generation);
    let record = installation_backup::source_bytes(root, &relative, 8192, true)?;
    if digest(&record) != input.failed_record_sha256 { return Err(denied()); }
    let (context, completion) = read_run(root, &input.failed_generation.to_string())?;
    if context.state != "stopped" || context.readiness.ready
        || context.postgres_pid.is_some() || context.node_pid.is_some() || context.auxiliary_pid.is_some()
        || !["auxiliary", "node", "postgres"].iter().all(|key| context.exits.get(*key) == Some(&0))
        || !completion.is_some_and(|entry| hash_valid(&entry.binding_sha256)
            && entry.consumed_by.as_ref().is_some_and(|id| installations::uuid(id))) {
        return Err(denied());
    }
    Ok(())
}

impl RetainedHayesPreopen {
    fn claim(&self) -> Result<Claim> {
        serde_json::from_value(self.observation["claim"].clone()).map_err(|_| denied())
    }

    pub(super) fn database_identity(&self) -> Result<String> {
        let claim = self.claim()?;
        let canonical_positive = |raw: &str, value: u64| value != 0 && value.to_string() == raw;
        let system = claim.system_id.parse::<u64>().map_err(|_| denied())?;
        let oid = claim.database_oid.parse::<u32>().map_err(|_| denied())?;
        if !claim.lease_expired || !canonical_positive(&claim.system_id, system)
            || !canonical_positive(&claim.database_oid, u64::from(oid))
        {
            return Err(denied());
        }
        Ok(format!("{}|{}", claim.system_id, claim.database_oid))
    }
}

fn claim_query(input: &Input, intent: &installations::native_upload::Intent, owner: (&str, &str, &str)) -> Result<String> {
    let hayes = intent.hayes_original.as_ref().ok_or_else(denied)?;
    let (workspace, tenant, principal) = owner;
    if ![workspace, tenant, principal, &intent.artwork_id].into_iter().all(installations::uuid)
        || !hash_valid(&hayes.display_reference_sha256) {
        return Err(denied());
    }
    let rights = serde_json::to_vec(&intent.rights).map_err(|_| denied())?
        .iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(format!(
        "SELECT json_build_object('systemId',db.system_id,'databaseOid',db.database_oid,
                    'attemptId',a.id,'leaseId',a.lease_id,'correlationId',a.correlation_id,
                    'generation',a.runtime_generation,'leaseUntil',j.lease_until,'leaseExpired',j.lease_until<=clock_timestamp())
         FROM ({DATABASE_IDENTITY_SQL}) db CROSS JOIN public.import_jobs j
         JOIN public.hayes_original_attempts a ON (a.tenant_id,a.job_id,a.id,a.lease_id,a.artwork_id,a.principal_id)
                =(j.tenant_id,j.id,j.hayes_attempt_id,j.lease_id,j.artwork_id,j.user_id)
         JOIN public.artworks w ON (w.tenant_id,w.id)=(j.tenant_id,j.artwork_id)
         JOIN public.local_workspaces lw ON (lw.id,lw.tenant_id,lw.user_id)=(a.workspace_id,a.tenant_id,a.principal_id)
         WHERE (a.tenant_id,a.workspace_id,a.principal_id)=('{tenant}','{workspace}','{principal}')
           AND (j.id,a.artwork_id,a.runtime_generation)=('{job}','{artwork}','{generation}')
           AND a.artwork_revision=1 AND w.revision=a.artwork_revision AND w.deleted_at IS NULL
           AND w.approved_revision IS NULL AND w.approved_asset_id IS NULL
           AND (a.source_sha256,a.source_bytes)=(j.expected_hash,j.expected_bytes)
           AND a.source_sha256='e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6' AND a.source_bytes=8383096
           AND j.object_key='{tenant}/quarantine/import-{job}/'||a.source_sha256
           AND j.mime='model/gltf-binary' AND j.derived IS NULL AND j.model_derivation IS NULL
           AND j.rights=w.metadata->'rights' AND j.rights=convert_from(decode('{rights}','hex'),'UTF8')::jsonb
           AND w.metadata->'displayReference'->>'status'='provisional-uncalibrated'
           AND w.metadata->'displayReference'->>'assetSha256'='{display}'
           AND w.metadata->'provenance'->>'sourceUnits'='unknown'
           AND w.metadata->'provenance'->'scaleApplied'='false'::jsonb
           AND j.state='processing' AND j.attempts=1 AND j.progress=25 AND j.lease_until IS NOT NULL
           AND j.asset_id IS NULL AND j.original_asset_id IS NULL AND j.hayes_disposition_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM public.hayes_output_stages s WHERE s.tenant_id=j.tenant_id AND (s.job_id=j.id OR s.attempt_id=a.id))
           AND NOT EXISTS (SELECT 1 FROM public.hayes_original_dispositions d WHERE d.tenant_id=j.tenant_id AND (d.job_id=j.id OR d.attempt_id=a.id))
           AND NOT EXISTS (SELECT 1 FROM public.hayes_native_releases r WHERE r.tenant_id=j.tenant_id AND (r.job_id=j.id OR r.attempt_id=a.id))",
        tenant=tenant, workspace=workspace, principal=principal,
        job=input.job_id, artwork=intent.artwork_id, generation=input.failed_generation, display=hayes.display_reference_sha256,
    ))
}
