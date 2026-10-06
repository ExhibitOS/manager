// SPDX-License-Identifier: Apache-2.0
//! Trusted running-candidate inventory via qualified maintenance code, not Runtime packaging.
use crate::*;
use std::fs::File;
const READER: &str = include_str!("native_inventory_reader.mjs");
pub(in super::super) struct Inputs<'a> {
    pub image: &'a str,
    pub root: &'a Path,
    pub manifest: &'a BundleManifest,
    pub app: &'a Value,
    pub blob_volume: &'a str,
    pub expected_manifest: &'a str,
    pub expected_system: Option<&'a str>,
}
fn network_name(m: &BundleManifest, app: &Value) -> Result<String> {
    let n = format!("{}_default", m.project_name);
    let rows = app["NetworkSettings"]["Networks"]
        .as_object()
        .ok_or_else(|| err("UPDATE_INVENTORY_NETWORK_INVALID"))?;
    if rows.len() != 1
        || rows
            .get(&n)
            .is_none_or(|r| r["NetworkID"].as_str().is_none_or(|id| !hash_valid(id)))
    {
        return Err(err("UPDATE_INVENTORY_NETWORK_INVALID"));
    }
    Ok(n)
}
pub(in super::super) fn observe(
    inputs: &Inputs<'_>,
    input: File,
    mut check: impl FnMut() -> Result<()>,
) -> Result<super::source_inventory::InventoryProof> {
    let raw = observe_raw(inputs, input, READER, &[], &mut check, |raw| {
        serde_json::from_slice::<super::source_inventory::InventoryProof>(raw).map(|_|()).map_err(|_|err("UPDATE_INVENTORY_RESULT_INVALID"))
    })?;
    serde_json::from_slice(&raw).map_err(|_| err("UPDATE_INVENTORY_RESULT_INVALID"))
}
fn observe_raw(inputs: &Inputs<'_>, input: File, reader: &str, extra: &[String], mut check: impl FnMut() -> Result<()>, validate: impl FnOnce(&[u8]) -> Result<()>) -> Result<Vec<u8>> {
    let i = inputs;
    if !i.image.strip_prefix("sha256:").is_some_and(hash_valid)
        || !hash_valid(i.expected_manifest)
        || i.expected_system
            .is_some_and(|s| !super::source_database::valid_system_identifier(s))
        || i.blob_volume.is_empty()
        || !i
            .blob_volume
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(err("UPDATE_INVENTORY_INPUT_INVALID"));
    }
    check()?;
    if crate::backup_creation::local_image("docker", i.image)? != i.image {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let network = network_name(i.manifest, i.app)?;
    let n = crate::backup_creation::inspected(
        "docker",
        &["network".into(), "inspect".into(), network.clone()],
    )?;
    if n["Id"] != i.app["NetworkSettings"]["Networks"][&network]["NetworkID"]
        || n["Labels"]["com.exhibitos.bundle"] != i.manifest.bundle_id
        || n["Labels"]["com.exhibitos.project"] != i.manifest.project_name
        || n["Labels"]["com.exhibitos.schema"] != i.manifest.schema_version
    {
        return Err(err("UPDATE_INVENTORY_NETWORK_INVALID"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut args = vec![
        "create".into(),
        "--pull".into(),
        "never".into(),
        "--name".into(),
        format!("exhibitos-inventory-reader-{nonce}"),
        "--label".into(),
        format!("com.exhibitos.inventory.reader={nonce}"),
        "--network".into(),
        network,
        "--env-file".into(),
        i.root.join("runtime.env").to_string_lossy().into_owned(),
        "--env".into(),
        "BLOB_ROOT=/data/blobs".into(),
        "--user".into(),
        "1000:1000".into(),
        "--read-only".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--security-opt".into(),
        "no-new-privileges:true".into(),
        "--pids-limit".into(),
        "32".into(),
        "--memory".into(),
        "256m".into(),
        "--tmpfs".into(),
        "/var/lib/postgresql:rw,nosuid,nodev,size=1m".into(),
        "--mount".into(),
        format!(
            "type=volume,source={},target=/data/blobs,readonly",
            i.blob_volume
        ),
        "--entrypoint".into(),
        "node".into(),
        "--interactive".into(),
        i.image.into(),
        "--input-type=module".into(),
        "-e".into(),
        reader.into(),
        i.expected_manifest.into(),
    ];
    if let Some(system) = i.expected_system {
        args.push(system.into());
    }
    args.extend_from_slice(extra);
    let created = run_observed("docker", &args, None, 30, &mut check)?;
    let helper = std::str::from_utf8(&created)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
        .trim();
    if !hash_valid(helper) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let args = vec![
        "start".into(),
        "--attach".into(),
        "--interactive".into(),
        helper.into(),
    ];
    let raw = run_observed_input("docker", &args, None, 180, Some(input), &mut check)?;
    let stopped = crate::backup_creation::inspected("docker", &["inspect".into(), helper.into()])?;
    if stopped["State"]["Running"] != false
        || stopped["State"]["ExitCode"] != 0
        || stopped["Config"]["Labels"]["com.exhibitos.inventory.reader"] != nonce
        || stopped["Image"] != i.image
        || stopped["HostConfig"]["ReadonlyRootfs"] != true
        || stopped["Mounts"].as_array().is_none_or(|a| {
            a.len() != 1
                || a[0]["Name"] != i.blob_volume
                || a[0]["Type"] != "volume"
                || a[0]["Destination"] != "/data/blobs"
                || a[0]["RW"] != false
        })
    {
        return Err(err("UPDATE_INVENTORY_HELPER_INVALID"));
    }
    validate(&raw)?;
    check()?;
    run("docker", &["rm".into(), helper.into()], None, 30)?;
    Ok(raw)
}

pub(in super::super) enum Observation {
    Original(super::source_inventory::InventoryProof),
    Migrated(super::source_inventory::MigratedInventoryProof),
}
impl Observation {
    pub(in super::super) fn schema(&self) -> &str {
        match self { Self::Original(p) => &p.schema_sha256, Self::Migrated(p) => &p.target_schema_sha256 }
    }
}
/// Runs only qualified native maintenance code. The catalog is a retained file
/// bound to the signed plan, not JSON supplied as a health/success receipt.
pub(super) fn observe_for_plan(
    inputs: &Inputs<'_>, input: File,
    catalog: Option<&mut super::migration_catalog::CatalogInput>,
    plan: &crate::update::Plan, mut check: impl FnMut() -> Result<()>,
) -> Result<Observation> {
    if inputs.expected_manifest != plan.backup_manifest { return Err(err("UPDATE_INVENTORY_INPUT_INVALID")); }
    if plan.source_schema == plan.target_schema {
        if catalog.is_some() { return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED")); }
        let proof = observe(inputs, input, &mut check)?;
        super::source_inventory::matched_candidate(&proof, plan)?;
        return Ok(Observation::Original(proof));
    }
    let catalog = catalog.ok_or_else(||err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"))?;
    if inputs.expected_system.is_none() { return Err(err("UPDATE_INVENTORY_INPUT_INVALID")); }
    catalog.recheck_for_schema(&plan.target_schema)?;
    let migrations = catalog.migrations_sha256()?;
    let extra = vec![catalog.text()?, catalog.pin().to_owned(), plan.source_schema.clone(), plan.target_schema.clone(), migrations.clone()];
    let raw = observe_raw(inputs, input, include_str!("native_migrated_inventory_reader.mjs"), &extra, || { catalog.recheck_for_schema(&plan.target_schema)?; check() }, |raw| {
        let proof: super::source_inventory::MigratedInventoryProof = serde_json::from_slice(raw).map_err(|_|err("UPDATE_INVENTORY_RESULT_INVALID"))?;
        super::source_inventory::matched_migrated(&proof, plan, &migrations)
    })?;
    let proof: super::source_inventory::MigratedInventoryProof = serde_json::from_slice(&raw).map_err(|_|err("UPDATE_INVENTORY_RESULT_INVALID"))?;
    catalog.recheck_for_schema(&plan.target_schema)?;
    super::source_inventory::matched_migrated(&proof, plan, &migrations)?;
    check()?;
    Ok(Observation::Migrated(proof))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit isolated complete original-restored candidate; readonly mismatched migrated inventory refusal"]
    fn actual_native_migrated_reader_refuses_original_schema_and_preserves_original_inventory() {
        let document:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_MIGRATED_NATIVE_CHECK_INPUT").unwrap()).unwrap()).unwrap();
        let root=PathBuf::from(document["root"].as_str().unwrap());
        let plan:crate::update::Plan=serde_json::from_value(document["plan"].clone()).unwrap();
        let service=LifecycleService::open_retry_diagnostics(root.clone()).unwrap();
        let guard=service.lock().unwrap();let manifest=service.manifest().unwrap();
        service.validate_ownership(&manifest,"docker").unwrap();service.validate_volumes(&manifest,"docker").unwrap();
        let input=PathBuf::from(document["manifest"].as_str().unwrap());
        let binding=crate::restoration::RestorationBinding::from_plan(&plan).unwrap();
        let job:crate::restoration::RestorationJob=crate::read_json(&root.join("restoration.json")).unwrap();
        assert_eq!(job.state,"completed");
        let saved:crate::restoration::RestorationReceipt=crate::read_json(&root.join(format!("restore-{}/receipt.json",job.id))).unwrap();
        binding.check_receipt(&saved).unwrap();
        let raw=crate::installation_backup::source_bytes(input.parent().unwrap(),"manifest.json",16*1024*1024,true).unwrap();
        binding.authenticated(&saved.backup_id,&saved.authenticated_manifest_sha256,&raw).unwrap();
        let mut catalog=super::super::migration_catalog::CatalogInput::read(Path::new(document["catalog"].as_str().unwrap()),document["catalogSha256"].as_str().unwrap(),&plan.target_schema).unwrap();
        let result=(|| -> Result<Value> {
            service.operation(&Action::Start)?;
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(90);
            let pair=loop {
                service.check_restoration_guard(&guard)?;
                match super::super::super::rollback_runtime::pair(&service,&manifest,&plan) {
                    Ok(pair)=>break pair,
                    Err(e) if e.code=="UPDATE_ROLLBACK_HEALTH_FAILED" && std::time::Instant::now()<deadline=>std::thread::sleep(std::time::Duration::from_millis(250)),
                    Err(e)=>return Err(e),
                }
            };
            let i=Inputs { image:document["maintenanceImage"].as_str().unwrap(),root:&root,manifest:&manifest,app:&pair.1,blob_volume:&pair.3[0],expected_manifest:&plan.backup_manifest,expected_system:None };
            let before=observe(&i,super::super::super::private_file(&input,false).map_err(|e|err(e.code()))?,||service.check_restoration_guard(&guard))?;
            super::super::source_inventory::matched_candidate(&before,&plan)?;
            let physical=crate::run("docker",&["exec".into(),pair.2["Id"].as_str().ok_or_else(||err("ENGINE_OUTPUT_INVALID"))?.into(),"psql".into(),"-U".into(),"exhibitos".into(),"-d".into(),"exhibitos".into(),"-Atc".into(),"SELECT system_identifier::text FROM pg_control_system()".into()],None,30)?;
            let system=std::str::from_utf8(&physical).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?.trim().to_owned();
            if !super::super::source_database::valid_system_identifier(&system) {return Err(err("UPDATE_DATABASE_ID_INVALID"));}
            let pinned=Inputs{expected_system:Some(&system),..i};
            let reader_ids=|| -> Result<std::collections::BTreeSet<String>> {
                let raw=crate::run("docker",&["ps".into(),"--all".into(),"--no-trunc".into(),"--filter".into(),"label=com.exhibitos.inventory.reader".into(),"--format".into(),"{{.ID}}".into()],None,30)?;
                Ok(std::str::from_utf8(&raw).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?.lines().map(str::to_owned).collect())
            };
            let existing=reader_ids()?;
            let error=observe_for_plan(&pinned,super::super::super::private_file(&input,false).map_err(|e|err(e.code()))?,Some(&mut catalog),&plan,||service.check_restoration_guard(&guard)).err().ok_or_else(||err("UPDATE_MIGRATED_INVENTORY_MISMATCH"))?;
            let new:Vec<_>=reader_ids()?.difference(&existing).cloned().collect();
            if new.len()!=1 {return Err(err("UPDATE_INVENTORY_HELPER_INVALID"));}
            let failed=&new[0];let metadata=crate::backup_creation::inspected("docker",&["inspect".into(),failed.clone()])?;
            let log=crate::run("docker",&["logs".into(),failed.clone()],None,30)?;
            if metadata["State"]["Running"]!=false || metadata["State"]["ExitCode"]!=1 || metadata["Image"]!=i.image || metadata["HostConfig"]["ReadonlyRootfs"]!=true || !String::from_utf8_lossy(&log).contains("MIGRATION_SCHEMA_MISMATCH") {
                return Err(err("UPDATE_INVENTORY_HELPER_INVALID"));
            }
            let after=observe(&pinned,super::super::super::private_file(&input,false).map_err(|e|err(e.code()))?,||service.check_restoration_guard(&guard))?;
            super::super::source_inventory::matched_candidate(&after,&plan)?;
            Ok(serde_json::json!({"state":"PASS","readerRefusedOriginalSchema":true,"errorCode":error.code,"retainedFailedHelper":failed,"exactRefusal":"MIGRATION_SCHEMA_MISMATCH","originalInventoryBefore":before,"originalInventoryAfter":after,"hostActivated":false,"preflightVerified":false,"updateExecuted":false}))
        })();
        service.validate_ownership(&manifest,"docker").unwrap();service.validate_volumes(&manifest,"docker").unwrap();
        service.operation(&Action::Stop).unwrap();service.check_restoration_guard(&guard).unwrap();
        let proof=result.unwrap();
        crate::restoration::private_bytes(&root.parent().unwrap().join(format!("native-migrated-reader-refusal-{}.json",uuid::Uuid::new_v4())),&serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    }
    #[test]
    fn network_scope_refuses_extra_aliases_unknown_ids_and_missing_network() {
        let m:BundleManifest=serde_json::from_value(serde_json::json!({"schemaVersion":"1","bundleId":"fixture","version":"1","protocolVersion":"1","composeSha256":"a".repeat(64),"projectName":"exhibitos-fixture","services":[],"images":[],"ports":[],"openUrl":"http://127.0.0.1:1234","readinessUrl":"http://127.0.0.1:1234","minimumFreeBytes":1})).unwrap();
        let valid = serde_json::json!({"NetworkSettings":{"Networks":{"exhibitos-fixture_default":{"NetworkID":"a".repeat(64)}}}});
        assert_eq!(
            network_name(&m, &valid).unwrap(),
            "exhibitos-fixture_default"
        );
        for row in [
            serde_json::json!({}),
            serde_json::json!({"NetworkSettings":{"Networks":{"exhibitos-fixture_default":{"NetworkID":"unknown"}}}}),
            serde_json::json!({"NetworkSettings":{"Networks":{"other":{"NetworkID":"a".repeat(64)}}}}),
            serde_json::json!({"NetworkSettings":{"Networks":{"exhibitos-fixture_default":{"NetworkID":"a".repeat(64)},"foreign":{"NetworkID":"b".repeat(64)}}}}),
        ] {
            assert!(network_name(&m, &row).is_err());
        }
    }
    #[test]
    #[ignore = "explicit isolated retained Docker candidate; starts/stops only existing candidate, no DB/archive copies"]
    fn native_inventory_actual_old_runtime_without_backup_cli_is_observed_via_qualified_helper() {
        let root =
            std::fs::canonicalize(std::env::var("EXHIBITOS_NATIVE_INVENTORY_FIXTURE").unwrap())
                .unwrap();
        let profile = root.join("profile");
        let store = super::super::super::Store::open(&profile, "default").unwrap();
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::RolledBack
        );
        let plan = store.intent().unwrap().update.plan().clone();
        let id = store.intent().unwrap().update.restore_candidate().unwrap();
        let target =
            LifecycleService::open_retry_diagnostics(profile.join("installations").join(id))
                .unwrap();
        let guard = target.lock().unwrap();
        let m = target.manifest().unwrap();
        target.validate_ownership(&m, "docker").unwrap();
        target.validate_volumes(&m, "docker").unwrap();
        let record = fs::read_dir(&target.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("restore-")
            })
            .unwrap()
            .join("authenticated/manifest.json");
        let before = fs::read(&record).unwrap();
        let generation = store.current.generation;
        run(
            "docker",
            &compose_args(&m, &["up", "--detach"]),
            Some(&target.root.join("bundle")),
            180,
        )
        .unwrap();
        let result = (|| -> Result<()> {
            let (_, app) = backup_creation::one_container(&target, &m, "docker", "platform")?;
            let (db, _) = backup_creation::one_container(&target, &m, "docker", "database")?;
            let blob = app["Mounts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["Destination"] == "/data/blobs")
                .unwrap()["Name"]
                .as_str()
                .unwrap();
            let raw = run(
                "docker",
                &[
                    "exec".into(),
                    db,
                    "psql".into(),
                    "-U".into(),
                    "exhibitos".into(),
                    "-d".into(),
                    "exhibitos".into(),
                    "-Atc".into(),
                    "SELECT system_identifier::text FROM pg_control_system()".into(),
                ],
                None,
                30,
            )?;
            let physical = std::str::from_utf8(&raw)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
                .trim();
            let inputs = Inputs {
                image: "sha256:4658383ee50284d85c20bbbd742feee684c094a60b02033f6bc60e9c56b0aad2",
                root: &target.root,
                manifest: &m,
                app: &app,
                blob_volume: blob,
                expected_manifest: &plan.backup_manifest,
                expected_system: Some(physical),
            };
            let proof = observe(
                &inputs,
                super::super::private_file(&record, false).map_err(|e| err(e.code()))?,
                || target.check_restoration_guard(&guard),
            )?;
            super::super::source_inventory::matched_candidate(&proof, &plan)?;
            let wrong = Inputs {
                expected_system: Some("1"),
                ..inputs
            };
            assert!(
                observe(
                    &wrong,
                    super::super::private_file(&record, false).unwrap(),
                    || target.check_restoration_guard(&guard)
                )
                .is_err()
            );
            if fs::read(&record).map_err(|_| err("UPDATE_INPUT_UNAVAILABLE"))? != before
                || store.current.generation != generation
            {
                return Err(err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(())
        })();
        run(
            "docker",
            &compose_args(&m, &["stop", "--timeout", "30"]),
            Some(&target.root.join("bundle")),
            180,
        )
        .unwrap();
        target.check_restoration_guard(&guard).unwrap();
        println!(
            "NATIVE_INVENTORY_RESULT={}",
            serde_json::json!({"status":if result.is_ok(){"PASS"}else{"FAIL"},"errorCode":result.as_ref().err().map(|e|e.code.as_str()),"candidateStopped":true,"newLargeCopies":0,"wrongPhysicalIdentityRefused":result.is_ok(),"fullUpdateExecuted":false,"changedMigrationVerified":false})
        );
        result.unwrap();
    }
}
