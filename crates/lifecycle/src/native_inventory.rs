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
        READER.into(),
        i.expected_manifest.into(),
    ];
    if let Some(system) = i.expected_system {
        args.push(system.into());
    }
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
    let proof: super::source_inventory::InventoryProof =
        serde_json::from_slice(&raw).map_err(|_| err("UPDATE_INVENTORY_RESULT_INVALID"))?;
    check()?;
    run("docker", &["rm".into(), helper.into()], None, 30)?;
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::*;
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
