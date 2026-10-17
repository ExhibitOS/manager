// SPDX-License-Identifier: Apache-2.0
//! Saved native references locate inputs; only a fresh owned preflight permits execution.
use super::*;
#[cfg(any(unix, windows))]
use crate::signed_release::trust::Store;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reference {
    pub format: u32,
    pub id: String,
    pub installation_id: String,
    pub scope: String,
    pub operation_id: String,
    pub generation: u64,
    pub registry_sha256: String,
    pub artifact: PathBuf,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub python: PathBuf,
    pub source_commit: String,
    pub maintenance_image: String,
    pub export_parent: PathBuf,
    pub host_archive: PathBuf,
    pub trust_archive: PathBuf,
    pub pair_binding: PathBuf,
    pub key_file: PathBuf,
    pub qualification_destination: PathBuf,
    pub qualification_sha256: String,
    pub created_at: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceSummary {
    pub reference_id: String,
    pub operation_id: String,
    pub generation: u64,
    pub version: String,
    pub source_schema: String,
    pub target_schema: String,
    pub changed_schema: bool,
    pub prior_qualification_recorded: bool,
    pub execution_requalification_required: bool,
}
pub(crate) fn load(profile: &Path, id: &str) -> Result<(Reference, Vec<u8>)> {
    if !installations::uuid(id) {
        return Err(err("UPDATE_REFERENCE_INVALID"));
    }
    let root = profile.join("update-execution-references");
    installations::private_directory(&root)?;
    let bytes = installation_backup::source_bytes(&root, &format!("{id}.json"), 16 * 1024, true)?;
    let value: Reference =
        serde_json::from_slice(&bytes).map_err(|_| err("UPDATE_REFERENCE_INVALID"))?;
    if value.format != 1
        || value.id != id
        || !installations::uuid(&value.installation_id)
        || !(value.scope == "default" || installations::uuid(&value.scope))
        || value.generation == 0
        || value.generation > 9_007_199_254_740_991
        || value.operation_id.is_empty()
        || value.operation_id.len() > 128
        || !value
            .operation_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        || ![
            &value.registry_sha256,
            &value.artifact_sha256,
            &value.qualification_sha256,
        ]
        .iter()
        .all(|s| hash_valid(s))
        || value.artifact_bytes == 0
        || value.artifact_bytes > 64 * 1024 * 1024 * 1024
        || value.created_at == 0
    {
        return Err(err("UPDATE_REFERENCE_INVALID"));
    }
    for path in [
        &value.artifact,
        &value.python,
        &value.export_parent,
        &value.host_archive,
        &value.trust_archive,
        &value.pair_binding,
        &value.key_file,
        &value.qualification_destination,
    ] {
        if !path.is_absolute() {
            return Err(err("UPDATE_REFERENCE_INVALID"));
        }
    }
    Ok((value, bytes))
}
pub(crate) fn bound(
    r: &Reference,
    installation: &str,
    scope: &str,
    registry: &[u8],
    plan: &update::Plan,
    generation: u64,
) -> Result<()> {
    if r.installation_id != installation
        || r.scope != scope
        || r.registry_sha256 != digest(registry)
        || r.operation_id != plan.operation_id
        || r.generation != generation
        || plan.source_instance != installation
    {
        return Err(err("UPDATE_REFERENCE_STALE"));
    }
    Ok(())
}
pub(crate) fn list(
    profile: &Path,
    installation: &str,
    scope: &str,
    registry: &[u8],
) -> Result<Vec<ReferenceSummary>> {
    let Some((plan, generation, version, sha, bytes)) =
        signed_release::trust::inspect_prepared_apply(profile, scope).map_err(|e| err(e.code()))?
    else {
        return Ok(vec![]);
    };
    let root = profile.join("update-execution-references");
    if !root.exists() {
        return Ok(vec![]);
    }
    installations::private_directory(&root)?;
    let mut entries = vec![];
    for (n, item) in fs::read_dir(&root)
        .map_err(|_| err("UPDATE_REFERENCE_INVALID"))?
        .enumerate()
    {
        if n >= 128 {
            return Err(err("UPDATE_REFERENCE_QUOTA"));
        }
        let name = item
            .map_err(|_| err("UPDATE_REFERENCE_INVALID"))?
            .file_name();
        let name = name
            .to_str()
            .ok_or_else(|| err("UPDATE_REFERENCE_INVALID"))?;
        let id = name
            .strip_suffix(".json")
            .ok_or_else(|| err("UPDATE_REFERENCE_INVALID"))?;
        let (r, _) = load(profile, id)?;
        if bound(&r, installation, scope, registry, &plan, generation).is_err() {
            continue;
        }
        if r.artifact_sha256 != sha || r.artifact_bytes != bytes {
            return Err(err("UPDATE_REFERENCE_INVALID"));
        }
        entries.push(ReferenceSummary {
            reference_id: r.id,
            operation_id: r.operation_id,
            generation: r.generation,
            version: version.clone(),
            source_schema: plan.source_schema.clone(),
            target_schema: plan.target_schema.clone(),
            changed_schema: plan.source_schema != plan.target_schema,
            prior_qualification_recorded: true,
            execution_requalification_required: true,
        });
    }
    entries.sort_by(|a, b| a.reference_id.cmp(&b.reference_id));
    Ok(entries)
}
#[cfg(any(unix, windows))]
pub(crate) fn apply(
    profile: &Path,
    r: &Reference,
    store: &mut Store,
    external_writers_quiesced: bool,
) -> Result<()> {
    if !external_writers_quiesced {
        return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
    }
    let plan = store
        .intent()
        .ok_or_else(|| err("UPDATE_INTENT_MISSING"))?
        .update()
        .plan();
    if store.intent().unwrap().update().stage() != update::Stage::Prepared
        || store.receipt().write_uncertain
    {
        return Err(err("UPDATE_REFERENCE_STALE"));
    }
    let (registry, bytes) =
        installations::load(profile)?.ok_or_else(|| err("UPDATE_REFERENCE_STALE"))?;
    if registry.active_id != r.installation_id {
        return Err(err("UPDATE_REFERENCE_STALE"));
    }
    bound(
        r,
        &registry.active_id,
        &r.scope,
        &bytes,
        plan,
        store.receipt().generation,
    )?;
    // Same-schema is the existing public execution boundary; no migration admission is invented here.
    if plan.source_schema != plan.target_schema {
        return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));
    }
    store
        .verify_authority_recovery()
        .map_err(|e| err(e.code()))?;
    let mut key = profile_backup::external_key(profile, &r.key_file)?;
    let result = (|| -> Result<()> {
        let array: [u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| err("UPDATE_KEY_INVALID"))?;
        let mut array = array;
        let result = (|| -> Result<()> {
            let checkpoint = signed_release::trust::CheckpointInputs {
                binding: &r.pair_binding,
                host: &r.host_archive,
                trust: &r.trust_archive,
                key: &array,
            };
            let proof = store
                .verify_checkpoint_pair(
                    checkpoint.binding,
                    checkpoint.host,
                    checkpoint.trust,
                    checkpoint.key,
                )
                .map_err(|e| err(&e.code))?;
            if !proof.receipt().source_plan_bound {
                return Err(err("UPDATE_RECOVERY_CHECKPOINT_UNBOUND"));
            }
            let mut session = store.execution()?;
            // Consent is separate from live engine observation, including foreign volume writers.
            session.verify_source_stopped(external_writers_quiesced)?;
            let parent = r
                .artifact
                .parent()
                .ok_or_else(|| err("UPDATE_REFERENCE_INVALID"))?;
            let mut artifact = session.stage_prepared_artifact(&r.artifact, parent)?;
            let inputs = signed_release::trust::RecoveryRuntimeInputs {
                checkpoint,
                export_parent: &r.export_parent,
                python: &r.python,
                source_commit: &r.source_commit,
                maintenance_image: &r.maintenance_image,
                external_writers_quiesced,
            };
            // Registration's extraction already exists. Repeat actual qualification into a NEW sibling.
            let destination = r
                .qualification_destination
                .parent()
                .ok_or_else(|| err("UPDATE_REFERENCE_INVALID"))?
                .join(format!("manager-apply-{}", Uuid::new_v4()));
            let admitted =
                session.prepare_owned_update(&mut artifact, &inputs, &destination, &r.key_file)?;
            admitted.begin()?.apply_candidate()?.activate()?;
            Ok(())
        })();
        array.fill(0);
        result
    })();
    key.fill(0);
    result
}
