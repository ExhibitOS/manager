// SPDX-License-Identifier: Apache-2.0
//! Desktop installation selection. Switching never stops, deletes or overwrites runtime data.
use super::installation_backup::source_bytes;
use super::*;
use std::sync::RwLock;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) created_at: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Registry {
    pub(crate) format: u32,
    pub(crate) active_id: String,
    pub(crate) installations: Vec<Entry>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationInfo {
    pub id: String,
    pub kind: String,
    pub created_at: u64,
    pub path: String,
    pub available: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationContext {
    pub selection_token: String,
    pub active_id: String,
    pub mode: String,
    pub installations: Vec<InstallationInfo>,
    pub error_code: Option<String>,
}
struct Selected {
    token: String,
    entry: Entry,
    path: PathBuf,
    service: Option<LifecycleService>,
    error_code: Option<String>,
}
pub struct InstallationController {
    profile: Option<LifecycleService>,
    _profile_session: Option<super::profile_backup::ProfileSession>,
    mode: String,
    current: RwLock<Selected>,
    maintenance_destination: RwLock<Option<MaintenanceRoute>>,
}
pub(crate) fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub(crate) fn private_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    super::windows_private::PrivateDirectory::inspect(path)?.check()?;
    let metadata = fs::symlink_metadata(path).map_err(|_| err("INSTALLATION_ROOT_UNAVAILABLE"))?;
    if metadata.is_symlink() || !metadata.is_dir() {
        return Err(err("INSTALLATION_ROOT_UNAVAILABLE"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fs::canonicalize(path).ok().as_deref() != Some(path)
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(err("INSTALLATION_ROOT_UNAVAILABLE"));
        }
    }
    Ok(())
}
pub(crate) fn profile_lock(profile: &LifecycleService) -> Result<File> {
    let file = profile.lock()?;
    let metadata = file
        .metadata()
        .map_err(|_| err("INSTALLATION_SELECTION_INVALID"))?;
    if !metadata.is_file() {
        return Err(err("INSTALLATION_SELECTION_INVALID"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(err("INSTALLATION_SELECTION_INVALID"));
        }
    }
    Ok(file)
}
pub(crate) fn new_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let _directory = super::windows_private::PrivateDirectory::create(path)?;
    #[cfg(not(windows))]
    fs::create_dir(path).map_err(|_| err("STATE_UNAVAILABLE"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
    }
    private_directory(path)
}
pub(crate) fn root(profile: &Path, entry: &Entry) -> PathBuf {
    if entry.kind == "default" {
        profile.join("local-runtime")
    } else {
        profile.join("installations").join(&entry.id)
    }
}
pub(crate) fn valid(registry: &Registry) -> Result<()> {
    if registry.format != 1
        || !uuid(&registry.active_id)
        || registry.installations.is_empty()
        || registry.installations.len() > 128
        || registry
            .installations
            .iter()
            .filter(|e| e.kind == "default")
            .count()
            != 1
        || !registry
            .installations
            .iter()
            .any(|e| e.id == registry.active_id)
    {
        return Err(err("INSTALLATION_SELECTION_INVALID"));
    }
    let mut ids = std::collections::HashSet::new();
    for e in &registry.installations {
        if !uuid(&e.id)
            || !ids.insert(&e.id)
            || !["default", "recovery"].contains(&e.kind.as_str())
            || e.created_at > 8_640_000_000_000_000
        {
            return Err(err("INSTALLATION_SELECTION_INVALID"));
        }
    }
    Ok(())
}
pub(crate) fn load(profile: &Path) -> Result<Option<(Registry, Vec<u8>)>> {
    match fs::symlink_metadata(profile.join("installation-selection.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(err("INSTALLATION_SELECTION_INVALID")),
        Ok(_) => {
            let bytes = source_bytes(profile, "installation-selection.json", 64 * 1024, true)
                .map_err(|_| err("INSTALLATION_SELECTION_INVALID"))?;
            let registry: Registry = serde_json::from_slice(&bytes)
                .map_err(|_| err("INSTALLATION_SELECTION_INVALID"))?;
            valid(&registry)?;
            Ok(Some((registry, bytes)))
        }
    }
}
pub(crate) fn save(profile: &Path, registry: &Registry, previous: Option<&[u8]>) -> Result<()> {
    valid(registry)?;
    if fs2::available_space(profile).map_err(|_| err("STORAGE_UNAVAILABLE"))? < 256 * 1024 {
        return Err(err("STORAGE_QUOTA"));
    }
    if let Some(bytes) = previous {
        let history = profile.join("selection-history");
        match fs::symlink_metadata(&history) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => new_directory(&history)?,
            Ok(_) => private_directory(&history)?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        }
        let name = format!("{}-{}-{}.json", now(), Uuid::new_v4(), digest(bytes));
        write_private_new(&history, &name, bytes, 64 * 1024)?;
        File::open(&history)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
    }
    write_json(profile, "installation-selection.json", registry)?;
    File::open(profile)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("INSTALLATION_SELECTION_UNCERTAIN"))
}
fn selected(profile: &Path, entry: Entry) -> Selected {
    let path = root(profile, &entry);
    let outcome = private_directory(&path).and_then(|()| LifecycleService::new(path.clone()));
    let (service, error_code) = match outcome {
        Ok(service) => (Some(service), None),
        Err(error) => (None, Some(error.code)),
    };
    Selected {
        token: Uuid::new_v4().to_string(),
        entry,
        path,
        service,
        error_code,
    }
}
struct MaintenanceRoute {
    path: PathBuf,
    audit_id: Option<String>,
}
pub struct RestorationRetryOptions<'a> {
    pub image: &'a str,
    pub key: &'a Path,
    pub archive: &'a Path,
    pub port: u16,
    pub preserve_candidates: bool,
    pub fresh_installation: bool,
}
struct DestinationLease<'a>(&'a RwLock<Option<MaintenanceRoute>>);
impl Drop for DestinationLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut route) = self.0.write() {
            *route = None;
        }
    }
}
impl InstallationController {
    pub fn new(profile: PathBuf, override_root: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = override_root {
            return Self::pinned(path, "override");
        }
        if cfg!(windows) {
            return Self::pinned(profile.join("local-runtime"), "platform-unverified");
        }
        let parent = profile
            .parent()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        match fs::symlink_metadata(&profile) {
            Ok(m) if m.is_dir() && !m.is_symlink() => {
                let canonical = fs::canonicalize(&profile).map_err(|_| err("STATE_UNAVAILABLE"))?;
                private_directory(&canonical)?;
            }
            Ok(_) => return Err(err("INSTALLATION_ROOT_UNAVAILABLE")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        }
        fs::create_dir_all(parent).map_err(|_| err("STATE_UNAVAILABLE"))?;
        let (profile, anchor) = super::profile_backup::anchor_lock(&profile, false)?;
        match fs::symlink_metadata(&profile) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&profile).map_err(|_| err("STATE_UNAVAILABLE"))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))
                        .map_err(|_| err("STATE_UNAVAILABLE"))?;
                }
            }
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
            Ok(m) if m.is_dir() && !m.is_symlink() => {}
            Ok(_) => return Err(err("INSTALLATION_ROOT_UNAVAILABLE")),
        }
        let profile = fs::canonicalize(profile).map_err(|_| err("STATE_UNAVAILABLE"))?;
        private_directory(&profile)?;
        let session = super::profile_backup::anchored_session(&profile, anchor, false)?;
        let profile = LifecycleService::new(profile)?;
        let profile = LifecycleService::bound_existing(
            fs::canonicalize(&profile.root).map_err(|_| err("STATE_UNAVAILABLE"))?,
        )?;
        private_directory(&profile.root)?;
        let _lock = profile_lock(&profile)?;
        let registry = match load(&profile.root)? {
            Some((registry, _)) => registry,
            None => {
                let entry = Entry {
                    id: Uuid::new_v4().to_string(),
                    kind: "default".into(),
                    created_at: now(),
                };
                let path = root(&profile.root, &entry);
                match fs::symlink_metadata(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => new_directory(&path)?,
                    Ok(_) => {}
                    Err(_) => return Err(err("INSTALLATION_ROOT_UNAVAILABLE")),
                }
                let registry = Registry {
                    format: 1,
                    active_id: entry.id.clone(),
                    installations: vec![entry],
                };
                save(&profile.root, &registry, None)?;
                registry
            }
        };
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == registry.active_id)
            .ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?
            .clone();
        let current = selected(&profile.root, entry);
        drop(_lock);
        Ok(Self {
            profile: Some(profile),
            _profile_session: Some(session),
            mode: "managed".into(),
            current: RwLock::new(current),
            maintenance_destination: RwLock::new(None),
        })
    }
    fn pinned(path: PathBuf, mode: &str) -> Result<Self> {
        let service = LifecycleService::new(path)?;
        #[cfg(unix)]
        let service = LifecycleService {
            root: fs::canonicalize(&service.root).map_err(|_| err("STATE_UNAVAILABLE"))?,
        };
        let entry = Entry {
            id: Uuid::new_v4().to_string(),
            kind: if mode == "override" {
                "override"
            } else {
                "default"
            }
            .into(),
            created_at: now(),
        };
        let current = Selected {
            token: Uuid::new_v4().to_string(),
            entry,
            path: service.root.clone(),
            service: Some(service),
            error_code: None,
        };
        Ok(Self {
            profile: None,
            _profile_session: None,
            mode: mode.into(),
            current: RwLock::new(current),
            maintenance_destination: RwLock::new(None),
        })
    }
    fn context_for(&self, current: &Selected, registry: Option<&Registry>) -> InstallationContext {
        let installations = match (&self.profile, registry) {
            (Some(profile), Some(registry)) => registry
                .installations
                .iter()
                .map(|entry| {
                    let path = root(&profile.root, entry);
                    InstallationInfo {
                        id: entry.id.clone(),
                        kind: entry.kind.clone(),
                        created_at: entry.created_at,
                        path: path.to_string_lossy().into(),
                        available: private_directory(&path).is_ok(),
                    }
                })
                .collect(),
            _ => vec![InstallationInfo {
                id: current.entry.id.clone(),
                kind: current.entry.kind.clone(),
                created_at: current.entry.created_at,
                path: current.path.to_string_lossy().into(),
                available: private_directory(&current.path).is_ok(),
            }],
        };
        InstallationContext {
            selection_token: current.token.clone(),
            active_id: current.entry.id.clone(),
            mode: self.mode.clone(),
            installations,
            error_code: current.error_code.clone(),
        }
    }
    pub fn context(&self) -> Result<InstallationContext> {
        let current = self.current.try_read().map_err(|_| err("BUSY"))?;
        if let Some(profile) = &self.profile {
            let _lock = profile_lock(profile)?;
            let (registry, _) =
                load(&profile.root)?.ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            if !registry
                .installations
                .iter()
                .any(|entry| entry.id == current.entry.id)
            {
                return Err(err("INSTALLATION_SELECTION_INVALID"));
            }
            Ok(self.context_for(&current, Some(&registry)))
        } else {
            Ok(self.context_for(&current, None))
        }
    }
    /// The read guard spans the entire IPC task; selection cannot race any in-flight request.
    pub fn with_current<T>(
        &self,
        token: &str,
        task: impl FnOnce(&LifecycleService) -> Result<T>,
    ) -> Result<T> {
        let current = self.current.try_read().map_err(|_| err("BUSY"))?;
        if !uuid(token) || current.token != token {
            return Err(err("INSTALLATION_SELECTION_CHANGED"));
        }
        let service = current.service.as_ref().ok_or_else(|| {
            err(current
                .error_code
                .as_deref()
                .unwrap_or("INSTALLATION_ROOT_UNAVAILABLE"))
        })?;
        private_directory(&current.path)?;
        task(service)
    }
    // Diagnosis must remain available when ordinary startup rejects an incomplete candidate.
    fn with_retry_diagnostics<T>(
        &self,
        token: &str,
        destination_id: Option<&str>,
        task: impl FnOnce(&LifecycleService, Option<&Path>) -> Result<T>,
    ) -> Result<T> {
        let current = self.current.try_read().map_err(|_| err("BUSY"))?;
        if !uuid(token) || current.token != token {
            return Err(err("INSTALLATION_SELECTION_CHANGED"));
        }
        let source = LifecycleService::open_retry_diagnostics(current.path.clone())?;
        if let Some(profile) = &self.profile {
            let _profile = profile_lock(profile)?;
            let (registry, _) =
                load(&profile.root)?.ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            let entry = registry
                .installations
                .iter()
                .find(|e| e.id == current.entry.id)
                .ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            if registry.active_id != entry.id || root(&profile.root, entry) != current.path {
                return Err(err("INSTALLATION_SELECTION_CHANGED"));
            }
            let destination = match destination_id {
                None => None,
                Some(id) => {
                    if !uuid(id) || id == entry.id {
                        return Err(err("RETRY_DESTINATION_INVALID"));
                    }
                    let target = registry
                        .installations
                        .iter()
                        .find(|e| e.id == id)
                        .ok_or_else(|| err("RETRY_DESTINATION_INVALID"))?;
                    let path = root(&profile.root, target);
                    private_directory(&path)?;
                    Some(path)
                }
            };
            task(&source, destination.as_deref())
        } else {
            if destination_id.is_some() {
                return Err(err("INSTALLATION_SELECTION_DISABLED"));
            }
            task(&source, None)
        }
    }
    pub fn retry_diagnostic_history(
        &self,
        token: &str,
    ) -> Result<Vec<super::retry::MaintenanceRetry>> {
        self.with_retry_diagnostics(token, None, |s, _| s.retry_diagnostic_history())
    }
    pub fn diagnose_retry(
        &self,
        token: &str,
        retry_id: &str,
        destination_id: Option<&str>,
    ) -> Result<super::retry::RetryDiagnosis> {
        self.with_retry_diagnostics(token, destination_id, |s, destination| {
            s.diagnose_maintenance_retry(retry_id, destination)
        })
    }
    pub fn reconcile_retry(
        &self,
        token: &str,
        retry_id: &str,
        destination_id: Option<&str>,
        preserve: bool,
    ) -> Result<super::retry::RetryRecoveryReceipt> {
        if !preserve {
            return Err(err("RETRY_ACK_REQUIRED"));
        }
        self.with_retry_diagnostics(token, destination_id, |s, destination| {
            s.reconcile_maintenance_retry(retry_id, destination, preserve)
        })
    }
    /// Keep selection and registry locked while using a distinct registered root.
    /// Raw paths never enter this desktop adapter. The core enforces freshness.
    fn with_retry_destination<T>(
        &self,
        token: &str,
        destination_id: &str,
        task: impl FnOnce(
            &LifecycleService,
            &LifecycleService,
            &dyn Fn(&str) -> Result<()>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.with_current(token, |source| {
            if !uuid(destination_id) {
                return Err(err("INSTALLATION_SELECTION_INVALID"));
            }
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("INSTALLATION_SELECTION_DISABLED"))?;
            let _profile = profile_lock(profile)?;
            let (registry, _) =
                load(&profile.root)?.ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            let source_entry = registry
                .installations
                .iter()
                .find(|entry| root(&profile.root, entry) == source.root)
                .ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            if registry.active_id != source_entry.id {
                return Err(err("INSTALLATION_SELECTION_CHANGED"));
            }
            let entry = registry
                .installations
                .iter()
                .find(|entry| entry.id == destination_id && entry.id != source_entry.id)
                .ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
            let path = root(&profile.root, entry);
            private_directory(&path)?;
            super::restoration::fresh_root(&path)?;
            let destination = LifecycleService::bound_existing(path.clone())?;
            {
                let mut route = self
                    .maintenance_destination
                    .try_write()
                    .map_err(|_| err("BUSY"))?;
                if route.is_some() {
                    return Err(err("BUSY"));
                }
                *route = Some(MaintenanceRoute {
                    path,
                    audit_id: None,
                });
            }
            let _route = DestinationLease(&self.maintenance_destination);
            let link = |id: &str| {
                if !uuid(id) {
                    return Err(err("STATE_INVALID"));
                }
                let mut route = self
                    .maintenance_destination
                    .write()
                    .map_err(|_| err("BUSY"))?;
                let route = route.as_mut().ok_or_else(|| err("STATE_INVALID"))?;
                if route.audit_id.is_some() {
                    return Err(err("STATE_INVALID"));
                }
                route.audit_id = Some(id.into());
                Ok(())
            };
            task(source, &destination, &link)
        })
    }
    pub fn retry_restoration(
        &self,
        token: &str,
        destination_id: &str,
        target: &str,
        input: RestorationRetryOptions<'_>,
    ) -> Result<super::retry::RetryReceipt<super::restoration::RestorationReceipt>> {
        self.with_retry_destination(token, destination_id, |source, destination, link| {
            source.retry_restoration_observed(
                target,
                super::retry::RestorationRetryInput {
                    destination,
                    image: input.image,
                    key: input.key,
                    archive: input.archive,
                    port: input.port,
                    preserve_candidates: input.preserve_candidates,
                    fresh_installation: input.fresh_installation,
                },
                link,
            )
        })
    }
    fn retry_child(
        source: &LifecycleService,
        route: &MaintenanceRoute,
    ) -> Result<(LifecycleService, String)> {
        let audit = source.retry_record(route.audit_id.as_deref().ok_or_else(|| err("BUSY"))?)?;
        if audit.kind != "restoration"
            || audit.destination_root_sha256 != digest(route.path.to_string_lossy().as_bytes())
        {
            return Err(err("STATE_INVALID"));
        }
        let id = audit.new_job_id.ok_or_else(|| err("BUSY"))?;
        private_directory(&route.path)?;
        Ok((LifecycleService::bound_existing(route.path.clone())?, id))
    }
    pub fn maintenance_context(
        &self,
        token: &str,
    ) -> Result<Option<super::cancellation::MaintenanceContext>> {
        self.with_current(token, |source| {
            let route = self
                .maintenance_destination
                .try_read()
                .map_err(|_| err("BUSY"))?;
            if let Some(route) = route.as_ref() {
                let (destination, id) = Self::retry_child(source, route)?;
                let context = destination.maintenance_context()?;
                if context
                    .as_ref()
                    .is_some_and(|v| v.kind != "restoration" || v.id != id)
                {
                    return Err(err("CANCEL_TARGET_INVALID"));
                }
                Ok(context)
            } else {
                source.maintenance_context()
            }
        })
    }
    pub fn request_maintenance_cancel(
        &self,
        token: &str,
        kind: &str,
        target: &str,
        preserve: bool,
    ) -> Result<super::cancellation::MaintenanceContext> {
        self.with_current(token, |source| {
            let route = self
                .maintenance_destination
                .try_read()
                .map_err(|_| err("BUSY"))?;
            if let Some(route) = route.as_ref() {
                let (destination, id) = Self::retry_child(source, route)?;
                if kind != "restoration" || target != id {
                    return Err(err("CANCEL_TARGET_INVALID"));
                }
                destination.request_maintenance_cancel(kind, &id, preserve)
            } else {
                source.request_maintenance_cancel(kind, target, preserve)
            }
        })
    }
    pub fn create(&self, token: &str, acknowledged: bool) -> Result<InstallationContext> {
        let mut current = self.current.try_write().map_err(|_| err("BUSY"))?;
        if current.token != token || !uuid(token) {
            return Err(err("INSTALLATION_SELECTION_CHANGED"));
        }
        if !acknowledged {
            return Err(err("INSTALLATION_SELECTION_ACK_REQUIRED"));
        }
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("INSTALLATION_SELECTION_DISABLED"))?;
        let _lock = profile_lock(profile)?;
        let (mut registry, previous) =
            load(&profile.root)?.ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
        if registry.installations.len() >= 128 {
            return Err(err("INSTALLATION_SELECTION_LIMIT"));
        }
        let parent = profile.root.join("installations");
        match fs::symlink_metadata(&parent) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => new_directory(&parent)?,
            Ok(_) => private_directory(&parent)?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        }
        let entry = Entry {
            id: Uuid::new_v4().to_string(),
            kind: "recovery".into(),
            created_at: now(),
        };
        new_directory(&root(&profile.root, &entry))?;
        let next = selected(&profile.root, entry.clone());
        if next.service.is_none() {
            return Err(err(next
                .error_code
                .as_deref()
                .unwrap_or("INSTALLATION_ROOT_UNAVAILABLE")));
        }
        registry.active_id = entry.id.clone();
        registry.installations.push(entry);
        save(&profile.root, &registry, Some(&previous))?;
        *current = next;
        Ok(self.context_for(&current, Some(&registry)))
    }
    pub fn select(
        &self,
        token: &str,
        target: &str,
        acknowledged: bool,
    ) -> Result<InstallationContext> {
        let mut current = self.current.try_write().map_err(|_| err("BUSY"))?;
        if current.token != token || !uuid(token) {
            return Err(err("INSTALLATION_SELECTION_CHANGED"));
        }
        if !acknowledged {
            return Err(err("INSTALLATION_SELECTION_ACK_REQUIRED"));
        }
        if !uuid(target) {
            return Err(err("INSTALLATION_SELECTION_INVALID"));
        }
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("INSTALLATION_SELECTION_DISABLED"))?;
        let _lock = profile_lock(profile)?;
        let (mut registry, previous) =
            load(&profile.root)?.ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == target)
            .ok_or_else(|| err("INSTALLATION_SELECTION_INVALID"))?
            .clone();
        let next = selected(&profile.root, entry);
        if next.service.is_none() {
            return Err(err(next
                .error_code
                .as_deref()
                .unwrap_or("INSTALLATION_ROOT_UNAVAILABLE")));
        }
        registry.active_id = target.into();
        save(&profile.root, &registry, Some(&previous))?;
        *current = next;
        Ok(self.context_for(&current, Some(&registry)))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fn fixture() -> (PathBuf, InstallationController) {
        let path = std::env::temp_dir().join(format!("manager-selection-{}", Uuid::new_v4()));
        let controller = InstallationController::new(path.clone(), None).unwrap();
        (fs::canonicalize(path).unwrap(), controller)
    }
    fn write_private(path: &Path, bytes: &[u8]) {
        let mut f = private_options().open(path).unwrap();
        f.write_all(bytes).unwrap();
        f.sync_all().unwrap();
    }
    #[test]
    fn raw_diagnosis_survives_actual_incomplete_candidate_startup_without_rewriting_it() {
        let (profile, controller) = fixture();
        let path = profile.join("local-runtime");
        let candidate = path.join(format!("backup-creation-{}", Uuid::new_v4()));
        fs::create_dir(&candidate).unwrap();
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o700)).unwrap();
        write_private(
            &candidate.join("witness"),
            b"preserved incomplete candidate",
        );
        drop(controller);
        let reopened = InstallationController::new(profile, None).unwrap();
        let context = reopened.context().unwrap();
        assert!(context.error_code.is_some());
        assert!(
            reopened
                .retry_diagnostic_history(&context.selection_token)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(candidate.join("witness")).unwrap(),
            b"preserved incomplete candidate"
        );
        assert!(!candidate.join("job.json").exists());
        assert_eq!(
            reopened
                .retry_diagnostic_history(&Uuid::new_v4().to_string())
                .unwrap_err()
                .code,
            "INSTALLATION_SELECTION_CHANGED"
        );
    }
    #[test]
    fn diagnosis_resolves_only_registered_roots_and_fences_selection_without_startup_recovery() {
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let second = controller.create(&first.selection_token, true).unwrap();
        let current = controller
            .select(&second.selection_token, &first.active_id, true)
            .unwrap();
        let destination = profile.join("installations").join(&second.active_id);
        let witness = destination.join("witness");
        write_private(&witness, b"not a fresh root");
        controller
            .with_retry_diagnostics(
                &current.selection_token,
                Some(&second.active_id),
                |source, target| {
                    assert_eq!(source.root, profile.join("local-runtime"));
                    assert_eq!(target, Some(destination.as_path()));
                    assert_eq!(
                        controller
                            .create(&current.selection_token, true)
                            .err()
                            .unwrap()
                            .code,
                        "BUSY"
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(fs::read(&witness).unwrap(), b"not a fresh root");
        for target in [
            current.active_id.clone(),
            Uuid::new_v4().to_string(),
            "../foreign".into(),
        ] {
            assert_eq!(
                controller
                    .with_retry_diagnostics(&current.selection_token, Some(&target), |_, _| Ok(()))
                    .unwrap_err()
                    .code,
                "RETRY_DESTINATION_INVALID"
            );
        }
        assert_eq!(
            controller.context().unwrap().selection_token,
            current.selection_token
        );
        assert_eq!(
            controller
                .reconcile_retry(
                    &current.selection_token,
                    &Uuid::new_v4().to_string(),
                    None,
                    false
                )
                .unwrap_err()
                .code,
            "RETRY_ACK_REQUIRED"
        );
    }
    #[test]
    fn nonfresh_registered_retry_destination_is_refused_without_recovering_its_journal() {
        let (profile, controller) = fixture();
        let original = controller.context().unwrap();
        let destination = controller.create(&original.selection_token, true).unwrap();
        let path = profile.join("installations").join(&destination.active_id);
        let job=serde_json::to_vec(&serde_json::json!({"id":Uuid::new_v4().to_string(),"state":"running","stage":"authenticating","errorCode":null,"createdAt":1,"updatedAt":2})).unwrap();
        write_private(&path.join("restoration.json"), &job);
        let source = controller
            .select(&destination.selection_token, &original.active_id, true)
            .unwrap();
        assert_eq!(
            controller
                .with_retry_destination(
                    &source.selection_token,
                    &destination.active_id,
                    |_, _, _| Ok(())
                )
                .unwrap_err()
                .code,
            "RESTORE_FRESH_ROOT_REQUIRED"
        );
        assert_eq!(fs::read(path.join("restoration.json")).unwrap(), job);
        assert!(!path.join("maintenance-active.json").exists());
    }
    #[test]
    fn registered_retry_destination_fences_selection_and_routes_only_live_child_cancellation() {
        use std::sync::{Arc, mpsc};
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let new = controller.create(&first.selection_token, true).unwrap();
        let current = controller
            .select(&new.selection_token, &first.active_id, true)
            .unwrap();
        let controller = Arc::new(controller);
        let before = fs::read(profile.join("installation-selection.json")).unwrap();
        for destination in [
            current.active_id.as_str(),
            "../foreign",
            "12345678-1234-1234-1234-123456789012",
        ] {
            assert!(
                controller
                    .with_retry_destination::<()>(
                        &current.selection_token,
                        destination,
                        |_, _, _| panic!("invalid destination invoked")
                    )
                    .is_err()
            );
        }
        assert_eq!(
            controller
                .with_retry_destination(&first.selection_token, &new.active_id, |_, _, _| Ok(()))
                .unwrap_err()
                .code,
            "INSTALLATION_SELECTION_CHANGED"
        );
        let child = Uuid::new_v4().to_string();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = controller.clone();
        let token = current.selection_token.clone();
        let target = new.active_id.clone();
        let child_id = child.clone();
        let thread = std::thread::spawn(move || {
            worker.with_retry_destination(&token, &target, |source, destination, link| {
                assert_ne!(source.root, destination.root);
                let _operation = destination.lock()?;
                let at = now();
                write_json(
                    &destination.root,
                    "restoration.json",
                    &super::restoration::RestorationJob {
                        id: child_id.clone(),
                        state: "running".into(),
                        stage: "authenticating".into(),
                        error_code: None,
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                let parent = Uuid::new_v4().to_string();
                write_json(
                    &source.root,
                    &format!("maintenance-retry-{parent}.json"),
                    &super::retry::MaintenanceRetry {
                        id: parent.clone(),
                        target_id: Uuid::new_v4().to_string(),
                        kind: "restoration".into(),
                        state: "running".into(),
                        new_job_id: Some(child_id.clone()),
                        original_job_sha256: "a".repeat(64),
                        destination_root_sha256: digest(
                            destination.root.to_string_lossy().as_bytes(),
                        ),
                        error_code: None,
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                link(&parent)?;
                destination.begin_maintenance("restoration", &child_id, "authenticating")?;
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        ready_rx.recv().unwrap();
        assert_eq!(
            controller
                .select(&current.selection_token, &new.active_id, true)
                .err()
                .unwrap()
                .code,
            "BUSY"
        );
        let context = controller
            .maintenance_context(&current.selection_token)
            .unwrap()
            .unwrap();
        assert_eq!(context.id, child);
        let cancel = controller
            .request_maintenance_cancel(&current.selection_token, "restoration", &child, true)
            .unwrap();
        assert_eq!(cancel.state, "requested");
        assert!(
            controller
                .with_current(&current.selection_token, |s| s.maintenance_context())
                .unwrap()
                .is_none()
        );
        release_tx.send(()).unwrap();
        thread.join().unwrap().unwrap();
        assert!(
            controller
                .maintenance_context(&current.selection_token)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fs::read(profile.join("installation-selection.json")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(
                profile
                    .join("installations")
                    .join(&new.active_id)
                    .join(format!("maintenance-{child}.json"))
            )
            .map(
                |bytes| serde_json::from_slice::<super::super::cancellation::MaintenanceContext>(
                    &bytes
                )
                .unwrap()
                .state
            )
            .unwrap(),
            "requested"
        );
    }
    #[test]
    fn retry_route_refuses_foreign_live_job_before_and_after_parent_link() {
        use std::sync::{Arc, mpsc};
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let destination = controller.create(&first.selection_token, true).unwrap();
        let current = controller
            .select(&destination.selection_token, &first.active_id, true)
            .unwrap();
        let controller = Arc::new(controller);
        let foreign = Uuid::new_v4().to_string();
        let intended = Uuid::new_v4().to_string();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let worker = controller.clone();
        let token = current.selection_token.clone();
        let destination_id = destination.active_id.clone();
        let foreign_id = foreign.clone();
        let thread = std::thread::spawn(move || {
            worker.with_retry_destination(&token, &destination_id, |source, destination, link| {
                let _operation = destination.lock()?;
                let at = now();
                write_json(
                    &destination.root,
                    "restoration.json",
                    &super::restoration::RestorationJob {
                        id: foreign_id.clone(),
                        state: "running".into(),
                        stage: "authenticating".into(),
                        error_code: None,
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                destination.begin_maintenance("restoration", &foreign_id, "authenticating")?;
                ready_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                let parent = Uuid::new_v4().to_string();
                write_json(
                    &source.root,
                    &format!("maintenance-retry-{parent}.json"),
                    &super::retry::MaintenanceRetry {
                        id: parent.clone(),
                        target_id: Uuid::new_v4().to_string(),
                        kind: "restoration".into(),
                        state: "running".into(),
                        new_job_id: Some(intended.clone()),
                        original_job_sha256: "a".repeat(64),
                        destination_root_sha256: digest(
                            destination.root.to_string_lossy().as_bytes(),
                        ),
                        error_code: None,
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                link(&parent)?;
                ready_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                Ok(())
            })
        });
        ready_rx.recv().unwrap();
        let root = profile.join("installations").join(&destination.active_id);
        let job_before = fs::read(root.join("restoration.json")).unwrap();
        let context_before = fs::read(root.join(format!("maintenance-{foreign}.json"))).unwrap();
        assert_eq!(
            controller
                .maintenance_context(&current.selection_token)
                .unwrap_err()
                .code,
            "BUSY"
        );
        assert_eq!(
            controller
                .request_maintenance_cancel(&current.selection_token, "restoration", &foreign, true)
                .unwrap_err()
                .code,
            "BUSY"
        );
        continue_tx.send(()).unwrap();
        ready_rx.recv().unwrap();
        assert_eq!(
            controller
                .maintenance_context(&current.selection_token)
                .unwrap_err()
                .code,
            "CANCEL_TARGET_INVALID"
        );
        assert_eq!(
            controller
                .request_maintenance_cancel(&current.selection_token, "restoration", &foreign, true)
                .unwrap_err()
                .code,
            "CANCEL_TARGET_INVALID"
        );
        assert_eq!(fs::read(root.join("restoration.json")).unwrap(), job_before);
        assert_eq!(
            fs::read(root.join(format!("maintenance-{foreign}.json"))).unwrap(),
            context_before
        );
        assert!(
            !root
                .join(format!("maintenance-cancel-{foreign}.json"))
                .exists()
        );
        continue_tx.send(()).unwrap();
        thread.join().unwrap().unwrap();
        assert!(
            controller
                .maintenance_context(&current.selection_token)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn create_select_and_restart_preserve_original_bytes_and_reject_stale_tokens() {
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let original = PathBuf::from(&first.installations[0].path);
        write_private(
            &original.join("runtime.env"),
            b"synthetic credential marker",
        );
        assert_eq!(
            controller
                .create(&first.selection_token, false)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_ACK_REQUIRED"
        );
        let new = controller.create(&first.selection_token, true).unwrap();
        assert_ne!(new.active_id, first.active_id);
        assert_ne!(new.selection_token, first.selection_token);
        assert_eq!(
            fs::metadata(profile.join("installation-selection.json"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        let fresh = controller
            .with_current(&new.selection_token, |service| {
                service.restoration_context()
            })
            .unwrap();
        assert!(fresh.fresh);
        assert_eq!(
            controller
                .with_current(&first.selection_token, |_| Ok(()))
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_CHANGED"
        );
        assert_eq!(
            controller
                .select(&new.selection_token, "../foreign", true)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_INVALID"
        );
        let old = controller
            .select(&new.selection_token, &first.active_id, true)
            .unwrap();
        assert_ne!(old.selection_token, first.selection_token);
        assert_eq!(
            controller
                .with_current(&first.selection_token, |_| Ok(()))
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_CHANGED"
        );
        let reopened = InstallationController::new(profile.clone(), None).unwrap();
        let context = reopened.context().unwrap();
        assert_eq!(context.active_id, first.active_id);
        assert_eq!(context.installations.len(), 2);
        assert_eq!(
            fs::read(original.join("runtime.env")).unwrap(),
            b"synthetic credential marker"
        );
        assert!(profile.join("installations").join(&new.active_id).is_dir());
        for p in fs::read_dir(profile.join("selection-history")).unwrap() {
            let p = p.unwrap().path();
            let b = fs::read(&p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy();
            assert!(name.ends_with(&format!("-{}.json", digest(&b))));
            valid(&serde_json::from_slice::<Registry>(&b).unwrap()).unwrap();
        }
    }
    #[test]
    fn in_flight_request_prevents_switch_or_creation_and_never_calls_a_stale_writer() {
        use std::sync::{Arc, mpsc};
        let (profile, controller) = fixture();
        let controller = Arc::new(controller);
        let first = controller.context().unwrap();
        let before = fs::read(profile.join("installation-selection.json")).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = controller.clone();
        let token = first.selection_token.clone();
        let thread = std::thread::spawn(move || {
            worker.with_current(&token, |_| {
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        ready_rx.recv().unwrap();
        assert_eq!(
            controller
                .create(&first.selection_token, true)
                .err()
                .unwrap()
                .code,
            "BUSY"
        );
        assert_eq!(
            controller
                .select(&first.selection_token, &first.active_id, true)
                .err()
                .unwrap()
                .code,
            "BUSY"
        );
        assert_eq!(
            before,
            fs::read(profile.join("installation-selection.json")).unwrap()
        );
        assert!(!profile.join("installations").exists());
        release_tx.send(()).unwrap();
        thread.join().unwrap().unwrap();
        let new = controller.create(&first.selection_token, true).unwrap();
        let called = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(
            controller
                .with_current(&first.selection_token, |_| {
                    called.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_CHANGED"
        );
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
        assert_ne!(new.active_id, first.active_id);
    }
    #[test]
    fn missing_selected_directory_can_create_new_recovery_without_recreating_original() {
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let original = PathBuf::from(&first.installations[0].path);
        write_private(&original.join("operator-data"), b"retained original");
        drop(controller);
        let retained = profile.join("preserved-original");
        fs::rename(&original, &retained).unwrap();
        let reopened = InstallationController::new(profile.clone(), None).unwrap();
        let missing = reopened.context().unwrap();
        assert_eq!(missing.active_id, first.active_id);
        assert!(!missing.installations[0].available);
        assert_eq!(
            missing.error_code.as_deref(),
            Some("INSTALLATION_ROOT_UNAVAILABLE")
        );
        assert!(!original.exists());
        let next = reopened.create(&missing.selection_token, true).unwrap();
        assert_ne!(next.active_id, first.active_id);
        assert!(!original.exists());
        assert_eq!(
            fs::read(retained.join("operator-data")).unwrap(),
            b"retained original"
        );
        assert_eq!(
            reopened
                .select(&next.selection_token, &first.active_id, true)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_ROOT_UNAVAILABLE"
        );
    }
    #[test]
    fn two_windows_keep_current_binding_and_merge_registered_spaces() {
        let (profile, one) = fixture();
        let two = InstallationController::new(profile.clone(), None).unwrap();
        let initial_one = one.context().unwrap();
        let initial_two = two.context().unwrap();
        let first = one.create(&initial_one.selection_token, true).unwrap();
        let other = two.context().unwrap();
        assert_eq!(other.active_id, initial_two.active_id);
        assert_eq!(other.selection_token, initial_two.selection_token);
        assert_eq!(other.installations.len(), 2);
        let second = two.create(&initial_two.selection_token, true).unwrap();
        assert_ne!(second.active_id, first.active_id);
        let current = one.context().unwrap();
        assert_eq!(current.active_id, first.active_id);
        assert_eq!(current.installations.len(), 3);
        let reopened = InstallationController::new(profile, None)
            .unwrap()
            .context()
            .unwrap();
        assert_eq!(reopened.active_id, second.active_id);
    }
    #[test]
    fn corrupt_or_untrusted_registry_never_overwrites_or_creates_a_candidate() {
        use std::os::unix::fs::symlink;
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let path = profile.join("installation-selection.json");
        let original = fs::read(&path).unwrap();
        let mut value: Value = serde_json::from_slice(&original).unwrap();
        value["externalPath"] = serde_json::json!("/foreign/private/data");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let corrupt = fs::read(&path).unwrap();
        assert_eq!(
            controller
                .create(&first.selection_token, true)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_INVALID"
        );
        assert_eq!(corrupt, fs::read(&path).unwrap());
        assert!(!profile.join("installations").exists());
        fs::write(&path, &original).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(controller.context().is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let alias = profile.join("retained-alias");
        fs::hard_link(&path, &alias).unwrap();
        assert!(controller.context().is_err());
        fs::remove_file(&alias).unwrap();
        let retained = profile.join("retained-registry");
        fs::rename(&path, &retained).unwrap();
        symlink(&retained, &path).unwrap();
        assert!(controller.context().is_err());
        assert_eq!(original, fs::read(&retained).unwrap());
    }
    #[test]
    fn selected_runtime_symlink_and_registry_lock_alias_are_rejected() {
        use std::os::unix::fs::symlink;
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let original = PathBuf::from(&first.installations[0].path);
        let retained = profile.join("retained-runtime");
        fs::rename(&original, &retained).unwrap();
        symlink(&retained, &original).unwrap();
        let context = controller.context().unwrap();
        assert!(!context.installations[0].available);
        let invoked = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(
            controller
                .with_current(&first.selection_token, |_| {
                    invoked.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
                .unwrap_err()
                .code,
            "INSTALLATION_ROOT_UNAVAILABLE"
        );
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            controller
                .select(&first.selection_token, &first.active_id, true)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_ROOT_UNAVAILABLE"
        );
        let alias = profile.join("retained-lock");
        fs::hard_link(profile.join("operation.lock"), &alias).unwrap();
        assert_eq!(
            controller.context().err().unwrap().code,
            "INSTALLATION_SELECTION_INVALID"
        );
    }
    #[test]
    fn explicit_development_root_is_pinned_without_creating_profile() {
        let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
        let profile = parent.join(format!("unused-profile-{}", Uuid::new_v4()));
        let root = parent.join(format!("pinned-runtime-{}", Uuid::new_v4()));
        let controller = InstallationController::new(profile.clone(), Some(root.clone())).unwrap();
        let context = controller.context().unwrap();
        assert_eq!(context.mode, "override");
        assert_eq!(context.installations[0].path, root.to_string_lossy());
        assert!(!profile.exists());
        assert_eq!(
            controller
                .create(&context.selection_token, true)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_SELECTION_DISABLED"
        );
        assert!(
            controller
                .with_current(&context.selection_token, |s| s.restoration_context())
                .unwrap()
                .fresh
        );
    }
    #[test]
    fn previous_selection_snapshot_restores_pointer_and_preserves_all_spaces() {
        let (profile, controller) = fixture();
        let first = controller.context().unwrap();
        let previous = fs::read(profile.join("installation-selection.json")).unwrap();
        let new = controller.create(&first.selection_token, true).unwrap();
        drop(controller);
        let p = fs::read_dir(profile.join("selection-history"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = fs::read(&p).unwrap();
        assert_eq!(bytes, previous);
        assert!(
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!("-{}.json", digest(&bytes)))
        );
        // Simulate closed-app operator metadata restoration, not a runtime data restore.
        fs::write(profile.join("installation-selection.json"), bytes).unwrap();
        let restored = InstallationController::new(profile.clone(), None)
            .unwrap()
            .context()
            .unwrap();
        assert_eq!(restored.active_id, first.active_id);
        assert!(profile.join("installations").join(new.active_id).is_dir());
    }
}
