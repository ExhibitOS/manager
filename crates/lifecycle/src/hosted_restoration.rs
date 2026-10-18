// SPDX-License-Identifier: Apache-2.0
//! Isolated authenticated hosted database restore checks. Never activates an installation.
use super::hosted_backup::{self, Source, uuid};
use super::installation_backup::source_bytes;
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub operation: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub files: u64,
}
impl Receipt {
    fn validate(&self, job: &CheckJob) -> Result<()> {
        if self.operation != "restored-and-verified"
            || self.backup_id != job.expected_backup_id
            || self.authenticated_manifest_sha256 != job.expected_manifest_sha256
            || self.files != job.expected_files
        {
            return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckJob {
    pub id: String,
    pub source_job_id: String,
    pub source: Source,
    pub image: String,
    pub expected_backup_id: String,
    pub expected_manifest_sha256: String,
    pub expected_files: u64,
    pub job_budget_bytes: u64,
    pub state: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub receipt: Option<Receipt>,
    pub created_at: u64,
    pub updated_at: u64,
}
impl CheckJob {
    pub(crate) fn validate(&self) -> Result<()> {
        self.source.validate()?;
        if !uuid(&self.id)
            || !uuid(&self.source_job_id)
            || self.id == self.source_job_id
            || !uuid(&self.expected_backup_id)
            || !hash_valid(&self.expected_manifest_sha256)
            || self.expected_files == 0
            || self.expected_files > 100000
            || self.job_budget_bytes < 4 * 1024 * 1024
            || self.job_budget_bytes > 32 * 1024 * 1024 * 1024
            || !self.image.strip_prefix("sha256:").is_some_and(hash_valid)
            || !["running", "completed", "failed", "interrupted"].contains(&self.state.as_str())
            || self.stage
                != if self.state == "running" {
                    "checking-archive-and-restoring"
                } else {
                    &self.state
                }
            || self.created_at > self.updated_at
            || self.updated_at > 8_640_000_000_000_000
            || (["failed", "interrupted"].contains(&self.state.as_str())
                != self.error_code.is_some())
            || self.error_code.as_ref().is_some_and(|code| {
                code.is_empty()
                    || code.len() > 128
                    || !code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            })
            || ((self.state == "completed") != self.receipt.is_some())
        {
            return Err(err("STATE_INVALID"));
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate(self)?;
        }
        Ok(())
    }
    fn matches_source(&self, source: &hosted_backup::Job) -> Result<()> {
        let receipt = source
            .receipt
            .as_ref()
            .ok_or_else(|| err("HOSTED_RECOVERY_RESULT_INVALID"))?;
        if source.id != self.source_job_id
            || source.source != self.source
            || source.state != "completed"
            || source.error_code.is_some()
            || receipt.backup_id != self.expected_backup_id
            || receipt.authenticated_manifest_sha256 != self.expected_manifest_sha256
            || receipt.files != self.expected_files
        {
            return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub job_id: String,
    pub source_job_id: String,
    pub helper_name: String,
    pub image: String,
    pub source_path: String,
    pub work_volume: String,
    pub docker_path: String,
    pub docker_endpoint: String,
    pub job_budget_bytes: u64,
    pub expected_backup_id: String,
    pub expected_manifest_sha256: String,
    pub expected_files: u64,
}
struct SecretInput(std::io::Cursor<Vec<u8>>);
impl Read for SecretInput {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(bytes)
    }
}
impl Drop for SecretInput {
    fn drop(&mut self) {
        self.0.get_mut().fill(0);
    }
}
impl LifecycleService {
    /// Reads only an authenticated-completion record and its retained encrypted archive.
    pub fn completed_hosted_backup(&self, id: &str) -> Result<hosted_backup::Job> {
        let job = self.hosted_backup_job(id)?;
        if job.state != "completed" || job.error_code.is_some() {
            return Err(err("HOSTED_RECOVERY_SOURCE_REQUIRED"));
        }
        let receipt = job
            .receipt
            .as_ref()
            .ok_or_else(|| err("HOSTED_RECOVERY_SOURCE_REQUIRED"))?;
        if self.validate_hosted_archive(id, job.job_budget_bytes.unwrap_or(u64::MAX))?
            != receipt.files
        {
            return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
        }
        Ok(job)
    }
    fn check_hosted_restore_work_volume(&self, id: &str) -> Result<()> {
        if !uuid(id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let name = format!("exhibitos-hosted-restore-work-{id}");
        let value: Value = serde_json::from_slice(&run(
            "docker",
            &["volume".into(), "inspect".into(), name.clone()],
            None,
            30,
        )?)
        .map_err(|_| err("OWNERSHIP_CONFLICT"))?;
        if value.as_array().is_none_or(|v| v.len() != 1)
            || value[0]["Name"] != name
            || value[0]["Driver"] != "local"
            || value[0]["Scope"] != "local"
            || value[0]["Labels"]["com.exhibitos.hosted-restore"] != id
            || !(value[0]["Options"].is_null()
                || value[0]["Options"]
                    .as_object()
                    .is_some_and(|v| v.is_empty()))
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(())
    }
    pub(crate) fn hosted_restore_helper_state(&self, job: &CheckJob) -> Result<String> {
        let name = format!("exhibitos-hosted-restore-{}", job.id);
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &name,
            "com.exhibitos.hosted-restore",
            &job.id,
        )?;
        let Some(id) = ids.first() else {
            return Ok("absent".into());
        };
        let value = backup_creation::inspected("docker", &["inspect".into(), id.clone()])?;
        self.check_hosted_restore_work_volume(&job.id)?;
        validate_helper(
            job,
            &value,
            &self.root.join(format!(
                "hosted-backup-{}/output/archive",
                job.source_job_id
            )),
            id,
            observed_desktop_mapping(&value)?,
        )?;
        Ok(if value["State"]["Running"] == true {
            "running"
        } else if value["State"]["Status"]
            .as_str()
            .is_some_and(|v| ["exited", "dead"].contains(&v))
        {
            "stopped"
        } else {
            "pending"
        }
        .into())
    }
    pub(crate) fn stop_owned_hosted_restore(&self, id: &str) -> Result<()> {
        let job = self.hosted_restore_check(id)?;
        let state = self.hosted_restore_helper_state(&job)?;
        if state == "absent" || state == "stopped" {
            return Ok(());
        }
        if state != "running" {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &format!("exhibitos-hosted-restore-{id}"),
            "com.exhibitos.hosted-restore",
            id,
        )?;
        let cid = ids.first().ok_or_else(|| err("CANCEL_UNCERTAIN"))?;
        let observed = backup_creation::inspected("docker", &["inspect".into(), cid.clone()])?;
        validate_helper(
            &job,
            &observed,
            &self.root.join(format!(
                "hosted-backup-{}/output/archive",
                job.source_job_id
            )),
            cid,
            observed_desktop_mapping(&observed)?,
        )?;
        run(
            "docker",
            &["stop".into(), "--time".into(), "30".into(), cid.clone()],
            None,
            60,
        )?;
        if self.hosted_restore_helper_state(&job)? != "stopped" {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        Ok(())
    }
    /// Trusted native caller supplies fixed pinned Node/runner paths and private input.
    /// Paths/keys/binaries are never accepted on a renderer recovery request.
    #[cfg(target_os = "macos")]
    pub fn create_hosted_restore_check(
        &self,
        source_job_id: &str,
        image: &str,
        node: &Path,
        runner: &Path,
        make_input: impl FnOnce(&Execution) -> Result<Vec<u8>>,
        check_resources: impl Fn() -> Result<()>,
    ) -> Result<CheckJob> {
        let _lock = self.lock()?;
        let source_job = self.completed_hosted_backup(source_job_id)?;
        let original = source_job
            .receipt
            .as_ref()
            .ok_or_else(|| err("HOSTED_RECOVERY_SOURCE_REQUIRED"))?;
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        for old in self.hosted_backup_jobs()? {
            if self.hosted_helper_state(&old)? == "running" {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        for old in self.hosted_restore_checks()? {
            if ["running", "pending"].contains(&self.hosted_restore_helper_state(&old)?.as_str()) {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        let available = fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?;
        let job_budget_bytes = hosted_backup::job_budget(available)?.min(32 * 1024 * 1024 * 1024);
        let observed: Value = serde_json::from_slice(&run(
            "docker",
            &["image".into(), "inspect".into(), image.into()],
            None,
            30,
        )?)
        .map_err(|_| err("BACKUP_IMAGE_INVALID"))?;
        if observed.as_array().is_none_or(|v| v.len() != 1) || observed[0]["Id"] != image {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("hosted-restoration-{id}"));
        let _workspace = restoration::directory(&workspace)?;
        let workspace_pin = hosted_backup::output_guard(&workspace)?;
        let source_path = self
            .root
            .join(format!("hosted-backup-{source_job_id}/output/archive"));
        let source_pins = [
            self.root.clone(),
            self.root.join(format!("hosted-backup-{source_job_id}")),
            self.root
                .join(format!("hosted-backup-{source_job_id}/output")),
            source_path.clone(),
            source_path.join("files"),
        ]
        .into_iter()
        .map(|path| hosted_backup::output_guard(&path).map(|pin| (path, pin)))
        .collect::<Result<Vec<_>>>()?;
        let check_source = || -> Result<()> {
            for (path, pin) in &source_pins {
                super::backup_export::native::check(pin, path)?;
            }
            Ok(())
        };
        let now = now();
        let mut job = CheckJob {
            id: id.clone(),
            source_job_id: source_job_id.into(),
            source: source_job.source.clone(),
            image: image.into(),
            expected_backup_id: original.backup_id.clone(),
            expected_manifest_sha256: original.authenticated_manifest_sha256.clone(),
            expected_files: original.files,
            job_budget_bytes,
            state: "running".into(),
            stage: "checking-archive-and-restoring".into(),
            error_code: None,
            receipt: None,
            created_at: now,
            updated_at: now,
        };
        self.save_hosted_restore_check(&job)?;
        if let Err(error) = self.begin_maintenance("hosted-restoration", &id, &job.stage) {
            job.state = "failed".into();
            job.stage = "failed".into();
            job.error_code = Some(error.code.clone());
            job.updated_at = super::now();
            self.save_hosted_restore_check(&job)?;
            return Err(error);
        }
        let outcome = (|| {
            let mut command = std::process::Command::new("docker");
            local_network::pin_command(&mut command)?;
            let endpoint = command
                .get_envs()
                .find(|(k, _)| *k == "DOCKER_HOST")
                .and_then(|(_, v)| v)
                .and_then(|v| v.to_str())
                .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?
                .to_owned();
            let docker = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
            let plan = Execution {
                job_id: id.clone(),
                source_job_id: source_job_id.into(),
                helper_name: format!("exhibitos-hosted-restore-{id}"),
                image: image.into(),
                source_path: source_path
                    .to_str()
                    .ok_or_else(|| err("BACKUP_PATH_INVALID"))?
                    .into(),
                work_volume: format!("exhibitos-hosted-restore-work-{id}"),
                docker_path: docker
                    .to_str()
                    .ok_or_else(|| err("RUNTIME_MISSING"))?
                    .into(),
                docker_endpoint: endpoint,
                job_budget_bytes,
                expected_backup_id: job.expected_backup_id.clone(),
                expected_manifest_sha256: job.expected_manifest_sha256.clone(),
                expected_files: job.expected_files,
            };
            let existing = run(
                "docker",
                &[
                    "volume".into(),
                    "ls".into(),
                    "--filter".into(),
                    format!("name=^{}$", plan.work_volume),
                    "--format".into(),
                    "{{.Name}}".into(),
                ],
                None,
                30,
            )?;
            if !existing.iter().all(u8::is_ascii_whitespace) {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            run(
                "docker",
                &[
                    "volume".into(),
                    "create".into(),
                    "--label".into(),
                    format!("com.exhibitos.hosted-restore={id}"),
                    plan.work_volume.clone(),
                ],
                None,
                30,
            )?;
            self.check_hosted_restore_work_volume(&id)?;
            let input = make_input(&plan)?;
            if input.len() > 131072 {
                return Err(err("ENGINE_INPUT_LIMIT"));
            }
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            check_source()?;
            let mut next = Instant::now();
            let bytes = run_observed_stream_environment(
                node.to_str()
                    .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?,
                &[
                    "--disable-proto=throw".into(),
                    "--no-warnings".into(),
                    runner
                        .to_str()
                        .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?
                        .into(),
                ],
                None,
                4200,
                (None, None),
                Some((Box::new(SecretInput(std::io::Cursor::new(input))), 131072)),
                true,
                || {
                    if Instant::now() >= next {
                        next = Instant::now() + Duration::from_millis(400);
                        check_resources()?;
                        super::backup_export::native::check(&workspace_pin, &workspace)?;
                        check_source()?;
                        if self.maintenance_requested("hosted-restoration", &id)?
                            && ["running", "stopped"]
                                .contains(&self.hosted_restore_helper_state(&job)?.as_str())
                        {
                            self.maintenance_checkpoint("hosted-restoration", &id)?;
                        }
                    }
                    Ok(())
                },
            )?;
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            check_source()?;
            self.maintenance_checkpoint("hosted-restoration", &id)?;
            if bytes.len() > 16384 {
                return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
            }
            let receipt: Receipt = serde_json::from_slice(&bytes)
                .map_err(|_| err("HOSTED_RECOVERY_EXECUTION_FAILED"))?;
            receipt.validate(&job)?;
            if self.hosted_restore_helper_state(&job)? != "stopped" {
                return Err(err("HOSTED_RECOVERY_HELPER_UNCERTAIN"));
            }
            let final_helper = backup_creation::inspected(
                "docker",
                &["inspect".into(), plan.helper_name.clone()],
            )?;
            let final_id = final_helper["Id"]
                .as_str()
                .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
            validate_helper(
                &job,
                &final_helper,
                &source_path,
                final_id,
                observed_desktop_mapping(&final_helper)?,
            )?;
            if final_helper["State"]["ExitCode"] != 0 || final_helper["State"]["OOMKilled"] != false
            {
                return Err(err("HOSTED_RECOVERY_EXECUTION_FAILED"));
            }
            self.check_hosted_restore_work_volume(&id)?;
            job.matches_source(&self.completed_hosted_backup(source_job_id)?)?;
            Ok(receipt)
        })();
        let _finish = self.maintenance_finish_guard("hosted-restoration", &id)?;
        match outcome {
            Ok(receipt) => {
                job.state = "completed".into();
                job.stage = "completed".into();
                job.receipt = Some(receipt);
                self.finish_maintenance("completed", None)?;
            }
            Err(error) => {
                job.state = if error.code == "CANCELLED" {
                    "interrupted"
                } else {
                    "failed"
                }
                .into();
                job.stage = job.state.clone();
                job.error_code = Some(error.code.clone());
                let terminal = match error.code.as_str() {
                    "CANCELLED" => "confirmed",
                    "CANCEL_UNCERTAIN" => "uncertain",
                    _ => "failed",
                };
                self.finish_maintenance(terminal, Some(&error.code))?;
            }
        }
        job.updated_at = super::now();
        self.save_hosted_restore_check(&job)?;
        Ok(job)
    }
    pub(crate) fn hosted_restore_check(&self, id: &str) -> Result<CheckJob> {
        if !uuid(id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let bytes = source_bytes(
            &self.root,
            &format!("hosted-restoration-{id}/job.json"),
            16384,
            true,
        )?;
        let job: CheckJob = serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
        job.validate()?;
        if job.id != id {
            return Err(err("STATE_INVALID"));
        }
        Ok(job)
    }
    pub fn hosted_restore_checks(&self) -> Result<Vec<CheckJob>> {
        let mut jobs = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let entry = entry.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name.strip_prefix("hosted-restoration-") else {
                continue;
            };
            if !uuid(id) {
                continue;
            }
            if jobs.len() >= 10000 {
                return Err(err("STATE_INVALID"));
            }
            jobs.push(self.hosted_restore_check(id)?);
        }
        jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(jobs)
    }
    pub(crate) fn save_hosted_restore_check(&self, job: &CheckJob) -> Result<()> {
        job.validate()?;
        write_json(
            &self.root.join(format!("hosted-restoration-{}", job.id)),
            "job.json",
            job,
        )
    }
    pub(crate) fn recover_hosted_restore_checks(&self) -> Result<()> {
        let _lock = match self.lock() {
            Ok(v) => v,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        for mut job in self.hosted_restore_checks()? {
            if job.state == "running" {
                job.state = "interrupted".into();
                job.stage = "interrupted".into();
                job.error_code = Some("HOSTED_RECOVERY_INTERRUPTED".into());
                job.updated_at = now();
                self.save_hosted_restore_check(&job)?;
            }
        }
        Ok(())
    }
}
fn observed_desktop_mapping(value: &Value) -> Result<bool> {
    if !cfg!(target_os = "macos")
        || !value["Mounts"].as_array().is_some_and(|mounts| {
            mounts.iter().any(|m| {
                m["Destination"] == "/source"
                    && m["Source"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("/host_mnt/"))
            })
        })
    {
        return Ok(false);
    }
    let bytes = run(
        "docker",
        &["info".into(), "--format".into(), "{{json .}}".into()],
        None,
        30,
    )?;
    if bytes.len() > 256 * 1024 {
        return Err(err("ENGINE_OUTPUT_LIMIT"));
    }
    let info: Value = serde_json::from_slice(&bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    Ok(info["OperatingSystem"] == "Docker Desktop" && info["OSType"] == "linux")
}
fn output_bind_matches(value: &Value, mount: &Value, expected: &str, desktop: bool) -> bool {
    if mount["Type"] != "bind" || mount["RW"] != false {
        return false;
    }
    let configured = value["HostConfig"]["Mounts"]
        .as_array()
        .is_some_and(|mounts| {
            let selected: Vec<_> = mounts.iter().filter(|m| m["Target"] == "/source").collect();
            selected.len() == 1
                && selected[0]["Type"] == "bind"
                && selected[0]["ReadOnly"] == true
                && (selected[0]["Source"] == expected
                    || desktop && selected[0]["Source"] == format!("/host_mnt{expected}"))
        });
    if !configured {
        return false;
    }
    if mount["Source"] == expected {
        return true;
    }
    cfg!(target_os = "macos")
        && desktop
        && mount["Source"] == format!("/host_mnt{expected}")
        && value["HostConfig"]["Mounts"]
            .as_array()
            .is_some_and(|mounts| {
                let selected: Vec<_> = mounts.iter().filter(|m| m["Target"] == "/source").collect();
                selected.len() == 1
                    && selected[0]["Type"] == "bind"
                    && (selected[0]["Source"] == expected
                        || selected[0]["Source"] == format!("/host_mnt{expected}"))
                    && selected[0]["ReadOnly"] == true
            })
}
fn restore_caps_match(value: &Value) -> bool {
    let Some(caps) = value.as_array() else {
        return false;
    };
    let expected = ["CHOWN", "SETUID", "SETGID", "DAC_READ_SEARCH", "KILL"];
    caps.len() == expected.len()
        && expected
            .iter()
            .all(|cap| caps.iter().filter(|v| v.as_str() == Some(*cap)).count() == 1)
}
fn validate_helper(
    job: &CheckJob,
    value: &Value,
    output: &Path,
    cid: &str,
    desktop: bool,
) -> Result<()> {
    let mounts = value["Mounts"]
        .as_array()
        .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
    if value["Id"] != cid
        || cid.len() != 64
        || !hash_valid(cid)
        || value["Name"] != format!("/exhibitos-hosted-restore-{}", job.id)
        || value["Config"]["Labels"]["com.exhibitos.hosted-restore"] != job.id
        || value["Config"]["Image"] != job.image
        || value["Image"] != job.image
        || value["HostConfig"]["Privileged"] != false
        || value["HostConfig"]["ReadonlyRootfs"] != true
        || value["HostConfig"]["AutoRemove"] != false
        || value["HostConfig"]["NetworkMode"] != "none"
        || value["HostConfig"]["PidMode"] != ""
        || value["HostConfig"]["CapDrop"] != serde_json::json!(["ALL"])
        || !restore_caps_match(&value["HostConfig"]["CapAdd"])
        || !hosted_backup::helper_resources_match(value)
        || value["Config"]["Entrypoint"] != serde_json::json!(["node"])
        || value["Config"]["Cmd"]
            != serde_json::json!([
                "--disable-proto=throw",
                "--no-warnings",
                "scripts/remote-restore-entry.mjs"
            ])
        || value["HostConfig"]["PortBindings"]
            .as_object()
            .is_some_and(|v| !v.is_empty())
        || !value["HostConfig"]["PortBindings"].is_null()
            && !value["HostConfig"]["PortBindings"].is_object()
        || mounts.len() != 2
        || !value["State"]["Running"].is_boolean()
        || value["State"]["Restarting"] != false
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let work = mounts
        .iter()
        .filter(|m| {
            m["Destination"] == "/work"
                && m["Type"] == "volume"
                && m["Name"] == format!("exhibitos-hosted-restore-work-{}", job.id)
                && m["RW"] == true
        })
        .count();
    let out = mounts
        .iter()
        .filter(|m| {
            m["Destination"] == "/source"
                && output_bind_matches(value, m, output.to_string_lossy().as_ref(), desktop)
        })
        .count();
    if work != 1 || out != 1 {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn job() -> CheckJob {
        CheckJob {
            id: Uuid::new_v4().to_string(),
            source_job_id: Uuid::new_v4().to_string(),
            source: Source {
                workspace_id: "tea-synthetic".into(),
                service_id: "srv-synthetic".into(),
                postgres_id: "dpg-synthetic".into(),
            },
            image: format!("sha256:{}", "a".repeat(64)),
            expected_backup_id: Uuid::new_v4().to_string(),
            expected_manifest_sha256: "b".repeat(64),
            expected_files: 2,
            job_budget_bytes: 64 * 1024 * 1024,
            state: "running".into(),
            stage: "checking-archive-and-restoring".into(),
            error_code: None,
            receipt: None,
            created_at: 1,
            updated_at: 1,
        }
    }
    fn receipt(job: &CheckJob) -> Receipt {
        Receipt {
            operation: "restored-and-verified".into(),
            backup_id: job.expected_backup_id.clone(),
            authenticated_manifest_sha256: job.expected_manifest_sha256.clone(),
            files: job.expected_files,
        }
    }
    fn original(job: &CheckJob) -> hosted_backup::Job {
        hosted_backup::Job {
            id: job.source_job_id.clone(),
            source: job.source.clone(),
            image: job.image.clone(),
            job_budget_bytes: Some(job.job_budget_bytes),
            state: "completed".into(),
            stage: "completed".into(),
            error_code: None,
            receipt: Some(hosted_backup::Receipt {
                provider: "render".into(),
                action: "local-encrypted-backup".into(),
                workspace_id: job.source.workspace_id.clone(),
                service_id: job.source.service_id.clone(),
                postgres_id: job.source.postgres_id.clone(),
                expires_at: None,
                operation: "created-and-verified".into(),
                backup_id: job.expected_backup_id.clone(),
                files: job.expected_files,
                authenticated_manifest_sha256: job.expected_manifest_sha256.clone(),
            }),
            created_at: 1,
            updated_at: 1,
        }
    }
    #[test]
    fn restore_receipt_binds_original_id_hash_and_file_count() {
        let job = job();
        assert!(receipt(&job).validate(&job).is_ok());
        for (pointer, value) in [
            ("/operation", serde_json::json!("verified")),
            ("/backupId", serde_json::json!(Uuid::new_v4().to_string())),
            (
                "/authenticatedManifestSha256",
                serde_json::json!("c".repeat(64)),
            ),
            ("/files", serde_json::json!(3)),
        ] {
            let mut v = serde_json::to_value(receipt(&job)).unwrap();
            *v.pointer_mut(pointer).unwrap() = value;
            assert!(
                serde_json::from_value::<Receipt>(v)
                    .unwrap()
                    .validate(&job)
                    .is_err()
            );
        }
        let mut v = serde_json::to_value(receipt(&job)).unwrap();
        v["keyBase64"] = "synthetic-private".into();
        assert!(serde_json::from_value::<Receipt>(v).is_err());
    }
    #[test]
    fn source_selection_cannot_change_after_check_starts() {
        let job = job();
        let source = original(&job);
        assert!(job.matches_source(&source).is_ok());
        for pointer in [
            "/id",
            "/source/serviceId",
            "/receipt/backupId",
            "/receipt/authenticatedManifestSha256",
        ] {
            let mut v = serde_json::to_value(&source).unwrap();
            *v.pointer_mut(pointer).unwrap() = "different".into();
            assert!(
                job.matches_source(&serde_json::from_value(v).unwrap())
                    .is_err()
            );
        }
        let mut changed = source.clone();
        changed.receipt.as_mut().unwrap().files += 1;
        assert!(job.matches_source(&changed).is_err());
        changed = source;
        changed.state = "failed".into();
        assert!(job.matches_source(&changed).is_err());
    }
    #[test]
    fn journal_completion_is_not_activation_or_backup_creation() {
        let mut job = job();
        assert!(job.validate().is_ok());
        let mut oversized = job.clone();
        oversized.job_budget_bytes = 32 * 1024 * 1024 * 1024 + 1;
        assert!(oversized.validate().is_err());
        job.state = "completed".into();
        job.stage = "completed".into();
        assert!(job.validate().is_err());
        job.receipt = Some(receipt(&job));
        assert!(job.validate().is_ok());
        job.receipt.as_mut().unwrap().operation = "created-and-verified".into();
        assert!(job.validate().is_err());
        let mut job = self::job();
        job.state = "interrupted".into();
        job.stage = "interrupted".into();
        assert!(job.validate().is_err());
        job.error_code = Some("HOSTED_RECOVERY_INTERRUPTED".into());
        assert!(job.validate().is_ok());
    }
    fn helper(job: &CheckJob, source: &str, cid: &str) -> Value {
        serde_json::json!({ "Id":cid,"Name":format!("/exhibitos-hosted-restore-{}",job.id),"Image":job.image,
            "Config":{"Image":job.image,"User":"0:0","Labels":{"com.exhibitos.hosted-restore":job.id},"Env":["TMPDIR=/work"],"Entrypoint":["node"],"Cmd":["--disable-proto=throw","--no-warnings","scripts/remote-restore-entry.mjs"]},
            "HostConfig":{"Privileged":false,"ReadonlyRootfs":true,"AutoRemove":false,"NetworkMode":"none","PidMode":"","CapDrop":["ALL"],"CapAdd":["CHOWN","SETUID","SETGID","DAC_READ_SEARCH","KILL"],"Memory":1610612736_u64,"PidsLimit":128,"SecurityOpt":["no-new-privileges:true"],"PortBindings":{},"Tmpfs":{"/tmp":"rw,nosuid,nodev,size=256m,mode=1777","/run/exhibitos":"rw,nosuid,nodev,size=1m,mode=0700","/var/lib/postgresql":"rw,nosuid,nodev,size=1m,mode=0700"},"Mounts":[{"Type":"bind","Source":source,"Target":"/source","ReadOnly":true}]},
            "Mounts":[{"Type":"volume","Name":format!("exhibitos-hosted-restore-work-{}",job.id),"Destination":"/work","RW":true},{"Type":"bind","Source":source,"Destination":"/source","RW":false}],
            "State":{"Running":false,"Restarting":false,"Status":"exited","ExitCode":0,"OOMKilled":false}
        })
    }
    #[test]
    fn helper_is_exact_owned_network_isolated_and_archive_read_only() {
        let job = job();
        let cid = "d".repeat(64);
        let path = Path::new("/Volumes/ExhibitSSD/synthetic/archive");
        let base = helper(&job, path.to_str().unwrap(), &cid);
        assert!(validate_helper(&job, &base, path, &cid, false).is_ok());
        for (pointer, value) in [
            ("/Name", serde_json::json!("/foreign")),
            (
                "/Image",
                serde_json::json!(format!("sha256:{}", "e".repeat(64))),
            ),
            ("/HostConfig/NetworkMode", serde_json::json!("bridge")),
            ("/HostConfig/Privileged", serde_json::json!(true)),
            (
                "/HostConfig/PortBindings",
                serde_json::json!({"55432/tcp":[{"HostPort":"55432"}]}),
            ),
            ("/Mounts/1/RW", serde_json::json!(true)),
            ("/HostConfig/PidMode", serde_json::json!("host")),
            ("/HostConfig/Mounts/0/ReadOnly", serde_json::json!(false)),
            (
                "/Config/Cmd",
                serde_json::json!(["scripts/remote-backup-entry.mjs"]),
            ),
        ] {
            let mut changed = base.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate_helper(&job, &changed, path, &cid, false).is_err(),
                "{pointer}"
            );
        }
        for caps in [
            serde_json::json!([]),
            serde_json::json!(["CHOWN", "SETUID", "SETGID", "DAC_READ_SEARCH"]),
            serde_json::json!(["CHOWN", "SETUID", "SETGID", "DAC_OVERRIDE"]),
            serde_json::json!(["CHOWN", "SETUID", "SETGID", "DAC_READ_SEARCH", "SYS_ADMIN"]),
            serde_json::json!(["CHOWN", "SETUID", "CHOWN", "DAC_READ_SEARCH"]),
        ] {
            let mut changed = base.clone();
            changed["HostConfig"]["CapAdd"] = caps;
            assert!(validate_helper(&job, &changed, path, &cid, false).is_err());
        }
        let mut changed = base.clone();
        changed["Mounts"].as_array_mut().unwrap().push(
            serde_json::json!({"Type":"bind","Source":"/foreign","Destination":"/other","RW":true}),
        );
        assert!(validate_helper(&job, &changed, path, &cid, false).is_err());
    }
    #[test]
    fn desktop_mapping_needs_exact_source_and_observed_engine() {
        let job = job();
        let cid = "d".repeat(64);
        let path = Path::new("/Volumes/ExhibitSSD/synthetic/archive");
        let mut v = helper(&job, path.to_str().unwrap(), &cid);
        v["Mounts"][1]["Source"] = format!("/host_mnt{}", path.display()).into();
        assert!(validate_helper(&job, &v, path, &cid, false).is_err());
        assert_eq!(
            validate_helper(&job, &v, path, &cid, true).is_ok(),
            cfg!(target_os = "macos")
        );
        v["HostConfig"]["Mounts"][0]["Source"] = "/foreign".into();
        assert!(validate_helper(&job, &v, path, &cid, true).is_err());
    }
    #[test]
    fn relaunch_marks_unfinished_check_interrupted_without_touching_original() {
        let root =
            std::env::temp_dir().join(format!("manager-hosted-restore-test-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        let job = job();
        let old = original(&job);
        let original_path = service.root.join(format!("hosted-backup-{}", old.id));
        let _old_dir = restoration::directory(&original_path).unwrap();
        service.save_hosted_backup_job(&old).unwrap();
        let original_bytes = fs::read(original_path.join("job.json")).unwrap();
        let _new_dir =
            restoration::directory(&service.root.join(format!("hosted-restoration-{}", job.id)))
                .unwrap();
        service.save_hosted_restore_check(&job).unwrap();
        service
            .begin_maintenance("hosted-restoration", &job.id, &job.stage)
            .unwrap();
        let reopened = LifecycleService::new(service.root.clone()).unwrap();
        let observed = reopened.hosted_restore_checks().unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].state, "interrupted");
        assert!(observed[0].receipt.is_none());
        assert_eq!(
            fs::read(original_path.join("job.json")).unwrap(),
            original_bytes
        );
        assert!(
            reopened
                .reconciliation_target("hosted-restoration", &job.id)
                .is_ok()
        );
        assert!(
            reopened
                .request_maintenance_cancel("hosted-restoration", &job.id, true)
                .is_err()
        );
        assert_eq!(
            reopened.maintenance_context().unwrap().unwrap().state,
            "interrupted"
        );
        fs::remove_dir_all(&reopened.root).unwrap();
    }
    #[test]
    fn cancellation_request_requires_live_owner_and_preserves_restore_candidate() {
        let root =
            std::env::temp_dir().join(format!("manager-hosted-cancel-test-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        let job = job();
        let workspace = service.root.join(format!("hosted-restoration-{}", job.id));
        let _workspace = restoration::directory(&workspace).unwrap();
        service.save_hosted_restore_check(&job).unwrap();
        service
            .begin_maintenance("hosted-restoration", &job.id, &job.stage)
            .unwrap();
        assert!(
            service
                .request_maintenance_cancel("hosted-restoration", &job.id, false)
                .is_err()
        );
        assert!(
            service
                .request_maintenance_cancel("hosted-restoration", &job.id, true)
                .is_err()
        );
        let operation = service.lock().unwrap();
        let requested = service
            .request_maintenance_cancel("hosted-restoration", &job.id, true)
            .unwrap();
        assert_eq!(requested.state, "requested");
        assert_eq!(
            service.hosted_restore_check(&job.id).unwrap().state,
            "running"
        );
        assert!(workspace.join("job.json").exists());
        drop(operation);
        fs::remove_dir_all(&service.root).unwrap();
    }
}
