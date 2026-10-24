// SPDX-License-Identifier: Apache-2.0
//! Native service-backup adapter. Existing authenticated archive/PG codecs are
//! reused; the controller/profile and Session/root fences stay held together.
use super::*;
use crate::native_runtime::Session;
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub key_file: PathBuf,
    pub destination: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub backup_id: String,
    pub manifest_sha256: String,
    pub state: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMap {
    pub format: u32,
    pub source_root: PathBuf,
    pub owner_kind: String,
    pub manifest_sha256: String,
    pub journal_names: BTreeMap<String, String>,
}
pub(crate) struct Secret(pub(crate) Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
struct Source {
    root: PathBuf,
    name: String,
    limit: u64,
    sha256: Option<String>,
}
pub(crate) fn resource_budget(source_free: u64, destination_free: u64) -> Result<u64> {
    // Keep the existing 6GiB floor. A separate 2GiB reserve bounds the
    // <=100,000-entry codec metadata, <=32MiB runtime, config/journals and
    // their plaintext/authenticated copies; DB/object copies use the codec's
    // existing inventory-derived four-copy dump budget instead.
    source_free
        .min(destination_free)
        .checked_sub(8 * 1024 * 1024 * 1024)
        .filter(|n| *n >= 4 * 1024 * 1024)
        .map(|n| n.min(9_007_199_254_740_991))
        .ok_or_else(|| err("NATIVE_BACKUP_RESOURCE_BUDGET"))
}
struct ConfigurationDirectory {
    path: PathBuf,
    file: File,
    stamp: [u64; 9],
}
fn directory_stamp(m: &fs::Metadata) -> [u64; 9] {
    [
        m.dev(),
        m.ino(),
        m.mode() as u64,
        m.uid() as u64,
        m.nlink(),
        m.mtime() as u64,
        m.mtime_nsec() as u64,
        m.ctime() as u64,
        m.ctime_nsec() as u64,
    ]
}
impl ConfigurationDirectory {
    fn capture(path: &Path) -> Result<Self> {
        private_directory(path)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| err("NATIVE_BACKUP_SOURCE_INVALID"))?;
        let stamp = directory_stamp(
            &file
                .metadata()
                .map_err(|_| err("NATIVE_BACKUP_SOURCE_INVALID"))?,
        );
        let value = Self {
            path: path.into(),
            file,
            stamp,
        };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        private_directory(&self.path)?;
        let check_identity = || -> Result<()> {
            let held = self
                .file
                .metadata()
                .map_err(|_| err("NATIVE_BACKUP_SOURCE_CHANGED"))?;
            let named = fs::symlink_metadata(&self.path)
                .map_err(|_| err("NATIVE_BACKUP_SOURCE_CHANGED"))?;
            if !held.is_dir()
                || !named.is_dir()
                || directory_stamp(&held) != self.stamp
                || directory_stamp(&named) != self.stamp
            {
                return Err(err("NATIVE_BACKUP_SOURCE_CHANGED"));
            }
            Ok(())
        };
        check_identity()?;
        let names: Vec<_> = fs::read_dir(&self.path)
            .map_err(|_| err("NATIVE_BACKUP_SOURCE_INVALID"))?
            .take(2)
            .map(|e| e.map(|v| v.file_name()))
            .collect::<std::io::Result<_>>()
            .map_err(|_| err("NATIVE_BACKUP_SOURCE_INVALID"))?;
        if names.len() != 1 || names[0] != "freeze-signing-key.json" {
            return Err(err("NATIVE_BACKUP_CONFIGURATION_UNSUPPORTED"));
        }
        check_identity()
    }
}
fn optional(root: &Path, name: &str, limit: u64) -> Result<Option<Secret>> {
    match fs::symlink_metadata(root.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(err("NATIVE_BACKUP_SOURCE_INVALID")),
        Ok(_) => installation_backup::source_bytes(root, name, limit, true)
            .map(Secret)
            .map(Some),
    }
}
pub(crate) fn text(path: &Path) -> Result<String> {
    path.to_str()
        .filter(|v| v.len() <= 4096 && !v.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))
}
fn absent_destination(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?;
    if !path.is_absolute()
        || path.file_name().is_none()
        || fs::canonicalize(parent).ok().as_deref() != Some(parent)
    {
        return Err(err("NATIVE_BACKUP_PATH_INVALID"));
    }
    private_directory(parent)?;
    text(path)?;
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(err("NATIVE_BACKUP_DESTINATION_EXISTS")),
    }
}
impl Source {
    fn check(&self) -> Result<()> {
        let current = optional(&self.root, &self.name, self.limit)?;
        if current.as_ref().map(|v| digest(&v.0)) != self.sha256 {
            return Err(err("NATIVE_BACKUP_SOURCE_CHANGED"));
        }
        Ok(())
    }
}
impl InstallationController {
    /// Synchronous core only. Host/GUI must first stop its exporter and prevent
    /// new HTTP authoring commands. The profile lock excludes journal writes and
    /// selection changes for the whole snapshot/codec/verification operation.
    pub fn create_native_backup(
        &self,
        token: &str,
        session: &mut Session,
        input: Input,
    ) -> Result<Receipt> {
        self.with_current(token, |service| {
            if !session.belongs_to(service) { return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE")); }
            session.backup_available()?;
            session.backup_source_current()?;
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _profile = profile_lock(profile)?;
            let prepared = service.native_prepared()?.ok_or_else(|| err("NATIVE_NOT_READY"))?;
            absent_destination(&input.destination)?;
            let budget = resource_budget(
                fs2::available_space(&service.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?,
                fs2::available_space(input.destination.parent().ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?).map_err(|_| err("STORAGE_UNAVAILABLE"))?,
            )?;
            if input.destination.starts_with(&profile.root) || input.destination.starts_with(&service.root)
                || !input.key_file.is_absolute() || input.key_file.starts_with(&profile.root)
                || input.key_file.starts_with(&service.root) || input.key_file.starts_with(&input.destination)
                || fs::canonicalize(&input.key_file).ok().as_deref() != Some(input.key_file.as_path())
            { return Err(err("NATIVE_BACKUP_PATH_INVALID")); }
            let parent = input.key_file.parent().ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?;
            private_directory(parent)?;
            let key_name = input.key_file.file_name().and_then(|v| v.to_str()).ok_or_else(|| err("NATIVE_BACKUP_PATH_INVALID"))?;
            let key = Secret(installation_backup::source_bytes(parent, key_name, 32, true)?);
            if key.0.len() != 32 { return Err(err("BACKUP_KEY_INVALID")); }
            drop(key);
            // No snapshot is taken until the actual owned API/worker is reaped.
            session.quiesce_for_backup()?;
            let id = Uuid::new_v4().to_string();
            let directory = service.root.join(format!("native-backup-{id}"));
            new_directory(&directory)?;
            let scratch = directory.join("scratch"); new_directory(&scratch)?;
            write_json(&directory, "job.json", &serde_json::json!({"format":1,"id":id,"state":"preparing"}))?;
            let result: Result<Receipt> = (|| {
                let mut sources = Vec::new();
                let mut configuration = BTreeMap::new();
                let mut deployment = BTreeMap::new();
                let mut journal_names = BTreeMap::new();
                let mut capture = |root: &Path, name: &str, limit: u64, logical: &str, required: bool, journal: bool| -> Result<()> {
                    let bytes = optional(root, name, limit)?;
                    if required && bytes.is_none() { return Err(err("NATIVE_BACKUP_SOURCE_REQUIRED")); }
                    let sha256 = bytes.as_ref().map(|v| digest(&v.0));
                    if let Some(bytes) = bytes {
                        write_private_new(&directory, logical, &bytes.0, limit as usize)?;
                        let path = text(&directory.join(logical))?;
                        if journal {
                            deployment.insert(format!("native-authoring/{logical}"), serde_json::json!({"path":path,"bytes":bytes.0.len(),"sha256":sha256}));
                            journal_names.insert(logical.to_string(), name.to_string());
                        } else { configuration.insert(logical.to_string(), path); }
                    }
                    sources.push(Source {root: root.into(), name:name.into(), limit, sha256});
                    Ok(())
                };
                capture(&service.root, "runtime.env", 8192, "native-runtime.env", true, false)?;
                let owner = if prepared.kind == "personal" {
                    service.personal_identity()?;
                    "native-personal.json"
                } else { "native-installation.json" };
                capture(&service.root, owner, 4096, owner, true, false)?;
                // The current runtime creates this one signing configuration.
                // Unknown files refuse rather than silently omitting user data.
                let config = service.root.join("config");
                let config_directory = ConfigurationDirectory::capture(&config)?;
                capture(&config, "freeze-signing-key.json", 65536, "freeze-signing-key.json", true, false)?;
                let suffix = digest(service.root.as_os_str().as_encoded_bytes());
                for (name, limit) in [("document",2_200_000),("upload",128*1024),("publication",2_200_000),("static-export",65536)] {
                    capture(&profile.root, &format!("native-{name}-{suffix}.json"), limit, &format!("{name}.json"), false, true)?;
                }
                let map = SourceMap {format:1, source_root:service.root.clone(), owner_kind:prepared.kind.clone(), manifest_sha256:prepared.manifest_sha256.clone(), journal_names};
                write_json(&directory, "native-source.json", &map)?;
                configuration.insert("native-source.json".into(), text(&directory.join("native-source.json"))?);
                let config_json = serde_json::to_string(&configuration).map_err(|_| err("STATE_INVALID"))?;
                let deployment_json = serde_json::to_string(&deployment).map_err(|_| err("STATE_INVALID"))?;
                config_directory.check()?;
                for source in &sources { source.check()?; }
                write_json(&directory, "job.json", &serde_json::json!({"format":1,"id":id,"state":"creating"}))?;
                let args = vec!["create".into(),"--key-file".into(),text(&input.key_file)?,"--destination".into(),text(&input.destination)?,"--quiesced".into()];
                let created = session.backup_tool(args, config_json.clone(), deployment_json.clone(), budget, &scratch)?;
                let backup_id = created["backupId"].as_str().filter(|v| uuid(v)).ok_or_else(|| err("NATIVE_BACKUP_FAILED"))?.to_string();
                let verified = directory.join("verified");
                session.backup_tool(vec!["verify".into(),"--key-file".into(),text(&input.key_file)?,"--source".into(),text(&input.destination)?,"--destination".into(),text(&verified)?], config_json.clone(), deployment_json.clone(), budget, &scratch)?;
                let manifest = installation_backup::source_bytes(&verified, "manifest.json", 16*1024*1024, true)?;
                let manifest_sha = digest(&manifest);
                for command in ["check-source-inventory", "check-source-configuration"] {
                    session.backup_tool(vec![command.into(),"--manifest-file".into(),text(&verified.join("manifest.json"))?,"--manifest-sha256".into(),manifest_sha.clone(),"--quiesced".into()], config_json.clone(), deployment_json.clone(), budget, &scratch)?;
                }
                for source in &sources { source.check()?; }
                config_directory.check()?;
                session.backup_source_current()?;
                let complete: Value = serde_json::from_slice(&installation_backup::source_bytes(&input.destination,"complete.json",65536,true)?).map_err(|_| err("NATIVE_BACKUP_FAILED"))?;
                let hash = complete["manifestSha256"].as_str().filter(|v| hash_valid(v)).ok_or_else(|| err("NATIVE_BACKUP_FAILED"))?;
                if complete["id"].as_str() != Some(&backup_id) { return Err(err("NATIVE_BACKUP_FAILED")); }
                let receipt = Receipt {id:id.clone(),backup_id,manifest_sha256:hash.into(),state:"completed".into()};
                write_json(&directory,"result.json",&receipt)?;
                Ok(receipt)
            })();
            if let Err(error) = &result {
                write_json(&directory,"job.json",&serde_json::json!({"format":1,"id":id,"state":"failed","errorCode":error.code}))?;
            }
            result
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    #[test]
    fn resource_budget_keeps_floor_on_both_filesystems() {
        let gib = 1024u64 * 1024 * 1024;
        assert!(resource_budget(6 * gib, 100 * gib).is_err());
        assert!(resource_budget(100 * gib, 8 * gib).is_err());
        assert_eq!(resource_budget(10 * gib, 9 * gib).unwrap(), gib);
        assert_eq!(
            resource_budget(8 * gib + 4 * 1024 * 1024, 100 * gib).unwrap(),
            4 * 1024 * 1024
        );
    }
    #[test]
    fn snapshot_witness_detects_changed_added_and_linked_journals_without_overwrite() {
        let root =
            std::env::temp_dir().join(format!("exhibitos-native-backup-source-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let absent = Source {
            root: root.clone(),
            name: "journal.json".into(),
            limit: 4096,
            sha256: None,
        };
        absent.check().unwrap();
        write_private_new(&root, "journal.json", b"saved", 4096).unwrap();
        assert!(absent.check().is_err());
        let current = Source {
            sha256: Some(digest(b"saved")),
            ..absent
        };
        current.check().unwrap();
        fs::write(root.join("journal.json"), b"changed").unwrap();
        assert!(current.check().is_err());
        assert_eq!(fs::read(root.join("journal.json")).unwrap(), b"changed");
        fs::rename(root.join("journal.json"), root.join("original.json")).unwrap();
        symlink(root.join("original.json"), root.join("journal.json")).unwrap();
        assert!(current.check().is_err());
        assert_eq!(fs::read(root.join("original.json")).unwrap(), b"changed");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn configuration_witness_rejects_added_files_and_directory_replacement() {
        let root = std::env::temp_dir().join(format!(
            "exhibitos-native-backup-directory-{}",
            Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let config = root.join("config");
        new_directory(&config).unwrap();
        write_private_new(
            &config,
            "freeze-signing-key.json",
            b"synthetic signing configuration",
            4096,
        )
        .unwrap();
        let witness = ConfigurationDirectory::capture(&config).unwrap();
        write_private_new(&config, "new.json", b"preserve", 4096).unwrap();
        assert!(witness.check().is_err());
        assert_eq!(fs::read(config.join("new.json")).unwrap(), b"preserve");
        let clean = root.join("clean-config");
        new_directory(&clean).unwrap();
        write_private_new(
            &clean,
            "freeze-signing-key.json",
            b"synthetic signing configuration",
            4096,
        )
        .unwrap();
        let clean_witness = ConfigurationDirectory::capture(&clean).unwrap();
        clean_witness.check().unwrap();
        fs::rename(&clean, root.join("original-clean-config")).unwrap();
        new_directory(&clean).unwrap();
        write_private_new(
            &clean,
            "freeze-signing-key.json",
            b"synthetic signing configuration",
            4096,
        )
        .unwrap();
        assert!(clean_witness.check().is_err());
        assert_eq!(fs::read(config.join("new.json")).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }
}
