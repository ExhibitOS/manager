// SPDX-License-Identifier: Apache-2.0
//! Bounded physical copy of an observed stopped native PostgreSQL18 volume.
//! This is not logical inventory equality, clean recovery or global writer fencing.
use crate::*;
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseCopyProof {
    pub clean_shutdown: bool,
    pub files: u64,
    pub entries: u64,
    pub bytes: u64,
    pub content_sha256: String,
    pub postgres_major: u8,
    pub pgdata: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSnapshotReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub source_volume: String,
    pub snapshot_volume: String,
    pub maintenance_image: String,
    pub proof: DatabaseCopyProof,
    pub observed_at: u64,
}
fn valid_volume(volume: &str) -> bool {
    !volume.is_empty()
        && volume
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(super) fn copy(image: &str, source: &str) -> Result<(String, DatabaseCopyProof)> {
    if !image.strip_prefix("sha256:").is_some_and(hash_valid) || !valid_volume(source) {
        return Err(err("BACKUP_IMAGE_INVALID"));
    }
    if crate::backup_creation::local_image("docker", image)? != image {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let volume = format!("exhibitos-source-db-copy-{nonce}");
    let label = format!("com.exhibitos.source.database={nonce}");
    run(
        "docker",
        &[
            "volume".into(),
            "create".into(),
            "--driver".into(),
            "local".into(),
            "--label".into(),
            label.clone(),
            volume.clone(),
        ],
        None,
        30,
    )?;
    let v = crate::backup_creation::inspected(
        "docker",
        &["volume".into(), "inspect".into(), volume.clone()],
    )?;
    if v["Name"] != volume
        || v["Driver"] != "local"
        || v["Labels"]["com.exhibitos.source.database"] != nonce
        || !matches!(&v["Options"], Value::Null)
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let name = format!("exhibitos-source-db-copy-helper-{nonce}");
    let args = vec![
        "create".into(),
        "--pull".into(),
        "never".into(),
        "--name".into(),
        name.clone(),
        "--label".into(),
        label,
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
        "--security-opt".into(),
        "no-new-privileges:true".into(),
        "--pids-limit".into(),
        "32".into(),
        "--memory".into(),
        "256m".into(),
        "--tmpfs".into(),
        "/var/lib/postgresql:rw,nosuid,nodev,size=1m".into(),
        "--mount".into(),
        format!("type=volume,source={source},target=/source,readonly,volume-nocopy"),
        "--mount".into(),
        format!("type=volume,source={volume},target=/snapshot,volume-nocopy"),
        "--entrypoint".into(),
        "node".into(),
        image.into(),
        "--input-type=module".into(),
        "-e".into(),
        include_str!("source_database_copy.mjs").into(),
    ];
    // Uncertain creation and all successful/failed snapshot volumes are retained.
    let created = String::from_utf8(run("docker", &args, None, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let id = created.trim();
    if !hash_valid(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let output = run(
        "docker",
        &["start".into(), "--attach".into(), id.into()],
        None,
        600,
    );
    let helper = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
    if helper["Id"] != id
        || helper["Name"] != format!("/{name}")
        || helper["Config"]["Labels"]["com.exhibitos.source.database"] != nonce
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
    if output.is_ok() {
        run("docker", &["rm".into(), id.into()], None, 15)?; // no force / volume deletion
    } // failed stopped helper retained for private diagnosis
    let proof: DatabaseCopyProof =
        serde_json::from_slice(&output?).map_err(|_| err("UPDATE_SOURCE_DATABASE_COPY_INVALID"))?;
    validate(&proof)?;
    Ok((volume, proof))
}
fn validate(proof: &DatabaseCopyProof) -> Result<()> {
    if !proof.clean_shutdown
        || proof.files == 0
        || proof.entries < proof.files
        || proof.entries > 200000
        || proof.bytes > 2 * 1024 * 1024 * 1024
        || !hash_valid(&proof.content_sha256)
        || proof.postgres_major != 18
        || proof.pgdata != "18/docker"
    {
        return Err(err("UPDATE_SOURCE_DATABASE_COPY_INVALID"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_proof_requires_bounded_supported_database_copy() {
        let mut p = DatabaseCopyProof {
            clean_shutdown: true,
            files: 4,
            entries: 6,
            bytes: 200,
            content_sha256: "a".repeat(64),
            postgres_major: 18,
            pgdata: "18/docker".into(),
        };
        assert!(validate(&p).is_ok());
        p.bytes = 2 * 1024 * 1024 * 1024 + 1;
        assert!(validate(&p).is_err());
        assert!(!valid_volume("volume,/host"));
        assert!(!valid_volume(""));
    }
}
