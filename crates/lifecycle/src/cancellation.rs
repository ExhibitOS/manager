// SPDX-License-Identifier: Apache-2.0
//! Durable cooperative cancellation; a request is never a stop receipt.
use super::installation_backup::source_bytes;
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceContext {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
fn uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id)
}
fn valid(v: &MaintenanceContext) -> Result<()> {
    if !uuid(&v.id)
        || !["backup", "restoration"].contains(&v.kind.as_str())
        || ![
            "running",
            "requested",
            "confirmed",
            "uncertain",
            "completed",
            "failed",
            "interrupted",
        ]
        .contains(&v.state.as_str())
        || v.stage.is_empty()
        || v.stage.len() > 64
        || !v.stage.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        || v.created_at > v.updated_at
        || v.updated_at > 8_640_000_000_000_000
        || v.error_code.as_ref().is_some_and(|s| {
            s.is_empty() || s.len() > 128 || !s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
        })
        || (["running", "requested", "completed"].contains(&v.state.as_str())
            != v.error_code.is_none())
        || v.state == "confirmed" && v.error_code.as_deref() != Some("CANCELLED")
        || v.state == "uncertain" && v.error_code.as_deref() != Some("CANCEL_UNCERTAIN")
    {
        return Err(err("STATE_INVALID"));
    }
    Ok(())
}
impl LifecycleService {
    fn cancellation_lock(&self) -> Result<OperationGuard> {
        let path = self.root.join("maintenance-cancel.lock");
        let file = match private_options().open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut options = OpenOptions::new();
                options.read(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                options.open(&path).map_err(|_| err("STATE_INVALID"))?
            }
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        let m = file.metadata().map_err(|_| err("STATE_INVALID"))?;
        if !m.is_file() {
            return Err(err("STATE_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::symlink_metadata(&path).map_err(|_| err("STATE_INVALID"))?;
            if m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.mode() & 0o7777 != 0o600
                || current.is_symlink()
                || (m.dev(), m.ino()) != (current.dev(), current.ino())
            {
                return Err(err("STATE_INVALID"));
            }
        }
        file.try_lock_exclusive().map_err(|_| err("BUSY"))?;
        #[cfg(windows)]
        {
            Ok(file)
        }
        #[cfg(not(windows))]
        {
            Ok(OperationGuard(file))
        }
    }
    fn read_maintenance(&self) -> Result<Option<MaintenanceContext>> {
        match fs::symlink_metadata(self.root.join("maintenance-active.json")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
            Ok(_) => {}
        }
        let id: String = serde_json::from_slice(&source_bytes(
            &self.root,
            "maintenance-active.json",
            128,
            true,
        )?)
        .map_err(|_| err("STATE_INVALID"))?;
        if !uuid(&id) {
            return Err(err("STATE_INVALID"));
        }
        let v: MaintenanceContext = serde_json::from_slice(&source_bytes(
            &self.root,
            &format!("maintenance-{id}.json"),
            4096,
            true,
        )?)
        .map_err(|_| err("STATE_INVALID"))?;
        valid(&v)?;
        if v.id != id {
            return Err(err("STATE_INVALID"));
        }
        Ok(Some(v))
    }
    fn save_maintenance(&self, v: &MaintenanceContext) -> Result<()> {
        valid(v)?;
        write_json(&self.root, &format!("maintenance-{}.json", v.id), v)?;
        self.sync_maintenance_directory()
    }
    fn sync_maintenance_directory(&self) -> Result<()> {
        #[cfg(unix)]
        {
            File::open(&self.root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        Ok(())
    }
    pub(crate) fn begin_maintenance(&self, kind: &str, id: &str, stage: &str) -> Result<()> {
        let _guard = self.cancellation_lock()?;
        if self
            .read_maintenance()?
            .is_some_and(|v| ["running", "requested"].contains(&v.state.as_str()))
        {
            return Err(err("CANCEL_RECOVERY_REQUIRED"));
        }
        let at = now();
        let v = MaintenanceContext {
            id: id.into(),
            kind: kind.into(),
            state: "running".into(),
            stage: stage.into(),
            error_code: None,
            created_at: at,
            updated_at: at,
        };
        self.save_maintenance(&v)?;
        write_json(&self.root, "maintenance-active.json", &id)?;
        self.sync_maintenance_directory()
    }
    pub fn maintenance_context(&self) -> Result<Option<MaintenanceContext>> {
        // A read of an empty root must not make it ineligible for fresh restoration.
        if self.read_maintenance()?.is_none() {
            return Ok(None);
        }
        let _guard = self.cancellation_lock()?;
        self.read_maintenance()
    }
    pub(crate) fn recover_maintenance_cancellation(&self) -> Result<()> {
        if self.read_maintenance()?.is_none() {
            return Ok(());
        }
        let _operation = match self.lock() {
            Ok(f) => f,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        let _guard = self.cancellation_lock()?;
        if let Some(mut v) = self.read_maintenance()?
            && ["running", "requested"].contains(&v.state.as_str())
        {
            v.state = "interrupted".into();
            v.error_code = Some("INTERRUPTED".into());
            v.updated_at = now();
            self.save_maintenance(&v)?;
        }
        Ok(())
    }
    pub fn request_maintenance_cancel(
        &self,
        kind: &str,
        id: &str,
        preserve: bool,
    ) -> Result<MaintenanceContext> {
        if !preserve {
            return Err(err("CANCEL_ACK_REQUIRED"));
        }
        if !uuid(id) || !["backup", "restoration"].contains(&kind) {
            return Err(err("CANCEL_INPUT_INVALID"));
        }
        if self.read_maintenance()?.is_none() {
            return Err(err("CANCEL_TARGET_INVALID"));
        }
        let _guard = self.cancellation_lock()?;
        let mut v = self
            .read_maintenance()?
            .ok_or_else(|| err("CANCEL_TARGET_INVALID"))?;
        if v.id != id || v.kind != kind || !["running", "requested"].contains(&v.state.as_str()) {
            return Err(err("CANCEL_TARGET_INVALID"));
        }
        // A lock file's existence is insufficient. Require a live exclusive owner.
        match self.lock() {
            Err(e) if e.code == "BUSY" => {}
            Err(e) => return Err(e),
            Ok(_) => return Err(err("CANCEL_TARGET_INVALID")),
        }
        let path = if kind == "backup" {
            format!("backup-creation-{id}/job.json")
        } else {
            "restoration.json".into()
        };
        let bytes = source_bytes(&self.root, &path, 16 * 1024, true)?;
        let (job_id, state, stage, error, created, updated) = if kind == "backup" {
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
            || state != "running"
            || error.is_some()
            || stage.is_empty()
            || stage.len() > 64
            || created > updated
            || updated > 8_640_000_000_000_000
        {
            return Err(err("CANCEL_TARGET_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            let f = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(self.root.join("operation.lock"))
                .map_err(|_| err("STATE_INVALID"))?;
            let m = f.metadata().map_err(|_| err("STATE_INVALID"))?;
            let current = fs::symlink_metadata(self.root.join("operation.lock"))
                .map_err(|_| err("STATE_INVALID"))?;
            if !m.is_file()
                || m.nlink() != 1
                || m.mode() & 0o7777 != 0o600
                || m.uid() != unsafe { libc::geteuid() }
                || (m.dev(), m.ino()) != (current.dev(), current.ino())
            {
                return Err(err("STATE_INVALID"));
            }
        }
        v.state = "requested".into();
        v.updated_at = now();
        self.save_maintenance(&v)?;
        Ok(v)
    }
    pub(crate) fn maintenance_requested(&self, kind: &str, id: &str) -> Result<bool> {
        let v = self
            .read_maintenance()?
            .ok_or_else(|| err("STATE_INVALID"))?;
        if v.kind != kind || v.id != id || !["running", "requested"].contains(&v.state.as_str()) {
            return Err(err("STATE_INVALID"));
        }
        Ok(v.state == "requested")
    }
    pub(crate) fn maintenance_stage(&self, kind: &str, id: &str, stage: &str) -> Result<()> {
        self.maintenance_checkpoint(kind, id)?;
        let _guard = self.cancellation_lock()?;
        let mut v = self
            .read_maintenance()?
            .ok_or_else(|| err("STATE_INVALID"))?;
        if v.kind != kind || v.id != id {
            return Err(err("STATE_INVALID"));
        }
        v.stage = stage.into();
        v.updated_at = now();
        self.save_maintenance(&v)
    }
    pub(crate) fn stop_cancelled_candidate(&self, kind: &str) -> Result<()> {
        if kind != "restoration" || !self.root.join("bundle/manifest.json").exists() {
            return Ok(());
        }
        let m = self.manifest()?;
        self.validate_ownership(&m, "docker")?;
        let bundle = self.root.join("bundle");
        run(
            "docker",
            &compose_args(&m, &["stop", "--timeout", "30"]),
            Some(&bundle),
            180,
        )?;
        self.validate_ownership(&m, "docker")?;
        let ids = run(
            "docker",
            &compose_args(&m, &["ps", "--all", "--quiet"]),
            Some(&bundle),
            30,
        )?;
        let ids = String::from_utf8(ids).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        for id in ids.lines() {
            if id.len() != 64 || !hash_valid(id) {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            let v = backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
            if v["Id"] != id
                || v["Config"]["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                || v["Config"]["Labels"]["com.exhibitos.project"] != m.project_name
                || v["State"]["Running"] != false
                || v["State"]["Restarting"] != false
            {
                return Err(err("CANCEL_UNCERTAIN"));
            }
        }
        Ok(())
    }
    fn confirm_maintenance_stop(&self, kind: &str, id: &str) -> Result<()> {
        let nonce = Uuid::new_v4().to_string();
        self.reconcile_helper_engine(kind, id, &nonce, |args| run("docker", args, None, 60))?;
        self.stop_cancelled_candidate(kind)
    }
    pub(crate) fn maintenance_checkpoint(&self, kind: &str, id: &str) -> Result<()> {
        if self.maintenance_requested(kind, id)? {
            self.confirm_maintenance_stop(kind, id)
                .map_err(|_| err("CANCEL_UNCERTAIN"))?;
            return Err(err("CANCELLED"));
        }
        Ok(())
    }
    /// Only interrupt attach after the owned daemon helper is observed running and stopped.
    /// Absence/created before launch completion cannot prove cancellation; keep waiting.
    pub(crate) fn run_maintenance_helper(
        &self,
        kind: &str,
        id: &str,
        args: &[String],
    ) -> Result<Vec<u8>> {
        self.maintenance_checkpoint(kind, id)?;
        let mut next = Instant::now();
        let result = run_observed("docker", args, None, 3600, || {
            if Instant::now() < next {
                return Ok(());
            }
            next = Instant::now() + Duration::from_millis(400);
            if !self.maintenance_requested(kind, id)? {
                return Ok(());
            }
            let prefix = if kind == "backup" {
                "exhibitos-backup"
            } else {
                "exhibitos-restore"
            };
            let label = if kind == "backup" {
                "com.exhibitos.backup"
            } else {
                "com.exhibitos.restoration"
            };
            let ids = helper_reconciliation::list(
                |args| run("docker", args, None, 30),
                &format!("{prefix}-{id}"),
                label,
                id,
            )?;
            if let Some(cid) = ids.first() {
                let v = backup_creation::inspected("docker", &["inspect".into(), cid.clone()])?;
                if v["State"]["Running"] == true {
                    self.confirm_maintenance_stop(kind, id)
                        .map_err(|_| err("CANCEL_UNCERTAIN"))?;
                    return Err(err("CANCELLED"));
                }
            }
            Ok(())
        });
        // Do not turn a failed launch/client timeout into a confirmed stop.
        if result.is_err() && self.maintenance_requested(kind, id)? {
            return Err(err(
                if result.as_ref().err().is_some_and(|e| e.code == "CANCELLED") {
                    "CANCELLED"
                } else {
                    "CANCEL_UNCERTAIN"
                },
            ));
        }
        self.maintenance_checkpoint(kind, id)?;
        result
    }
    /// Hold this guard through original job/receipt and cancellation terminal writes.
    pub(crate) fn maintenance_finish_guard(&self, kind: &str, id: &str) -> Result<OperationGuard> {
        let guard = self.cancellation_lock()?;
        let v = self
            .read_maintenance()?
            .ok_or_else(|| err("STATE_INVALID"))?;
        if v.id != id || v.kind != kind {
            return Err(err("STATE_INVALID"));
        }
        Ok(guard)
    }
    pub(crate) fn finish_maintenance(&self, state: &str, error: Option<&str>) -> Result<()> {
        let mut v = self
            .read_maintenance()?
            .ok_or_else(|| err("STATE_INVALID"))?;
        v.state = state.into();
        v.error_code = error.map(String::from);
        v.updated_at = now();
        self.save_maintenance(&v)
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture() -> (LifecycleService, String) {
        let s = LifecycleService::new(
            std::env::temp_dir().join(format!("cancel-test-{}", Uuid::new_v4())),
        )
        .unwrap();
        let id = Uuid::new_v4().to_string();
        (s, id)
    }
    fn job(s: &LifecycleService, id: &str) {
        write_json(
            &s.root,
            "restoration.json",
            &restoration::RestorationJob {
                id: id.into(),
                state: "running".into(),
                stage: "authenticating".into(),
                error_code: None,
                created_at: 1,
                updated_at: 2,
            },
        )
        .unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_scope_unlocks_even_when_duplicate_descriptor_survives() {
        let (s, _) = fixture();
        let guard = s.cancellation_lock().unwrap();
        // A duplicated descriptor retains the same open file description, as a
        // forked child does until exec. Closing just the parent is insufficient.
        let duplicate = guard.try_clone().unwrap();
        assert!(matches!(s.cancellation_lock(), Err(e) if e.code == "BUSY"));
        drop(guard);
        let next = s.cancellation_lock().unwrap();
        assert!(matches!(s.cancellation_lock(), Err(e) if e.code == "BUSY"));
        drop(duplicate);
        assert!(matches!(s.cancellation_lock(), Err(e) if e.code == "BUSY"));
        drop(next);
        assert!(s.cancellation_lock().is_ok());
    }
    #[test]
    fn empty_context_does_not_occupy_fresh_destination() {
        let (s, _) = fixture();
        assert!(s.maintenance_context().unwrap().is_none());
        assert!(!s.root.join("maintenance-cancel.lock").exists());
        assert!(
            s.request_maintenance_cancel("restoration", &Uuid::new_v4().to_string(), true)
                .is_err()
        );
        assert!(s.restoration_context().unwrap().fresh);
    }
    #[test]
    fn request_needs_live_operation_owner_matching_job_and_acknowledgement() {
        let (s, id) = fixture();
        job(&s, &id);
        s.begin_maintenance("restoration", &id, "authenticating")
            .unwrap();
        assert_eq!(
            s.request_maintenance_cancel("restoration", &id, true)
                .unwrap_err()
                .code,
            "CANCEL_TARGET_INVALID"
        );
        let _owner = s.lock().unwrap();
        assert_eq!(
            s.request_maintenance_cancel("restoration", &id, false)
                .unwrap_err()
                .code,
            "CANCEL_ACK_REQUIRED"
        );
        assert!(
            s.request_maintenance_cancel("restoration", &Uuid::new_v4().to_string(), true)
                .is_err()
        );
        assert!(s.request_maintenance_cancel("backup", &id, true).is_err());
        let r = s
            .request_maintenance_cancel("restoration", &id, true)
            .unwrap();
        assert_eq!(r.state, "requested");
        assert_eq!(
            s.request_maintenance_cancel("restoration", &id, true)
                .unwrap()
                .state,
            "requested"
        );
        assert_eq!(
            fs::metadata(s.root.join(format!("maintenance-{id}.json")))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(s.maintenance_context().unwrap().unwrap().state, "requested");
    }
    #[test]
    fn terminal_commit_serializes_request_and_completion_then_rejects_late_request() {
        let (s, id) = fixture();
        job(&s, &id);
        let _owner = s.lock().unwrap();
        s.begin_maintenance("restoration", &id, "authenticating")
            .unwrap();
        let guard = s.maintenance_finish_guard("restoration", &id).unwrap();
        assert_eq!(
            s.request_maintenance_cancel("restoration", &id, true)
                .unwrap_err()
                .code,
            "BUSY"
        );
        s.finish_maintenance("completed", None).unwrap();
        drop(guard);
        assert_eq!(
            s.request_maintenance_cancel("restoration", &id, true)
                .unwrap_err()
                .code,
            "CANCEL_TARGET_INVALID"
        );
    }
    #[test]
    fn request_wins_before_terminal_guard_and_stage_never_erases_it() {
        let (s, id) = fixture();
        job(&s, &id);
        let _owner = s.lock().unwrap();
        s.begin_maintenance("restoration", &id, "authenticating")
            .unwrap();
        s.request_maintenance_cancel("restoration", &id, true)
            .unwrap();
        let _guard = s.maintenance_finish_guard("restoration", &id).unwrap();
        assert!(s.maintenance_requested("restoration", &id).unwrap());
        s.finish_maintenance("uncertain", Some("CANCEL_UNCERTAIN"))
            .unwrap();
        assert_eq!(s.read_maintenance().unwrap().unwrap().state, "uncertain");
    }
    #[test]
    fn crash_recovery_is_interrupted_never_confirmed_and_preserves_candidate() {
        let (s, id) = fixture();
        job(&s, &id);
        s.begin_maintenance("restoration", &id, "authenticating")
            .unwrap();
        let owner = s.lock().unwrap();
        s.request_maintenance_cancel("restoration", &id, true)
            .unwrap();
        fs::write(s.root.join("retained"), b"candidate").unwrap();
        s.recover_maintenance_cancellation().unwrap();
        assert_eq!(s.read_maintenance().unwrap().unwrap().state, "requested");
        drop(owner);
        let reopened = LifecycleService::new(s.root.clone()).unwrap();
        assert_eq!(
            reopened.maintenance_context().unwrap().unwrap().state,
            "interrupted"
        );
        assert_eq!(fs::read(s.root.join("retained")).unwrap(), b"candidate");
    }
    #[test]
    fn alias_and_permissive_cancel_records_fail_closed() {
        let (s, id) = fixture();
        job(&s, &id);
        s.begin_maintenance("restoration", &id, "authenticating")
            .unwrap();
        let path = s.root.join(format!("maintenance-{id}.json"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(s.maintenance_context().is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let lock = s.root.join("maintenance-cancel.lock");
        fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink(s.root.join("missing"), &lock).unwrap();
        assert!(s.maintenance_context().is_err());
    }
}
