// SPDX-License-Identifier: Apache-2.0
//! Explicit maintenance retry into new jobs. Original journals and candidate data are retained.
use super::installation_backup::source_bytes;
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceRetry {
    pub id: String,
    pub target_id: String,
    pub kind: String,
    pub state: String,
    pub new_job_id: Option<String>,
    pub original_job_sha256: String,
    pub destination_root_sha256: String,
    pub error_code: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryReceipt<T> {
    pub id: String,
    pub target_id: String,
    pub new_job_id: String,
    pub kind: String,
    pub data_preserved: bool,
    pub result: T,
}
pub struct RestorationRetryInput<'a> {
    pub destination: &'a LifecycleService,
    pub image: &'a str,
    pub key: &'a Path,
    pub archive: &'a Path,
    pub port: u16,
    pub preserve_candidates: bool,
    pub fresh_installation: bool,
}
pub(crate) struct RetryLink<'a> {
    owner: &'a LifecycleService,
    record: &'a mut MaintenanceRetry,
}
fn uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn validate(v: &MaintenanceRetry) -> Result<()> {
    if !uuid(&v.id)
        || !uuid(&v.target_id)
        || v.id == v.target_id
        || !["backup", "restoration"].contains(&v.kind.as_str())
        || !["preparing", "running", "completed", "failed", "interrupted"]
            .contains(&v.state.as_str())
        || !hash_valid(&v.original_job_sha256)
        || !hash_valid(&v.destination_root_sha256)
        || v.new_job_id
            .as_ref()
            .is_some_and(|id| !uuid(id) || id == &v.target_id || id == &v.id)
        || v.created_at > v.updated_at
        || v.updated_at > 8_640_000_000_000_000
        || v.error_code.as_ref().is_some_and(|s| {
            s.is_empty() || s.len() > 128 || !s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
        })
        || (["preparing", "running", "completed"].contains(&v.state.as_str())
            != v.error_code.is_none())
        || v.state == "preparing" && v.new_job_id.is_some()
        || ["running", "completed"].contains(&v.state.as_str()) && v.new_job_id.is_none()
    {
        return Err(err("STATE_INVALID"));
    }
    Ok(())
}
impl RetryLink<'_> {
    pub(crate) fn started(self, id: &str) -> Result<()> {
        self.record.new_job_id = Some(id.into());
        self.record.state = "running".into();
        self.record.updated_at = now();
        self.owner.save_retry(self.record)
    }
}
impl LifecycleService {
    fn save_retry(&self, v: &MaintenanceRetry) -> Result<()> {
        validate(v)?;
        write_json(&self.root, &format!("maintenance-retry-{}.json", v.id), v)?;
        #[cfg(unix)]
        File::open(&self.root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        Ok(())
    }
    pub(crate) fn retry_record(&self, id: &str) -> Result<MaintenanceRetry> {
        if !uuid(id) {
            return Err(err("STATE_INVALID"));
        }
        let v: MaintenanceRetry = serde_json::from_slice(&source_bytes(
            &self.root,
            &format!("maintenance-retry-{id}.json"),
            8192,
            true,
        )?)
        .map_err(|_| err("STATE_INVALID"))?;
        validate(&v)?;
        if v.id != id {
            return Err(err("STATE_INVALID"));
        }
        Ok(v)
    }
    fn retry_history(&self) -> Result<Vec<MaintenanceRetry>> {
        let mut all = Vec::new();
        for e in fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let e = e.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(id) = name
                .strip_prefix("maintenance-retry-")
                .and_then(|n| n.strip_suffix(".json"))
            {
                if !uuid(id) || all.len() >= 1000 {
                    return Err(err("STATE_INVALID"));
                }
                let v: MaintenanceRetry =
                    serde_json::from_slice(&source_bytes(&self.root, name, 8192, true)?)
                        .map_err(|_| err("STATE_INVALID"))?;
                validate(&v)?;
                if v.id != id {
                    return Err(err("STATE_INVALID"));
                }
                all.push(v);
            }
        }
        all.sort_by_key(|v| v.created_at);
        Ok(all)
    }
    pub(crate) fn recover_maintenance_retries(&self) -> Result<()> {
        let _lock = match self.lock() {
            Ok(f) => f,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        self.recover_retries_locked()
    }
    fn recover_retries_locked(&self) -> Result<()> {
        for mut v in self.retry_history()? {
            if ["preparing", "running"].contains(&v.state.as_str()) {
                v.state = "interrupted".into();
                v.error_code = Some("INTERRUPTED".into());
                v.updated_at = now();
                self.save_retry(&v)?;
            }
        }
        Ok(())
    }
    pub fn maintenance_retries(&self) -> Result<Vec<MaintenanceRetry>> {
        let _lock = self.lock()?;
        self.recover_retries_locked()?;
        self.retry_history()
    }
    fn retry_target_bytes(&self, kind: &str, target: &str) -> Result<Vec<u8>> {
        let name = if kind == "backup" {
            format!("backup-creation-{target}/job.json")
        } else {
            "restoration.json".into()
        };
        source_bytes(&self.root, &name, 16 * 1024, true)
    }
    fn begin_retry(
        &self,
        kind: &str,
        target: &str,
        destination: &Path,
    ) -> Result<MaintenanceRetry> {
        self.reconciliation_target(kind, target)?;
        let all = self.retry_history()?;
        if all.len() >= 1000 {
            return Err(err("JOB_HISTORY_FULL"));
        }
        if all.iter().any(|v| {
            v.target_id == target
                && v.kind == kind
                && (["preparing", "running", "interrupted"].contains(&v.state.as_str())
                    || v.state == "failed" && v.new_job_id.is_some())
        }) {
            return Err(err("RETRY_RECOVERY_REQUIRED"));
        }
        let at = now();
        let v = MaintenanceRetry {
            id: Uuid::new_v4().to_string(),
            target_id: target.into(),
            kind: kind.into(),
            state: "preparing".into(),
            new_job_id: None,
            original_job_sha256: digest(&self.retry_target_bytes(kind, target)?),
            destination_root_sha256: digest(destination.to_string_lossy().as_bytes()),
            error_code: None,
            created_at: at,
            updated_at: at,
        };
        self.save_retry(&v)?;
        Ok(v)
    }
    fn finish_retry<T>(
        &self,
        mut v: MaintenanceRetry,
        result: Result<T>,
    ) -> Result<RetryReceipt<T>> {
        let result =
            if digest(&self.retry_target_bytes(&v.kind, &v.target_id)?) != v.original_job_sha256 {
                Err(err("RETRY_SOURCE_CHANGED"))
            } else {
                result
            };
        v.updated_at = now();
        match result {
            Ok(result) => {
                let new_job_id = v.new_job_id.clone().ok_or_else(|| err("STATE_INVALID"))?;
                v.state = "completed".into();
                self.save_retry(&v)?;
                Ok(RetryReceipt {
                    id: v.id,
                    target_id: v.target_id,
                    new_job_id,
                    kind: v.kind,
                    data_preserved: true,
                    result,
                })
            }
            Err(e) => {
                v.state = "failed".into();
                v.error_code = Some(e.code.clone());
                self.save_retry(&v)?;
                Err(e)
            }
        }
    }
    pub fn retry_backup(
        &self,
        target: &str,
        image: &str,
        key: &Path,
        preserve_candidates: bool,
        external_writers_quiesced: bool,
    ) -> Result<RetryReceipt<backup_creation::BackupCreationReceipt>> {
        if !preserve_candidates || !external_writers_quiesced {
            return Err(err("RETRY_ACK_REQUIRED"));
        }
        if !uuid(target) {
            return Err(err("RETRY_TARGET_INVALID"));
        }
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        maintenance::input_path(&self.root, true)?;
        maintenance::input_path(key, false)?;
        if key.starts_with(&self.root) {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        let _lock = self.lock()?;
        let mut v = self.begin_retry("backup", target, &self.root)?;
        let result = (|| {
            self.reconcile_helper_locked("backup", target, true)?;
            self.create_backup_locked(
                image,
                key,
                true,
                Some(RetryLink {
                    owner: self,
                    record: &mut v,
                }),
            )
        })();
        self.finish_retry(v, result)
    }
    pub fn retry_restoration(
        &self,
        target: &str,
        input: RestorationRetryInput<'_>,
    ) -> Result<RetryReceipt<restoration::RestorationReceipt>> {
        self.retry_restoration_observed(target, input, &|_| Ok(()))
    }
    pub(crate) fn retry_restoration_observed(
        &self,
        target: &str,
        input: RestorationRetryInput<'_>,
        observe: &dyn Fn(&str) -> Result<()>,
    ) -> Result<RetryReceipt<restoration::RestorationReceipt>> {
        if !input.preserve_candidates || !input.fresh_installation {
            return Err(err("RETRY_ACK_REQUIRED"));
        }
        if !uuid(target) {
            return Err(err("RETRY_TARGET_INVALID"));
        }
        if !input.image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        if input.port < 1024 {
            return Err(err("BACKUP_PATH_INVALID"));
        }
        for p in [&self.root, &input.destination.root, input.archive] {
            maintenance::input_path(p, true)?;
        }
        maintenance::input_path(input.key, false)?;
        let dst = &input.destination.root;
        if dst.starts_with(&self.root)
            || self.root.starts_with(dst)
            || input.key.starts_with(dst)
            || input.key.starts_with(&self.root)
            || input.key.starts_with(input.archive)
            || input.archive.starts_with(dst)
            || dst.starts_with(input.archive)
            || input.archive.starts_with(&self.root)
            || self.root.starts_with(input.archive)
        {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        let _source_lock = self.lock()?;
        let _destination_lock = input.destination.lock()?;
        restoration::fresh_root(dst)?;
        let mut v = self.begin_retry("restoration", target, dst)?;
        let result = (|| {
            observe(&v.id)?;
            self.reconcile_helper_locked("restoration", target, true)?;
            self.stop_cancelled_candidate("restoration")?;
            input.destination.restore_backup_locked(
                input.image,
                input.key,
                input.archive,
                input.port,
                true,
                Some(RetryLink {
                    owner: self,
                    record: &mut v,
                }),
            )
        })();
        self.finish_retry(v, result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> LifecycleService {
        LifecycleService::new(
            fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("retry-proof-{}", Uuid::new_v4())),
        )
        .unwrap()
    }
    fn failed(s: &LifecycleService) -> String {
        let id = Uuid::new_v4().to_string();
        let at = now();
        let job = restoration::RestorationJob {
            id: id.clone(),
            state: "failed".into(),
            stage: "authenticating".into(),
            error_code: Some("BACKUP_AUTHENTICATION_FAILED".into()),
            created_at: at,
            updated_at: at,
        };
        write_json(&s.root, "restoration.json", &job).unwrap();
        id
    }
    #[test]
    fn retry_records_link_distinct_new_job_without_rewriting_original_and_recover_crash() {
        let s = fixture();
        let id = failed(&s);
        let before = fs::read(s.root.join("restoration.json")).unwrap();
        let mut v = s.begin_retry("restoration", &id, &fixture().root).unwrap();
        let new_id = Uuid::new_v4().to_string();
        RetryLink {
            owner: &s,
            record: &mut v,
        }
        .started(&new_id)
        .unwrap();
        let h = s.maintenance_retries().unwrap();
        assert_eq!(h[0].state, "interrupted");
        assert_eq!(h[0].new_job_id.as_deref(), Some(new_id.as_str()));
        assert_eq!(
            s.begin_retry("restoration", &id, &fixture().root)
                .unwrap_err()
                .code,
            "RETRY_RECOVERY_REQUIRED"
        );
        assert_eq!(fs::read(s.root.join("restoration.json")).unwrap(), before);
    }
    #[test]
    fn terminal_failure_retains_new_job_fence_and_original_bytes() {
        let s = fixture();
        let id = failed(&s);
        let original = s.retry_target_bytes("restoration", &id).unwrap();
        let mut v = s.begin_retry("restoration", &id, &fixture().root).unwrap();
        RetryLink {
            owner: &s,
            record: &mut v,
        }
        .started(&Uuid::new_v4().to_string())
        .unwrap();
        assert_eq!(
            s.finish_retry::<()>(v, Err(err("ENGINE_UNAVAILABLE")))
                .unwrap_err()
                .code,
            "ENGINE_UNAVAILABLE"
        );
        assert_eq!(
            s.begin_retry("restoration", &id, &fixture().root)
                .unwrap_err()
                .code,
            "RETRY_RECOVERY_REQUIRED"
        );
        assert_eq!(s.retry_target_bytes("restoration", &id).unwrap(), original);
    }
    #[test]
    fn success_requires_new_job_and_unchanged_original_and_never_relabels_failed_target() {
        let s = fixture();
        let id = failed(&s);
        let original = s.retry_target_bytes("restoration", &id).unwrap();
        let mut v = s.begin_retry("restoration", &id, &fixture().root).unwrap();
        RetryLink {
            owner: &s,
            record: &mut v,
        }
        .started(&Uuid::new_v4().to_string())
        .unwrap();
        let receipt = s.finish_retry(v, Ok(())).unwrap();
        assert_ne!(receipt.new_job_id, id);
        assert!(receipt.data_preserved);
        assert_eq!(s.retry_target_bytes("restoration", &id).unwrap(), original);
        assert_eq!(s.maintenance_retries().unwrap()[0].state, "completed");
        let mut v = s.begin_retry("restoration", &id, &fixture().root).unwrap();
        RetryLink {
            owner: &s,
            record: &mut v,
        }
        .started(&Uuid::new_v4().to_string())
        .unwrap();
        fs::write(s.root.join("restoration.json"), b"changed").unwrap();
        assert_eq!(
            s.finish_retry(v, Ok(())).unwrap_err().code,
            "RETRY_SOURCE_CHANGED"
        );
        assert_eq!(
            fs::read(s.root.join("restoration.json")).unwrap(),
            b"changed"
        );
    }
    #[test]
    fn acknowledgements_invalid_targets_busy_owner_and_completed_target_refuse() {
        let s = fixture();
        let key = Path::new("relative");
        assert_eq!(
            s.retry_backup("bad", "tag", key, false, true)
                .unwrap_err()
                .code,
            "RETRY_ACK_REQUIRED"
        );
        assert_eq!(
            s.retry_backup("bad", "tag", key, true, true)
                .unwrap_err()
                .code,
            "RETRY_TARGET_INVALID"
        );
        let id = failed(&s);
        let before = s.retry_target_bytes("restoration", &id).unwrap();
        let held = s.lock().unwrap();
        assert_eq!(s.maintenance_retries().unwrap_err().code, "BUSY");
        drop(held);
        let mut j: restoration::RestorationJob = serde_json::from_slice(&before).unwrap();
        j.state = "completed".into();
        j.error_code = None;
        write_json(&s.root, "restoration.json", &j).unwrap();
        assert_eq!(
            s.begin_retry("restoration", &id, &fixture().root)
                .unwrap_err()
                .code,
            "RECONCILIATION_TARGET_INVALID"
        );
        assert!(s.retry_history().unwrap().is_empty());
    }
}
