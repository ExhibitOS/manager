// SPDX-License-Identifier: Apache-2.0
//! Two current physical/logical observations in bounded tmpfs; no persistent snapshot.
use super::*;
use crate::{Result, err, hash_valid, run};
use serde_json::Value;
const MEMORY: u64 = 3 * 1024 * 1024 * 1024;
const MIN_ENGINE_MEMORY: u64 = MEMORY + 512 * 1024 * 1024;
const SNAPSHOT: &str = "/snapshot:rw,nosuid,nodev,noexec,size=4g";
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EphemeralObservation {
    pub physical: source_database::DatabaseCopyProof,
    pub inventory: source_inventory::InventoryProof,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EphemeralCandidateInventoryReceipt {
    pub source_instance: String,
    pub candidate_instance: String,
    pub observation: EphemeralObservation,
    pub repeated_observation: EphemeralObservation,
    pub storage: &'static str,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
fn validate_inputs(
    image: &str,
    database: &str,
    blobs: &str,
    manifest: &Path,
    hash: &str,
) -> Result<()> {
    if !image.strip_prefix("sha256:").is_some_and(hash_valid)
        || !hash_valid(hash)
        || [database, blobs].iter().any(|v| {
            v.is_empty()
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
        || database == blobs
        || !manifest.is_absolute()
        || manifest
            .to_str()
            .is_none_or(|p| p.contains(',') || p.chars().any(char::is_control))
    {
        return Err(err("UPDATE_SOURCE_INVENTORY_INVALID"));
    }
    Ok(())
}
fn script() -> String {
    // Children receive compiled-in scripts, never caller-provided JavaScript.
    format!(
        r#"import {{spawnSync}} from 'node:child_process';
const copy={copy}, reader={reader};
function child(script,env){{
 const r=spawnSync(process.execPath,['--input-type=module','-e',script],{{env,encoding:'utf8',timeout:270000,maxBuffer:65536}});
 if(r.error||r.status!==0)throw Error('EPHEMERAL_CHILD_FAILED');
 return JSON.parse(r.stdout);
}}
try{{
 const physical=child(copy,process.env);
 const inventory=child(reader,{{...process.env,EXHIBITOS_CANDIDATE_SYSTEM_IDENTIFIER:physical.systemIdentifier}});
 console.log(JSON.stringify({{physical,inventory}}));
}}catch{{console.error('EPHEMERAL_INVENTORY_REFUSED');process.exitCode=1;}}
"#,
        copy = serde_json::to_string(include_str!("source_database_copy.mjs")).unwrap(),
        reader = serde_json::to_string(include_str!("source_inventory_reader.mjs")).unwrap()
    )
}
fn bind_matches(actual: &Value, requested: &str) -> bool {
    if actual == requested {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        actual
            .as_str()
            .is_some_and(|s| s == format!("/host_mnt{requested}"))
    }
    #[cfg(not(target_os = "macos"))]
    false
}
#[allow(clippy::too_many_arguments)]
fn helper_valid(
    v: &Value,
    id: &str,
    name: &str,
    nonce: &str,
    image: &str,
    database: &str,
    blobs: &str,
    manifest: &Path,
) -> bool {
    let mounts = v["Mounts"].as_array();
    let expected = [
        ("/source", "volume", database),
        ("/blobs", "volume", blobs),
        ("/manifest.json", "bind", manifest.to_str().unwrap_or("")),
    ];
    v["Id"] == id
        && v["Name"] == format!("/{name}")
        && v["Image"] == image
        && v["Config"]["Labels"]["com.exhibitos.ephemeral.inventory"] == nonce
        && v["HostConfig"]["NetworkMode"] == "none"
        && v["HostConfig"]["ReadonlyRootfs"] == true
        && v["HostConfig"]["Privileged"] == false
        && v["HostConfig"]["PidsLimit"] == 64
        && v["Config"]["User"] == "0:0"
        && v["HostConfig"]["Memory"] == MEMORY
        && v["HostConfig"]["MemorySwap"] == MEMORY
        && v["HostConfig"]["Tmpfs"]["/snapshot"] == SNAPSHOT.split_once(':').unwrap().1
        && v["HostConfig"]["Tmpfs"].as_object().is_some_and(|t| {
            t.len() == 3 && t.contains_key("/tmp") && t.contains_key("/var/lib/postgresql")
        })
        && mounts.is_some_and(|m| {
            m.len() == 3
                && expected.iter().all(|(dest, kind, source)| {
                    m.iter().any(|x| {
                        x["Destination"] == *dest
                            && x["Type"] == *kind
                            && x["RW"] == false
                            && if *kind == "volume" {
                                x["Name"] == *source
                            } else {
                                bind_matches(&x["Source"], source)
                            }
                    })
                })
        })
}
pub(super) fn observe(
    ctx: &candidate_inventory::CandidateContext<'_>,
    image: &str,
) -> Result<EphemeralObservation> {
    let database = &ctx.candidate_before.database_volume;
    let blobs = &ctx.candidate_before.blob_volume;
    validate_inputs(
        image,
        database,
        blobs,
        ctx.manifest_path,
        &ctx.receipt.authenticated_manifest_sha256,
    )?;
    if crate::backup_creation::local_image("docker", image)? != image {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let memory = run(
        "docker",
        &["info".into(), "--format".into(), "{{.MemTotal}}".into()],
        None,
        30,
    )?;
    if std::str::from_utf8(&memory)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_none_or(|n| n < MIN_ENGINE_MEMORY)
    {
        return Err(err("RESTORE_SPACE_REQUIRED"));
    }
    // Every declared image volume must be masked; otherwise create could allocate an anonymous volume.
    let image_info = crate::backup_creation::inspected(
        "docker",
        &["image".into(), "inspect".into(), image.into()],
    )?;
    if !image_info["Config"]["Volumes"].is_null()
        && image_info["Config"]["Volumes"]
            .as_object()
            .is_none_or(|v| v.keys().any(|p| p != "/var/lib/postgresql"))
    {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let name = format!("exhibitos-ephemeral-inventory-{nonce}");
    let args: Vec<String> = vec![
        "create".into(),
        "--pull".into(),
        "never".into(),
        "--name".into(),
        name.clone(),
        "--label".into(),
        format!("com.exhibitos.ephemeral.inventory={nonce}"),
        "--network".into(),
        "none".into(),
        "--user".into(),
        "0:0".into(),
        "--read-only".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--cap-add".into(),
        "DAC_OVERRIDE".into(),
        "--cap-add".into(),
        "CHOWN".into(),
        "--cap-add".into(),
        "FOWNER".into(),
        "--cap-add".into(),
        "SETUID".into(),
        "--cap-add".into(),
        "SETGID".into(),
        "--cap-add".into(),
        "KILL".into(),
        "--security-opt".into(),
        "no-new-privileges:true".into(),
        "--pids-limit".into(),
        "64".into(),
        "--memory".into(),
        MEMORY.to_string(),
        "--memory-swap".into(),
        MEMORY.to_string(),
        "--tmpfs".into(),
        SNAPSHOT.into(),
        "--tmpfs".into(),
        "/tmp:rw,nosuid,nodev,noexec,size=64m".into(),
        "--tmpfs".into(),
        "/var/lib/postgresql:rw,nosuid,nodev,noexec,size=1m".into(),
        "--mount".into(),
        format!("type=volume,source={database},target=/source,readonly"),
        "--mount".into(),
        format!("type=volume,source={blobs},target=/blobs,readonly"),
        "--mount".into(),
        format!(
            "type=bind,source={},target=/manifest.json,readonly",
            ctx.manifest_path.display()
        ),
        "--env".into(),
        format!(
            "EXHIBITOS_MANIFEST_SHA256={}",
            ctx.receipt.authenticated_manifest_sha256
        ),
        "--entrypoint".into(),
        "node".into(),
        image.into(),
        "--input-type=module".into(),
        "-e".into(),
        script(),
    ];
    let output = run("docker", &args, None, 30)?;
    let id = std::str::from_utf8(&output)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
        .trim();
    if !hash_valid(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let inspect = || crate::backup_creation::inspected("docker", &["inspect".into(), id.into()]);
    if !helper_valid(
        &inspect()?,
        id,
        &name,
        &nonce,
        image,
        database,
        blobs,
        ctx.manifest_path,
    ) {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let result = run(
        "docker",
        &["start".into(), "--attach".into(), id.into()],
        None,
        600,
    );
    let helper = inspect()?;
    if !helper_valid(
        &helper,
        id,
        &name,
        &nonce,
        image,
        database,
        blobs,
        ctx.manifest_path,
    ) {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    if helper["State"]["Running"] != false {
        run(
            "docker",
            &["stop".into(), "--time".into(), "5".into(), id.into()],
            None,
            15,
        )?;
    }
    let stopped = inspect()?;
    if stopped["State"]["Running"] != false || stopped["State"]["Restarting"] != false {
        return Err(err("CANCEL_UNCERTAIN"));
    }
    let observation: EphemeralObservation =
        serde_json::from_slice(&result?).map_err(|_| err("UPDATE_SOURCE_INVENTORY_INVALID"))?;
    if stopped["State"]["OOMKilled"] != false || stopped["State"]["ExitCode"] != 0 {
        return Err(err("UPDATE_SOURCE_INVENTORY_INVALID"));
    }
    source_database::validate(&observation.physical)?;
    source_inventory::matched_candidate(&observation.inventory, ctx.plan)?;
    // Remove only this stopped, owned successful helper. No volume deletion operation.
    run("docker", &["rm".into(), id.into()], None, 15)?;
    Ok(observation)
}
impl ExecutionSession<'_> {
    pub fn verify_restored_candidate_inventory_ephemeral(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> Result<EphemeralCandidateInventoryReceipt> {
        self.inspect_restored_candidate(acknowledged, |ctx| {
            let first = observe(ctx, image)?;
            let repeated = observe(ctx, image)?;
            if !candidate_inventory::copies_match(&first.physical, &repeated.physical) {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
            Ok(EphemeralCandidateInventoryReceipt {
                source_instance: ctx.plan.source_instance.clone(),
                candidate_instance: ctx.plan.target_instance.clone(),
                observation: first,
                repeated_observation: repeated,
                storage: "bounded-tmpfs-no-persistent-snapshot",
                preflight_verified: false,
                update_executed: false,
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_and_mount_injection_refuse_before_engine_access() {
        let image = format!("sha256:{}", "a".repeat(64));
        let hash = "b".repeat(64);
        assert!(
            validate_inputs(
                &image,
                "database",
                "blobs",
                Path::new("/manifest.json"),
                &hash
            )
            .is_ok()
        );
        for (db, blobs, path) in [
            ("same", "same", "/manifest.json"),
            ("database,readonly=false", "blobs", "/manifest.json"),
            ("database", "blobs", "relative"),
            ("database", "blobs", "/manifest,other"),
            ("database", "blobs", "/manifest\n"),
        ] {
            assert!(validate_inputs(&image, db, blobs, Path::new(path), &hash).is_err());
        }
    }
    #[test]
    fn persistent_mounts_writable_originals_and_unbounded_helpers_are_refused() {
        let v = serde_json::json!({"Id":"id","Name":"/helper","Image":"image","Config":{"User":"0:0","Labels":{"com.exhibitos.ephemeral.inventory":"nonce"}},"HostConfig":{"NetworkMode":"none","ReadonlyRootfs":true,"Privileged":false,"PidsLimit":64,"Memory":MEMORY,"MemorySwap":MEMORY,"Tmpfs":{"/snapshot":"rw,nosuid,nodev,noexec,size=4g","/tmp":"rw","/var/lib/postgresql":"rw"}},"Mounts":[{"Destination":"/source","Type":"volume","Name":"database","RW":false},{"Destination":"/blobs","Type":"volume","Name":"blobs","RW":false},{"Destination":"/manifest.json","Type":"bind","Source":"/manifest.json","RW":false}]});
        let valid = |v: &Value| {
            helper_valid(
                v,
                "id",
                "helper",
                "nonce",
                "image",
                "database",
                "blobs",
                Path::new("/manifest.json"),
            )
        };
        assert!(valid(&v));
        let mut mapped = v.clone();
        mapped["Mounts"][2]["Source"] = Value::from("/host_mnt/manifest.json");
        assert_eq!(valid(&mapped), cfg!(target_os = "macos"));
        for path in ["/host_mnt/foreign.json", "/other/manifest.json"] {
            let mut bad = v.clone();
            bad["Mounts"][2]["Source"] = Value::from(path);
            assert!(!valid(&bad));
        }
        for pointer in [
            "/HostConfig/Memory",
            "/HostConfig/MemorySwap",
            "/HostConfig/PidsLimit",
        ] {
            let mut bad = v.clone();
            *bad.pointer_mut(pointer).unwrap() = Value::from(0);
            assert!(!valid(&bad));
        }
        for index in 0..3 {
            let mut bad = v.clone();
            bad["Mounts"][index]["RW"] = Value::from(true);
            assert!(!valid(&bad));
        }
        let mut bad = v.clone();
        bad["Mounts"].as_array_mut().unwrap().push(serde_json::json!({"Destination":"/snapshot","Type":"volume","Name":"persistent-copy","RW":true}));
        assert!(!valid(&bad));
        let mut bad = v.clone();
        bad["HostConfig"]["Privileged"] = Value::from(true);
        assert!(!valid(&bad));
        let mut bad = v.clone();
        bad["HostConfig"]["Tmpfs"]["/snapshot"] = Value::from("rw,size=64g");
        assert!(!valid(&bad));
    }
}
