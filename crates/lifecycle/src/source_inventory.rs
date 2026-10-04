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
    if proof.operation != "source-inventory-matched"
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
        "--mount".into(),
        format!(
            "type=bind,source={},target=/manifest.json,readonly",
            manifest.display()
        ),
        "--env".into(),
        format!("EXHIBITOS_MANIFEST_SHA256={manifest_hash}"),
        "--entrypoint".into(),
        "node".into(),
        image.into(),
        "--input-type=module".into(),
        "-e".into(),
        include_str!("source_inventory_reader.mjs").into(),
    ]);
    let output = String::from_utf8(run("docker", &args, None, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let id = output.trim();
    if !hash_valid(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let result = run(
        "docker",
        &["start".into(), "--attach".into(), id.into()],
        None,
        600,
    );
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
#[cfg(test)]
mod tests {
    use super::*;
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
