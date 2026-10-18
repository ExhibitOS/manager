// SPDX-License-Identifier: Apache-2.0
//! Hosted archives retain their actual source class, without installation records.
use super::installation_backup::source_bytes;
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub workspace_id: String,
    pub service_id: String,
    pub postgres_id: String,
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.workspace_id, &self.service_id, &self.postgres_id] {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(err("HOSTED_RECOVERY_REQUEST"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub provider: String,
    pub action: String,
    pub workspace_id: String,
    pub service_id: String,
    pub postgres_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub operation: String,
    pub backup_id: String,
    pub files: u64,
    pub authenticated_manifest_sha256: String,
}
impl Receipt {
    pub fn validate(&self, source: &Source) -> Result<()> {
        if self.provider != "render"
            || self.action != "local-encrypted-backup"
            || self.operation != "created-and-verified"
            || self.workspace_id != source.workspace_id
            || self.service_id != source.service_id
            || self.postgres_id != source.postgres_id
            || !uuid(&self.backup_id)
            || self.files == 0
            || self.files > 100000
            || !hash_valid(&self.authenticated_manifest_sha256)
            || self.expires_at.as_ref().is_some_and(|v| {
                v.len() > 32
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b"-T:.Z+".contains(&b))
            })
        {
            return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub source: Source,
    pub image: String,
    #[serde(default)]
    pub job_budget_bytes: Option<u64>,
    pub state: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub receipt: Option<Receipt>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub job_id: String,
    pub helper_name: String,
    pub image: String,
    pub output_path: String,
    pub work_volume: String,
    pub docker_path: String,
    pub docker_endpoint: String,
    pub max_dump_bytes: u64,
    pub job_budget_bytes: u64,
}
pub(crate) fn runner_error(bytes: &[u8]) -> LifecycleError {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Failure {
        error_code: String,
    }
    let allowed = [
        "HOSTED_TOKEN_REQUIRED",
        "HOSTED_TOKEN_INVALID",
        "HOSTED_PERMISSION_REQUIRED",
        "HOSTED_PAID_PLAN_REFUSED",
        "HOSTED_RATE_LIMITED",
        "HOSTED_CONNECTION_FAILED",
        "HOSTED_REDIRECT_REFUSED",
        "HOSTED_RESPONSE_INVALID",
        "HOSTED_REMOTE_REJECTED",
        "HOSTED_RESOURCE_CONFLICT",
        "HOSTED_RECOVERY_REQUEST",
        "HOSTED_RECOVERY_CONFIGURATION_REQUIRED",
        "HOSTED_RECOVERY_CONFIGURATION_CHANGED",
        "HOSTED_RECOVERY_NETWORK_REQUIRED",
        "HOSTED_RECOVERY_DATABASE_EXPIRED",
        "HOSTED_RECOVERY_WRITERS_ACTIVE",
        "HOSTED_RECOVERY_CONNECTION_INVALID",
        "HOSTED_RECOVERY_TLS_REQUIRED",
        "HOSTED_RECOVERY_OPERATOR_REQUIRED",
        "HOSTED_RECOVERY_EXECUTION_FAILED",
        "HOSTED_RECOVERY_SOURCE_CHANGED",
        "HOSTED_RECOVERY_RESULT_INVALID",
        "HOSTED_BACKUP_REQUEST_INVALID",
        "HOSTED_BACKUP_OUTPUT_CHANGED",
        "HOSTED_BACKUP_HELPER_FAILED",
        "HOSTED_BACKUP_RESULT_INVALID",
        "HOSTED_RESTORE_CHECK_FAILED",
    ];
    if bytes.len() <= 1024
        && let Ok(value) = serde_json::from_slice::<Failure>(bytes)
        && allowed.contains(&value.error_code.as_str()) {
        return err(&value.error_code);
    }
    err("HOSTED_RECOVERY_EXECUTION_FAILED")
}
pub fn dump_budget(available: u64) -> Result<u64> {
    let usable = job_budget(available)?;
    let budget = (usable / 4).min(32 * 1024 * 1024 * 1024);
    if budget < 1024 * 1024 {
        return Err(err("INSUFFICIENT_STORAGE"));
    }
    Ok(budget)
}
pub fn job_budget(available: u64) -> Result<u64> {
    let usable = available
        .saturating_sub(6 * 1024 * 1024 * 1024)
        .saturating_sub(64 * 1024 * 1024);
    if usable < 4 * 1024 * 1024 {
        return Err(err("INSUFFICIENT_STORAGE"));
    }
    Ok(usable)
}
struct SecretInput(std::io::Cursor<Vec<u8>>);
impl Read for SecretInput {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(b)
    }
}
impl Drop for SecretInput {
    fn drop(&mut self) {
        self.0.get_mut().fill(0);
    }
}
#[cfg(target_os = "macos")]
pub(crate) fn output_guard(path: &Path) -> Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| err("HOSTED_RECOVERY_PATH_CHANGED"))?;
    super::backup_export::native::check(&file, path)?;
    if file.metadata().map_err(|_| err("STATE_INVALID"))?.mode() & 0o7777 != 0o700 {
        return Err(err("BACKUP_PRIVATE_PERMISSIONS"));
    }
    Ok(file)
}
pub(crate) fn uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
impl Job {
    pub(crate) fn validate(&self) -> Result<()> {
        self.source.validate()?;
        if !uuid(&self.id)
            || !self.image.strip_prefix("sha256:").is_some_and(hash_valid)
            || !["running", "completed", "failed", "interrupted"].contains(&self.state.as_str())
            || ![
                "checking-source-and-creating",
                "completed",
                "failed",
                "interrupted",
            ]
            .contains(&self.stage.as_str())
            || self.job_budget_bytes.is_some_and(|v| v < 4 * 1024 * 1024)
            || self.created_at > self.updated_at
            || self.updated_at > 8_640_000_000_000_000
            || (["failed", "interrupted"].contains(&self.state.as_str())
                != self.error_code.is_some())
            || self.error_code.as_ref().is_some_and(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            })
            || ((self.state == "completed") != self.receipt.is_some())
        {
            return Err(err("STATE_INVALID"));
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate(&self.source)?;
        }
        Ok(())
    }
}
impl LifecycleService {
    fn check_hosted_work_volume(&self, id: &str) -> Result<()> {
        if !uuid(id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let name = format!("exhibitos-hosted-backup-work-{id}");
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
            || value[0]["Labels"]["com.exhibitos.hosted-backup"] != id
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(())
    }
    pub(crate) fn hosted_helper_state(&self, job: &Job) -> Result<String> {
        let name = format!("exhibitos-hosted-backup-{}", job.id);
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &name,
            "com.exhibitos.hosted-backup",
            &job.id,
        )?;
        let Some(id) = ids.first() else {
            return Ok("absent".into());
        };
        let value = backup_creation::inspected("docker", &["inspect".into(), id.clone()])?;
        self.check_hosted_work_volume(&job.id)?;
        validate_helper(
            job,
            &value,
            &self.root.join(format!("hosted-backup-{}/output", job.id)),
            id,
            observed_desktop_mapping(&value)?,
        )?;
        Ok(if value["State"]["Running"] == true {
            "running"
        } else {
            "stopped"
        }
        .into())
    }
    pub(crate) fn stop_owned_hosted_backup(&self, id: &str) -> Result<()> {
        let job = self.hosted_backup_job(id)?;
        let state = self.hosted_helper_state(&job)?;
        if state == "absent" || state == "stopped" {
            return Ok(());
        }
        let ids = helper_reconciliation::list(
            |args| run("docker", args, None, 30),
            &format!("exhibitos-hosted-backup-{id}"),
            "com.exhibitos.hosted-backup",
            id,
        )?;
        let cid = ids.first().ok_or_else(|| err("CANCEL_UNCERTAIN"))?;
        let observed = backup_creation::inspected("docker", &["inspect".into(), cid.clone()])?;
        validate_helper(
            &job,
            &observed,
            &self.root.join(format!("hosted-backup-{id}/output")),
            cid,
            observed_desktop_mapping(&observed)?,
        )?;
        run(
            "docker",
            &["stop".into(), "--time".into(), "30".into(), cid.clone()],
            None,
            60,
        )?;
        if self.hosted_helper_state(&job)? != "stopped" {
            return Err(err("CANCEL_UNCERTAIN"));
        }
        Ok(())
    }
    pub(crate) fn validate_hosted_archive(&self, id: &str, budget: u64) -> Result<u64> {
        let relative = format!("hosted-backup-{id}/output/archive");
        let archive = self.root.join(&relative);
        maintenance::input_path(&archive, true)?;
        let mut root_names = std::collections::BTreeSet::new();
        let mut total = 0u64;
        for entry in fs::read_dir(&archive).map_err(|_| err("BACKUP_SOURCE_INVALID"))? {
            let entry = entry.map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
            if !["complete.json", "writing.json", "manifest.gcm", "files"].contains(&name.as_str())
                || !root_names.insert(name.clone())
            {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
            if name != "files" {
                let limit = if name == "manifest.gcm" {
                    16 * 1024 * 1024
                } else {
                    1024 * 1024
                };
                let bytes = source_bytes(&self.root, &format!("{relative}/{name}"), limit, true)?;
                total += bytes.len() as u64;
            }
        }
        if root_names.len() != 4 {
            return Err(err("BACKUP_SOURCE_INVALID"));
        }
        maintenance::input_path(&archive.join("files"), true)?;
        let mut count = 0u64;
        for entry in
            fs::read_dir(archive.join("files")).map_err(|_| err("BACKUP_SOURCE_INVALID"))?
        {
            count += 1;
            if count > 100000 {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
            let entry = entry.map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
            if name.len() != 12
                || !name.ends_with(".gcm")
                || !name.as_bytes()[..8].iter().all(u8::is_ascii_digit)
            {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
            if meta.file_type().is_symlink()
                || !meta.is_file()
                || fs::canonicalize(&path).ok() != Some(path.clone())
            {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() != 1
                    || meta.mode() & 0o7777 != 0o600
                    || meta.uid() != unsafe { libc::geteuid() }
                {
                    return Err(err("BACKUP_PRIVATE_PERMISSIONS"));
                }
            }
            total = total
                .checked_add(meta.len())
                .ok_or_else(|| err("BACKUP_SOURCE_INVALID"))?;
            if total > budget {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
        }
        if count == 0 {
            return Err(err("BACKUP_SOURCE_INVALID"));
        }
        Ok(count)
    }
    /// Existing private hosted-job workspace, only for native companion metadata.
    /// This does not turn the DB archive into a complete installation archive.
    #[cfg(target_os="macos")]
    pub fn hosted_publication_workspace(&self,id:&str)->Result<PathBuf>{
        if !uuid(id){return Err(err("HOSTED_RECOVERY_REQUEST"));}
        self.hosted_backup_job(id)?;
        let path=fs::canonicalize(self.root.join(format!("hosted-backup-{id}"))).map_err(|_|err("BACKUP_SOURCE_INVALID"))?;
        let expected=self.root.join(format!("hosted-backup-{id}"));
        if path!=expected{return Err(err("BACKUP_SOURCE_INVALID"));}
        let _pin=output_guard(&path)?;
        Ok(path)
    }

    #[cfg(target_os="macos")]
    pub fn hosted_publication_budget(&self,id:&str)->Result<u64>{
        let folder=self.hosted_publication_workspace(id)?;
        job_budget(fs2::available_space(folder).map_err(|_|err("STORAGE_UNAVAILABLE"))?)
    }

    /// Trusted native caller supplies fixed pinned Node/runner paths and private input.
    /// Paths/keys/binaries are never accepted on a renderer recovery request.
    #[cfg(target_os = "macos")]
    pub fn create_hosted_backup(
        &self,
        source: Source,
        image: &str,
        node: &Path,
        runner: &Path,
        make_input: impl FnOnce(&Execution) -> Result<Vec<u8>>,
        check_resources: impl Fn() -> Result<()>,
    ) -> Result<Job> {
        let _lock = self.lock()?;
        source.validate()?;
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
        let max_dump_bytes = dump_budget(available)?;
        let job_budget_bytes = job_budget(available)?;
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
        let workspace = self.root.join(format!("hosted-backup-{id}"));
        let _workspace = restoration::directory(&workspace)?;
        let output = workspace.join("output");
        let _output = restoration::directory(&output)?;
        let workspace_pin = output_guard(&workspace)?;
        let output_pin = output_guard(&output)?;
        let now = now();
        let mut job = Job {
            id: id.clone(),
            source,
            image: image.into(),
            job_budget_bytes: Some(job_budget_bytes),
            state: "running".into(),
            stage: "checking-source-and-creating".into(),
            error_code: None,
            receipt: None,
            created_at: now,
            updated_at: now,
        };
        self.save_hosted_backup_job(&job)?;
        self.begin_maintenance("hosted-backup", &id, &job.stage)?;
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
                helper_name: format!("exhibitos-hosted-backup-{id}"),
                image: image.into(),
                output_path: output
                    .to_str()
                    .ok_or_else(|| err("BACKUP_PATH_INVALID"))?
                    .into(),
                work_volume: format!("exhibitos-hosted-backup-work-{id}"),
                docker_path: docker
                    .to_str()
                    .ok_or_else(|| err("RUNTIME_MISSING"))?
                    .into(),
                docker_endpoint: endpoint,
                max_dump_bytes,
                job_budget_bytes,
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
                    format!("com.exhibitos.hosted-backup={id}"),
                    plan.work_volume.clone(),
                ],
                None,
                30,
            )?;
            self.check_hosted_work_volume(&id)?;
            let input = make_input(&plan)?;
            if input.len() > 131072 {
                return Err(err("ENGINE_INPUT_LIMIT"));
            }
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            super::backup_export::native::check(&output_pin, &output)?;
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
                ObservedInputs { files: (None, None), streamed: Some((Box::new(SecretInput(std::io::Cursor::new(input))), 131072)) },
                true,
                || {
                    if Instant::now() >= next {
                        next = Instant::now() + Duration::from_millis(400);
                        check_resources()?;
                        super::backup_export::native::check(&workspace_pin, &workspace)?;
                        super::backup_export::native::check(&output_pin, &output)?;
                        if self.maintenance_requested("hosted-backup", &id)?
                            && self.hosted_helper_state(&job)? != "absent"
                        {
                            self.maintenance_checkpoint("hosted-backup", &id)?;
                        }
                    }
                    Ok(())
                },
            )?;
            check_resources()?;
            super::backup_export::native::check(&workspace_pin, &workspace)?;
            super::backup_export::native::check(&output_pin, &output)?;
            self.maintenance_checkpoint("hosted-backup", &id)?;
            if bytes.len() > 16384 {
                return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
            }
            let receipt: Receipt = serde_json::from_slice(&bytes)
                .map_err(|_| err("HOSTED_RECOVERY_EXECUTION_FAILED"))?;
            receipt.validate(&job.source)?;
            if self.hosted_helper_state(&job)? != "stopped" {
                return Err(err("HOSTED_RECOVERY_HELPER_UNCERTAIN"));
            }
            if self.validate_hosted_archive(&id, job_budget_bytes)? != receipt.files {
                return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
            }
            Ok(receipt)
        })();
        let _finish = self.maintenance_finish_guard("hosted-backup", &id)?;
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
        self.save_hosted_backup_job(&job)?;
        Ok(job)
    }
    pub(crate) fn hosted_backup_job(&self, id: &str) -> Result<Job> {
        if !uuid(id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let bytes = source_bytes(
            &self.root,
            &format!("hosted-backup-{id}/job.json"),
            16384,
            true,
        )?;
        let job: Job = serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
        job.validate()?;
        if job.id != id {
            return Err(err("STATE_INVALID"));
        }
        Ok(job)
    }
    pub fn hosted_backup_jobs(&self) -> Result<Vec<Job>> {
        let mut jobs = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let entry = entry.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name.strip_prefix("hosted-backup-") else {
                continue;
            };
            if !uuid(id) {
                continue;
            }
            if jobs.len() >= 10000 {
                return Err(err("STATE_INVALID"));
            }
            jobs.push(self.hosted_backup_job(id)?);
        }
        jobs.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(jobs)
    }
    pub(crate) fn save_hosted_backup_job(&self, job: &Job) -> Result<()> {
        job.validate()?;
        write_json(
            &self.root.join(format!("hosted-backup-{}", job.id)),
            "job.json",
            job,
        )
    }
    pub(crate) fn recover_hosted_backups(&self) -> Result<()> {
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        for mut job in self.hosted_backup_jobs()? {
            if job.state == "running" {
                job.state = "interrupted".into();
                job.stage = "interrupted".into();
                job.error_code = Some("HOSTED_RECOVERY_INTERRUPTED".into());
                job.updated_at = now();
                self.save_hosted_backup_job(&job)?;
            }
        }
        Ok(())
    }
}
pub(crate) fn helper_resources_match(value: &Value) -> bool {
    fn tmpfs(value: &Value, bytes: u64, mode: u32) -> bool {
        let Some(text) = value.as_str() else {
            return false;
        };
        let options: std::collections::BTreeSet<_> = text.split(',').collect();
        if options.len() != 5
            || !["rw", "nosuid", "nodev"]
                .iter()
                .all(|v| options.contains(v))
        {
            return false;
        }
        let size = options
            .iter()
            .find_map(|v| v.strip_prefix("size="))
            .and_then(|v| {
                v.strip_suffix('m')
                    .and_then(|n| n.parse::<u64>().ok())
                    .and_then(|n| n.checked_mul(1024 * 1024))
                    .or_else(|| v.parse::<u64>().ok())
            });
        let actual_mode = options
            .iter()
            .find_map(|v| v.strip_prefix("mode="))
            .and_then(|v| u32::from_str_radix(v, 8).ok());
        size == Some(bytes) && actual_mode == Some(mode)
    }
    let temporary = &value["HostConfig"]["Tmpfs"];
    let env = value["Config"]["Env"].as_array();
    value["Config"]["User"] == "0:0"
        && value["HostConfig"]["Memory"] == 1536_u64 * 1024 * 1024
        && value["HostConfig"]["PidsLimit"] == 128
        && value["HostConfig"]["SecurityOpt"] == serde_json::json!(["no-new-privileges:true"])
        && temporary.as_object().is_some_and(|v| v.len() == 3)
        && tmpfs(&temporary["/tmp"], 256 * 1024 * 1024, 0o1777)
        && tmpfs(&temporary["/run/exhibitos"], 1024 * 1024, 0o700)
        && tmpfs(&temporary["/var/lib/postgresql"], 1024 * 1024, 0o700)
        && env.is_some_and(|v| {
            v.iter()
                .filter_map(Value::as_str)
                .filter(|s| s.starts_with("TMPDIR="))
                .collect::<Vec<_>>()
                == ["TMPDIR=/work"]
        })
}
fn observed_desktop_mapping(value: &Value) -> Result<bool> {
    if !cfg!(target_os = "macos") || !value["Mounts"].as_array().is_some_and(|mounts| mounts.iter().any(|m|m["Destination"]=="/output" && m["Source"].as_str().is_some_and(|s|s.starts_with("/host_mnt/")))) { return Ok(false); }
    let bytes=run("docker", &["info".into(),"--format".into(),"{{json .}}".into()],None,30)?;
    if bytes.len()>256*1024 { return Err(err("ENGINE_OUTPUT_LIMIT")); }
    let info:Value=serde_json::from_slice(&bytes).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?;
    Ok(info["OperatingSystem"]=="Docker Desktop" && info["OSType"]=="linux")
}
fn output_bind_matches(value: &Value, mount: &Value, expected: &str, desktop: bool) -> bool {
    if mount["Type"]!="bind" || mount["RW"]!=true { return false; }
    if mount["Source"]==expected { return true; }
    cfg!(target_os="macos") && desktop && mount["Source"]==format!("/host_mnt{expected}")
        && value["HostConfig"]["Mounts"].as_array().is_some_and(|mounts| {
            let selected:Vec<_>=mounts.iter().filter(|m|m["Target"]=="/output").collect();
            selected.len()==1 && selected[0]["Type"]=="bind"
                && (selected[0]["Source"]==expected || selected[0]["Source"]==format!("/host_mnt{expected}"))
                && (selected[0]["ReadOnly"].is_null() || selected[0]["ReadOnly"]==false)
        })
}
fn validate_helper(job: &Job, value: &Value, output: &Path, cid: &str, desktop: bool) -> Result<()> {
    let mounts = value["Mounts"]
        .as_array()
        .ok_or_else(|| err("OWNERSHIP_CONFLICT"))?;
    if value["Id"] != cid
        || cid.len() != 64
        || !hash_valid(cid)
        || value["Name"] != format!("/exhibitos-hosted-backup-{}", job.id)
        || value["Config"]["Labels"]["com.exhibitos.hosted-backup"] != job.id
        || value["Config"]["Image"] != job.image
        || value["Image"] != job.image
        || value["HostConfig"]["Privileged"] != false
        || value["HostConfig"]["ReadonlyRootfs"] != true
        || value["HostConfig"]["AutoRemove"] != false
        || value["HostConfig"]["NetworkMode"] != "bridge"
        || value["HostConfig"]["CapDrop"] != serde_json::json!(["ALL"])
        || !helper_resources_match(value)
        || value["Config"]["Entrypoint"] != serde_json::json!(["node"])
        || value["Config"]["Cmd"]
            != serde_json::json!([
                "--disable-proto=throw",
                "--no-warnings",
                "scripts/remote-backup-entry.mjs"
            ])
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
                && m["Name"] == format!("exhibitos-hosted-backup-work-{}", job.id)
                && m["RW"] == true
        })
        .count();
    let out = mounts
        .iter()
        .filter(|m| {
            m["Destination"] == "/output"
                && output_bind_matches(value,m,output.to_string_lossy().as_ref(),desktop)
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
    fn source() -> Source {
        Source {
            workspace_id: "tea-synthetic".into(),
            service_id: "srv-synthetic".into(),
            postgres_id: "dpg-synthetic".into(),
        }
    }
    #[cfg(target_os="macos")]
    #[test]
    fn publication_workspace_requires_known_job_and_private_canonical_directory(){
        let root=std::env::temp_dir().join(format!("exhibitos-publication-path-{}",Uuid::new_v4()));
        fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();
        let service=LifecycleService::new(root.clone()).unwrap();let id=Uuid::new_v4().to_string();
        assert!(service.hosted_publication_workspace(&id).is_err());
        assert!(service.hosted_publication_workspace("../outside").is_err());
        let job=Job{id:id.clone(),source:source(),image:format!("sha256:{}","a".repeat(64)),job_budget_bytes:Some(4194304),state:"running".into(),stage:"checking-source-and-creating".into(),error_code:None,receipt:None,created_at:1,updated_at:1};
        let folder=root.join(format!("hosted-backup-{id}"));
        fs::create_dir(&folder).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&folder,fs::Permissions::from_mode(0o700)).unwrap();
        service.save_hosted_backup_job(&job).unwrap();
        assert_eq!(service.hosted_publication_workspace(&id).unwrap(),folder);
        fs::set_permissions(&folder,fs::Permissions::from_mode(0o755)).unwrap();
        assert!(service.hosted_publication_workspace(&id).is_err());
        fs::set_permissions(&folder,fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(service.hosted_backup_jobs().unwrap().len(),1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn helper_resource_identity_preserves_private_key_and_disk_work_boundaries() {
        let v = serde_json::json!({"Config":{"User":"0:0","Env":["PATH=/usr/bin:/bin","TMPDIR=/work"]},"HostConfig":{"Memory":1610612736_u64,"PidsLimit":128,"SecurityOpt":["no-new-privileges:true"],"Tmpfs":{"/tmp":"rw,nosuid,nodev,size=256m,mode=1777","/run/exhibitos":"rw,nosuid,nodev,size=1m,mode=0700","/var/lib/postgresql":"rw,nosuid,nodev,size=1m,mode=0700"}}});
        assert!(helper_resources_match(&v));
        for (pointer, value) in [
            ("/HostConfig/Memory", serde_json::json!(0)),
            ("/HostConfig/PidsLimit", serde_json::json!(0)),
            ("/HostConfig/SecurityOpt", serde_json::json!([])),
            ("/Config/Env", serde_json::json!(["TMPDIR=/tmp"])),
            (
                "/HostConfig/Tmpfs",
                serde_json::json!({"/tmp":"rw,nosuid,nodev,size=256m,mode=1777"}),
            ),
        ] {
            let mut changed = v.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(!helper_resources_match(&changed));
        }
    }
    #[test]
    fn output_mapping_requires_exact_host_mount_and_observed_desktop() {
        let expected="/Volumes/ExhibitSSD/private/output";
        let value=serde_json::json!({"HostConfig":{"Mounts":[{"Type":"bind","Source":expected,"Target":"/output","ReadOnly":false}]}});
        let mut mount=serde_json::json!({"Type":"bind","Source":expected,"RW":true});
        assert!(output_bind_matches(&value,&mount,expected,false));
        mount["Source"]=format!("/host_mnt{expected}").into();
        assert!(!output_bind_matches(&value,&mount,expected,false));
        assert_eq!(output_bind_matches(&value,&mount,expected,true),cfg!(target_os="macos"));
        for path in [format!("/host_mnt{expected}/../other"),"/host_mnt/foreign/output".into()] {
            mount["Source"]=path.into();assert!(!output_bind_matches(&value,&mount,expected,true));
        }
        mount["Source"]=format!("/host_mnt{expected}").into();
        let mut wrong=value.clone();wrong["HostConfig"]["Mounts"][0]["Source"]="/foreign".into();
        assert!(!output_bind_matches(&wrong,&mount,expected,true));
        wrong=value.clone();wrong["HostConfig"]["Mounts"][0]["ReadOnly"]=true.into();
        assert!(!output_bind_matches(&wrong,&mount,expected,true));
        wrong=value.clone();wrong["HostConfig"]["Mounts"].as_array_mut().unwrap().push(value["HostConfig"]["Mounts"][0].clone());
        assert!(!output_bind_matches(&wrong,&mount,expected,true));
    }
    #[test]
    fn receipt_is_closed_and_bound_to_the_selected_source() {
        let s = source();
        assert!(s.validate().is_ok());
        let receipt = Receipt {
            provider: "render".into(),
            action: "local-encrypted-backup".into(),
            workspace_id: s.workspace_id.clone(),
            service_id: s.service_id.clone(),
            postgres_id: s.postgres_id.clone(),
            expires_at: Some("2099-01-01T00:00:00.000Z".into()),
            operation: "created-and-verified".into(),
            backup_id: Uuid::new_v4().to_string(),
            files: 4,
            authenticated_manifest_sha256: "a".repeat(64),
        };
        assert!(receipt.validate(&s).is_ok());
        let mut wrong = s;
        wrong.postgres_id = "dpg-other".into();
        assert!(receipt.validate(&wrong).is_err());
        let mut v = serde_json::to_value(&receipt).unwrap();
        v["databaseURL"] = serde_json::json!("synthetic-private");
        assert!(serde_json::from_value::<Receipt>(v).is_err());
    }
    #[test]
    fn running_job_never_means_backup_completed() {
        let mut job = Job {
            id: Uuid::new_v4().to_string(),
            source: source(),
            image: format!("sha256:{}", "a".repeat(64)),
            job_budget_bytes: Some(64 * 1024 * 1024 * 1024),
            state: "running".into(),
            stage: "checking-source-and-creating".into(),
            error_code: None,
            receipt: None,
            created_at: 1,
            updated_at: 1,
        };
        assert!(job.validate().is_ok());
        job.state = "completed".into();
        job.stage = "completed".into();
        assert!(job.validate().is_err());
    }
    #[test]
    fn budget_preserves_floor_and_uses_no_more_than_existing_dump_limit() {
        let floor = 6 * 1024 * 1024 * 1024;
        assert!(job_budget(floor).is_err());
        assert!(dump_budget(floor + 64 * 1024 * 1024 + 4 * 1024 * 1024 - 1).is_err());
        assert_eq!(
            dump_budget(floor + 64 * 1024 * 1024 + 4 * 1024 * 1024).unwrap(),
            1024 * 1024
        );
        assert_eq!(
            dump_budget(1024 * 1024 * 1024 * 1024).unwrap(),
            32 * 1024 * 1024 * 1024
        );
        let available = 14 * 1024 * 1024 * 1024;
        assert!(4 * dump_budget(available).unwrap() + floor + 64 * 1024 * 1024 <= available);
        // Dump allowance alone does not establish the separate object-copy budget.
    }
    #[test]
    fn private_runner_errors_are_fixed_codes_only() {
        assert_eq!(
            runner_error(br#"{"errorCode":"HOSTED_RECOVERY_WRITERS_ACTIVE"}"#).code,
            "HOSTED_RECOVERY_WRITERS_ACTIVE"
        );
        for bytes in [
            br#"{"errorCode":"synthetic-private-token"}"#.as_slice(),
            br#"{"errorCode":"HOSTED_TOKEN_INVALID","url":"synthetic-private"}"#.as_slice(),
            b"unstructured provider exception".as_slice(),
        ] {
            assert_eq!(runner_error(bytes).code, "HOSTED_RECOVERY_EXECUTION_FAILED");
        }
    }
}
