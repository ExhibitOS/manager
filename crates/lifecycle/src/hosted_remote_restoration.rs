// SPDX-License-Identifier: Apache-2.0
//! Distinct remote restore operations; local CheckJob remains a local copy check.
use super::*;
use crate::hosted_backup::{Job, Source};
// Private streamed native input owns its bytes until the observer finishes.
// Keep the same Read + Drop zeroization boundary as hosted_backup's private input.
#[cfg(target_os = "macos")]
struct SecretInput(std::io::Cursor<Vec<u8>>);
#[cfg(target_os = "macos")]
impl std::io::Read for SecretInput {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, bytes)
    }
}
#[cfg(target_os = "macos")]
impl Drop for SecretInput {
    fn drop(&mut self) { self.0.get_mut().fill(0); }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub workspace_id: String,
    pub postgres_id: Option<String>,
    pub create_requested: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub source_job_id: String,
    pub source: Source,
    pub target: Target,
    pub expected_backup_id: String,
    pub expected_manifest_sha256: String,
    pub expected_files: u64,
    pub source_job_sha256: String,
    pub target_identity_sha256: Option<String>,
    pub state: String,
    pub error_code: Option<String>,
}
fn valid_target(t: &Target) -> bool {
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    };
    valid(&t.workspace_id)
        && match &t.postgres_id {
            Some(id) => valid(id),
            None => t.create_requested,
        }
}

