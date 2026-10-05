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
        "const DATABASE_COPY={};\n{}",
        serde_json::to_string(&copy).map_err(|_| err("UPDATE_RUNTIME_INPUT_INVALID"))?,
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
            "{}\n{}",
            include_str!("../../../scripts/runtime-probe/copy.mjs"),
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
        validate_proof(&proof, ctx.plan)?;
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
fn validate_proof(v: &Value, p: &crate::update::Plan) -> Result<()> {
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
    let inventory: source_inventory::InventoryProof = serde_json::from_value(v["logical"].clone())
        .map_err(|_| err("UPDATE_RUNTIME_PROBE_FAILED"))?;
    source_inventory::matched_candidate(&inventory, p)
}
impl ExecutionSession<'_> {
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
        let artifact_cell = std::cell::RefCell::new(&mut *artifact);
        let mut outcome = None;
        self.inspect_restored_candidate_finalized(true,|ctx| {
            self.reverify_prepared_artifact(&mut artifact_cell.borrow_mut())?;
            if crate::backup_creation::local_image("docker",maintenance)?!=maintenance {return Err(err("IMAGE_INTEGRITY"));}
            let files=source_deployment::authenticated_files(&self.source.root,ctx.workspace,ctx.receipt,ctx.plan)?;
            let env=crate::installation_backup::source_bytes(ctx.root,"runtime.env",8192,true)?;
            let expected=source_configuration::expected(ctx.raw)?;
            let key=source_configuration::observe(maintenance,&ctx.candidate_before.configuration_volume,&expected)?;
            let source=ephemeral_inventory::observe_source(ctx,maintenance)?;
            let candidate=ephemeral_inventory::observe(ctx,maintenance)?;
            let proof=run_probe(ctx,maintenance,&candidate.physical)?;
            let candidate_after=ephemeral_inventory::observe(ctx,maintenance)?;
            let source_after=ephemeral_inventory::observe_source(ctx,maintenance)?;
            if !candidate_inventory::copies_match(&source.physical,&source_after.physical) || !candidate_inventory::copies_match(&candidate.physical,&candidate_after.physical)
                || files!=source_deployment::authenticated_files(&self.source.root,ctx.workspace,ctx.receipt,ctx.plan)?
                || env!=crate::installation_backup::source_bytes(ctx.root,"runtime.env",8192,true)?
                || key!=source_configuration::observe(maintenance,&ctx.candidate_before.configuration_volume,&expected)? {return Err(err("UPDATE_SOURCE_CHANGED"));}
            Ok(serde_json::json!({"sourceBefore":source,"candidateBefore":candidate,"runtime":proof,"candidateAfter":candidate_after,"sourceAfter":source_after,"oci":oci,"preflightVerified":false,"updateExecuted":false,"imageOnlyRollbackVerified":false}))
        },|ctx,receipt|{
            self.reverify_prepared_artifact(&mut artifact_cell.borrow_mut())?;
            let held=artifact_cell.borrow();
            receipt["operationId"]=serde_json::json!(held.plan.operation_id);
            receipt["trustGeneration"]=serde_json::json!(held.generation);
            let proof=RuntimeCompatibility{plan:held.plan.clone(),generation:held.generation,envelope:held.envelope.clone(),receipt:receipt.clone()};
            if proof.plan != *ctx.plan || proof.generation != held.generation || proof.envelope != held.envelope {return Err(err("UPDATE_RUNTIME_PROOF_STALE"));}
            drop(held);
            // Borrowed opaque proof cannot outlive this callback or either operation lock.
            outcome=Some(work(&proof)?);
            self.reverify_prepared_artifact(&mut artifact_cell.borrow_mut())?;
            Ok(())
        })?;
        drop(artifact_cell);
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
