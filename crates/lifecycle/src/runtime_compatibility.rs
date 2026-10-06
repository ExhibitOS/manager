// SPDX-License-Identifier: Apache-2.0
//! Actual unchanged-schema development runtime observation under all execution fences.
use super::*;
use crate::{Result, Value, err, hash_valid, run};

/// Opaque observation, never a preflight/apply or image-only rollback permit.
pub struct RuntimeCompatibility {
    plan: crate::update::Plan,
    generation: u64,
    envelope: String,
    receipt: Value,
}
impl RuntimeCompatibility {
    pub fn receipt(&self) -> &Value {
        &self.receipt
    }
}
fn engine(args: &[String], timeout: u64) -> Result<Vec<u8>> {
    run("docker", args, None, timeout)
}
fn command(args: &[&str]) -> Result<Vec<u8>> {
    engine(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), 60)
}
fn inspect(id: &str) -> Result<Value> {
    crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])
}
fn common() -> Vec<String> {
    let mut a: Vec<String> = [
        "--pull",
        "never",
        "--user",
        "0:0",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges:true",
        "--pids-limit",
        "128",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for cap in [
        "DAC_OVERRIDE",
        "CHOWN",
        "FOWNER",
        "SETUID",
        "SETGID",
        "KILL",
    ] {
        a.extend(["--cap-add".into(), cap.into()]);
    }
    a
}
fn bind_matches(actual: &Value, expected: &str) -> bool {
    actual == expected
        || cfg!(target_os = "macos")
            && actual.as_str() == Some(format!("/host_mnt{expected}").as_str())
}
struct Helper {
    id: String,
    name: String,
    nonce: String,
    image: String,
    network: String,
    memory: u64,
    mounts: Vec<(String, String, String)>,
    script: String,
    tmpfs: Value,
}
impl Helper {
    fn matches(&self, v: &Value) -> bool {
        let h = &v["HostConfig"];
        let expected_caps = serde_json::json!([
            "CAP_CHOWN",
            "CAP_DAC_OVERRIDE",
            "CAP_FOWNER",
            "CAP_KILL",
            "CAP_SETGID",
            "CAP_SETUID"
        ]);
        let mounts = v["Mounts"].as_array();
        v["Id"] == self.id
            && v["Name"] == format!("/{}", self.name)
            && v["Image"] == self.image
            && v["Config"]["Labels"]["com.exhibitos.runtime.probe"] == self.nonce
            && v["Config"]["User"] == "0:0"
            && v["Config"]["Entrypoint"] == serde_json::json!(["node"])
            && v["Config"]["Cmd"] == serde_json::json!(["--input-type=module", "-e", self.script])
            && h["NetworkMode"] == self.network
            && h["ReadonlyRootfs"] == true
            && h["Privileged"] == false
            && h["Memory"] == self.memory
            && h["MemorySwap"] == self.memory
            && h["PidsLimit"] == 128
            && h["CapDrop"] == serde_json::json!(["ALL"])
            && h["CapAdd"] == expected_caps
            && h["SecurityOpt"] == serde_json::json!(["no-new-privileges:true"])
            && h["Tmpfs"] == self.tmpfs
            && h["PublishAllPorts"] == false
            && h["PortBindings"].as_object().is_some_and(|m| m.is_empty())
            && ["Devices", "DeviceRequests", "VolumesFrom", "Binds"]
                .iter()
                .all(|k| h[k].is_null() || h[k].as_array().is_some_and(|a| a.is_empty()))
            && mounts.is_some_and(|rows| {
                rows.len() == self.mounts.len()
                    && self.mounts.iter().all(|(target, kind, source)| {
                        rows.iter()
                            .filter(|r| {
                                r["Destination"] == *target
                                    && r["Type"] == *kind
                                    && r["RW"] == false
                                    && if kind == "volume" {
                                        r["Name"] == *source
                                    } else {
                                        bind_matches(&r["Source"], source)
                                    }
                            })
                            .count()
                            == 1
                    })
                    && rows.iter().filter(|r| r["Type"] == "tmpfs").all(|r| {
                        self.tmpfs
                            .get(r["Destination"].as_str().unwrap_or(""))
                            .is_some()
                    })
            })
    }
    fn check(&self) -> Result<Value> {
        let v = inspect(&self.id)?;
        if !self.matches(&v) {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(v)
    }
    fn stop(&self) -> Result<Value> {
        if self.check()?["State"]["Running"] == true {
            command(&["stop", "--time", "10", &self.id])?;
        }
        let v = self.check()?;
        if v["State"]["Running"] != false || v["State"]["Restarting"] != false {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        Ok(v)
    }
    fn retire(&self) -> Result<()> {
        let v = self.check()?;
        if v["State"]["Running"] != false
            || v["State"]["ExitCode"] != 0
            || v["State"]["OOMKilled"] != false
        {
            return Err(err("UPDATE_RUNTIME_PROBE_FAILED"));
        }
        command(&["rm", &self.id])?;
        Ok(())
    }
}
fn create(mut args: Vec<String>, mut h: Helper) -> Result<Helper> {
    args.splice(
        0..0,
        [
            "create".into(),
            "--name".into(),
            h.name.clone(),
            "--label".into(),
            format!("com.exhibitos.runtime.probe={}", h.nonce),
            "--network".into(),
            h.network.clone(),
        ],
    );
    args.extend(common());
    args.extend([
        "--memory".into(),
        h.memory.to_string(),
        "--memory-swap".into(),
        h.memory.to_string(),
    ]);
    for (path, opt) in h
        .tmpfs
        .as_object()
        .ok_or_else(|| err("UPDATE_RUNTIME_INPUT_INVALID"))?
    {
        args.extend([
            "--tmpfs".into(),
            format!(
                "{path}:{}",
                opt.as_str()
                    .ok_or_else(|| err("UPDATE_RUNTIME_INPUT_INVALID"))?
            ),
        ]);
    }
    for (target, kind, source) in &h.mounts {
        args.extend([
            "--mount".into(),
            format!(
                "type={kind},source={source},target={target},readonly{}",
                if kind == "volume" {
                    ",volume-nocopy"
                } else {
                    ""
                }
            ),
        ]);
    }
    args.extend([
        "--entrypoint".into(),
        "node".into(),
        h.image.clone(),
        "--input-type=module".into(),
        "-e".into(),
        h.script.clone(),
    ]);
    h.id = std::str::from_utf8(&engine(&args, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
        .trim()
        .into();
    if !hash_valid(&h.id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    h.check()?;
    Ok(h)
}
fn run_probe(
    ctx: &candidate_inventory::CandidateContext<'_>,
    maintenance: &str,
    expected: &source_database::DatabaseCopyProof,
    migration: Option<&Value>,
) -> Result<Value> {
    let info: Value = serde_json::from_slice(&command(&["info", "--format", "{{json .}}"])?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    if info["MemTotal"]
        .as_u64()
        .is_none_or(|n| n < 2304 * 1024 * 1024)
    {
        return Err(err("UPDATE_RUNTIME_MEMORY_REQUIRED"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let copy = include_str!("source_database_copy.mjs")
        .replace("limit=2n*1024n*1024n*1024n", "limit=256n*1024n*1024n");
    let script = format!(
        "const DATABASE_COPY={};\n{}\n{}",
        serde_json::to_string(&copy).map_err(|_| err("UPDATE_RUNTIME_INPUT_INVALID"))?,
        migration
            .map(|m| format!("const MIGRATION_INPUT={m};"))
            .unwrap_or_default(),
        include_str!("../../../scripts/runtime-probe/database.mjs")
    );
    let db = create(
        vec![
            "--env".into(),
            format!(
                "EXHIBITOS_MANIFEST_SHA256={}",
                ctx.receipt.authenticated_manifest_sha256
            ),
        ],
        Helper {
            id: String::new(),
            name: format!("exhibitos-runtime-db-{nonce}"),
            nonce: nonce.clone(),
            image: maintenance.into(),
            network: "none".into(),
            memory: 1024 * 1024 * 1024,
            mounts: vec![
                (
                    "/source".into(),
                    "volume".into(),
                    ctx.candidate_before.database_volume.clone(),
                ),
                (
                    "/blobs".into(),
                    "volume".into(),
                    ctx.candidate_before.blob_volume.clone(),
                ),
                (
                    "/manifest.json".into(),
                    "bind".into(),
                    ctx.manifest_path.to_string_lossy().into_owned(),
                ),
            ],
            script,
            tmpfs: serde_json::json!({"/snapshot":"rw,nosuid,nodev,noexec,size=1g","/tmp":"rw,nosuid,nodev,noexec,size=64m","/var/lib/postgresql":"rw,nosuid,nodev,noexec,size=1m"}),
        },
    )?;
    let mut runtime: Option<Helper> = None;
    let attempt = (|| {
        command(&["start", &db.id])?;
        let mut ready = false;
        for _ in 0..240 {
            let logs = command(&["logs", &db.id])?;
            if std::str::from_utf8(&logs)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .any(|v| v["ready"] == true)
            {
                let row = std::str::from_utf8(&logs)
                    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
                    .lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .find(|v| v["ready"] == true)
                    .ok_or_else(|| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
                let physical: source_database::DatabaseCopyProof =
                    serde_json::from_value(row["physical"].clone())
                        .map_err(|_| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
                source_database::validate(&physical)?;
                if !candidate_inventory::copies_match(&physical, expected) {
                    return Err(err("UPDATE_TARGET_CHANGED"));
                }
                ready = true;
                break;
            }
            if db.check()?["State"]["Running"] != true {
                return Err(err("UPDATE_RUNTIME_PROBE_FAILED"));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if !ready {
            return Err(err("UPDATE_RUNTIME_PROBE_FAILED"));
        }
        let env = crate::checked_path(ctx.root, "runtime.env")?;
        let script = format!(
            "{}\n{}\n{}\n{}",
            migration
                .map(|m| format!("const MIGRATION_INPUT={m};"))
                .unwrap_or_default(),
            include_str!("../../../scripts/runtime-probe/copy.mjs"),
            include_str!("../../../scripts/runtime-probe/migration.mjs"),
            include_str!("../../../scripts/runtime-probe/target.mjs")
        );
        runtime = Some(create(
            vec![],
            Helper {
                id: String::new(),
                name: format!("exhibitos-runtime-target-{nonce}"),
                nonce: nonce.clone(),
                image: format!("sha256:{}", ctx.plan.target_image),
                network: format!("container:{}", db.id),
                memory: 768 * 1024 * 1024,
                mounts: vec![
                    (
                        "/source-blobs".into(),
                        "volume".into(),
                        ctx.candidate_before.blob_volume.clone(),
                    ),
                    (
                        "/source-config".into(),
                        "volume".into(),
                        ctx.candidate_before.configuration_volume.clone(),
                    ),
                    (
                        "/runtime.env".into(),
                        "bind".into(),
                        env.to_string_lossy().into_owned(),
                    ),
                ],
                script,
                tmpfs: serde_json::json!({"/probe":"rw,nosuid,nodev,noexec,size=256m,mode=0700","/tmp":"rw,nosuid,nodev,noexec,size=64m"}),
            },
        )?);
        let target = runtime
            .as_ref()
            .ok_or_else(|| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
        let raw = engine(&["start".into(), "--attach".into(), target.id.clone()], 360)?;
        let proof: Value =
            serde_json::from_slice(&raw).map_err(|_| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
        validate_proof(&proof, ctx.plan, migration)?;
        for _ in 0..100 {
            if db.check()?["State"]["Running"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let done: Value = std::str::from_utf8(&command(&["logs", &db.id])?)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            .lines()
            .last()
            .and_then(|l| serde_json::from_str(l).ok())
            .ok_or_else(|| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
        if done["completed"] != true
            || done["preflightVerified"] != false
            || done["updateExecuted"] != false
        {
            return Err(err("UPDATE_RUNTIME_PROBE_FAILED"));
        }
        target.retire()?;
        db.retire()?;
        Ok(proof)
    })();
    if attempt.is_err() {
        // Retain new stopped failures, never delete an unknown or original container.
        if let Some(target) = &runtime {
            target.stop()?;
        }
        db.stop()?;
    }
    attempt
}
fn validate_proof(v: &Value, p: &crate::update::Plan, migration: Option<&Value>) -> Result<()> {
    let valid = v["uid"] == 1000
        && v["originalMountsReadOnly"] == true
        && v["preflightVerified"] == false
        && v["updateExecuted"] == false
        && v["health"]["status"] == "ok"
        && v["health"]["service"] == "exhibitos-api"
        && v["health"]["version"] == "0.1.0"
        && v["copiedBlobBytes"]
            .as_u64()
            .is_some_and(|b| b <= 128 * 1024 * 1024)
        && v["copiedConfigurationBytes"]
            .as_u64()
            .is_some_and(|b| b <= 8 * 1024 * 1024)
        && v["web"]["bytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 16 * 1024 * 1024)
        && v["web"]["sha256"].as_str().is_some_and(hash_valid)
        && v["readiness"].as_array().is_some_and(|rs| {
            rs.len() == 3
                && rs.iter().all(|r| {
                    r["ready"] == true
                        && r["protocolVersion"] == "1"
                        && r["services"].as_array().is_some_and(|ss| {
                            ss.len() == 5 && ss.iter().all(|s| s["status"] == "ready")
                        })
                })
        });
    if !valid {
        return Err(err("UPDATE_RUNTIME_PROBE_FAILED"));
    }
    if let Some(input) = migration {
        return validate_migrated_inventory(&v["logical"], p, input);
    }
    let inventory: source_inventory::InventoryProof = serde_json::from_value(v["logical"].clone())
        .map_err(|_| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
    source_inventory::matched_candidate(&inventory, p)
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MigratedInventory {
    operation: String,
    backup_id: String,
    authenticated_manifest_sha256: String,
    source_inventory_sha256: String,
    source_schema_sha256: String,
    target_schema_sha256: String,
    target_migrations_sha256: String,
    observed_at: String,
    original_data_preserved: bool,
    current_inventory_verified: bool,
    configuration_verified: bool,
    preflight_verified: bool,
    update_executed: bool,
}
fn validate_migrated_inventory(v: &Value, p: &crate::update::Plan, input: &Value) -> Result<()> {
    let proof: MigratedInventory = serde_json::from_value(v.clone())
        .map_err(|_| err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"))?;
    if p.source_schema == p.target_schema
        || proof.operation != "migrated-inventory-preserved"
        || proof.backup_id != p.backup_id
        || proof.authenticated_manifest_sha256 != p.backup_manifest
        || proof.source_inventory_sha256 != p.source_inventory
        || proof.source_schema_sha256 != p.source_schema
        || proof.target_schema_sha256 != p.target_schema
        || proof.target_migrations_sha256 != input["targetMigrationsSha256"]
        || !hash_valid(&proof.target_migrations_sha256)
        || proof.observed_at.is_empty()
        || proof.observed_at.len() > 40
        || !proof.original_data_preserved
        || proof.current_inventory_verified
        || proof.configuration_verified
        || proof.preflight_verified
        || proof.update_executed
    {
        return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));
    }
    Ok(())
}
impl ExecutionSession<'_> {
    pub(super) fn qualify_runtime_artifact(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
    ) -> Result<PreparedOciReceipt> {
        self.reverify_prepared_artifact(artifact)?;
        if artifact.plan.source_schema != artifact.plan.target_schema {
            return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));
        }
        let oci = self.qualify_prepared_oci(artifact, python, source_commit, false, true)?;
        let image = format!("sha256:{}", artifact.plan.target_image);
        let observed = crate::backup_creation::inspected(
            "docker",
            &["image".into(), "inspect".into(), image.clone()],
        )?;
        if observed["Id"] != image
            || observed["Os"] != "linux"
            || observed["Architecture"] != "arm64"
            || observed["RootFS"]["Layers"] != oci.proof["layerDiffIds"]
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
        Ok(oci)
    }
    pub(super) fn observe_runtime_at(
        &self,
        artifact: &mut PreparedArtifact,
        ctx: &candidate_inventory::CandidateContext<'_>,
        maintenance: &str,
        oci: &PreparedOciReceipt,
    ) -> Result<Value> {
        self.observe_runtime_at_inner(artifact, ctx, maintenance, oci, None)
    }
    pub(super) fn observe_runtime_at_inner(
        &self,
        artifact: &mut PreparedArtifact,
        ctx: &candidate_inventory::CandidateContext<'_>,
        maintenance: &str,
        oci: &PreparedOciReceipt,
        migration: Option<&Value>,
    ) -> Result<Value> {
        self.reverify_prepared_artifact(artifact)?;
        if crate::backup_creation::local_image("docker", maintenance)? != maintenance {
            return Err(err("IMAGE_INTEGRITY"));
        }
        let files = source_deployment::authenticated_files(
            &self.source.root,
            ctx.workspace,
            ctx.receipt,
            ctx.plan,
        )?;
        let env = crate::installation_backup::source_bytes(ctx.root, "runtime.env", 8192, true)?;
        let expected = source_configuration::expected(ctx.raw)?;
        let key = source_configuration::observe(
            maintenance,
            &ctx.candidate_before.configuration_volume,
            &expected,
        )?;
        let source = ephemeral_inventory::observe_source(ctx, maintenance)?;
        let candidate = ephemeral_inventory::observe(ctx, maintenance)?;
        let proof = run_probe(ctx, maintenance, &candidate.physical, migration)?;
        let candidate_after = ephemeral_inventory::observe(ctx, maintenance)?;
        let source_after = ephemeral_inventory::observe_source(ctx, maintenance)?;
        if !candidate_inventory::copies_match(&source.physical, &source_after.physical)
            || !candidate_inventory::copies_match(&candidate.physical, &candidate_after.physical)
            || files
                != source_deployment::authenticated_files(
                    &self.source.root,
                    ctx.workspace,
                    ctx.receipt,
                    ctx.plan,
                )?
            || env != crate::installation_backup::source_bytes(ctx.root, "runtime.env", 8192, true)?
            || key
                != source_configuration::observe(
                    maintenance,
                    &ctx.candidate_before.configuration_volume,
                    &expected,
                )?
        {
            return Err(err("UPDATE_SOURCE_CHANGED"));
        }
        Ok(
            serde_json::json!({"sourceBefore":source,"candidateBefore":candidate,"runtime":proof,"candidateAfter":candidate_after,"sourceAfter":source_after,"oci":oci,"preflightVerified":false,"updateExecuted":false,"imageOnlyRollbackVerified":false}),
        )
    }
    /// No caller-supplied success flags, plan, image override, journal transition or activation.
    pub fn with_runtime_compatibility<T>(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
        maintenance: &str,
        acknowledged: bool,
        work: impl FnOnce(&RuntimeCompatibility) -> Result<T>,
    ) -> Result<T> {
        if !acknowledged {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !maintenance.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        // This bounded initial development observer supports unchanged-schema releases only.
        let oci = self.qualify_runtime_artifact(artifact, python, source_commit)?;
        self.with_runtime_compatibility_inner(artifact, maintenance, oci, None, work)
    }
    /// Native changed-schema observation only, never an admission/application permit.
    /// Catalog input pins bytes; actual original/target SQL and data are observed afresh.
    #[allow(clippy::too_many_arguments)]
    pub fn with_migrated_runtime_compatibility<T>(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
        maintenance: &str,
        catalog: &Path,
        catalog_sha256: &str,
        acknowledged: bool,
        work: impl FnOnce(&RuntimeCompatibility) -> Result<T>,
    ) -> Result<T> {
        if !acknowledged {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !maintenance.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let (oci, mut held, input) = self.qualify_migrated_runtime_artifact(
            artifact,
            python,
            source_commit,
            catalog,
            catalog_sha256,
        )?;
        let result =
            self.with_runtime_compatibility_inner(artifact, maintenance, oci, Some(input), work);
        held.recheck()?;
        result
    }
    pub(super) fn qualify_migrated_runtime_artifact(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
        catalog: &Path,
        catalog_sha256: &str,
    ) -> Result<(
        PreparedOciReceipt,
        super::migration_catalog::CatalogInput,
        Value,
    )> {
        if artifact.plan.source_schema == artifact.plan.target_schema {
            return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));
        }
        let mut held = super::migration_catalog::CatalogInput::read(
            catalog,
            catalog_sha256,
            &artifact.plan.target_schema,
        )?;
        let oci = self.qualify_prepared_oci_with_catalog(
            artifact,
            python,
            source_commit,
            catalog,
            catalog_sha256,
            true,
        )?;
        let image = crate::backup_creation::inspected(
            "docker",
            &[
                "image".into(),
                "inspect".into(),
                format!("sha256:{}", artifact.plan.target_image),
            ],
        )?;
        if image["Id"] != format!("sha256:{}", artifact.plan.target_image)
            || image["Os"] != "linux"
            || image["Architecture"] != "arm64"
            || image["RootFS"]["Layers"] != oci.proof["layerDiffIds"]
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
        held.recheck()?;
        let input = serde_json::json!({
            "catalog": serde_json::from_str::<Value>(&held.text()?).map_err(|_|err("UPDATE_CATALOG_INVALID"))?,
            "manifestSha256": artifact.plan.backup_manifest,
            "sourceSchemaSha256": artifact.plan.source_schema,
            "targetSchemaSha256": artifact.plan.target_schema,
            "targetMigrationsSha256": oci.proof["targetMigrationsSha256"],
        });
        Ok((oci, held, input))
    }
    fn with_runtime_compatibility_inner<T>(
        &self,
        artifact: &mut PreparedArtifact,
        maintenance: &str,
        oci: PreparedOciReceipt,
        migration: Option<Value>,
        work: impl FnOnce(&RuntimeCompatibility) -> Result<T>,
    ) -> Result<T> {
        let artifact_cell = std::cell::RefCell::new(&mut *artifact);
        let mut outcome = None;
        self.inspect_restored_candidate_finalized(
            true,
            |ctx| {
                self.observe_runtime_at_inner(
                    &mut artifact_cell.borrow_mut(),
                    ctx,
                    maintenance,
                    &oci,
                    migration.as_ref(),
                )
            },
            |ctx, receipt| {
                self.reverify_prepared_artifact(&mut artifact_cell.borrow_mut())?;
                let held = artifact_cell.borrow();
                receipt["operationId"] = serde_json::json!(held.plan.operation_id);
                receipt["trustGeneration"] = serde_json::json!(held.generation);
                let proof = RuntimeCompatibility {
                    plan: held.plan.clone(),
                    generation: held.generation,
                    envelope: held.envelope.clone(),
                    receipt: receipt.clone(),
                };
                if proof.plan != *ctx.plan
                    || proof.generation != held.generation
                    || proof.envelope != held.envelope
                {
                    return Err(err("UPDATE_RUNTIME_PROOF_STALE"));
                }
                drop(held);
                // Borrowed opaque proof cannot outlive this callback or either operation lock.
                outcome = Some(work(&proof)?);
                self.reverify_prepared_artifact(&mut artifact_cell.borrow_mut())?;
                Ok(())
            },
        )?;
        outcome.ok_or_else(|| err("UPDATE_RUNTIME_PROBE_FAILED"))
    }
    /// Diagnostic receipt only; the typed proof is never exported after locks release.
    pub fn qualify_runtime_compatibility(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
        maintenance: &str,
        acknowledged: bool,
    ) -> Result<Value> {
        self.with_runtime_compatibility(
            artifact,
            python,
            source_commit,
            maintenance,
            acknowledged,
            |proof| Ok(proof.receipt().clone()),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrated_receipt_requires_original_data_and_exact_plan_catalog_identity() {
        let p = crate::update::Plan {
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
        let input = serde_json::json!({"targetMigrationsSha256":"0".repeat(64)});
        let valid = serde_json::json!({"operation":"migrated-inventory-preserved","backupId":p.backup_id,"authenticatedManifestSha256":p.backup_manifest,"sourceInventorySha256":p.source_inventory,"sourceSchemaSha256":p.source_schema,"targetSchemaSha256":p.target_schema,"targetMigrationsSha256":"0".repeat(64),"observedAt":"2026-10-06T00:00:00.000Z","originalDataPreserved":true,"currentInventoryVerified":false,"configurationVerified":false,"preflightVerified":false,"updateExecuted":false});
        validate_migrated_inventory(&valid, &p, &input).unwrap();
        for key in [
            "backupId",
            "authenticatedManifestSha256",
            "sourceInventorySha256",
            "sourceSchemaSha256",
            "targetSchemaSha256",
            "targetMigrationsSha256",
            "operation",
        ] {
            let mut v = valid.clone();
            v[key] = "foreign".into();
            assert!(
                validate_migrated_inventory(&v, &p, &input).is_err(),
                "{key}"
            );
        }
        for key in [
            "originalDataPreserved",
            "currentInventoryVerified",
            "configurationVerified",
            "preflightVerified",
            "updateExecuted",
        ] {
            let mut v = valid.clone();
            v[key] = (!v[key].as_bool().unwrap()).into();
            assert!(
                validate_migrated_inventory(&v, &p, &input).is_err(),
                "{key}"
            );
        }
        let mut foreign = valid.clone();
        foreign["callerSuccess"] = true.into();
        assert!(validate_migrated_inventory(&foreign, &p, &input).is_err());
        let mut same = p.clone();
        same.target_schema = same.source_schema.clone();
        assert!(validate_migrated_inventory(&valid, &same, &input).is_err());
    }
    #[test]
    #[ignore = "explicit retained stopped synthetic candidate and cached genuine migrated runtime; scratch tmpfs only"]
    fn actual_native_migrated_probe_preserves_retained_candidate() {
        let input =
            |name| std::env::var(name).expect("explicit actual retained qualification input");
        let fixture = fs::canonicalize(input("EXHIBITOS_MIGRATED_FIXTURE")).unwrap();
        let profile = fixture.join("profile");
        let store = super::super::super::Store::open(&profile, "default").unwrap();
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::RolledBack
        );
        let generation = store.current.generation;
        let mut plan = store.intent().unwrap().update.plan().clone();
        let id = store.intent().unwrap().update.restore_candidate().unwrap();
        let root = profile.join("installations").join(id);
        let target = LifecycleService::open_retry_diagnostics(root.clone()).unwrap();
        let _lock = target.lock().unwrap();
        plan.target_instance = id.to_owned();
        plan.target_image = input("EXHIBITOS_MIGRATED_IMAGE");
        plan.target_schema = input("EXHIBITOS_MIGRATED_SCHEMA");
        let manifest_path = PathBuf::from(input("EXHIBITOS_MIGRATED_MANIFEST"));
        let workspace = manifest_path.parent().unwrap().parent().unwrap();
        let raw = fs::read(&manifest_path).unwrap();
        assert_eq!(crate::digest(&raw), plan.backup_manifest);
        let receipt: crate::restoration::RestorationReceipt =
            crate::read_json(&workspace.join("receipt.json")).unwrap();
        let manifest = target.manifest().unwrap();
        let before = source_stopped::observe(&target, &plan).unwrap();
        let ctx = candidate_inventory::CandidateContext {
            target: &target,
            root: &root,
            workspace,
            raw: &raw,
            manifest_path: &manifest_path,
            manifest: &manifest,
            receipt: &receipt,
            plan: &plan,
            source_before: &before,
            candidate_before: &before,
        };
        let maintenance = input("EXHIBITOS_MIGRATED_MAINTENANCE");
        let catalog_path = PathBuf::from(input("EXHIBITOS_MIGRATED_CATALOG"));
        let mut catalog = super::super::migration_catalog::CatalogInput::read(
            &catalog_path,
            &input("EXHIBITOS_MIGRATED_CATALOG_PIN"),
            &plan.target_schema,
        )
        .unwrap();
        let catalog_json: Value = serde_json::from_str(&catalog.text().unwrap()).unwrap();
        let payload = serde_json::json!({"catalog":catalog_json,"manifestSha256":plan.backup_manifest,"sourceSchemaSha256":plan.source_schema,"targetSchemaSha256":plan.target_schema,"targetMigrationsSha256":input("EXHIBITOS_MIGRATED_SQL_PIN")});
        let physical = ephemeral_inventory::observe(&ctx, &maintenance).unwrap();
        let proof = run_probe(&ctx, &maintenance, &physical.physical, Some(&payload)).unwrap();
        let after = ephemeral_inventory::observe(&ctx, &maintenance).unwrap();
        assert!(candidate_inventory::copies_match(
            &physical.physical,
            &after.physical
        ));
        assert_eq!(
            physical.inventory.inventory_sha256,
            after.inventory.inventory_sha256
        );
        assert_eq!(fs::read(&manifest_path).unwrap(), raw);
        catalog.recheck().unwrap();
        assert_eq!(store.current.generation, generation);
        source_stopped::observe(&target, &plan).unwrap();
        let mut unchanged_plan = plan.clone();
        unchanged_plan.target_schema = unchanged_plan.source_schema.clone();
        unchanged_plan.target_image = before.runtime_image_sha256.clone();
        let unchanged_ctx = candidate_inventory::CandidateContext {
            plan: &unchanged_plan,
            ..ctx
        };
        let unchanged = run_probe(&unchanged_ctx, &maintenance, &after.physical, None).unwrap();
        let final_physical = ephemeral_inventory::observe(&unchanged_ctx, &maintenance).unwrap();
        assert!(candidate_inventory::copies_match(
            &after.physical,
            &final_physical.physical
        ));
        assert_eq!(
            after.inventory.inventory_sha256,
            final_physical.inventory.inventory_sha256
        );
        source_stopped::observe(&target, &plan).unwrap();
        let output = PathBuf::from(input("EXHIBITOS_MIGRATED_REPORT"));
        let mut file = private_file(&output, true).unwrap();
        file.write_all(&serde_json::to_vec_pretty(&serde_json::json!({"scope":"actual compiled native helper/copy/SQL/runtime/preservation component; not positive ExecutionSession/admission/activation","runtime":proof,"unchangedRuntimeRegression":unchanged,"before":physical,"after":after,"finalPhysical":final_physical,"originalManifestSha256":crate::digest(&raw),"authorityGenerationUnchanged":generation,"preflightVerified":false,"updateExecuted":false})).unwrap()).unwrap();
        file.sync_all().unwrap();
    }
    #[test]
    #[ignore = "explicit signed genuine artifact, local Docker and retained encrypted backup; creates one fresh complete profile/candidate"]
    fn actual_public_signed_migrated_session_preserves_original_and_prepared_intent() {
        let input = |name| std::env::var(name).expect("explicit qualification input");
        let backup = fs::canonicalize(input("EXHIBITOS_SIGNED_BACKUP_FIXTURE")).unwrap();
        let original = backup.join("retained-source-manager");
        let creation: Value =
            serde_json::from_slice(&fs::read(backup.join("backup-creation-report.json")).unwrap())
                .unwrap();
        let archive = original.join(format!(
            "backup-creation-{}/archive",
            creation["receipt"]["id"].as_str().unwrap()
        ));
        let key = backup.join("key.bin");
        let template: Value =
            serde_json::from_slice(&fs::read(input("EXHIBITOS_SIGNED_PLAN_REPORT")).unwrap())
                .unwrap();
        let mut plan: crate::update::Plan =
            serde_json::from_value(template["plan"].clone()).unwrap();
        plan.operation_id = uuid::Uuid::new_v4().to_string();
        plan.source_instance = uuid::Uuid::new_v4().to_string();
        plan.target_instance = uuid::Uuid::new_v4().to_string();
        plan.target_image = input("EXHIBITOS_MIGRATED_IMAGE");
        plan.target_schema = input("EXHIBITOS_MIGRATED_SCHEMA");
        let artifact_path = fs::canonicalize(input("EXHIBITOS_SIGNED_ARTIFACT")).unwrap();
        let (profile, signing, mut policy, mut release) = super::super::super::tests::fixture();
        let root = profile.parent().unwrap();
        println!("PRIVATE_SIGNED_SESSION_ROOT={}", root.display());
        let source = profile.join("local-runtime");
        installations::new_directory(&source).unwrap();
        installations::new_directory(&source.join("bundle")).unwrap();
        let names = [
            "installed.json",
            "engine.json",
            "runtime.env",
            "bundle/manifest.json",
            "bundle/compose.yaml",
        ];
        let before = names
            .iter()
            .map(|n| (n, fs::read(original.join(n)).unwrap()))
            .collect::<Vec<_>>();
        for (n, raw) in &before {
            crate::restoration::private_bytes(&source.join(n), raw).unwrap();
        }
        installations::save(
            &profile,
            &installations::Registry {
                format: 1,
                active_id: plan.source_instance.clone(),
                installations: vec![installations::Entry {
                    id: plan.source_instance.clone(),
                    kind: "default".into(),
                    created_at: 1,
                }],
            },
            None,
        )
        .unwrap();
        let now = release_now().unwrap();
        policy.source_schema_sha256 = plan.source_schema.clone();
        policy.minimum_issued_at = now - 10;
        release.source_schemas = vec![plan.source_schema.clone()];
        release.version = "0.1.2-dev.1".into();
        release.issued_at = now - 1;
        release.expires_at = now + 3600;
        release.artifact.runtime_image_sha256 = plan.target_image.clone();
        release.artifact.schema_sha256 = plan.target_schema.clone();
        let artifact_raw = fs::read(&artifact_path).unwrap();
        release.artifact.bytes = artifact_raw.len() as u64;
        release.artifact.sha256 = crate::digest(&artifact_raw);
        drop(artifact_raw);
        let envelope = super::super::super::tests::seal(&signing, &release);
        let mut store =
            super::super::super::Store::provision(&profile, "default", policy, now).unwrap();
        // Registration creates its own fresh identity; bind the plan before preparation.
        let registered = store.register_update_target(true).unwrap();
        plan.target_instance = registered.target_instance;
        let mut verified = store.verify_for_preparation(&envelope, now).unwrap();
        verified
            .verify_artifact(&mut File::open(&artifact_path).unwrap())
            .unwrap();
        store
            .prepare_update(&envelope, &verified, plan.clone(), now)
            .unwrap();
        let port = {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        let maintenance = input("EXHIBITOS_MIGRATED_MAINTENANCE");
        let restored = store
            .execution()
            .unwrap()
            .prepare_target_candidate(&maintenance, &key, &archive, port, true)
            .unwrap();
        let target_root = profile.join("installations").join(&plan.target_instance);
        let target = LifecycleService::open_retry_diagnostics(target_root.clone()).unwrap();
        target.execute(crate::Action::Stop).unwrap();
        let generation = store.current.generation;
        let staging = root.join("staging");
        installations::new_directory(&staging).unwrap();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact(&artifact_path, &staging)
            .unwrap();
        let catalog = PathBuf::from(input("EXHIBITOS_MIGRATED_CATALOG"));
        let pin = input("EXHIBITOS_MIGRATED_CATALOG_PIN");
        let python = PathBuf::from(input("EXHIBITOS_SIGNED_PYTHON"));
        let revision = input("EXHIBITOS_SIGNED_REVISION");
        assert_eq!(
            session
                .with_migrated_runtime_compatibility(
                    &mut artifact,
                    &python,
                    &revision,
                    &maintenance,
                    &catalog,
                    &pin,
                    false,
                    |_| Ok(())
                )
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        let receipt = session
            .with_migrated_runtime_compatibility(
                &mut artifact,
                &python,
                &revision,
                &maintenance,
                &catalog,
                &pin,
                true,
                |proof| Ok(proof.receipt().clone()),
            )
            .unwrap();
        assert_eq!(receipt["trustGeneration"], generation);
        assert_eq!(receipt["operationId"], plan.operation_id);
        drop(session);
        assert_eq!(store.current.generation, generation);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        assert_eq!(
            installations::load(&profile).unwrap().unwrap().0.active_id,
            plan.source_instance
        );
        for (name, bytes) in before {
            assert_eq!(fs::read(original.join(name)).unwrap(), bytes);
            assert_eq!(fs::read(source.join(name)).unwrap(), bytes);
        }
        let result = serde_json::json!({"status":"PASS","scope":"actual public signed Prepared ExecutionSession and distinct native encrypted restored candidate; not update admission/application or failed recovery","profile":profile,"source":source,"candidate":target_root,"plan":plan,"restoration":restored,"receipt":receipt,"authorityGenerationUnchanged":generation,"preparedIntentUnchanged":true,"originalSelectionPreserved":true,"originalFiveFilesPreserved":true,"preflightVerified":false,"updateExecuted":false});
        crate::restoration::private_bytes(
            &root.join("report.json"),
            &serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("PASS_PUBLIC_SIGNED_MIGRATED_SESSION");
    }
    #[test]
    #[ignore = "explicit already-created complete signed Prepared fixture; reuse candidate without restoration or host copies"]
    fn actual_public_signed_migrated_session_reopens_prepared_fixture() {
        let input = |name| std::env::var(name).expect("explicit qualification input");
        let root = fs::canonicalize(input("EXHIBITOS_SIGNED_SESSION_ROOT")).unwrap();
        let profile = root.join("profile");
        let mut store = super::super::super::Store::open(&profile, "default").unwrap();
        let plan = store.intent().unwrap().update.plan().clone();
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        let generation = store.current.generation;
        let original = fs::canonicalize(input("EXHIBITOS_SIGNED_BACKUP_FIXTURE"))
            .unwrap()
            .join("retained-source-manager");
        let source = profile.join("local-runtime");
        let names = [
            "installed.json",
            "engine.json",
            "runtime.env",
            "bundle/manifest.json",
            "bundle/compose.yaml",
        ];
        let before = names
            .iter()
            .map(|n| (*n, fs::read(original.join(n)).unwrap()))
            .collect::<Vec<_>>();
        let staging = root.join(format!("session-staging-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&staging).unwrap();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact(&PathBuf::from(input("EXHIBITOS_SIGNED_ARTIFACT")), &staging)
            .unwrap();
        let receipt = session
            .with_migrated_runtime_compatibility(
                &mut artifact,
                &PathBuf::from(input("EXHIBITOS_SIGNED_PYTHON")),
                &input("EXHIBITOS_SIGNED_REVISION"),
                &input("EXHIBITOS_MIGRATED_MAINTENANCE"),
                &PathBuf::from(input("EXHIBITOS_MIGRATED_CATALOG")),
                &input("EXHIBITOS_MIGRATED_CATALOG_PIN"),
                true,
                |proof| Ok(proof.receipt().clone()),
            )
            .unwrap();
        drop(session);
        assert_eq!(receipt["trustGeneration"], generation);
        assert_eq!(store.current.generation, generation);
        assert_eq!(store.intent().unwrap().update.plan(), &plan);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        assert_eq!(
            installations::load(&profile).unwrap().unwrap().0.active_id,
            plan.source_instance
        );
        for (name, raw) in before {
            assert_eq!(fs::read(original.join(name)).unwrap(), raw);
            assert_eq!(fs::read(source.join(name)).unwrap(), raw);
        }
        crate::restoration::private_bytes(&root.join("reopened-session-report.json"),&serde_json::to_vec_pretty(&serde_json::json!({"status":"PASS","scope":"actual reopened public signed Prepared ExecutionSession; no admission/application/activation","receipt":receipt,"plan":plan,"authorityGenerationUnchanged":generation,"preparedIntentUnchanged":true,"originalSelectionPreserved":true,"originalFiveFilesPreserved":true,"preflightVerified":false,"updateExecuted":false})).unwrap()).unwrap();
    }
    #[test]
    fn inspected_helper_refuses_writable_sources_and_privilege_network_or_budget_changes() {
        let h = Helper {
            id: "a".repeat(64),
            name: "owned-probe".into(),
            nonce: "nonce".into(),
            image: format!("sha256:{}", "b".repeat(64)),
            network: "none".into(),
            memory: 1024,
            mounts: vec![("/source".into(), "volume".into(), "own-volume".into())],
            script: "trusted".into(),
            tmpfs: serde_json::json!({"/snapshot":"rw,nosuid,nodev,noexec,size=1g"}),
        };
        let valid = serde_json::json!({"Id":h.id,"Name":"/owned-probe","Image":h.image,"Config":{"Labels":{"com.exhibitos.runtime.probe":"nonce"},"User":"0:0","Entrypoint":["node"],"Cmd":["--input-type=module","-e","trusted"]},"HostConfig":{"NetworkMode":"none","ReadonlyRootfs":true,"Privileged":false,"Memory":1024,"MemorySwap":1024,"PidsLimit":128,"CapDrop":["ALL"],"CapAdd":["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FOWNER","CAP_KILL","CAP_SETGID","CAP_SETUID"],"SecurityOpt":["no-new-privileges:true"],"Tmpfs":h.tmpfs,"PublishAllPorts":false,"PortBindings":{},"Devices":null,"DeviceRequests":null,"VolumesFrom":null,"Binds":null},"Mounts":[{"Destination":"/source","Type":"volume","Name":"own-volume","RW":false}]});
        assert!(h.matches(&valid));
        for (pointer, value) in [
            ("/Mounts/0/RW", serde_json::json!(true)),
            ("/Mounts/0/Name", serde_json::json!("foreign")),
            ("/HostConfig/NetworkMode", serde_json::json!("host")),
            ("/HostConfig/Privileged", serde_json::json!(true)),
            ("/HostConfig/ReadonlyRootfs", serde_json::json!(false)),
            ("/HostConfig/MemorySwap", serde_json::json!(2048)),
            ("/HostConfig/Tmpfs", serde_json::json!({})),
            (
                "/HostConfig/PortBindings",
                serde_json::json!({"8080/tcp":[{"HostPort":"8080"}]}),
            ),
            ("/Config/Cmd", serde_json::json!(["sh", "foreign"])),
            ("/Config/User", serde_json::json!("root")),
        ] {
            let mut v = valid.clone();
            *v.pointer_mut(pointer).unwrap() = value;
            assert!(!h.matches(&v), "{pointer}");
        }
        let mut extra = valid.clone();
        extra["Mounts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"Destination":"/foreign","Type":"bind","RW":true}));
        assert!(!h.matches(&extra));
    }
}
