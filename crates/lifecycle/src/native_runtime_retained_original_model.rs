// SPDX-License-Identifier: Apache-2.0
//! Preserve a terminal, drained original-model import across one clean transition.
use super::*;
use crate::installations::native_maintenance::RetainedOriginalModel;

const ERROR: &str = "NATIVE_RETAINED_ORIGINAL_MODEL_CHANGED";
pub(super) struct Trial { proof: RetainedOriginalModel, before: Option<Value> }

fn custody_query(owner: &FurnishingOwner, proof: &RetainedOriginalModel) -> Result<String> {
    if !proof.valid() || ![&owner.workspace_id, &owner.tenant_id, &owner.principal_id]
        .into_iter().all(|id| oex_preflight_frames::uuid(id)) { return Err(err(ERROR)); }
    let tenant = &owner.tenant_id; let principal = &owner.principal_id;
    let job = &proof.job_id; let artwork = &proof.artwork_id;
    let upload = &proof.upload_idempotency_key; let generation = &proof.generation;
    let source = &proof.source_sha256; let bytes = proof.source_bytes;
    let receipt = &proof.receipt_sha256; let rights = &proof.rights_sha256; let revision = proof.artwork_revision; let scale = proof.scale_meters;
    Ok(format!(r#"
SELECT jsonb_build_object(
 'receipt',j.model_preparation->'result',
 'custodySha256',encode(sha256(convert_to(jsonb_build_array(
  to_jsonb(j),to_jsonb(a),to_jsonb(rev),
  (SELECT jsonb_agg(jsonb_build_array(to_jsonb(asset),to_jsonb(rights),
    (SELECT jsonb_agg(to_jsonb(d) ORDER BY d.id) FROM public.asset_derivatives d
     WHERE (d.tenant_id,d.asset_id)=(asset.tenant_id,asset.id))) ORDER BY asset.id)
   FROM public.assets asset JOIN public.rights rights ON (rights.tenant_id,rights.id)=(asset.tenant_id,asset.rights_id)
   WHERE (asset.tenant_id,asset.artwork_id)=(a.tenant_id,a.id)),
  (SELECT jsonb_agg(to_jsonb(e) ORDER BY e.id) FROM public.exhibitions e WHERE e.tenant_id=j.tenant_id)
 )::text,'UTF8')),'hex'))
FROM public.import_jobs j
JOIN public.artworks a ON (a.tenant_id,a.id)=(j.tenant_id,j.artwork_id)
JOIN public.artwork_revisions rev ON (rev.tenant_id,rev.artwork_id,rev.revision)=(a.tenant_id,a.id,a.revision)
JOIN public.assets original ON (original.tenant_id,original.id)=(j.tenant_id,COALESCE(j.original_asset_id,j.asset_id))
JOIN public.assets display ON (display.tenant_id,display.id)=(j.tenant_id,j.asset_id)
JOIN public.rights original_rights ON (original_rights.tenant_id,original_rights.id)=(original.tenant_id,original.rights_id)
JOIN public.rights display_rights ON (display_rights.tenant_id,display_rights.id)=(display.tenant_id,display.rights_id)
WHERE j.tenant_id='{tenant}' AND j.user_id='{principal}' AND j.id='{job}' AND j.artwork_id='{artwork}'
 AND a.revision={revision} AND a.approved_revision IS NULL AND a.approved_asset_id IS NULL AND a.deleted_at IS NULL
 AND rev.snapshot=a.metadata AND a.metadata->'rights'=j.rights AND j.rights#>>'{{permissions,display}}'='true'
 AND NOT EXISTS(SELECT 1 FROM public.artwork_approvals approval WHERE approval.tenant_id=a.tenant_id AND approval.artwork_id=a.id)
 AND NOT EXISTS(SELECT 1 FROM public.exhibitions e,
  LATERAL jsonb_array_elements(COALESCE(e.metadata#>'{{candidate,artworks}}','[]'::jsonb)) item
  WHERE e.tenant_id=j.tenant_id AND e.deleted_at IS NULL AND item->>'id'='{artwork}')
 AND j.state='approved' AND j.progress=100 AND j.error_code IS NULL
 AND j.png_drain_version=1 AND j.lease_id IS NULL AND j.lease_until IS NULL AND cardinality(j.png_worker_tokens)=0
 AND j.mime='model/gltf-binary' AND j.derived IS NULL AND j.derived_object_key IS NULL AND j.model_derivation IS NULL
 AND j.idempotency_key='{upload}' AND j.expected_hash='{source}' AND j.expected_bytes={bytes} AND j.scale_meters={scale}
 AND j.model_preparation->>'kind'='required-model-compact' AND NOT(j.model_preparation ? 'claim')
 AND j.model_preparation#>>'{{request,profileId}}'='embedded-model-compact-20k-512-v1'
 AND j.model_preparation#>>'{{request,operationID}}'='{upload}' AND j.model_preparation#>>'{{request,generation}}'='{generation}'
 AND j.model_preparation#>>'{{request,artworkRevision}}'='{revision}' AND NOT(j.model_preparation->'request' ? 'sourceFull')
 AND j.model_preparation#>>'{{result,sha256}}'='{receipt}' AND j.model_preparation#>>'{{result,jobId}}'='{job}'
 AND j.model_preparation#>>'{{result,artworkId}}'='{artwork}' AND j.model_preparation#>>'{{result,artworkRevision}}'='{revision}'
 AND j.model_preparation#>>'{{result,operationID}}'='{upload}' AND j.model_preparation#>>'{{result,generation}}'='{generation}'
 AND j.model_preparation#>>'{{result,profileId}}'='embedded-model-compact-20k-512-v1'
 AND j.model_preparation#>>'{{result,rightsSHA256}}'='{rights}'
 AND j.model_preparation#>>'{{result,original,assetId}}'=original.id::text
 AND j.model_preparation#>>'{{result,display,assetId}}'=display.id::text
 AND j.model_preparation#>>'{{result,original,sha256}}'='{source}' AND j.model_preparation#>>'{{result,original,bytes}}'='{bytes}'
 AND j.model_preparation#>>'{{result,display,sha256}}'=display.sha256
 AND j.model_preparation#>>'{{result,display,bytes}}'=display.bytes::text
 AND original.artwork_id=a.id AND display.artwork_id=a.id AND original.mime='model/gltf-binary' AND display.mime='model/gltf-binary'
 AND original.sha256='{source}' AND original.bytes={bytes} AND original.scale_meters={scale} AND display.scale_meters={scale}
 AND display.state='approved' AND original.state=CASE WHEN original.id=display.id THEN 'approved' ELSE 'quarantine' END
 AND original.deleted_at IS NULL AND display.deleted_at IS NULL AND original_rights.deleted_at IS NULL AND display_rights.deleted_at IS NULL
 AND original_rights.metadata=j.rights AND display_rights.metadata=j.rights
 AND j.object_key='{tenant}/quarantine/import-{job}/{source}'
 AND display.object_key=j.approved_key AND j.approved_key='{tenant}/approved/'||display.sha256
 AND original.object_key=CASE WHEN original.id=display.id THEN j.approved_key ELSE j.object_key END
 AND (SELECT count(*) FROM public.asset_derivatives d WHERE d.tenant_id=j.tenant_id AND d.asset_id=original.id
      AND d.metadata->>'kind'='required-model-preparation' AND d.deleted_at IS NULL)=1
 AND EXISTS(SELECT 1 FROM public.asset_derivatives d WHERE d.tenant_id=j.tenant_id AND d.asset_id=original.id AND d.deleted_at IS NULL
  AND d.metadata=jsonb_build_object('version',1,'kind','required-model-preparation','originalAssetId',original.id::text,
   'displayAssetId',display.id::text,'serverReceipt',j.model_preparation->'result','provenance','server-generated'))
"#))
}

impl Session {
    pub(crate) fn arm_retained_original_model(&mut self, proof: RetainedOriginalModel) -> Result<()> {
        let (_, transition) = self.furnishing_transition_trial.as_ref().ok_or_else(|| err(ERROR))?;
        if !proof.valid() || self.job_id.is_some() || self.transition_fence.is_none()
            || self.retained_original_model.is_some() || self.retained_product_image.is_some()
            || self.startup_cancellation_045.is_some() || self.oex_retained_cleanup.is_some()
            || !crate::native_payload::current_image_thumbnail_transition((transition.source().2,transition.target().2)) {
            return Err(err(ERROR));
        }
        self.retained_original_model=Some(Trial { proof,before:None }); Ok(())
    }
    fn retained_original_model_custody(&mut self) -> Result<Value> {
        let trial=self.retained_original_model.as_ref().ok_or_else(|| err(ERROR))?;
        let owner=&self.active_furnishing_binding().ok_or_else(|| err(ERROR))?.owner;
        let sql=custody_query(owner,&trial.proof)?;
        let row=self.query_database("exhibitos",&sql)?;
        let value: Value=serde_json::from_str(&row).map_err(|_|err(ERROR))?;
        if !value["custodySha256"].as_str().is_some_and(hash_valid)||!value["receipt"].is_object(){return Err(err(ERROR));}
        Ok(value)
    }
    pub(super) fn check_retained_original_model_before_node(&mut self) -> Result<()> {
        if self.retained_original_model.is_some(){
            let before=self.retained_original_model_custody()?;
            self.retained_original_model.as_mut().ok_or_else(||err(ERROR))?.before=Some(before);
        }
        Ok(())
    }
    pub(crate) fn verify_retained_original_model(&mut self,authoring:&Value)->Result<Value>{
        let before=self.retained_original_model_custody()?;
        let trial=self.retained_original_model.as_ref().ok_or_else(||err(ERROR))?;
        if trial.before.as_ref()!=Some(&before){return Err(err(ERROR));}
        let mut input=serde_json::to_vec(&serde_json::json!({"origin":authoring["origin"],"tenant":authoring["tenantId"],
            "token":authoring["token"],"job":trial.proof.job_id,"receipt":before["receipt"]})).map_err(|_|err(ERROR))?;
        // Normal issued native-session import readback validates the complete
        // canonical server receipt and current asset/rights/derivative context.
        let script=r#"
const {readFileSync}=require('node:fs');const {isDeepStrictEqual}=require('node:util');
(async()=>{const {origin,tenant,token,job,receipt}=JSON.parse(readFileSync(0,'utf8'));
const response=await fetch(origin+'/api/v1/tenants/'+tenant+'/imports/'+job,{redirect:'error',signal:AbortSignal.timeout(7000),
 headers:{origin,accept:'application/json',cookie:'exhibitos_local_session='+token}});
if(!response.ok)return false;const chunks=[];let bytes=0;
for await(const chunk of response.body){bytes+=chunk.length;if(bytes>32768)return false;chunks.push(chunk);}
const value=JSON.parse(Buffer.concat(chunks).toString('utf8')),model=value.modelPreparation;
return value.id===job&&value.state==='approved'&&model?.required===true&&model.status==='ready'
 &&model.workerDrained===true&&isDeepStrictEqual(model.serverReceipt,receipt);
})().then(ok=>process.stdout.write(ok?'ok':'changed'),()=>process.stdout.write('changed'));
"#;
        let result=self.auxiliary_timed(self.payload.node(),vec!["-e".into(),script.into()],vec![],Some(&input),Duration::from_secs(15));
        input.fill(0);
        if !matches!(result,Ok((0,ref output))if output==b"ok") || self.retained_original_model_custody()?!=before{return Err(err(ERROR));}
        Ok(before["receipt"].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> FurnishingOwner {
        FurnishingOwner {workspace_id:"10000000-0000-4000-8000-000000000001".into(),
            tenant_id:"10000000-0000-4000-8000-000000000002".into(),
            principal_id:"10000000-0000-4000-8000-000000000003".into(),owner_uid:501}
    }
    #[test]
    fn retained_original_query_rejects_open_or_foreign_input_before_sql() {
        let mut proof=RetainedOriginalModel::synthetic();
        assert!(custody_query(&owner(),&proof).is_ok());
        proof.generation="x';SELECT 1".into();assert!(custody_query(&owner(),&proof).is_err());
        proof=RetainedOriginalModel::synthetic();proof.scale_meters=f64::NAN;
        assert!(custody_query(&owner(),&proof).is_err());
        let mut foreign=owner();foreign.tenant_id="unbound".into();
        assert!(custody_query(&foreign,&RetainedOriginalModel::synthetic()).is_err());
    }
    #[test]
    #[ignore="requires an owned ephemeral synthetic PostgreSQL database; never a product profile"]
    fn retained_original_postgres_predicate_rejects_pending_and_custody_changes() {
        use std::io::Write;
        use std::process::{Command,Stdio};
        let psql=std::env::var_os("EXHIBITOS_TEST_RETAINED_MODEL_PSQL").expect("isolated psql");
        let dsn=std::env::var("EXHIBITOS_TEST_RETAINED_MODEL_DSN").expect("isolated connection");
        assert!(dsn.contains("dbname=retained_original_synthetic"));
        let query=custody_query(&owner(),&RetainedOriginalModel::synthetic()).unwrap();
        let evaluate=|mutation:&str| -> Option<Value> {
            let mut child=Command::new(&psql).args(["-X","-qAt","-v","ON_ERROR_STOP=1","-d",&dsn])
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            let sql=format!("BEGIN;\n{}\n{mutation}\n{query};\nROLLBACK;",include_str!("../tests/fixtures/retained_original_model.sql"));
            child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
            let output=child.wait_with_output().unwrap();
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            let text=String::from_utf8(output.stdout).unwrap();
            if text.trim().is_empty(){None}else{Some(serde_json::from_str(text.trim()).unwrap())}
        };
        let good=evaluate("").unwrap();assert_eq!(good["receipt"]["sha256"],"c".repeat(64));
        assert_eq!(Some(good.clone()),evaluate(""));
        for mutation in [
            "UPDATE import_jobs SET state='processing';",
            "UPDATE import_jobs SET png_worker_tokens=ARRAY['retained'];",
            "UPDATE import_jobs SET lease_id='10000000-0000-4000-8000-000000000012';",
            "UPDATE import_jobs SET lease_until=now();",
            "UPDATE import_jobs SET png_drain_version=0;",
            "UPDATE import_jobs SET user_id='10000000-0000-4000-8000-000000000012';",
            "UPDATE import_jobs SET model_preparation=jsonb_set(model_preparation,'{claim}','{}');",
            "UPDATE import_jobs SET model_preparation=jsonb_set(model_preparation,'{request,generation}','\"other\"');",
            "UPDATE import_jobs SET model_preparation=jsonb_set(model_preparation,'{result,sha256}','\"bad\"');",
            "UPDATE import_jobs SET rights='{}';",
            "UPDATE artworks SET revision=2;",
            "UPDATE artworks SET approved_revision=1;",
            "INSERT INTO artwork_approvals SELECT tenant_id,id FROM artworks;",
            "UPDATE exhibitions SET metadata=jsonb_set(metadata,'{candidate,artworks}','[{\"id\":\"10000000-0000-4000-8000-000000000005\"}]');",
            "UPDATE assets SET bytes=101 WHERE state='quarantine';",
            "UPDATE assets SET object_key='different' WHERE state='approved';",
            "UPDATE rights SET deleted_at=now();",
            "UPDATE asset_derivatives SET metadata=metadata-'serverReceipt';"
        ] {assert!(evaluate(mutation).is_none(),"accepted {mutation}");}
        let drift=evaluate("UPDATE exhibitions SET metadata=jsonb_set(metadata,'{editVersion}','100');").unwrap();
        assert_eq!(drift["receipt"],good["receipt"]);assert_ne!(drift["custodySha256"],good["custodySha256"]);
        // SQL pins receipt fields, while the mandatory normal GET supplies
        // canonical recomputation and current authorization/rights validity.
    }
}
