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
    _profile_session: Option<File>,
    mode: String,
    current: RwLock<Selected>,
}
pub(crate) fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub(crate) fn private_directory(path: &Path) -> Result<()> {
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
fn save(profile: &Path, registry: &Registry, previous: Option<&[u8]>) -> Result<()> {
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
        let mut file = private_options()
            .open(history.join(format!(
                "{}-{}-{}.json",
                now(),
                Uuid::new_v4(),
                digest(bytes)
            )))
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
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
impl InstallationController {
    pub fn new(profile: PathBuf, override_root: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = override_root {
            return Self::pinned(path, "override");
        }
        if cfg!(windows) {
            return Self::pinned(profile.join("local-runtime"), "platform-unverified");
        }
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
        let session = super::profile_backup::session_lock(&profile, false)?;
        let profile = LifecycleService::new(profile)?;
        let profile = LifecycleService {
            root: fs::canonicalize(&profile.root).map_err(|_| err("STATE_UNAVAILABLE"))?,
        };
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
