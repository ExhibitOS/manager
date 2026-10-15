// SPDX-License-Identifier: Apache-2.0
//! Logical inventory on a freshly copied isolated database plus readonly source blobs.
use crate::*;
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryProof {
    pub operation: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub inventory_sha256: String,
    pub schema_sha256: String,
    pub observed_at: String,
    pub current_inventory_verified: bool,
    pub configuration_verified: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
/// Independent maintenance observation of the signed target schema and unchanged
/// original data. Deserializing this value alone never grants an execution permit.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigratedInventoryProof {
    pub operation: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub source_inventory_sha256: String,
    pub source_schema_sha256: String,
    pub target_schema_sha256: String,
    pub target_migrations_sha256: String,
    pub observed_at: String,
    pub original_data_preserved: bool,
    pub current_inventory_verified: bool,
    pub configuration_verified: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
pub(super) fn matched_migrated(
    proof: &MigratedInventoryProof,
    plan: &crate::update::Plan,
    migrations_sha256: &str,
) -> Result<()> {
    if plan.source_schema == plan.target_schema
        || !crate::hash_valid(migrations_sha256)
        || proof.operation != "migrated-inventory-preserved"
        || proof.backup_id != plan.backup_id
        || proof.authenticated_manifest_sha256 != plan.backup_manifest
        || proof.source_inventory_sha256 != plan.source_inventory
        || proof.source_schema_sha256 != plan.source_schema
        || proof.target_schema_sha256 != plan.target_schema
        || proof.target_migrations_sha256 != migrations_sha256
        || !proof.original_data_preserved
        || proof.current_inventory_verified
        || proof.configuration_verified
        || proof.preflight_verified
        || proof.update_executed
    { return Err(err("UPDATE_MIGRATED_INVENTORY_MISMATCH")); }
    Ok(())
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInventoryReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub source_database_volume: String,
    pub source_blob_volume: String,
    pub snapshot_volume: String,
    pub repeated_snapshot_volume: String,
    pub source_content_sha256: String,
    pub inventory: InventoryProof,
    pub observed_at: u64,
}
pub(super) fn matched(proof: &InventoryProof, plan: &crate::update::Plan) -> Result<()> {
    matched_operation(proof, plan, "source-inventory-matched")
}
pub(super) fn matched_candidate(proof: &InventoryProof, plan: &crate::update::Plan) -> Result<()> {
    matched_operation(proof, plan, "restored-inventory-matched")
}
fn matched_operation(
    proof: &InventoryProof,
    plan: &crate::update::Plan,
    operation: &str,
) -> Result<()> {
    if proof.operation != operation
        || proof.backup_id != plan.backup_id
        || proof.authenticated_manifest_sha256 != plan.backup_manifest
        || proof.inventory_sha256 != plan.source_inventory
        || proof.schema_sha256 != plan.source_schema
        || !proof.current_inventory_verified
        || proof.configuration_verified
        || proof.preflight_verified
        || proof.update_executed
    {
        return Err(err("UPDATE_SOURCE_INVENTORY_MISMATCH"));
    }
    Ok(())
}
pub(super) fn compare(
    image: &str,
    snapshot: &str,
    blobs: &str,
    manifest: &Path,
    manifest_hash: &str,
) -> Result<InventoryProof> {
    compare_at(image, snapshot, blobs, manifest, manifest_hash, None)
}
pub(super) fn compare_candidate(
    image: &str,
    snapshot: &str,
    blobs: &str,
    manifest: &Path,
    manifest_hash: &str,
    system_identifier: &str,
) -> Result<InventoryProof> {
    if !super::source_database::valid_system_identifier(system_identifier) {
        return Err(err("UPDATE_SOURCE_INVENTORY_INVALID"));
    }
    compare_at(
        image,
        snapshot,
        blobs,
        manifest,
        manifest_hash,
        Some(system_identifier),
    )
}
fn compare_at(
    image: &str,
    snapshot: &str,
    blobs: &str,
    manifest: &Path,
    manifest_hash: &str,
    candidate_system_identifier: Option<&str>,
) -> Result<InventoryProof> {
    if !image.strip_prefix("sha256:").is_some_and(hash_valid)
        || !hash_valid(manifest_hash)
        || [snapshot, blobs].iter().any(|v| {
            v.is_empty()
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
        || !manifest.is_absolute()
        || manifest
            .to_str()
            .is_none_or(|p| p.contains(',') || p.chars().any(char::is_control))
    {
        return Err(err("UPDATE_SOURCE_INVENTORY_INVALID"));
    }
    if crate::backup_creation::local_image("docker", image)? != image {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let name = format!("exhibitos-source-inventory-{nonce}");
    #[cfg(windows)]
    let mut native_manifest = crate::windows_private::HostFile::open(manifest)?;
    #[cfg(windows)]
    let native_payload = {
        native_manifest.check_private()?;
        let mut bytes = Vec::new();
        (&mut native_manifest).take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_|err("UPDATE_SOURCE_INVENTORY_INVALID"))?;
        native_manifest.check_private()?;
        if bytes.len() > 16 * 1024 * 1024 || crate::digest(&bytes) != manifest_hash { return Err(err("UPDATE_SOURCE_INVENTORY_INVALID")); }
        bytes
    };
    let mut args = vec![
        "create".into(),
        "--pull".into(),
        "never".into(),
        "--name".into(),
        name.clone(),
        "--label".into(),
        format!("com.exhibitos.source.inventory={nonce}"),
        "--network".into(),
        "none".into(),
        "--user".into(),
        "0:0".into(),
        "--read-only".into(),
        "--cap-drop".into(),
        "ALL".into(),
    ];
    for cap in ["DAC_OVERRIDE", "CHOWN", "SETUID", "SETGID", "KILL"] {
        args.extend(["--cap-add".into(), cap.into()]);
    }
    if let Some(id) = candidate_system_identifier {
        args.extend([
            "--env".into(),
            format!("EXHIBITOS_CANDIDATE_SYSTEM_IDENTIFIER={id}"),
        ]);
    }
    args.extend([
        "--security-opt".into(),
        "no-new-privileges:true".into(),
        "--memory".into(),
        "512m".into(),
        "--pids-limit".into(),
        "64".into(),
        "--tmpfs".into(),
        "/tmp:rw,nosuid,nodev,size=256m,mode=1777".into(),
        "--tmpfs".into(),
        "/var/lib/postgresql:rw,nosuid,nodev,size=1m".into(),
        "--mount".into(),
        format!("type=volume,source={snapshot},target=/snapshot,volume-nocopy"),
        "--mount".into(),
        format!("type=volume,source={blobs},target=/blobs,readonly,volume-nocopy"),

        "--env".into(),
        format!("EXHIBITOS_MANIFEST_SHA256={manifest_hash}"),
        "--entrypoint".into(),
        "node".into(),
        image.into(),
        "--input-type=module".into(),
        "-e".into(),
        {
            #[cfg(not(windows))]
            { include_str!("source_inventory_reader.mjs").into() }
            #[cfg(windows)]
            {
                format!("{}\nawait import('data:text/javascript;charset=utf-8,'+encodeURIComponent({}));", native_manifest_bootstrap(manifest_hash), serde_json::to_string(&include_str!("source_inventory_reader.mjs").replace("'/manifest.json'","'/input/manifest.json'").replace("from '/opt/exhibitos/scripts/service-backup.mjs'", "from 'file:///opt/exhibitos/scripts/service-backup.mjs'")).map_err(|_|err("UPDATE_SOURCE_INVENTORY_INVALID"))?)
            }
        },
    ]);
    let entry = args.iter().position(|value|value=="--entrypoint").ok_or_else(||err("UPDATE_SOURCE_INVENTORY_INVALID"))?;
    #[cfg(not(windows))]
    args.splice(entry..entry,["--mount".into(),format!("type=bind,source={},target=/manifest.json,readonly",manifest.display())]);
    #[cfg(windows)]
    args.splice(entry..entry,["--interactive".into(),"--tmpfs".into(),"/input:rw,nosuid,nodev,noexec,size=32m,mode=0700".into()]);
    let output = String::from_utf8(run("docker", &args, None, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let id = output.trim();
    if !hash_valid(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    #[cfg(not(windows))]
    let result = run("docker", &["start".into(),"--attach".into(),id.into()],None,600);
    #[cfg(windows)]
    let result = crate::run_observed_stream(
        "docker", &["start".into(),"--attach".into(),"--interactive".into(),id.into()],None,600,(None,None),
        Some((Box::new(std::io::Cursor::new(native_payload)),16 * 1024 * 1024)),
        || {
            native_manifest.check_private()?;
            let helper=crate::backup_creation::inspected("docker", &["inspect".into(),id.into()])?;
            if helper["Id"] != id || helper["Name"] != format!("/{name}") || helper["Config"]["Labels"]["com.exhibitos.source.inventory"] != nonce { return Err(err("OWNERSHIP_CONFLICT")); }
            Ok(())
        },
    );
    #[cfg(windows)]
    native_manifest.check_private()?;
    let helper = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
    if helper["Id"] != id
        || helper["Name"] != format!("/{name}")
        || helper["Config"]["Labels"]["com.exhibitos.source.inventory"] != nonce
    {
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
    let stopped = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
    if stopped["State"]["Running"] != false || stopped["State"]["Restarting"] != false {
        return Err(err("CANCEL_UNCERTAIN"));
    }
    if result.is_ok() {
        run("docker", &["rm".into(), id.into()], None, 15)?;
    }
    serde_json::from_slice(&result?).map_err(|_| err("UPDATE_SOURCE_INVENTORY_INVALID"))
}
#[cfg(windows)]
fn native_manifest_bootstrap(pin: &str) -> String {
    // Exact bytes from the retained native private reader, never daemon NTFS binds.
    format!(r#"import {{open as nativeOpen}} from 'node:fs/promises';
import {{constants as nativeConstants}} from 'node:fs';
import {{createHash as nativeHash}} from 'node:crypto';
const nativeParts=[];let nativeLength=0;
const nativeStdin=await nativeOpen('/dev/stdin',nativeConstants.O_RDONLY);
try{{const nativeBuffer=Buffer.alloc(65536);for(;;){{const {{bytesRead}}=await nativeStdin.read(nativeBuffer,0,nativeBuffer.length,null);if(bytesRead===0)break;nativeLength+=bytesRead;if(nativeLength>16777216)throw Error('NATIVE_MANIFEST_INPUT_INVALID');nativeParts.push(Buffer.from(nativeBuffer.subarray(0,bytesRead)));}}}}finally{{await nativeStdin.close();}}
const nativeBytes=Buffer.concat(nativeParts);if(nativeBytes.length===0||nativeHash('sha256').update(nativeBytes).digest('hex')!=='{pin}')throw Error('NATIVE_MANIFEST_INPUT_INVALID');
const nativeFile=await nativeOpen('/input/manifest.json',nativeConstants.O_WRONLY|nativeConstants.O_CREAT|nativeConstants.O_EXCL|nativeConstants.O_NOFOLLOW,0o600);
try{{await nativeFile.writeFile(nativeBytes);await nativeFile.sync();}}finally{{await nativeFile.close();}}
"#)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrated_receipt_requires_exact_original_data_target_catalog_and_no_admission_claim() {
        let plan = crate::update::Plan { operation_id:"operation".into(), source_instance:"source".into(),target_instance:"target".into(),source_image:"a".repeat(64),target_image:"b".repeat(64),source_schema:"c".repeat(64),target_schema:"d".repeat(64),backup_id:"backup".into(),backup_manifest:"e".repeat(64),source_inventory:"f".repeat(64),required_free_bytes:1 };
        let migration_pin="1".repeat(64);
        let base=serde_json::json!({"operation":"migrated-inventory-preserved","backupId":plan.backup_id,"authenticatedManifestSha256":plan.backup_manifest,"sourceInventorySha256":plan.source_inventory,"sourceSchemaSha256":plan.source_schema,"targetSchemaSha256":plan.target_schema,"targetMigrationsSha256":migration_pin,"observedAt":"2026-10-06T00:00:00Z","originalDataPreserved":true,"currentInventoryVerified":false,"configurationVerified":false,"preflightVerified":false,"updateExecuted":false});
        let decode=|v|serde_json::from_value::<MigratedInventoryProof>(v).unwrap();
        assert!(matched_migrated(&decode(base.clone()),&plan,&migration_pin).is_ok());
        for name in ["backupId","authenticatedManifestSha256","sourceInventorySha256","sourceSchemaSha256","targetSchemaSha256","targetMigrationsSha256","operation"] {
            let mut wrong=base.clone(); wrong[name]=Value::from("2".repeat(64));
            assert!(matched_migrated(&decode(wrong),&plan,&migration_pin).is_err());
        }
        for name in ["currentInventoryVerified","configurationVerified","preflightVerified","updateExecuted"] {
            let mut wrong=base.clone();wrong[name]=Value::from(true);
            assert!(matched_migrated(&decode(wrong),&plan,&migration_pin).is_err());
        }
        let mut wrong=base.clone();wrong["originalDataPreserved"]=Value::from(false);
        assert!(matched_migrated(&decode(wrong),&plan,&migration_pin).is_err());
        let mut extra=base.clone();extra["ownedPermit"]=Value::from(true);
        assert!(serde_json::from_value::<MigratedInventoryProof>(extra).is_err());
        let mut unchanged=plan.clone();unchanged.target_schema=plan.source_schema.clone();
        assert!(matched_migrated(&decode(base),&unchanged,&migration_pin).is_err());
    }
    #[test]
    #[ignore = "explicit retained real migrated Runtime observation; no engine mutation"]
    fn actual_retained_migrated_observation_binds_to_signed_plan_catalog_without_admission() {
        let input:Value=serde_json::from_slice(&fs::read(std::env::var("EXHIBITOS_RETAINED_MIGRATED_OBSERVATION").unwrap()).unwrap()).unwrap();
        let plan:crate::update::Plan=serde_json::from_value(input["plan"].clone()).unwrap();
        let mut catalog=super::super::migration_catalog::CatalogInput::read(Path::new(input["catalog"].as_str().unwrap()),input["catalogSha256"].as_str().unwrap(),&plan.target_schema).unwrap();
        catalog.recheck_for_schema(&plan.target_schema).unwrap();
        let proof:MigratedInventoryProof=serde_json::from_value(input["proof"].clone()).unwrap();
        matched_migrated(&proof,&plan,&catalog.migrations_sha256().unwrap()).unwrap();
        assert!(!proof.preflight_verified && !proof.update_executed);
    }
    #[test]
    fn logical_receipt_must_match_exact_plan_and_never_claim_full_preflight() {
        let plan = crate::update::Plan {
            operation_id: "operation".into(),
            source_instance: "source".into(),
            target_instance: "target".into(),
            source_image: "a".repeat(64),
            target_image: "b".repeat(64),
            source_schema: "c".repeat(64),
            target_schema: "d".repeat(64),
            backup_id: "backup".into(),
            backup_manifest: "e".repeat(64),
            source_inventory: "f".repeat(64),
            required_free_bytes: 1,
        };
        let base = serde_json::json!({"operation":"source-inventory-matched","backupId":plan.backup_id,"authenticatedManifestSha256":plan.backup_manifest,"inventorySha256":plan.source_inventory,"schemaSha256":plan.source_schema,"observedAt":"2026-10-04T00:00:00Z","currentInventoryVerified":true,"configurationVerified":false,"preflightVerified":false,"updateExecuted":false});
        let proof = |v| serde_json::from_value::<InventoryProof>(v).unwrap();
        assert!(matched(&proof(base.clone()), &plan).is_ok());
        assert!(matched_candidate(&proof(base.clone()), &plan).is_err());
        let mut candidate = base.clone();
        candidate["operation"] = Value::from("restored-inventory-matched");
        assert!(matched_candidate(&proof(candidate.clone()), &plan).is_ok());
        assert!(matched(&proof(candidate), &plan).is_err());
        for field in [
            "operation",
            "backupId",
            "authenticatedManifestSha256",
            "inventorySha256",
            "schemaSha256",
        ] {
            let mut v = base.clone();
            v[field] = Value::from("foreign");
            assert!(matched(&proof(v), &plan).is_err());
        }
        for field in [
            "configurationVerified",
            "preflightVerified",
            "updateExecuted",
        ] {
            let mut v = base.clone();
            v[field] = Value::from(true);
            assert!(matched(&proof(v), &plan).is_err());
        }
        let mut v = base;
        v["currentInventoryVerified"] = Value::from(false);
        assert!(matched(&proof(v), &plan).is_err());
    }
}
