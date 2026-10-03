// SPDX-License-Identifier: Apache-2.0
//! Process-crash diagnosis using private preparation/reservation evidence; no engine mutations.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preparation {
    format_version: u8,
    retry_id: String,
    target_id: String,
    kind: String,
    original_job_sha256: String,
    destination_root_sha256: String,
    backup_workspace_ids: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reservation {
    format_version: u8,
    retry_id: String,
    job_id: String,
    preparation_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetryDiagnosis {
    pub retry_id: String,
    pub target_id: String,
    pub kind: String,
    pub outcome: String,
    pub new_job_id: Option<String>,
    pub child_job_sha256: Option<String>,
    pub can_reconcile: bool,
    pub data_preserved: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryRecoveryReceipt {
    pub diagnosis_id: String,
    pub retry_id: String,
    pub outcome: String,
    pub new_job_id: Option<String>,
    pub data_preserved: bool,
}
fn immutable(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.len() > 128 * 1024 {
        return Err(err("JOB_HISTORY_FULL"));
    }
    let mut file = private_options()
        .open(root.join(name))
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    #[cfg(unix)]
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    Ok(())
}
fn optional(root: &Path, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(_) => source_bytes(root, name, limit, true).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(err("STATE_UNAVAILABLE")),
    }
}
fn empty_workspace(root: &Path, kind: &str, id: &str) -> Result<bool> {
    let path = root.join(if kind == "backup" {
        format!("backup-creation-{id}")
    } else {
        format!("restore-{id}")
    });
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(err("STATE_UNAVAILABLE")),
        Ok(_) => {
            maintenance::input_path(&path, true)?;
            Ok(fs::read_dir(path)
                .map_err(|_| err("STATE_UNAVAILABLE"))?
                .next()
                .is_none())
        }
    }
}
fn backup_inventory(
    service: &LifecycleService,
    empty_reserved: Option<&str>,
) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for item in fs::read_dir(&service.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
        let item = item.map_err(|_| err("STATE_UNAVAILABLE"))?;
        let name = item.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_prefix("backup-creation-"))
        else {
            continue;
        };
        if !uuid(id) || ids.len() >= 1000 {
            return Err(err("STATE_INVALID"));
        }
        maintenance::input_path(&item.path(), true)?;
        if empty_reserved == Some(id) && empty_workspace(&service.root, "backup", id)? {
            continue;
        }
        if empty_workspace(&service.root, "backup", id)?
            && reserved_empty_backup_workspace(service, id)?
        {
            ids.push(id.to_string());
            continue;
        }
        let bytes = source_bytes(
            &service.root,
            &format!("backup-creation-{id}/job.json"),
            16 * 1024,
            true,
        )?;
        let job: backup_creation::BackupCreationJob =
            serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
        if job.id != id
            || job.operation != "create"
            || !["running", "completed", "failed", "interrupted"].contains(&job.state.as_str())
        {
            return Err(err("STATE_INVALID"));
        }
        ids.push(id.to_string());
    }
    ids.sort();
    Ok(ids)
}
pub(super) fn prepare(service: &LifecycleService, record: &MaintenanceRetry) -> Result<()> {
    let v = Preparation {
        format_version: 1,
        retry_id: record.id.clone(),
        target_id: record.target_id.clone(),
        kind: record.kind.clone(),
        original_job_sha256: record.original_job_sha256.clone(),
        destination_root_sha256: record.destination_root_sha256.clone(),
        backup_workspace_ids: backup_inventory(service, None)?,
    };
    immutable(
        &service.root,
        &format!("retry-preparation-{}.json", record.id),
        &serde_json::to_vec(&v).map_err(|_| err("STATE_INVALID"))?,
    )
}
fn preparation(
    service: &LifecycleService,
    record: &MaintenanceRetry,
) -> Result<Option<(Preparation, Vec<u8>)>> {
    let Some(bytes) = optional(
        &service.root,
        &format!("retry-preparation-{}.json", record.id),
        128 * 1024,
    )?
    else {
        return Ok(None);
    };
    let v: Preparation = serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
    if v.format_version != 1
        || v.retry_id != record.id
        || v.target_id != record.target_id
        || v.kind != record.kind
        || v.original_job_sha256 != record.original_job_sha256
        || v.destination_root_sha256 != record.destination_root_sha256
        || v.backup_workspace_ids.len() > 1000
        || v.backup_workspace_ids.iter().any(|s| !uuid(s))
        || v.backup_workspace_ids.windows(2).any(|v| v[0] >= v[1])
    {
        return Err(err("STATE_INVALID"));
    }
    Ok(Some((v, bytes)))
}
pub(super) fn reserve(
    service: &LifecycleService,
    record: &MaintenanceRetry,
    job: &str,
) -> Result<()> {
    if !uuid(job)
        || job == record.id
        || job == record.target_id
        || record.state != "preparing"
        || record.new_job_id.is_some()
    {
        return Err(err("STATE_INVALID"));
    }
    let (p, bytes) = preparation(service, record)?.ok_or_else(|| err("RETRY_PROOF_MISSING"))?;
    if p.backup_workspace_ids.iter().any(|id| id == job) {
        return Err(err("STATE_INVALID"));
    }
    let v = Reservation {
        format_version: 1,
        retry_id: record.id.clone(),
        job_id: job.into(),
        preparation_sha256: digest(&bytes),
    };
    immutable(
        &service.root,
        &format!("retry-child-intent-{}.json", record.id),
        &serde_json::to_vec(&v).map_err(|_| err("STATE_INVALID"))?,
    )
}
fn evaluate(
    source: &LifecycleService,
    destination: &LifecycleService,
    record: &MaintenanceRetry,
) -> Result<RetryDiagnosis> {
    let mut result = RetryDiagnosis {
        retry_id: record.id.clone(),
        target_id: record.target_id.clone(),
        kind: record.kind.clone(),
        outcome: "unproven".into(),
        new_job_id: record.new_job_id.clone(),
        child_job_sha256: None,
        can_reconcile: false,
        data_preserved: true,
    };
    if digest(&source.retry_target_bytes(&record.kind, &record.target_id)?)
        != record.original_job_sha256
    {
        return Err(err("RETRY_SOURCE_CHANGED"));
    }
    let Some((p, preparation_bytes)) = preparation(source, record)? else {
        return Ok(result);
    };
    let reservation = optional(
        &source.root,
        &format!("retry-child-intent-{}.json", record.id),
        8192,
    )?;
    let reserved = if let Some(bytes) = reservation {
        let v: Reservation = serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
        if v.format_version != 1
            || v.retry_id != record.id
            || !uuid(&v.job_id)
            || v.job_id == record.id
            || v.job_id == record.target_id
            || v.preparation_sha256 != digest(&preparation_bytes)
            || p.backup_workspace_ids.contains(&v.job_id)
            || record.new_job_id.as_ref().is_some_and(|id| id != &v.job_id)
        {
            return Err(err("STATE_INVALID"));
        }
        Some(v.job_id)
    } else {
        None
    };
    if record.new_job_id.is_some() && reserved.is_none() {
        return Ok(result);
    }
    if let Some(id) = reserved.as_deref() {
        result.new_job_id = Some(id.into());
        let relative = if record.kind == "backup" {
            format!("backup-creation-{id}/job.json")
        } else {
            "restoration.json".into()
        };
        if let Some(bytes) = optional(&destination.root, &relative, 16 * 1024)? {
            let (job_id, state, stage, error_code, created_at, updated_at) =
                if record.kind == "backup" {
                    let j: backup_creation::BackupCreationJob =
                        serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
                    if j.operation != "create" {
                        return Err(err("STATE_INVALID"));
                    }
                    (
                        j.id,
                        j.state,
                        j.stage,
                        j.error_code,
                        j.created_at,
                        j.updated_at,
                    )
                } else {
                    let j: restoration::RestorationJob =
                        serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
                    (
                        j.id,
                        j.state,
                        j.stage,
                        j.error_code,
                        j.created_at,
                        j.updated_at,
                    )
                };
            if job_id != id
                || !["running", "completed", "failed", "interrupted"].contains(&state.as_str())
            {
                return Err(err("RETRY_CANDIDATE_CONFLICT"));
            }
            if stage.is_empty()
                || stage.len() > 64
                || created_at > updated_at
                || updated_at > 8_640_000_000_000_000
                || (["running", "completed"].contains(&state.as_str()) != error_code.is_none())
                || error_code.as_ref().is_some_and(|v| {
                    v.is_empty()
                        || v.len() > 128
                        || !v
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                })
            {
                return Err(err("STATE_INVALID"));
            }
            let workspace = destination.root.join(if record.kind == "backup" {
                format!("backup-creation-{id}")
            } else {
                format!("restore-{id}")
            });
            maintenance::input_path(&workspace, true)?;
            result.child_job_sha256 = Some(digest(&bytes));
            result.outcome = if record.new_job_id.is_some() {
                "already-linked"
            } else {
                "child-found"
            }
            .into();
            result.can_reconcile = record.new_job_id.is_none()
                && ["preparing", "interrupted", "failed"].contains(&record.state.as_str());
            return Ok(result);
        }
        if record.new_job_id.is_some() {
            result.outcome = "child-journal-missing".into();
            return Ok(result);
        }
        if !empty_workspace(&destination.root, &record.kind, id)? {
            result.outcome = "candidate-incomplete".into();
            return Ok(result);
        }
    }
    if backup_inventory(
        source,
        if record.kind == "backup" {
            reserved.as_deref()
        } else {
            None
        },
    )? != p.backup_workspace_ids
    {
        result.outcome = "inventory-changed".into();
        return Ok(result);
    }
    if record.kind == "restoration" {
        for item in fs::read_dir(&destination.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let name = item.map_err(|_| err("STATE_UNAVAILABLE"))?.file_name();
            if name != "operation.lock"
                && !reserved
                    .as_ref()
                    .is_some_and(|id| name == format!("restore-{id}").as_str())
            {
                result.outcome = "destination-changed".into();
                return Ok(result);
            }
        }
    }
    result.outcome = "no-child-created".into();
    result.can_reconcile = ["preparing", "interrupted", "failed"].contains(&record.state.as_str())
        && record.new_job_id.is_none();
    Ok(result)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Clearance {
    format_version: u8,
    retry_id: String,
    diagnosis_id: String,
    audit_sha256: String,
    proof: RetryDiagnosis,
}
pub(super) fn has_reservation(
    source: &LifecycleService,
    record: &MaintenanceRetry,
) -> Result<bool> {
    optional(
        &source.root,
        &format!("retry-child-intent-{}.json", record.id),
        8192,
    )
    .map(|v| v.is_some())
}
pub(super) fn assert_reserved(
    source: &LifecycleService,
    record: &MaintenanceRetry,
    child: &str,
) -> Result<()> {
    let (p, bytes) = preparation(source, record)?.ok_or_else(|| err("RETRY_PROOF_MISSING"))?;
    let reservation = optional(
        &source.root,
        &format!("retry-child-intent-{}.json", record.id),
        8192,
    )?
    .ok_or_else(|| err("RETRY_PROOF_MISSING"))?;
    let v: Reservation = serde_json::from_slice(&reservation).map_err(|_| err("STATE_INVALID"))?;
    if v.format_version != 1
        || v.retry_id != record.id
        || !uuid(child)
        || v.job_id != child
        || child == record.id
        || child == record.target_id
        || v.preparation_sha256 != digest(&bytes)
        || p.backup_workspace_ids.iter().any(|id| id == child)
    {
        return Err(err("STATE_INVALID"));
    }
    Ok(())
}
pub(super) fn cleared(source: &LifecycleService, record: &MaintenanceRetry) -> Result<bool> {
    let prefix = format!("retry-clearance-{}-", record.id);
    let hash = digest(&source_bytes(
        &source.root,
        &format!("maintenance-retry-{}.json", record.id),
        8192,
        true,
    )?);
    let mut count = 0;
    for item in fs::read_dir(&source.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
        let name = item.map_err(|_| err("STATE_UNAVAILABLE"))?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name
            .strip_prefix(&prefix)
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        count += 1;
        if !uuid(id) || count > 1000 {
            return Err(err("STATE_INVALID"));
        }
        let v: Clearance =
            serde_json::from_slice(&source_bytes(&source.root, name, 32 * 1024, true)?)
                .map_err(|_| err("STATE_INVALID"))?;
        if v.format_version != 1
            || v.retry_id != record.id
            || v.diagnosis_id != id
            || !hash_valid(&v.audit_sha256)
            || v.proof.retry_id != record.id
            || v.proof.target_id != record.target_id
            || v.proof.kind != record.kind
            || v.proof.outcome != "no-child-created"
            || v.proof.child_job_sha256.is_some()
            || !v.proof.can_reconcile
            || !v.proof.data_preserved
            || v.proof
                .new_job_id
                .as_ref()
                .is_some_and(|id| !uuid(id) || id == &record.id || id == &record.target_id)
        {
            return Err(err("STATE_INVALID"));
        }
        if v.audit_sha256 == hash {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn reserved_empty_backup_workspace(
    source: &LifecycleService,
    child: &str,
) -> Result<bool> {
    if !uuid(child) || !empty_workspace(&source.root, "backup", child)? {
        return Ok(false);
    }
    let mut count = 0;
    for item in fs::read_dir(&source.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
        let name = item.map_err(|_| err("STATE_UNAVAILABLE"))?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name
            .strip_prefix("retry-child-intent-")
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        count += 1;
        if !uuid(id) || count > 1000 {
            return Err(err("STATE_INVALID"));
        }
        let bytes = source_bytes(&source.root, name, 8192, true)?;
        let intent: Reservation =
            serde_json::from_slice(&bytes).map_err(|_| err("STATE_INVALID"))?;
        if intent.job_id != child {
            continue;
        }
        let record = source.retry_record(id)?;
        if record.kind != "backup"
            || record.state != "failed"
            || record.new_job_id.is_some()
            || record.error_code.as_deref() != Some("RETRY_NOT_STARTED")
            || record.destination_root_sha256 != digest(source.root.to_string_lossy().as_bytes())
        {
            return Ok(false);
        }
        assert_reserved(source, &record, child)?;
        if digest(&source.retry_target_bytes("backup", &record.target_id)?)
            != record.original_job_sha256
        {
            return Err(err("RETRY_SOURCE_CHANGED"));
        }
        return cleared(source, &record);
    }
    Ok(false)
}
impl LifecycleService {
    /// Existing private root only; bypasses normal job recovery so incomplete candidates can be diagnosed.
    pub fn open_retry_diagnostics(root: PathBuf) -> Result<Self> {
        maintenance::input_path(&root, true)?;
        Ok(Self { root })
    }
    pub fn retry_diagnostic_history(&self) -> Result<Vec<MaintenanceRetry>> {
        maintenance::input_path(&self.root, true)?;
        let _lock = self.lock()?;
        self.retry_history()
    }
    fn with_retry_diagnosis<T>(
        &self,
        retry: &str,
        destination: Option<&Path>,
        task: impl FnOnce(&MaintenanceRetry, &LifecycleService) -> Result<T>,
    ) -> Result<T> {
        if cfg!(windows) {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        maintenance::input_path(&self.root, true)?;
        let _source = self.lock()?;
        let record = self.retry_record(retry)?;
        if !["preparing", "running", "failed", "interrupted"].contains(&record.state.as_str()) {
            return Err(err("RETRY_DIAGNOSIS_TARGET_INVALID"));
        }
        if record.kind == "backup" {
            if destination.is_some()
                || record.destination_root_sha256 != digest(self.root.to_string_lossy().as_bytes())
            {
                return Err(err("RETRY_DESTINATION_INVALID"));
            }
            return task(&record, self);
        }
        let path = destination.ok_or_else(|| err("RETRY_DESTINATION_INVALID"))?;
        maintenance::input_path(path, true)?;
        if path.starts_with(&self.root)
            || self.root.starts_with(path)
            || record.destination_root_sha256 != digest(path.to_string_lossy().as_bytes())
        {
            return Err(err("RETRY_DESTINATION_INVALID"));
        }
        let service = LifecycleService {
            root: path.to_path_buf(),
        };
        let _destination = service.lock()?;
        task(&record, &service)
    }
    /// Read typed private evidence under both locks. Does not stop helpers, relabel jobs or change the retry.
    pub fn diagnose_maintenance_retry(
        &self,
        retry: &str,
        destination: Option<&Path>,
    ) -> Result<RetryDiagnosis> {
        self.with_retry_diagnosis(retry, destination, |record, destination| {
            evaluate(self, destination, record)
        })
    }
    /// Reconcile only a proven orphan child link or proven pre-child interruption, preserving the before image.
    pub fn reconcile_maintenance_retry(
        &self,
        retry: &str,
        destination: Option<&Path>,
        preserve_candidates: bool,
    ) -> Result<RetryRecoveryReceipt> {
        if !preserve_candidates {
            return Err(err("RETRY_ACK_REQUIRED"));
        }
        self.with_retry_diagnosis(retry, destination, |record, destination| {
            let proof = evaluate(self, destination, record)?;
            if !proof.can_reconcile {
                return Err(err("RETRY_RECOVERY_UNPROVEN"));
            }
            let diagnosis = Uuid::new_v4().to_string();
            let mut next = record.clone();
            if proof.outcome == "child-found" {
                next.new_job_id = proof.new_job_id.clone();
                next.state = "interrupted".into();
                next.error_code = Some("INTERRUPTED".into());
            } else if proof.outcome == "no-child-created" {
                next.state = "failed".into();
                next.error_code = Some("RETRY_NOT_STARTED".into());
            } else {
                return Err(err("RETRY_RECOVERY_UNPROVEN"));
            }
            next.updated_at = now().max(next.updated_at);
            immutable(
                &self.root,
                &format!("retry-diagnosis-{diagnosis}-before.json"),
                &source_bytes(
                    &self.root,
                    &format!("maintenance-retry-{}.json", record.id),
                    8192,
                    true,
                )?,
            )?;
            immutable(
                &self.root,
                &format!("retry-diagnosis-{diagnosis}.json"),
                &serde_json::to_vec(&proof).map_err(|_| err("STATE_INVALID"))?,
            )?;
            if proof.outcome == "no-child-created" {
                let clearance = Clearance {
                    format_version: 1,
                    retry_id: record.id.clone(),
                    diagnosis_id: diagnosis.clone(),
                    audit_sha256: digest(
                        &serde_json::to_vec(&next).map_err(|_| err("STATE_INVALID"))?,
                    ),
                    proof: proof.clone(),
                };
                immutable(
                    &self.root,
                    &format!("retry-clearance-{}-{diagnosis}.json", record.id),
                    &serde_json::to_vec(&clearance).map_err(|_| err("STATE_INVALID"))?,
                )
                .map_err(|_| err("RETRY_RECOVERY_UNCERTAIN"))?;
            }
            // The interrupted fence remains until proof is durably published; older readers also fail closed.
            self.save_retry(&next)?;
            Ok(RetryRecoveryReceipt {
                diagnosis_id: diagnosis,
                retry_id: record.id.clone(),
                outcome: proof.outcome,
                new_job_id: next.new_job_id,
                data_preserved: true,
            })
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> LifecycleService {
        LifecycleService::new(
            fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("retry-recovery-proof-{}", Uuid::new_v4())),
        )
        .unwrap()
    }
    fn original(source: &LifecycleService, kind: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let at = now();
        if kind == "restoration" {
            write_json(
                &source.root,
                "restoration.json",
                &restoration::RestorationJob {
                    id: id.clone(),
                    state: "failed".into(),
                    stage: "authenticating".into(),
                    error_code: Some("CANCELLED".into()),
                    created_at: at,
                    updated_at: at,
                },
            )
            .unwrap();
        } else {
            let path = source.root.join(format!("backup-creation-{id}"));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            write_json(
                &path,
                "job.json",
                &backup_creation::BackupCreationJob {
                    id: id.clone(),
                    operation: "create".into(),
                    state: "failed".into(),
                    stage: "preflight".into(),
                    error_code: Some("CANCELLED".into()),
                    created_at: at,
                    updated_at: at,
                },
            )
            .unwrap();
        }
        id
    }
    fn pending(
        source: &LifecycleService,
        destination: &LifecycleService,
        kind: &str,
    ) -> MaintenanceRetry {
        let id = original(source, kind);
        let v = source.begin_retry(kind, &id, &destination.root).unwrap();
        source.maintenance_retries().unwrap();
        source.retry_record(&v.id).unwrap()
    }
    fn child(destination: &LifecycleService, kind: &str, id: &str) -> PathBuf {
        let workspace = destination.root.join(if kind == "backup" {
            format!("backup-creation-{id}")
        } else {
            format!("restore-{id}")
        });
        fs::create_dir(&workspace).unwrap();
        fs::set_permissions(&workspace, fs::Permissions::from_mode(0o700)).unwrap();
        let at = now();
        if kind == "backup" {
            write_json(
                &workspace,
                "job.json",
                &backup_creation::BackupCreationJob {
                    id: id.into(),
                    operation: "create".into(),
                    state: "running".into(),
                    stage: "preflight".into(),
                    error_code: None,
                    created_at: at,
                    updated_at: at,
                },
            )
            .unwrap();
        } else {
            write_json(
                &destination.root,
                "restoration.json",
                &restoration::RestorationJob {
                    id: id.into(),
                    state: "running".into(),
                    stage: "authenticating".into(),
                    error_code: None,
                    created_at: at,
                    updated_at: at,
                },
            )
            .unwrap();
        }
        immutable(
            &workspace,
            "candidate-witness",
            b"synthetic candidate: preserve these bytes",
        )
        .unwrap();
        workspace
    }
    fn reserve_interrupted(source: &LifecycleService, v: &MaintenanceRetry, id: &str) {
        let mut preparing = v.clone();
        preparing.state = "preparing".into();
        preparing.error_code = None;
        reserve(source, &preparing, id).unwrap();
    }
    #[test]
    fn pre_child_crash_diagnosis_is_read_only_then_reconciles_without_touching_original() {
        for kind in ["backup", "restoration"] {
            let source = fixture();
            let other = fixture();
            let destination = if kind == "backup" { &source } else { &other };
            let v = pending(&source, destination, kind);
            let root = if kind == "backup" {
                None
            } else {
                Some(destination.root.as_path())
            };
            let original = source.retry_target_bytes(kind, &v.target_id).unwrap();
            let before = source_bytes(
                &source.root,
                &format!("maintenance-retry-{}.json", v.id),
                8192,
                true,
            )
            .unwrap();
            let proof = source.diagnose_maintenance_retry(&v.id, root).unwrap();
            assert_eq!(proof.outcome, "no-child-created");
            assert!(proof.can_reconcile);
            assert_eq!(
                source_bytes(
                    &source.root,
                    &format!("maintenance-retry-{}.json", v.id),
                    8192,
                    true
                )
                .unwrap(),
                before
            );
            assert_eq!(
                source
                    .reconcile_maintenance_retry(&v.id, root, false)
                    .unwrap_err()
                    .code,
                "RETRY_ACK_REQUIRED"
            );
            let receipt = source
                .reconcile_maintenance_retry(&v.id, root, true)
                .unwrap();
            assert!(receipt.data_preserved);
            assert!(receipt.new_job_id.is_none());
            assert_eq!(
                source_bytes(
                    &source.root,
                    &format!("retry-diagnosis-{}-before.json", receipt.diagnosis_id),
                    8192,
                    true
                )
                .unwrap(),
                before
            );
            assert_eq!(
                source.retry_target_bytes(kind, &v.target_id).unwrap(),
                original
            );
            assert!(
                source
                    .begin_retry(kind, &v.target_id, &fixture().root)
                    .is_ok()
            );
        }
    }
    #[test]
    fn reserved_orphan_journal_links_exact_child_and_keeps_candidate_and_parent_fence() {
        for kind in ["backup", "restoration"] {
            let source = fixture();
            let other = fixture();
            let destination = if kind == "backup" { &source } else { &other };
            let v = pending(&source, destination, kind);
            let id = Uuid::new_v4().to_string();
            reserve_interrupted(&source, &v, &id);
            let workspace = child(destination, kind, &id);
            let path = if kind == "backup" {
                workspace.join("job.json")
            } else {
                destination.root.join("restoration.json")
            };
            let original = source.retry_target_bytes(kind, &v.target_id).unwrap();
            let bytes = fs::read(&path).unwrap();
            let witness = fs::read(workspace.join("candidate-witness")).unwrap();
            let root = if kind == "backup" {
                None
            } else {
                Some(destination.root.as_path())
            };
            let proof = source.diagnose_maintenance_retry(&v.id, root).unwrap();
            assert_eq!(proof.outcome, "child-found");
            assert_eq!(
                proof.child_job_sha256.as_deref(),
                Some(digest(&bytes).as_str())
            );
            let receipt = source
                .reconcile_maintenance_retry(&v.id, root, true)
                .unwrap();
            assert_eq!(receipt.new_job_id.as_deref(), Some(id.as_str()));
            assert_eq!(source.retry_record(&v.id).unwrap().state, "interrupted");
            assert_eq!(
                source
                    .begin_retry(kind, &v.target_id, &fixture().root)
                    .unwrap_err()
                    .code,
                "RETRY_RECOVERY_REQUIRED"
            );
            assert_eq!(fs::read(path).unwrap(), bytes);
            assert_eq!(
                fs::read(workspace.join("candidate-witness")).unwrap(),
                witness
            );
            assert_eq!(
                source.retry_target_bytes(kind, &v.target_id).unwrap(),
                original
            );
        }
    }
    #[test]
    fn reservation_without_child_needs_matching_clearance_and_repairs_incomplete_metadata() {
        let source = fixture();
        let v = pending(&source, &source, "backup");
        let id = Uuid::new_v4().to_string();
        reserve_interrupted(&source, &v, &id);
        let root = source.root.join(format!("backup-creation-{id}"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let receipt = source
            .reconcile_maintenance_retry(&v.id, None, true)
            .unwrap();
        let clearance = source.root.join(format!(
            "retry-clearance-{}-{}.json",
            v.id, receipt.diagnosis_id
        ));
        let saved = fs::read(&clearance).unwrap();
        // Model an incomplete metadata restore with missing clearance; retained reservation still fences new execution.
        fs::remove_file(&clearance).unwrap();
        assert_eq!(
            source
                .begin_retry("backup", &v.target_id, &source.root)
                .unwrap_err()
                .code,
            "RETRY_RECOVERY_REQUIRED"
        );
        let repaired = source
            .reconcile_maintenance_retry(&v.id, None, true)
            .unwrap();
        assert_ne!(repaired.diagnosis_id, receipt.diagnosis_id);
        assert!(
            source
                .begin_retry("backup", &v.target_id, &source.root)
                .is_ok()
        );
        assert!(root.is_dir());
        assert!(!saved.is_empty());
    }
    #[test]
    fn legacy_and_incomplete_candidates_never_guess_child_or_release_fence() {
        let source = fixture();
        let destination = fixture();
        let id = original(&source, "restoration");
        let at = now();
        let legacy = MaintenanceRetry {
            id: Uuid::new_v4().to_string(),
            target_id: id.clone(),
            kind: "restoration".into(),
            state: "interrupted".into(),
            new_job_id: None,
            original_job_sha256: digest(&source.retry_target_bytes("restoration", &id).unwrap()),
            destination_root_sha256: digest(destination.root.to_string_lossy().as_bytes()),
            error_code: Some("INTERRUPTED".into()),
            created_at: at,
            updated_at: at,
        };
        source.save_retry(&legacy).unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&legacy.id, Some(&destination.root))
                .unwrap()
                .outcome,
            "unproven"
        );
        assert_eq!(
            source
                .reconcile_maintenance_retry(&legacy.id, Some(&destination.root), true)
                .unwrap_err()
                .code,
            "RETRY_RECOVERY_UNPROVEN"
        );
        let source = fixture();
        let destination = fixture();
        let v = pending(&source, &destination, "restoration");
        let child_id = Uuid::new_v4().to_string();
        reserve_interrupted(&source, &v, &child_id);
        let workspace = destination.root.join(format!("restore-{child_id}"));
        fs::create_dir(&workspace).unwrap();
        fs::set_permissions(&workspace, fs::Permissions::from_mode(0o700)).unwrap();
        immutable(&workspace, "candidate-witness", b"retain incomplete data").unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap()
                .outcome,
            "candidate-incomplete"
        );
        assert_eq!(
            source
                .reconcile_maintenance_retry(&v.id, Some(&destination.root), true)
                .unwrap_err()
                .code,
            "RETRY_RECOVERY_UNPROVEN"
        );
        assert_eq!(
            fs::read(workspace.join("candidate-witness")).unwrap(),
            b"retain incomplete data"
        );
    }
    #[test]
    fn source_change_foreign_child_destination_inventory_and_busy_locks_refuse() {
        let source = fixture();
        let destination = fixture();
        let v = pending(&source, &destination, "restoration");
        let lock = source.lock().unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap_err()
                .code,
            "BUSY"
        );
        drop(lock);
        let lock = destination.lock().unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap_err()
                .code,
            "BUSY"
        );
        drop(lock);
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&fixture().root))
                .unwrap_err()
                .code,
            "RETRY_DESTINATION_INVALID"
        );
        let reserved = Uuid::new_v4().to_string();
        reserve_interrupted(&source, &v, &reserved);
        let foreign = Uuid::new_v4().to_string();
        let workspace = child(&destination, "restoration", &foreign);
        let bytes = fs::read(destination.root.join("restoration.json")).unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap_err()
                .code,
            "RETRY_CANDIDATE_CONFLICT"
        );
        assert_eq!(
            fs::read(destination.root.join("restoration.json")).unwrap(),
            bytes
        );
        assert!(workspace.is_dir());
        fs::write(source.root.join("restoration.json"), b"changed original").unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap_err()
                .code,
            "RETRY_SOURCE_CHANGED"
        );
        let source = fixture();
        let v = pending(&source, &source, "backup");
        original(&source, "backup");
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, None)
                .unwrap()
                .outcome,
            "inventory-changed"
        );
    }
    #[test]
    fn symlink_corrupt_reservation_and_unreserved_link_fail_without_rewriting_audit() {
        let source = fixture();
        let destination = fixture();
        let mut v = source
            .begin_retry(
                "restoration",
                &original(&source, "restoration"),
                &destination.root,
            )
            .unwrap();
        let before =
            fs::read(source.root.join(format!("maintenance-retry-{}.json", v.id))).unwrap();
        let id = Uuid::new_v4().to_string();
        assert_eq!(
            RetryLink {
                owner: &source,
                record: &mut v
            }
            .started(&id)
            .unwrap_err()
            .code,
            "RETRY_PROOF_MISSING"
        );
        assert_eq!(
            fs::read(source.root.join(format!("maintenance-retry-{}.json", v.id))).unwrap(),
            before
        );
        source.maintenance_retries().unwrap();
        let path = source
            .root
            .join(format!("retry-child-intent-{}.json", v.id));
        symlink(destination.root.join("absent"), &path).unwrap();
        assert!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .is_err()
        );
        fs::remove_file(&path).unwrap();
        immutable(
            &source.root,
            path.file_name().unwrap().to_str().unwrap(),
            b"{incomplete reservation",
        )
        .unwrap();
        assert_eq!(
            source
                .diagnose_maintenance_retry(&v.id, Some(&destination.root))
                .unwrap_err()
                .code,
            "STATE_INVALID"
        );
    }
    #[test]
    fn contradictory_child_state_and_timestamp_refuse_without_linking_or_rewriting() {
        for kind in ["backup", "restoration"] {
            let source = fixture();
            let other = fixture();
            let destination = if kind == "backup" { &source } else { &other };
            let v = pending(&source, destination, kind);
            let id = Uuid::new_v4().to_string();
            reserve_interrupted(&source, &v, &id);
            let workspace = child(destination, kind, &id);
            let path = if kind == "backup" {
                workspace.join("job.json")
            } else {
                destination.root.join("restoration.json")
            };
            let mut job: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if kind == "backup" {
                job["updatedAt"] = 0.into();
            } else {
                job["errorCode"] = "UNKNOWN_FAILURE".into();
            }
            write_json(
                path.parent().unwrap(),
                path.file_name().unwrap().to_str().unwrap(),
                &job,
            )
            .unwrap();
            let bytes = fs::read(&path).unwrap();
            let audit =
                fs::read(source.root.join(format!("maintenance-retry-{}.json", v.id))).unwrap();
            let root = if kind == "backup" {
                None
            } else {
                Some(destination.root.as_path())
            };
            assert_eq!(
                source
                    .reconcile_maintenance_retry(&v.id, root, true)
                    .unwrap_err()
                    .code,
                "STATE_INVALID"
            );
            assert_eq!(fs::read(path).unwrap(), bytes);
            assert_eq!(
                fs::read(source.root.join(format!("maintenance-retry-{}.json", v.id))).unwrap(),
                audit
            );
        }
    }
}
