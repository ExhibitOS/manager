// SPDX-License-Identifier: Apache-2.0
//! A pre-Node cancellation shape is eligible for authenticated inspection only.
//! This policy does not publish/consume a binding or grant service/recovery admission.
use super::*;
use super::owner_inspection_sql::Owner;

const ERROR: &str = "NATIVE_OWNER_INSPECTION_SOURCE_INVALID";
const SOURCE: (&str, &str, &str) = (
    "3ea221c2ea0d4f419c6625ff63e7405a721de095",
    "6b133c8f1372b8d886d08c894b43a17d8300fc7577db4b430874d8568557bbec",
    "ad6cd9b1ae8941ce99eadb9673c7e4a6c72689aaedd628bcbe33e623eb524c13",
);

fn quiet(record: &Context) -> bool {
    record.format == 1 && !record.log_truncated && record.postgres_pid.is_none() && record.node_pid.is_none()
        && record.auxiliary_pid.is_none() && !record.readiness.ready
        && record.readiness.version.is_none() && record.readiness.protocol_version.is_none()
        && record.readiness.error_code.is_none()
}

/// Must be combined with held profile/installation/operation/writer locks,
/// pinned files and a fresh no-live/port/postmaster check before any owned PG launch.
/// Exact stopped predecessor evidence is compared to its retained completion;
/// it is never presented as a SQL workspace-generation claim.
pub(super) fn check_shape(current: &Current, failed: &Context, failed_completion: Option<&FurnishingCompletion>,
    predecessor: &Binding, stopped: &Context, completion: &FurnishingCompletion,
    owner: &Owner, physical: furnishing_lifetime::WriterIdentity, owner_uid: u32,
) -> Result<()> {
    let binding = current.furnishing.as_ref().ok_or_else(|| err(ERROR))?;
    if current.format != 1 || current.id != binding.generation
        || !installations::uuid(&current.id) || binding.version != 1
        || binding.phase != Phase::Active || binding.admission_kind != Admission::CleanReopen
        || binding.catalog != 45 || binding.restore.is_some() || !binding.interrupted_claim_generations.is_empty()
        || (binding.source_commit.as_str(), binding.producer_sha256.as_str(), binding.payload_manifest_sha256.as_str()) != SOURCE
        || binding.physical != physical || owner_uid == 0 || owner.owner_uid != owner_uid
        || binding.owner.workspace_id != owner.workspace_id || binding.owner.tenant_id != owner.tenant_id
        || binding.owner.principal_id != owner.principal_id || binding.owner.owner_uid != owner_uid
        || [&owner.workspace_id, &owner.tenant_id, &owner.principal_id].iter().any(|id| !installations::uuid(id))
        || !quiet(failed) || failed.job_id.as_deref() != Some(current.id.as_str())
        || failed.state != "failed" || failed.error_code.as_deref() != Some("NATIVE_CANCELLED")
        || failed.failure_code.as_deref() != Some("NATIVE_CANCELLED") || failed_completion.is_some()
        || failed.exits.len() != 1 || failed.exits.get("postgres") != Some(&-15)
        || failed.ports.validate().is_err() || failed.ports != stopped.ports
        || predecessor.version != 1 || predecessor.phase != Phase::Clean
        || predecessor.admission_kind != Admission::CleanReopen || predecessor.restore.is_some()
        || !predecessor.interrupted_claim_generations.is_empty() || !predecessor.same_installation(binding)
        || binding.previous_generation.as_deref() != Some(predecessor.generation.as_str())
        || !installations::uuid(&predecessor.generation) || predecessor.generation == current.id
        || !predecessor.previous_generation.as_ref().is_some_and(|id| installations::uuid(id) && id != &predecessor.generation)
        || !quiet(stopped) || stopped.job_id.as_deref() != Some(predecessor.generation.as_str())
        || stopped.state != "stopped" || stopped.error_code.is_some() || stopped.failure_code.is_some()
        || stopped.exits.len() != 3 || stopped.exits.values().any(|code| *code != 0)
        || ["postgres", "node", "auxiliary"].iter().any(|name| !stopped.exits.contains_key(*name))
        || completion.consumed_by.as_deref() != Some(current.id.as_str())
        || completion.binding_sha256 != digest(&serde_json::to_vec(predecessor).map_err(|_| err(ERROR))?)
    { return Err(err(ERROR)); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Current, Context, Binding, Context, FurnishingCompletion, Owner, furnishing_lifetime::WriterIdentity) {
        let owner = Owner { workspace_id:"11111111-1111-4111-8111-111111111111".into(),
            tenant_id:"22222222-2222-4222-8222-222222222222".into(),
            principal_id:"33333333-3333-4333-8333-333333333333".into(), owner_uid:501 };
        let physical = furnishing_lifetime::WriterIdentity { root_device:1,root_inode:2,lock_device:1,lock_inode:3 };
        let previous = "44444444-4444-4444-8444-444444444444";
        let generation = "55555555-5555-4555-8555-555555555555";
        let predecessor = Binding { version:1,phase:Phase::Clean,generation:previous.into(),
            previous_generation:Some("66666666-6666-4666-8666-666666666666".into()),
            admission_kind:Admission::CleanReopen,
            owner:furnishing_lifecycle::Owner { workspace_id:owner.workspace_id.clone(),tenant_id:owner.tenant_id.clone(),
                principal_id:owner.principal_id.clone(),owner_uid:501 },
            source_commit:SOURCE.0.into(),producer_sha256:SOURCE.1.into(),payload_manifest_sha256:SOURCE.2.into(),
            catalog:45,physical,restore:None,interrupted_claim_generations:vec![] };
        let mut binding = predecessor.clone();binding.phase=Phase::Active;binding.generation=generation.into();
        binding.previous_generation=Some(previous.into());
        let current = Current { format:1,id:generation.into(),furnishing:Some(binding) };
        let ports = Ports { web:18091,api:18092,database:18093 };
        let failed = Context { format:1,job_id:Some(generation.into()),state:"failed".into(),ports,
            readiness:Readiness::default(),error_code:Some("NATIVE_CANCELLED".into()),failure_code:Some("NATIVE_CANCELLED".into()),
            postgres_pid:None,node_pid:None,auxiliary_pid:None,exits:BTreeMap::from([("postgres".into(),-15)]),log_truncated:false };
        let stopped = Context { format:1,job_id:Some(previous.into()),state:"stopped".into(),ports,
            readiness:Readiness::default(),error_code:None,failure_code:None,postgres_pid:None,node_pid:None,auxiliary_pid:None,
            exits:BTreeMap::from([("postgres".into(),0),("node".into(),0),("auxiliary".into(),0)]),log_truncated:false };
        let completion = FurnishingCompletion { binding_sha256:digest(&serde_json::to_vec(&predecessor).unwrap()),consumed_by:Some(generation.into()) };
        (current,failed,predecessor,stopped,completion,owner,physical)
    }
    #[test]
    fn owner_inspection_source_accepts_exact_pre_node_cancelled_shape_only_for_inspection() {
        let (current,failed,predecessor,stopped,completion,owner,physical)=fixture();
        check_shape(&current,&failed,None,&predecessor,&stopped,&completion,&owner,physical,501).unwrap();
        assert!(check_shape(&current,&failed,Some(&completion),&predecessor,&stopped,&completion,&owner,physical,501).is_err());
    }
    #[test]
    fn owner_inspection_source_rejects_ready_node_pid_exit_error_and_port_variants() {
        for variant in 0..10 {
            let (current,mut failed,predecessor,stopped,completion,owner,physical)=fixture();
            match variant {
                0=>failed.readiness.ready=true,
                1=>failed.readiness.version=Some("1".into()),
                2=>failed.node_pid=Some(123),
                3=>failed.postgres_pid=Some(123),
                4=>{failed.exits.insert("node".into(),-15);},
                5=>{failed.exits.insert("postgres".into(),0);},
                6=>failed.failure_code=Some("OTHER".into()),
                7=>failed.error_code=None,
                8=>failed.ports.database+=1,
                _=>failed.log_truncated=true,
            }
            assert!(check_shape(&current,&failed,None,&predecessor,&stopped,&completion,&owner,physical,501).is_err(),"variant{variant}");
        }
    }
    #[test]
    fn owner_inspection_source_rejects_wrong_owner_physical_tuple_phase_and_consumed_evidence() {
        for variant in 0..10 {
            let (mut current,failed,predecessor,mut stopped,mut completion,mut owner,mut physical)=fixture();
            match variant {
                0=>owner.tenant_id="77777777-7777-4777-8777-777777777777".into(),
                1=>owner.owner_uid=502,
                2=>physical.root_inode+=1,
                3=>physical.lock_inode+=1,
                4=>current.furnishing.as_mut().unwrap().payload_manifest_sha256="a".repeat(64),
                5=>current.furnishing.as_mut().unwrap().phase=Phase::Clean,
                6=>current.furnishing.as_mut().unwrap().admission_kind=Admission::InterruptedReopen,
                7=>completion.consumed_by=None,
                8=>completion.binding_sha256="a".repeat(64),
                _=>{stopped.exits.insert("node".into(),-15);},
            }
            assert!(check_shape(&current,&failed,None,&predecessor,&stopped,&completion,&owner,physical,501).is_err(),"variant{variant}");
        }
    }
}