impl LifecycleService {
    pub fn hosted_remote_restores(&self) -> Result<Vec<Operation>> {
        let mut result = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let entry = entry.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_prefix("hosted-remote-restoration-") else {
                continue;
            };
            if !hosted_backup::uuid(id) {
                continue;
            }
            if result.len() >= 10000 {
                return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
            }
            let bytes =
                installation_backup::source_bytes(&entry.path(), "operation.json", 16384, true)?;
            let op: Operation = serde_json::from_slice(&bytes)
                .map_err(|_| err("HOSTED_RECOVERY_RESULT_INVALID"))?;
            let job = self.completed_hosted_backup(&op.source_job_id)?;
            validate(&op, &job, id)?;
            let source = installation_backup::source_bytes(
                &self
                    .root
                    .join(format!("hosted-backup-{}", op.source_job_id)),
                "job.json",
                16384,
                true,
            )?;
            if format!("{:x}", Sha256::digest(source)) != op.source_job_sha256 {
                return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
            }
            result.push(op);
        }
        Ok(result)
    }
    /// Fixed pinned native runner only. No credential or script path is a renderer input.
    #[cfg(target_os = "macos")]
    pub fn restore_hosted_remote(
        &self,
        source_job_id: &str,
        target: Target,
        image: &str,
        operation_id: Option<&str>,
        node: &Path,
        runner: &Path,
        make_input: impl FnOnce(&Operation, &serde_json::Value) -> Result<Vec<u8>>,
        check: impl Fn() -> Result<()>,
    ) -> Result<Operation> {
        let _lock = self.lock()?;
        if operation_id.is_none() && target.create_requested && target.postgres_id.is_some() {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let original = self.completed_hosted_backup(source_job_id)?;
        if !valid_target(&target)
            || target.workspace_id != original.source.workspace_id
            || target.postgres_id.as_deref() == Some(original.source.postgres_id.as_str())
        {
            return Err(err("HOSTED_RESOURCE_CONFLICT"));
        }
        for j in self.hosted_backup_jobs()? {
            if self.hosted_helper_state(&j)? == "running" {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        for j in self.hosted_restore_checks()? {
            if ["running", "pending"].contains(&self.hosted_restore_helper_state(&j)?.as_str()) {
                return Err(err("HOSTED_RECOVERY_HELPER_ACTIVE"));
            }
        }
        let original_bytes = installation_backup::source_bytes(
            &self.root.join(format!("hosted-backup-{source_job_id}")),
            "job.json",
            16384,
            true,
        )?;
        let receipt = original
            .receipt
            .as_ref()
            .ok_or_else(|| err("HOSTED_RECOVERY_SOURCE_REQUIRED"))?;
        if operation_id.is_none()
            && self.hosted_remote_restores()?.iter().any(|old| {
                target.postgres_id.is_some() && old.target.postgres_id == target.postgres_id
            })
        {
            return Err(err("HOSTED_REMOTE_RECONCILIATION_REQUIRED"));
        }
        let id = operation_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if !hosted_backup::uuid(&id) {
            return Err(err("HOSTED_RECOVERY_REQUEST"));
        }
        let folder = self.root.join(format!("hosted-remote-restoration-{id}"));
        let mut op = if operation_id.is_some() {
            let bytes = installation_backup::source_bytes(&folder, "operation.json", 16384, true)?;
            let prior: Operation =
                serde_json::from_slice(&bytes).map_err(|_| err("HOSTED_RECOVERY_REQUEST"))?;
            validate(&prior, &original, &id)?;
            if prior.target.workspace_id != target.workspace_id
                || (prior.target.postgres_id != target.postgres_id
                    && !(target.create_requested && target.postgres_id.is_none()))
                || prior.target.create_requested != target.create_requested
                || prior.source_job_id != source_job_id
            {
                return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
            }
            // Once dispatched, interruption can mean partial remote SQL. Never replay import.
            if prior.state != "prepared" {
                return Err(err("HOSTED_REMOTE_RECONCILIATION_REQUIRED"));
            }
            prior
        } else {
            let _folder = restoration::directory(&folder)?;
            let value = Operation {
                id: id.clone(),
                source_job_id: source_job_id.into(),
                source: original.source.clone(),
                target,
                expected_backup_id: receipt.backup_id.clone(),
                expected_manifest_sha256: receipt.authenticated_manifest_sha256.clone(),
                expected_files: receipt.files,
                source_job_sha256: format!("{:x}", Sha256::digest(&original_bytes)),
                target_identity_sha256: None,
                state: "prepared".into(),
                error_code: None,
            };
            write_private_new(
                &folder,
                "operation.json",
                &serde_json::to_vec(&value).map_err(|_| err("STATE_INVALID"))?,
                16384,
            )?;
            value
        };
        if format!("{:x}", Sha256::digest(&original_bytes)) != op.source_job_sha256 {
            return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
        }
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        let budget = hosted_backup::job_budget(
            fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?,
        )?;
        if self.validate_hosted_archive(source_job_id, budget)? != receipt.files {
            return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
        }
        let mut docker_command = std::process::Command::new("docker");
        local_network::pin_command(&mut docker_command)?;
        let endpoint = docker_command
            .get_envs()
            .find(|(k, _)| *k == "DOCKER_HOST")
            .and_then(|(_, v)| v)
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?;
        let docker = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
        let execution = serde_json::json!({"image":image,"sourcePath":self.root.join(format!("hosted-backup-{source_job_id}/output/archive")),"dockerPath":docker,"dockerEndpoint":endpoint,"workVolume":format!("exhibitos-hosted-remote-work-{id}"),"jobBudgetBytes":budget.min(32*1024*1024*1024)});
        let input = make_input(&op, &execution)?;
        if input.len() > 131072 {
            return Err(err("ENGINE_INPUT_LIMIT"));
        }
        check()?;
        op.state = "dispatched".into();
        save(&folder, &op)?;
        let outcome = run_observed_stream_environment(
            node.to_str()
                .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?,
            &[
                "--disable-proto=throw".into(),
                "--no-warnings".into(),
                runner.to_string_lossy().into_owned(),
            ],
            None,
            7200,
            ObservedInputs {
                files: (None, None),
                streamed: Some((Box::new(SecretInput(std::io::Cursor::new(input))), 131072)),
            },
            true,
            || check(),
        );
        match outcome {
            Ok(bytes) => {
                check()?;
                let v: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|_| err("HOSTED_RECOVERY_RESULT_INVALID"))?;
                if v.as_object().is_none_or(|fields| fields.len() != 9)
                    || v["serviceConnected"] != false
                    || v["operationId"] != op.id
                    || v["sourceJobId"] != op.source_job_id
                    || v["backupId"] != op.expected_backup_id
                    || v["authenticatedManifestSha256"] != op.expected_manifest_sha256
                    || v["files"] != op.expected_files
                    || v["targetPostgresId"].as_str().is_none_or(|id| {
                        id.is_empty()
                            || id.len() > 128
                            || !id
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                            || id == op.source.postgres_id
                            || op
                                .target
                                .postgres_id
                                .as_deref()
                                .is_some_and(|expected| id != expected)
                    })
                    || v["state"] != "verified"
                {
                    return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
                }
                op.target_identity_sha256 = v["targetIdentitySha256"]
                    .as_str()
                    .filter(|v| hash_valid(v))
                    .map(str::to_owned);
                if op.target_identity_sha256.is_none() {
                    return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
                }
                op.target.postgres_id = v["targetPostgresId"].as_str().map(str::to_owned);
                op.state = "verified".into();
                op.error_code = None;
            }
            Err(_) => {
                // Only a fixed-runner checkpoint published before helper launch permits a
                // retry of this exact target. Missing/partial checkpoints remain uncertain.
                let checkpoint =
                    installation_backup::source_bytes(&folder, "runner-state.json", 16384, true)
                        .ok()
                        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
                if let Some(id) = checkpoint
                    .as_ref()
                    .filter(|v| v["operationId"] == op.id && v["sourceJobId"] == op.source_job_id)
                    .and_then(|v| v["targetPostgresId"].as_str())
                {
                    if id != op.source.postgres_id
                        && id.len() <= 128
                        && id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                        && op.target.postgres_id.as_deref().is_none_or(|old| old == id)
                    {
                        op.target.postgres_id = Some(id.into());
                    }
                }
                let before_import = checkpoint.as_ref().is_some_and(|v| {
                    v["operationId"] == op.id
                        && v["sourceJobId"] == op.source_job_id
                        && v["phase"] == "admitting"
                });
                op.state = if before_import {
                    "prepared"
                } else {
                    "recovery-required"
                }
                .into();
                op.error_code = Some(
                    if before_import {
                        checkpoint.as_ref().and_then(|v|v["errorCode"].as_str()).filter(|code|["HOSTED_RESOURCE_CONFLICT","HOSTED_RESULT_UNCERTAIN","HOSTED_RECOVERY_TARGET_NOT_READY","HOSTED_RECOVERY_NETWORK_REQUIRED","HOSTED_RECOVERY_CREATION_UNPROVEN","HOSTED_RECOVERY_DATABASE_EXPIRED","HOSTED_RECOVERY_WRITERS_ACTIVE","HOSTED_RECOVERY_CONFIGURATION_CHANGED","HOSTED_TOKEN_INVALID","HOSTED_PERMISSION_REQUIRED","HOSTED_CONNECTION_FAILED"].contains(code)).unwrap_or("HOSTED_RECOVERY_EXECUTION_FAILED")
                    } else {
                        "HOSTED_REMOTE_RECONCILIATION_REQUIRED"
                    }
                    .into(),
                );
            }
        }
        save(&folder, &op)?;
        Ok(op)
    }
    /// Explicit connection step only after the original archive was restored and
    /// the real target identity was recorded. Credentials stay in native input.
    #[cfg(target_os = "macos")]
    pub fn connect_hosted_remote(
        &self,
        id: &str,
        image: &str,
        node: &Path,
        runner: &Path,
        make_input: impl FnOnce(&Operation, &serde_json::Value) -> Result<Vec<u8>>,
        check: impl Fn() -> Result<()>,
    ) -> Result<Operation> {
        let _lock = self.lock()?;
        let mut op = self
            .hosted_remote_restores()?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| err("HOSTED_RECOVERY_REQUEST"))?;
        if op.state == "connected" {
            return Ok(op);
        }
        if ![
            "verified",
            "connecting",
            "connection-restored",
            "connection-recovery-required",
        ]
        .contains(&op.state.as_str())
            || op
                .target_identity_sha256
                .as_ref()
                .is_none_or(|v| !hash_valid(v))
            || !image.strip_prefix("sha256:").is_some_and(hash_valid)
        {
            return Err(err("HOSTED_REMOTE_RECONCILIATION_REQUIRED"));
        }
        let original = self.completed_hosted_backup(&op.source_job_id)?;
        let receipt = original
            .receipt
            .as_ref()
            .ok_or_else(|| err("HOSTED_RECOVERY_SOURCE_REQUIRED"))?;
        let folder = self.root.join(format!("hosted-remote-restoration-{id}"));
        let original_bytes = installation_backup::source_bytes(
            &self
                .root
                .join(format!("hosted-backup-{}", op.source_job_id)),
            "job.json",
            16384,
            true,
        )?;
        if format!("{:x}", Sha256::digest(&original_bytes)) != op.source_job_sha256 {
            return Err(err("HOSTED_RECOVERY_SOURCE_CHANGED"));
        }
        let budget = hosted_backup::job_budget(
            fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?,
        )?;
        if self.validate_hosted_archive(&op.source_job_id, budget)? != receipt.files {
            return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
        }
        let mut command = std::process::Command::new("docker");
        local_network::pin_command(&mut command)?;
        let endpoint = command
            .get_envs()
            .find(|(k, _)| *k == "DOCKER_HOST")
            .and_then(|(_, v)| v)
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?;
        let docker = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
        let execution = serde_json::json!({"image":image,"sourcePath":self.root.join(format!("hosted-backup-{}/output/archive",op.source_job_id)),"dockerPath":docker,"dockerEndpoint":endpoint});
        let input = make_input(&op, &execution)?;
        if input.len() > 131072 {
            return Err(err("ENGINE_INPUT_LIMIT"));
        }
        check()?;
        op.state = "connecting".into();
        save(&folder, &op)?;
        let result = run_observed_stream_environment(
            node.to_str()
                .ok_or_else(|| err("HOSTED_RUNTIME_UNAVAILABLE"))?,
            &[
                "--disable-proto=throw".into(),
                "--no-warnings".into(),
                runner.to_string_lossy().into_owned(),
            ],
            None,
            4200,
            ObservedInputs {
                files: (None, None),
                streamed: Some((Box::new(SecretInput(std::io::Cursor::new(input))), 131072)),
            },
            true,
            || check(),
        );
        let outcome = result
            .ok()
            .filter(|b| b.len() <= 16384)
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let state = outcome
            .as_ref()
            .filter(|v| {
                v["operationId"] == op.id
                    && v["sourceJobId"] == op.source_job_id
                    && v["targetPostgresId"].as_str() == op.target.postgres_id.as_deref()
            })
            .and_then(|v| v["state"].as_str())
            .filter(|s| {
                [
                    "connected",
                    "connection-restored",
                    "connection-recovery-required",
                ]
                .contains(s)
            })
            .unwrap_or("connection-recovery-required");
        op.state = state.into();
        op.error_code = if state == "connected" {
            None
        } else {
            Some("HOSTED_REMOTE_CONNECTION_RECOVERY_REQUIRED".into())
        };
        check()?;
        save(&folder, &op)?;
        Ok(op)
    }
}
fn validate(op: &Operation, job: &Job, id: &str) -> Result<()> {
    let receipt = job
        .receipt
        .as_ref()
        .ok_or_else(|| err("HOSTED_RECOVERY_SOURCE_REQUIRED"))?;
    if op.id != id
        || !hosted_backup::uuid(&op.id)
        || op.source_job_id != job.id
        || op.source != job.source
        || !valid_target(&op.target)
        || op.target.workspace_id != job.source.workspace_id
        || op.target.postgres_id.as_deref() == Some(job.source.postgres_id.as_str())
        || op.expected_backup_id != receipt.backup_id
        || op.expected_manifest_sha256 != receipt.authenticated_manifest_sha256
        || op.expected_files != receipt.files
        || !hash_valid(&op.source_job_sha256)
        || ![
            "prepared",
            "dispatched",
            "verified",
            "recovery-required",
            "connecting",
            "connected",
            "connection-restored",
            "connection-recovery-required",
        ]
        .contains(&op.state.as_str())
    {
        return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
    }
    if [
        "verified",
        "connecting",
        "connected",
        "connection-restored",
        "connection-recovery-required",
    ]
    .contains(&op.state.as_str())
        && (op.target.postgres_id.is_none()
            || op
                .target_identity_sha256
                .as_ref()
                .is_none_or(|v| !hash_valid(v)))
    {
        return Err(err("HOSTED_RECOVERY_RESULT_INVALID"));
    }
    Ok(())
}
fn save(folder: &Path, op: &Operation) -> Result<()> {
    write_json(folder, "operation.json", op)
}
