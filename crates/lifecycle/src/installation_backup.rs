// SPDX-License-Identifier: Apache-2.0
//! Private installation configuration snapshot for inclusion in encrypted service backups.
//! This receipt is not an encrypted backup or a database/image restore point.
use super::*;
use std::collections::BTreeMap;

const SOURCES: [(&str, &str, u64, bool); 5] = [
    (
        "bundle/manifest.json",
        "manager-bundle-manifest.json",
        1024 * 1024,
        false,
    ),
    (
        "bundle/compose.yaml",
        "manager-compose.yaml",
        256 * 1024,
        false,
    ),
    ("runtime.env", "manager-runtime.env", 8192, true),
    (
        "installed.json",
        "manager-installed.json",
        1024 * 1024,
        true,
    ),
    ("engine.json", "manager-engine.json", 64, true),
];
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationBackupReceipt {
    pub id: String,
    pub operation: String,
    pub files: u64,
    pub inventory_sha256: String,
    pub at: u64,
}
pub(crate) fn source_bytes(
    root: &Path,
    relative: &str,
    limit: u64,
    private: bool,
) -> Result<Vec<u8>> {
    let path = checked_path(root, relative)?;
    #[cfg(windows)]
    {
        let limit = usize::try_from(limit).map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
        if !private {
            let bytes = super::windows_private::PublicRecord::open(&path)?.read_bounded(limit)?;
            if bytes.is_empty() {
                return Err(err("BACKUP_SOURCE_INVALID"));
            }
            return Ok(bytes);
        }
        let _root_fence = super::windows_private::PrivateDirectory::inspect(root)?;
        let parent = path.parent().ok_or_else(|| err("BACKUP_SOURCE_INVALID"))?;
        let directory = super::windows_private::PrivateDirectory::inspect(parent)?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("BACKUP_SOURCE_INVALID"))?;
        let bytes = directory.read_record(name)?.read_bounded(limit)?;
        if bytes.is_empty() {
            return Err(err("BACKUP_SOURCE_INVALID"));
        }
        _root_fence.check()?;
        return Ok(bytes);
    }
    #[cfg(not(windows))]
    {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
        let before = file.metadata().map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
        if !before.is_file() || before.len() == 0 || before.len() > limit {
            return Err(err("BACKUP_SOURCE_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if before.nlink() != 1
                || before.uid()
                    != fs::metadata(root)
                        .map_err(|_| err("BACKUP_SOURCE_INVALID"))?
                        .uid()
                || before.permissions().mode() & 0o022 != 0
                || private && before.permissions().mode() & 0o7777 != 0o600
            {
                return Err(err("BACKUP_PRIVATE_PERMISSIONS"));
            }
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
        let after = file.metadata().map_err(|_| err("BACKUP_SOURCE_INVALID"))?;
        let path_after = fs::symlink_metadata(checked_path(root, relative)?)
            .map_err(|_| err("BACKUP_SOURCE_CHANGED"))?;
        if bytes.len() as u64 != before.len()
            || after.len() != before.len()
            || !path_after.is_file()
        {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (
                before.dev(),
                before.ino(),
                before.mtime(),
                before.mtime_nsec(),
                before.ctime(),
                before.ctime_nsec(),
                before.mode(),
                before.nlink(),
            ) != (
                after.dev(),
                after.ino(),
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec(),
                after.mode(),
                after.nlink(),
            ) || (after.dev(), after.ino()) != (path_after.dev(), path_after.ino())
            {
                return Err(err("BACKUP_SOURCE_CHANGED"));
            }
        }
        Ok(bytes)
    }
}
pub(crate) fn environment_valid(bytes: &[u8], manifest: &BundleManifest) -> Result<()> {
    let port = manifest
        .ports
        .first()
        .ok_or_else(|| err("BACKUP_CONFIGURATION_INVALID"))?;
    let value = std::str::from_utf8(bytes).map_err(|_| err("BACKUP_CONFIGURATION_INVALID"))?;
    let mut values = BTreeMap::new();
    for line in value.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| err("BACKUP_CONFIGURATION_INVALID"))?;
        if ![
            "EXHIBITOS_PORT",
            "POSTGRES_PASSWORD",
            "DATABASE_URL",
            "ADMIN_SUBJECT",
            "ADMIN_PASSWORD",
            "TENANT_ID",
        ]
        .contains(&key)
            || value.is_empty()
            || value.len() > 4096
            || value.chars().any(char::is_control)
            || values.insert(key, value).is_some()
        {
            return Err(err("BACKUP_CONFIGURATION_INVALID"));
        }
    }
    if values.len() != 6
        || values["EXHIBITOS_PORT"] != port.to_string()
        || Uuid::parse_str(values["TENANT_ID"]).is_err()
        || values["ADMIN_SUBJECT"].len() > 128
        || values["POSTGRES_PASSWORD"].len() < 12
        || values["ADMIN_PASSWORD"].len() < 12
        || values["POSTGRES_PASSWORD"].len() > 1024
        || values["ADMIN_PASSWORD"].len() > 1024
        || values["DATABASE_URL"]
            != format!(
                "postgresql://exhibitos:{}@database:5432/exhibitos",
                values["POSTGRES_PASSWORD"]
            )
    {
        return Err(err("BACKUP_CONFIGURATION_INVALID"));
    }
    Ok(())
}
impl LifecycleService {
    /// Copies only validated installation inputs; never reads artwork, generates credentials,
    /// starts/stops an engine, activates a restore or deletes earlier workspaces.
    pub fn prepare_installation_backup(&self) -> Result<InstallationBackupReceipt> {
        let _lock = self.lock()?;
        self.prepare_installation_backup_locked()
    }
    pub(crate) fn prepare_installation_backup_locked(&self) -> Result<InstallationBackupReceipt> {
        if cfg!(windows) {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        if fs::canonicalize(&self.root).map_err(|_| err("BACKUP_PATH_INVALID"))? != self.root {
            return Err(err("BACKUP_PATH_INVALID"));
        }
        let mut files = BTreeMap::new();
        for (source, name, limit, private) in SOURCES {
            files.insert(name, source_bytes(&self.root, source, limit, private)?);
        }
        let manifest: BundleManifest =
            serde_json::from_slice(&files["manager-bundle-manifest.json"])
                .map_err(|_| err("BUNDLE_INVALID"))?;
        let validated = self.manifest()?;
        let installed: BundleManifest = serde_json::from_slice(&files["manager-installed.json"])
            .map_err(|_| err("BUNDLE_INVALID"))?;
        if serde_json::to_value(&manifest).ok() != serde_json::to_value(&validated).ok()
            || serde_json::to_value(&manifest).ok() != serde_json::to_value(&installed).ok()
            || digest(&files["manager-compose.yaml"]) != manifest.compose_sha256
        {
            return Err(err("BUNDLE_CHANGED"));
        }
        let engine: String = serde_json::from_slice(&files["manager-engine.json"])
            .map_err(|_| err("STATE_INVALID"))?;
        if !matches!(engine.as_str(), "docker" | "podman")
            || manifest
                .preferred_engine
                .as_ref()
                .is_some_and(|preferred| preferred != &engine)
        {
            return Err(err("BUNDLE_CHANGED"));
        }
        environment_valid(&files["manager-runtime.env"], &manifest)?;
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("backup-preparation-{id}"));
        fs::create_dir(&workspace).map_err(|_| err("STATE_UNAVAILABLE"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&workspace, fs::Permissions::from_mode(0o700))
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        let outcome = (|| -> Result<InstallationBackupReceipt> {
            let mut inventory = BTreeMap::new();
            let mut configuration = BTreeMap::new();
            for (name, bytes) in &files {
                let path = workspace.join(name);
                let mut output = private_options()
                    .open(&path)
                    .map_err(|_| err("STATE_UNAVAILABLE"))?;
                output
                    .write_all(bytes)
                    .and_then(|_| output.sync_all())
                    .map_err(|_| err("STATE_UNAVAILABLE"))?;
                inventory.insert(
                    *name,
                    serde_json::json!({"bytes":bytes.len(),"sha256":digest(bytes)}),
                );
                configuration.insert(*name, path);
            }
            for (source, name, limit, private) in SOURCES {
                if source_bytes(&self.root, source, limit, private)? != files[name] {
                    return Err(err("BACKUP_SOURCE_CHANGED"));
                }
            }
            let encoded = serde_json::to_vec(&inventory).map_err(|_| err("STATE_INVALID"))?;
            write_json(&workspace, "inventory.json", &inventory)?;
            write_json(&workspace, "configuration-files.json", &configuration)?;
            let receipt = InstallationBackupReceipt {
                id: id.clone(),
                operation: "prepared-configuration".into(),
                files: 5,
                inventory_sha256: digest(&encoded),
                at: now(),
            };
            write_json(&workspace, "receipt.json", &receipt)?;
            Ok(receipt)
        })();
        if outcome.is_err() {
            write_json(
                &workspace,
                "failed.json",
                &serde_json::json!({"id":id,"operation":"preparation-failed","at":now()}),
            )?;
        }
        outcome
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (LifecycleService, String) {
        let path = std::env::temp_dir().join(format!("installation-backup-{}", Uuid::new_v4()));
        let _ = LifecycleService::new(path.clone()).unwrap();
        let root = fs::canonicalize(path).unwrap();
        let service = LifecycleService::new(root.clone()).unwrap();
        fs::create_dir(root.join("bundle")).unwrap();
        let mut m = crate::tests::manifest_for_detection();
        let compose = b"services: {}\n";
        m.compose_sha256 = digest(compose);
        fs::write(root.join("bundle/compose.yaml"), compose).unwrap();
        write_json(&root.join("bundle"), "manifest.json", &m).unwrap();
        write_json(&root, "installed.json", &m).unwrap();
        write_json(&root, "engine.json", &"docker").unwrap();
        service.runtime_env(&m).unwrap();
        let password = String::from_utf8(fs::read(root.join("runtime.env")).unwrap()).unwrap();
        (service, password)
    }
    #[test]
    fn actual_private_snapshot_preserves_existing_installation_and_never_returns_credentials() {
        let (service, environment) = fixture();
        let before = fs::read(service.root.join("runtime.env")).unwrap();
        let result = service.prepare_installation_backup().unwrap();
        let workspace = service
            .root
            .join(format!("backup-preparation-{}", result.id));
        assert_eq!(result.operation, "prepared-configuration");
        assert_eq!(result.files, 5);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains(&environment)
        );
        assert_eq!(
            fs::read(workspace.join("manager-runtime.env")).unwrap(),
            before
        );
        assert_eq!(fs::read(service.root.join("runtime.env")).unwrap(), before);
        let inventory: Value = read_json(&workspace.join("inventory.json")).unwrap();
        assert_eq!(
            digest(&serde_json::to_vec(&inventory).unwrap()),
            result.inventory_sha256
        );
        let mapping: BTreeMap<String, PathBuf> =
            read_json(&workspace.join("configuration-files.json")).unwrap();
        assert_eq!(mapping.len(), 5);
        assert_eq!(
            fs::metadata(&workspace).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for path in mapping.values() {
            assert!(path.starts_with(&workspace));
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_ne!(service.prepare_installation_backup().unwrap().id, result.id);
    }
    #[test]
    fn inconsistent_installed_bundle_or_compose_is_not_prepared() {
        let (service, _) = fixture();
        fs::write(service.root.join("bundle/compose.yaml"), b"changed").unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BUNDLE_CHANGED"
        );
        let (service, _) = fixture();
        let mut m: BundleManifest = read_json(&service.root.join("installed.json")).unwrap();
        m.version = "different".into();
        write_json(&service.root, "installed.json", &m).unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BUNDLE_CHANGED"
        );
    }
    #[test]
    fn public_secret_symlink_hardlink_and_operation_overlap_are_rejected() {
        let (service, _) = fixture();
        let path = service.root.join("runtime.env");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BACKUP_PRIVATE_PERMISSIONS"
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(&path, service.root.join("alias")).unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BACKUP_PRIVATE_PERMISSIONS"
        );
        let (service, _) = fixture();
        let path = service.root.join("runtime.env");
        let moved = service.root.join("retained.env");
        fs::rename(&path, &moved).unwrap();
        symlink(&moved, &path).unwrap();
        assert!(service.prepare_installation_backup().is_err());
        assert!(moved.exists());
        let (service, _) = fixture();
        let _guard = service.lock().unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BUSY"
        );
    }
    #[test]
    fn unknown_duplicate_and_changed_configuration_fail_without_rotation() {
        for suffix in ["UNKNOWN_KEY=synthetic\n", "TENANT_ID=duplicate\n"] {
            let (service, original) = fixture();
            let changed = format!("{original}{suffix}");
            fs::write(service.root.join("runtime.env"), &changed).unwrap();
            assert_eq!(
                service.prepare_installation_backup().unwrap_err().code,
                "BACKUP_CONFIGURATION_INVALID"
            );
            assert_eq!(
                fs::read_to_string(service.root.join("runtime.env")).unwrap(),
                changed
            );
        }
        let (service, original) = fixture();
        let changed = original.replace("EXHIBITOS_PORT=13200", "EXHIBITOS_PORT=13201");
        fs::write(service.root.join("runtime.env"), &changed).unwrap();
        assert_eq!(
            service.prepare_installation_backup().unwrap_err().code,
            "BACKUP_CONFIGURATION_INVALID"
        );
    }
}
