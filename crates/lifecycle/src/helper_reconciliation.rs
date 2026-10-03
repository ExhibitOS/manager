// SPDX-License-Identifier: Apache-2.0
//! Reconcile failed maintenance helpers; retain every candidate and volume. Engine-configured --rm may remove a stopped helper.
use super::backup_creation::BackupCreationJob;
use super::installation_backup::source_bytes;
use super::restoration::RestorationJob;
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconciliationJob {
    pub id: String,
    pub target_id: String,
    pub kind: String,
    pub state: String,
    pub helper_state: Option<String>,
    pub error_code: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconciliationReceipt {
    pub id: String,
    pub target_id: String,
    pub kind: String,
    pub operation: String,
    pub helper_state: String,
    pub data_preserved: bool,
    pub writers_resumed: bool,
    pub at: u64,
}
fn uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn container_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn names(kind: &str, id: &str) -> Result<(String, String)> {
    let (prefix, label) = match kind {
        "backup" => ("exhibitos-backup", "com.exhibitos.backup"),
        "restoration" => ("exhibitos-restore", "com.exhibitos.restoration"),
        _ => return Err(err("RECONCILIATION_INPUT_INVALID")),
    };
    if !uuid(id) {
        return Err(err("RECONCILIATION_INPUT_INVALID"));
    }
    Ok((format!("{prefix}-{id}"), label.into()))
}
fn validate_record(job: &ReconciliationJob) -> Result<()> {
    names(&job.kind, &job.target_id)?;
    if !uuid(&job.id)
        || !["checking", "completed", "failed", "interrupted"].contains(&job.state.as_str())
        || job.state == "checking" && job.error_code.is_some()
        || ["failed", "interrupted"].contains(&job.state.as_str()) && job.error_code.is_none()
        || job.created_at > job.updated_at
        || job.updated_at > 8_640_000_000_000_000
        || job.error_code.as_ref().is_some_and(|code| {
            code.is_empty()
                || code.len() > 128
                || !code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
        })
        || if job.state == "completed" {
            job.error_code.is_some()
                || !job
                    .helper_state
                    .as_deref()
                    .is_some_and(|s| ["absent", "stopped"].contains(&s))
        } else {
            job.helper_state.is_some()
        }
    {
        return Err(err("STATE_INVALID"));
    }
    Ok(())
}
fn list(
    mut command: impl FnMut(&[String]) -> Result<Vec<u8>>,
    name: &str,
    label: &str,
    id: &str,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for filter in [format!("name={name}"), format!("label={label}={id}")] {
        let bytes = command(&[
            "ps".into(),
            "--all".into(),
            "--no-trunc".into(),
            "--filter".into(),
            filter,
            "--format".into(),
            "{{.ID}}".into(),
        ])?;
        if bytes.len() > 16 * 1024 {
            return Err(err("ENGINE_OUTPUT_LIMIT"));
        }
        let text = String::from_utf8(bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        for id in text.lines() {
            if !container_id(id) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            ids.insert(id.into());
        }
        if ids.len() > 128 {
            return Err(err("ENGINE_OUTPUT_LIMIT"));
        }
    }
    if ids.len() > 1 {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(ids)
}
fn validate_helper(
    service: &LifecycleService,
    kind: &str,
    target: &str,
    cid: &str,
    value: &Value,
    declared: &BTreeSet<String>,
) -> Result<bool> {
    let (name, label) = names(kind, target)?;
    if !container_id(cid)
        || value["Id"] != cid
        || value["Name"] != format!("/{name}")
        || value["Config"]["Labels"][&label] != target
        || value["HostConfig"]["Privileged"] != false
        || value["HostConfig"]["ReadonlyRootfs"] != true
        || !value["HostConfig"]["AutoRemove"].is_boolean()
        || !value["Image"]
            .as_str()
            .is_some_and(|s| s.strip_prefix("sha256:").is_some_and(hash_valid))
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let mounts = value["Mounts"]
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
    let expected_work = if kind == "backup" {
        format!("exhibitos-backup-work-{target}")
    } else {
        service
            .root
            .join(format!("restore-{target}"))
            .to_string_lossy()
            .into()
    };
    let mut work = 0;
    let mut destinations = BTreeSet::new();
    for mount in mounts {
        let dest = mount["Destination"]
            .as_str()
            .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
        if !destinations.insert(dest) {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        let rw = mount["RW"]
            .as_bool()
            .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
        if dest == "/work" {
            let valid = if kind == "backup" {
                mount["Type"] == "volume" && mount["Name"] == expected_work
            } else {
                mount["Type"] == "bind" && mount["Source"] == expected_work
            };
            if !valid || !rw {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            work += 1;
        } else if rw {
            if declared.contains(dest)
                && mount["Type"] == "volume"
                && mount["Name"].as_str().is_some_and(container_id)
                && mount["Driver"] == "local"
            {
                continue;
            }
            if mount["Type"] == "tmpfs" && dest == "/tmp" {
                continue;
            }
            if kind != "restoration"
                || mount["Type"] != "volume"
                || !["/data/blobs", "/data/config"].contains(&dest)
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            let manifest = service.manifest()?;
            let suffix = if dest == "/data/blobs" {
                "objects"
            } else {
                "configuration"
            };
            if mount["Name"] != format!("{}_{suffix}", manifest.project_name) {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
        }
    }
    if work != 1 {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    value["State"]["Running"]
        .as_bool()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))
}

fn declared_volumes(
    command: &mut impl FnMut(&[String]) -> Result<Vec<u8>>,
    helper: &Value,
) -> Result<BTreeSet<String>> {
    let bytes = command(&[
        "image".into(),
        "inspect".into(),
        helper["Image"]
            .as_str()
            .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?
            .into(),
    ])?;
    let values: Value = serde_json::from_slice(&bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    if values.as_array().is_none_or(|v| v.len() != 1) || values[0]["Id"] != helper["Image"] {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let mut declared = BTreeSet::new();
    match &values[0]["Config"]["Volumes"] {
        Value::Null => {}
        Value::Object(volumes) => {
            for (path, options) in volumes {
                if path != "/var/lib/postgresql"
                    || !options.as_object().is_some_and(|v| v.is_empty())
                {
                    return Err(err("OWNERSHIP_CONFLICT"));
                }
                declared.insert(path.clone());
            }
        }
        _ => return Err(err("OWNERSHIP_CONFLICT")),
    }
    Ok(declared)
}
fn auxiliary_volumes(helper: &Value, declared: &BTreeSet<String>) -> Vec<(String, String)> {
    helper["Mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let destination = m["Destination"].as_str()?;
            if declared.contains(destination) {
                Some((destination.into(), m["Name"].as_str()?.into()))
            } else {
                None
            }
        })
        .collect()
}
fn check_volume(command: &mut impl FnMut(&[String]) -> Result<Vec<u8>>, name: &str) -> Result<()> {
    if !container_id(name) {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let values: Value =
        serde_json::from_slice(&command(&["volume".into(), "inspect".into(), name.into()])?)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    if values.as_array().is_none_or(|v| v.len() != 1)
        || values[0]["Name"] != name
        || values[0]["Driver"] != "local"
        || !(values[0]["Options"].is_null()
            || values[0]["Options"]
                .as_object()
                .is_some_and(|v| v.is_empty()))
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(())
}
fn check_retention(
    command: &mut impl FnMut(&[String]) -> Result<Vec<u8>>,
    id: &str,
    target: &str,
    nonce: &str,
    helper: &Value,
    volumes: &[(String, String)],
) -> Result<()> {
    if !container_id(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let values: Value = serde_json::from_slice(&command(&["inspect".into(), id.into()])?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    if values.as_array().is_none_or(|v| v.len() != 1) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let v = &values[0];
    if v["Id"] != id
        || v["Name"] != format!("/exhibitos-helper-retention-{nonce}")
        || v["Image"] != helper["Image"]
        || v["Config"]["Labels"]["com.exhibitos.reconciliation"] != nonce
        || v["Config"]["Labels"]["com.exhibitos.helper.job"] != target
        || v["State"]["Running"] != false
        || v["State"]["Status"] != "created"
        || v["HostConfig"]["AutoRemove"] != false
        || v["HostConfig"]["Privileged"] != false
        || v["HostConfig"]["ReadonlyRootfs"] != true
        || v["HostConfig"]["NetworkMode"] != "none"
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let mounts = v["Mounts"]
        .as_array()
        .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
    if mounts.len() != volumes.len()
        || volumes.iter().any(|(dest, name)| {
            !mounts.iter().any(|m| {
                m["Destination"] == *dest
                    && m["Name"] == *name
                    && m["Type"] == "volume"
                    && m["Driver"] == "local"
                    && m["RW"] == false
            })
        })
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(())
}
impl LifecycleService {
    fn retain_auxiliary_volumes(
        &self,
        target: &str,
        nonce: &str,
        helper: &Value,
        volumes: &[(String, String)],
        command: &mut impl FnMut(&[String]) -> Result<Vec<u8>>,
    ) -> Result<String> {
        if !uuid(nonce) || !uuid(target) || volumes.is_empty() || volumes.len() > 1 {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        let mut args: Vec<String> = vec!["create", "--pull", "never", "--name"]
            .into_iter()
            .map(String::from)
            .collect();
        args.push(format!("exhibitos-helper-retention-{nonce}"));
        args.extend([
            "--label".into(),
            format!("com.exhibitos.reconciliation={nonce}"),
            "--label".into(),
            format!("com.exhibitos.helper.job={target}"),
        ]);
        args.extend(
            [
                "--network",
                "none",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                "--pids-limit",
                "16",
                "--memory",
                "64m",
            ]
            .into_iter()
            .map(String::from),
        );
        for (dest, name) in volumes {
            if dest != "/var/lib/postgresql" || !container_id(name) {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            args.extend([
                "--mount".into(),
                format!("type=volume,source={name},target={dest},readonly,volume-nocopy"),
            ]);
        }
        args.extend([
            "--entrypoint".into(),
            "node".into(),
            helper["Image"]
                .as_str()
                .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?
                .into(),
            "-e".into(),
            "process.exit(0)".into(),
        ]);
        let id = String::from_utf8(command(&args)?)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            .trim()
            .to_string();
        check_retention(command, &id, target, nonce, helper, volumes)?;
        write_json(
            &self.root,
            &format!("helper-retention-{nonce}.json"),
            &serde_json::json!({"format":1,"reconciliationId":nonce,"targetId":target,"containerId":id,"image":helper["Image"],"volumes":volumes,"createdAt":now()}),
        )?;
        Ok(id)
    }
    fn reconciliation_target(&self, kind: &str, id: &str) -> Result<()> {
        names(kind, id)?;
        let (target, state, stage, created, updated, error) = if kind == "backup" {
            let bytes = source_bytes(
                &self.root,
                &format!("backup-creation-{id}/job.json"),
                16 * 1024,
                true,
            )?;
            let job: BackupCreationJob =
                serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
            if job.operation != "create" {
                return Err(err("STATE_INVALID"));
            }
            (
                job.id,
                job.state,
                job.stage,
                job.created_at,
                job.updated_at,
                job.error_code,
            )
        } else {
            let bytes = source_bytes(&self.root, "restoration.json", 16 * 1024, true)?;
            let job: RestorationJob =
                serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
            (
                job.id,
                job.state,
                job.stage,
                job.created_at,
                job.updated_at,
                job.error_code,
            )
        };
        if target != id
            || !["failed", "interrupted"].contains(&state.as_str())
            || stage.is_empty()
            || stage.len() > 64
            || created > updated
            || updated > 8_640_000_000_000_000
            || error.as_ref().is_none_or(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            })
        {
            return Err(err("RECONCILIATION_TARGET_INVALID"));
        }
        Ok(())
    }
    fn reconciliation_history(&self) -> Result<Vec<ReconciliationJob>> {
        let mut jobs = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let entry = entry.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(id) = name
                .strip_prefix("helper-reconciliation-")
                .and_then(|v| v.strip_suffix(".json"))
            {
                if !uuid(id) || jobs.len() >= 1000 {
                    return Err(err("STATE_INVALID"));
                }
                let bytes = source_bytes(&self.root, name, 16 * 1024, true)?;
                let job: ReconciliationJob =
                    serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
                validate_record(&job)?;
                if job.id != id {
                    return Err(err("STATE_INVALID"));
                }
                jobs.push(job);
            }
        }
        jobs.sort_by_key(|j| j.created_at);
        Ok(jobs)
    }
    pub(crate) fn recover_helper_reconciliations(&self) -> Result<()> {
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        for mut job in self.reconciliation_history()? {
            if job.state == "checking" {
                job.state = "interrupted".into();
                job.error_code = Some("INTERRUPTED".into());
                job.updated_at = now();
                write_json(
                    &self.root,
                    &format!("helper-reconciliation-{}.json", job.id),
                    &job,
                )?;
            }
        }
        Ok(())
    }
    pub fn helper_reconciliations(&self) -> Result<Vec<ReconciliationJob>> {
        self.recover_helper_reconciliations()?;
        let _lock = self.lock()?;
        self.reconciliation_history()
    }
    /// Stop only an exact, re-inspected failed-job helper. No rm, volumes, writers or job rewriting.
    pub fn reconcile_helper(
        &self,
        kind: &str,
        target: &str,
        preserve_candidates: bool,
    ) -> Result<ReconciliationReceipt> {
        if !preserve_candidates {
            return Err(err("RECONCILIATION_ACK_REQUIRED"));
        }
        names(kind, target)?;
        let _lock = self.lock()?;
        if cfg!(windows) {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        maintenance::input_path(&self.root, true)?;
        self.reconciliation_target(kind, target)?;
        if self.reconciliation_history()?.len() >= 1000 {
            return Err(err("JOB_HISTORY_FULL"));
        }
        let mut job = ReconciliationJob {
            id: Uuid::new_v4().to_string(),
            target_id: target.into(),
            kind: kind.into(),
            state: "checking".into(),
            helper_state: None,
            error_code: None,
            created_at: now(),
            updated_at: now(),
        };
        let name = format!("helper-reconciliation-{}.json", job.id);
        write_json(&self.root, &name, &job)?;
        let outcome = (|| -> Result<String> {
            if !self
                .detect()?
                .iter()
                .any(|e| e.kind == "docker" && e.available)
            {
                return Err(err("ENGINE_UNAVAILABLE"));
            }
            self.reconcile_helper_engine(kind, target, &job.id, |args| {
                run("docker", args, None, 60)
            })
        })();
        job.updated_at = now();
        match outcome {
            Ok(state) => {
                job.state = "completed".into();
                job.helper_state = Some(state.clone());
                write_json(&self.root, &name, &job)?;
                Ok(ReconciliationReceipt {
                    id: job.id,
                    target_id: target.into(),
                    kind: kind.into(),
                    operation: "helper-reconciled".into(),
                    helper_state: state,
                    data_preserved: true,
                    writers_resumed: false,
                    at: job.updated_at,
                })
            }
            Err(error) => {
                job.state = "failed".into();
                job.error_code = Some(error.code.clone());
                write_json(&self.root, &name, &job)?;
                Err(error)
            }
        }
    }
    fn reconcile_helper_engine(
        &self,
        kind: &str,
        target: &str,
        nonce: &str,
        mut command: impl FnMut(&[String]) -> Result<Vec<u8>>,
    ) -> Result<String> {
        let (name, label) = names(kind, target)?;
        // Absence requires successful scoped engine queries, never a failed inspect.
        let ids = list(&mut command, &name, &label, target)?;
        let Some(id) = ids.first() else {
            if !list(&mut command, &name, &label, target)?.is_empty() {
                return Err(err("RECONCILIATION_UNCERTAIN"));
            }
            return Ok("absent".into());
        };
        let inspect = |command: &mut dyn FnMut(&[String]) -> Result<Vec<u8>>| -> Result<Value> {
            let value: Value = serde_json::from_slice(&command(&["inspect".into(), id.clone()])?)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            if value.as_array().is_none_or(|v| v.len() != 1) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            Ok(value[0].clone())
        };
        let first = inspect(&mut command)?;
        let declared = declared_volumes(&mut command, &first)?;
        let running = validate_helper(self, kind, target, id, &first, &declared)?;
        let auxiliary = auxiliary_volumes(&first, &declared);
        for (_, volume) in &auxiliary {
            check_volume(&mut command, volume)?;
        }
        let retention = if running
            && first["HostConfig"]["AutoRemove"] == true
            && !auxiliary.is_empty()
        {
            Some(self.retain_auxiliary_volumes(target, nonce, &first, &auxiliary, &mut command)?)
        } else {
            None
        };
        if running {
            let current = inspect(&mut command)?;
            if !validate_helper(self, kind, target, id, &current, &declared)?
                || first["Image"] != current["Image"]
                || first["Mounts"] != current["Mounts"]
                || first["HostConfig"]["AutoRemove"] != current["HostConfig"]["AutoRemove"]
            {
                return Err(err("RECONCILIATION_UNCERTAIN"));
            }
            if let Some(retention) = &retention {
                check_retention(&mut command, retention, target, nonce, &first, &auxiliary)?;
            }
            command(&["stop".into(), "--time".into(), "30".into(), id.clone()])?;
        }
        if let Some(retention) = &retention {
            check_retention(&mut command, retention, target, nonce, &first, &auxiliary)?;
        }
        for (_, volume) in &auxiliary {
            check_volume(&mut command, volume)?;
        }
        let remaining = list(&mut command, &name, &label, target)?;
        if remaining.is_empty() {
            return Ok("absent".into());
        }
        if remaining != ids {
            return Err(err("RECONCILIATION_UNCERTAIN"));
        }
        if validate_helper(self, kind, target, id, &inspect(&mut command)?, &declared)? {
            return Err(err("RECONCILIATION_UNCERTAIN"));
        }
        Ok("stopped".into())
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(kind: &str) -> (LifecycleService, String) {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("helper-reconciliation-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        let id = Uuid::new_v4().to_string();
        if kind == "backup" {
            let folder = service.root.join(format!("backup-creation-{id}"));
            fs::create_dir(&folder).unwrap();
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
            write_json(
                &folder,
                "job.json",
                &BackupCreationJob {
                    id: id.clone(),
                    operation: "create".into(),
                    state: "failed".into(),
                    stage: "encrypting-and-authenticating".into(),
                    error_code: Some("ENGINE_OPERATION_FAILED".into()),
                    created_at: 1,
                    updated_at: 2,
                },
            )
            .unwrap();
        } else {
            let folder = service.root.join(format!("restore-{id}"));
            fs::create_dir(&folder).unwrap();
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
            write_json(
                &service.root,
                "restoration.json",
                &RestorationJob {
                    id: id.clone(),
                    state: "interrupted".into(),
                    stage: "authenticating".into(),
                    error_code: Some("INTERRUPTED".into()),
                    created_at: 1,
                    updated_at: 2,
                },
            )
            .unwrap();
        }
        (service, id)
    }
    fn helper(service: &LifecycleService, kind: &str, id: &str) -> Value {
        let (name, label) = names(kind, id).unwrap();
        let mount = if kind == "backup" {
            serde_json::json!({"Type":"volume","Name":format!("exhibitos-backup-work-{id}"),"Destination":"/work","RW":true})
        } else {
            serde_json::json!({"Type":"bind","Source":service.root.join(format!("restore-{id}")).to_string_lossy(),"Destination":"/work","RW":true})
        };
        serde_json::json!({"Id":"a".repeat(64),"Name":format!("/{name}"),"Image":format!("sha256:{}","b".repeat(64)),"Config":{"Labels":{label:id}},"HostConfig":{"Privileged":false,"ReadonlyRootfs":true,"AutoRemove":false},"Mounts":[mount],"State":{"Running":true}})
    }
    fn image_metadata(helper: &Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{"Id":helper["Image"],"Config":{"Volumes":null}}]))
            .unwrap()
    }
    #[test]
    fn both_kinds_stop_exact_identity_without_deleting_candidates_or_rewriting_jobs() {
        for kind in ["backup", "restoration"] {
            let (service, id) = fixture(kind);
            let mut value = helper(&service, kind, &id);
            let mut calls = Vec::new();
            let path = if kind == "backup" {
                service.root.join(format!("backup-creation-{id}/job.json"))
            } else {
                service.root.join("restoration.json")
            };
            let before = fs::read(&path).unwrap();
            let state = service
                .reconcile_helper_engine(kind, &id, &Uuid::new_v4().to_string(), |args| {
                    calls.push(args.to_vec());
                    match args[0].as_str() {
                        "ps" => Ok(format!("{}\n", "a".repeat(64)).into_bytes()),
                        "image" => Ok(image_metadata(&value)),
                        "inspect" => Ok(serde_json::to_vec(&vec![value.clone()]).unwrap()),
                        "stop" => {
                            assert_eq!(args, &["stop", "--time", "30", &"a".repeat(64)]);
                            value["State"]["Running"] = false.into();
                            Ok(Vec::new())
                        }
                        _ => panic!("unexpected command"),
                    }
                })
                .unwrap();
            assert_eq!(state, "stopped");
            assert_eq!(calls.iter().filter(|c| c[0] == "stop").count(), 1);
            assert_eq!(before, fs::read(path).unwrap());
            assert!(
                calls
                    .iter()
                    .all(|c| !["rm", "volume", "start", "compose"].contains(&c[0].as_str()))
            );
        }
    }
    #[test]
    fn ambiguous_aliases_privileged_or_foreign_writes_never_issue_stop() {
        let (service, id) = fixture("backup");
        let good = helper(&service, "backup", &id);
        let mut bad = Vec::new();
        for (pointer, value) in [
            ("/Name", serde_json::json!("/renamed")),
            (
                "/Config/Labels/com.exhibitos.backup",
                serde_json::json!(Uuid::new_v4().to_string()),
            ),
            ("/Id", serde_json::json!("c".repeat(64))),
            ("/HostConfig/Privileged", serde_json::json!(true)),
            ("/HostConfig/ReadonlyRootfs", serde_json::json!(false)),
            ("/Mounts/0/Name", serde_json::json!("foreign-volume")),
            ("/State/Running", serde_json::json!("true")),
        ] {
            let mut changed = good.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            bad.push(changed);
        }
        let mut extra = good.clone();
        extra["Mounts"].as_array_mut().unwrap().push(serde_json::json!({"Type":"bind","Destination":"/foreign","Source":"/private/data","RW":true}));
        bad.push(extra);
        for value in bad {
            let mut stopped = false;
            let outcome = service.reconcile_helper_engine(
                "backup",
                &id,
                &Uuid::new_v4().to_string(),
                |args| match args[0].as_str() {
                    "ps" => Ok("a".repeat(64).into_bytes()),
                    "image" => Ok(image_metadata(&value)),
                    "inspect" => Ok(serde_json::to_vec(&vec![value.clone()]).unwrap()),
                    "stop" => {
                        stopped = true;
                        Ok(Vec::new())
                    }
                    _ => panic!(),
                },
            );
            assert!(outcome.is_err());
            assert!(!stopped);
        }
        let mut stopped = false;
        assert!(
            service
                .reconcile_helper_engine("backup", &id, &Uuid::new_v4().to_string(), |args| {
                    if args[0] == "stop" {
                        stopped = true;
                    }
                    Ok(format!("{}\n{}", "a".repeat(64), "c".repeat(64)).into_bytes())
                })
                .is_err()
        );
        assert!(!stopped);
    }
    #[test]
    fn reinspect_and_post_stop_uncertainty_never_invent_quiescence() {
        let (service, id) = fixture("backup");
        let good = helper(&service, "backup", &id);
        for scenario in [
            "changed",
            "auto-remove-changed",
            "still-running",
            "inspect-failure",
        ] {
            let mut inspections = 0;
            let mut stopped = false;
            let outcome = service.reconcile_helper_engine(
                "backup",
                &id,
                &Uuid::new_v4().to_string(),
                |args| {
                    if args[0] == "ps" {
                        return Ok("a".repeat(64).into_bytes());
                    }
                    if args[0] == "stop" {
                        stopped = true;
                        return Ok(Vec::new());
                    }
                    if args[0] == "image" {
                        return Ok(image_metadata(&good));
                    }
                    inspections += 1;
                    if scenario == "inspect-failure" {
                        return Err(err("ENGINE_UNAVAILABLE"));
                    }
                    let mut value = good.clone();
                    if scenario == "changed" && inspections == 2 {
                        value["Image"] = format!("sha256:{}", "c".repeat(64)).into();
                    }
                    if scenario == "auto-remove-changed" && inspections == 2 {
                        value["HostConfig"]["AutoRemove"] = true.into();
                    }
                    Ok(serde_json::to_vec(&vec![value]).unwrap())
                },
            );
            assert!(outcome.is_err());
            assert_eq!(stopped, scenario == "still-running");
        }
    }
    #[test]
    fn absence_requires_successful_queries_and_an_unchanged_scope() {
        let (service, id) = fixture("backup");
        let mut calls = 0;
        assert_eq!(
            service
                .reconcile_helper_engine("backup", &id, &Uuid::new_v4().to_string(), |args| {
                    calls += 1;
                    assert_eq!(args[0], "ps");
                    Ok(Vec::new())
                })
                .unwrap(),
            "absent"
        );
        assert_eq!(calls, 4);
        assert!(
            service
                .reconcile_helper_engine("backup", &id, &Uuid::new_v4().to_string(), |_| Err(err(
                    "ENGINE_UNAVAILABLE"
                )))
                .is_err()
        );
        let mut calls = 0;
        assert!(
            service
                .reconcile_helper_engine("backup", &id, &Uuid::new_v4().to_string(), |_| {
                    calls += 1;
                    Ok(if calls > 2 {
                        "a".repeat(64).into_bytes()
                    } else {
                        Vec::new()
                    })
                })
                .is_err()
        );
    }
    #[test]
    fn auxiliary_auto_remove_is_retained_before_stop_and_unknown_volumes_are_rejected() {
        for conflict in [false, true] {
            let (service, target) = fixture("restoration");
            let nonce = Uuid::new_v4().to_string();
            let mut helper = helper(&service, "restoration", &target);
            helper["HostConfig"]["AutoRemove"] = true.into();
            let volume = "d".repeat(64);
            helper["Mounts"].as_array_mut().unwrap().push(serde_json::json!({"Destination":"/var/lib/postgresql","Name":volume,"Type":"volume","Driver":"local","RW":true}));
            let anchor_id = "e".repeat(64);
            let anchor = serde_json::json!({"Id":anchor_id,"Name":format!("/exhibitos-helper-retention-{nonce}"),"Image":helper["Image"],"Config":{"Labels":{"com.exhibitos.reconciliation":nonce,"com.exhibitos.helper.job":target}},"HostConfig":{"AutoRemove":false,"ReadonlyRootfs":true,"Privileged":false,"NetworkMode":"none"},"State":{"Running":conflict,"Status":"created"},"Mounts":[{"Destination":"/var/lib/postgresql","Name":volume,"Type":"volume","Driver":"local","RW":false}]});
            let mut stopped = false;
            let mut retained = false;
            let result=service.reconcile_helper_engine("restoration",&target,&nonce,|args|match args[0].as_str(){
                "ps"=>Ok(if stopped{Vec::new()}else{"a".repeat(64).into_bytes()}),
                "image"=>Ok(serde_json::to_vec(&serde_json::json!([{"Id":helper["Image"],"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap()),
                "volume"=>{assert_eq!(args[1],"inspect");Ok(serde_json::to_vec(&serde_json::json!([{"Name":volume,"Driver":"local","Options":null}])).unwrap())},
                "inspect"=>Ok(serde_json::to_vec(&vec![if args[1]==anchor_id{anchor.clone()}else{helper.clone()}]).unwrap()),
                "create"=>{assert!(args.iter().any(|a|a.ends_with("readonly,volume-nocopy")));assert!(!args.iter().any(|a|a=="--rm"));retained=true;Ok(anchor_id.as_bytes().to_vec())},
                "stop"=>{assert!(retained);stopped=true;Ok(Vec::new())},
                _=>panic!("unexpected mutation"),
            });
            assert_eq!(stopped, !conflict);
            if conflict {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), "absent");
                let journal = service.root.join(format!("helper-retention-{nonce}.json"));
                assert_eq!(
                    fs::metadata(journal).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        let helper = serde_json::json!({"Image":format!("sha256:{}","b".repeat(64))});
        assert!(declared_volumes(&mut |_|Ok(serde_json::to_vec(&serde_json::json!([{"Id":helper["Image"],"Config":{"Volumes":{"/foreign":{}}}}])).unwrap()),&helper).is_err());
    }
    #[test]
    fn acknowledgement_root_membership_and_active_lock_precede_engine_and_history_changes() {
        let (service, id) = fixture("backup");
        assert_eq!(
            service
                .reconcile_helper("backup", &id, false)
                .unwrap_err()
                .code,
            "RECONCILIATION_ACK_REQUIRED"
        );
        assert_eq!(
            service
                .reconcile_helper("shell", "../private", true)
                .unwrap_err()
                .code,
            "RECONCILIATION_INPUT_INVALID"
        );
        let lock = service.lock().unwrap();
        assert_eq!(
            service
                .reconcile_helper("backup", &id, true)
                .unwrap_err()
                .code,
            "BUSY"
        );
        drop(lock);
        let path = service.root.join(format!("backup-creation-{id}/job.json"));
        let mut job: BackupCreationJob = read_json(&path).unwrap();
        for state in ["running", "completed", "unknown"] {
            job.state = state.into();
            write_json(path.parent().unwrap(), "job.json", &job).unwrap();
            assert_eq!(
                service
                    .reconcile_helper("backup", &id, true)
                    .unwrap_err()
                    .code,
                "RECONCILIATION_TARGET_INVALID"
            );
        }
        assert!(service.helper_reconciliations().unwrap().is_empty());
    }
    #[test]
    fn unsafe_target_and_history_aliases_are_rejected_without_overwrite() {
        use std::os::unix::fs::symlink;
        let (service, id) = fixture("backup");
        let path = service.root.join(format!("backup-creation-{id}/job.json"));
        let original = fs::read(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(service.reconciliation_target("backup", &id).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let retained = path.with_file_name("retained.json");
        fs::rename(&path, &retained).unwrap();
        symlink(&retained, &path).unwrap();
        assert!(service.reconciliation_target("backup", &id).is_err());
        assert_eq!(original, fs::read(&retained).unwrap());
        let audit = service
            .root
            .join(format!("helper-reconciliation-{}.json", Uuid::new_v4()));
        symlink(&retained, &audit).unwrap();
        assert!(service.helper_reconciliations().is_err());
    }
    #[test]
    fn interrupted_reconciliation_is_not_completed_and_retained_job_bytes_are_preserved() {
        let (service, id) = fixture("restoration");
        let original = fs::read(service.root.join("restoration.json")).unwrap();
        let audit = ReconciliationJob {
            id: Uuid::new_v4().to_string(),
            target_id: id,
            kind: "restoration".into(),
            state: "checking".into(),
            helper_state: None,
            error_code: None,
            created_at: 1,
            updated_at: 2,
        };
        write_json(
            &service.root,
            &format!("helper-reconciliation-{}.json", audit.id),
            &audit,
        )
        .unwrap();
        let lock = service.lock().unwrap();
        service.recover_helper_reconciliations().unwrap();
        assert_eq!(
            service.reconciliation_history().unwrap()[0].state,
            "checking"
        );
        drop(lock);
        let reopened = LifecycleService::new(service.root.clone()).unwrap();
        let records = reopened.helper_reconciliations().unwrap();
        assert_eq!(records[0].state, "interrupted");
        assert!(records[0].helper_state.is_none());
        assert_eq!(
            original,
            fs::read(service.root.join("restoration.json")).unwrap()
        );
    }
}
