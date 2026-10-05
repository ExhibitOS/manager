// SPDX-License-Identifier: Apache-2.0
//! NTFS identity/ACL/namespace and record locks for the future Windows Store.
//! Whole-Store platform/durability qualification remains a separate gate.
use super::*;
use crate::windows_private::{ParentDirectory, PrivateDirectory};

pub(super) struct NativeRoot {
    directory: PrivateDirectory,
}
impl NativeRoot {
    pub(super) fn create(path: &Path) -> Result<Self, Error> {
        if fs::symlink_metadata(path).is_ok() {
            return Err(Error::TrustExists);
        }
        let directory = PrivateDirectory::create(path).map_err(|_| invalid())?;
        let result = Self { directory };
        result.check()?;
        Ok(result)
    }
    pub(super) fn inspect(path: &Path) -> Result<Self, Error> {
        let directory = PrivateDirectory::inspect(path).map_err(|_| invalid())?;
        let result = Self { directory };
        result.check()?;
        Ok(result)
    }
    pub(super) fn check(&self) -> Result<(), Error> {
        self.directory.check().map_err(|_| invalid())
    }
    pub(super) fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.check()?;
        let mut reader = self.directory.read_record(name).map_err(|_| invalid())?;
        let bytes = reader
            .read_bounded(MAX_RECORD as usize)
            .map_err(|_| invalid())?;
        if bytes.is_empty() {
            return Err(invalid());
        }
        reader.check().map_err(|_| invalid())?;
        self.check()?;
        Ok(bytes)
    }
    pub(super) fn validate_pending(&self, name: &str) -> Result<(), Error> {
        let id = name
            .strip_prefix("pending-")
            .and_then(|n| n.strip_suffix(".json"))
            .ok_or_else(invalid)?;
        if !installations::uuid(id) {
            return Err(invalid());
        }
        self.check()?;
        let mut reader = self.directory.read_record(name).map_err(|_| invalid())?;
        let _ = reader
            .read_bounded(MAX_RECORD as usize)
            .map_err(|_| invalid())?;
        reader.check().map_err(|_| invalid())?;
        self.check()
    }
    pub(super) fn lock(&self) -> Result<File, Error> {
        self.check()?;
        let file = self
            .directory
            .lock_record("trust.lock")
            .map_err(|_| invalid())?;
        file.try_lock_exclusive().map_err(|_| Error::TrustBusy)?;
        self.directory
            .check_record(&file, "trust.lock")
            .map_err(|_| invalid())?;
        Ok(file)
    }
}
pub(super) fn namespace(profile: &Path, installation: &str) -> Result<(PathBuf, String), Error> {
    if !profile.is_absolute() || (installation != "default" && !installations::uuid(installation)) {
        return Err(invalid());
    }
    let parent =
        ParentDirectory::inspect(profile.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let name = profile
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(invalid)?;
    let child = parent.logical_child_key(name).map_err(|_| invalid())?;
    // Parent native ID and folded ASCII leaf bind both existing and missing
    // replacement profiles. Path spelling/case cannot create another trust floor.
    let id =
        hash(format!("ExhibitOS-release-trust-windows-v1\0{child}\0{installation}").as_bytes());
    let root = parent.path().join(format!(".exhibitos-release-trust-{id}"));
    parent.check().map_err(|_| invalid())?;
    Ok((root, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fresh() -> PrivateDirectory {
        let parent = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        PrivateDirectory::create(&parent.join(format!(
            "exhibitos-trust-root-test-{}",
            uuid::Uuid::new_v4()
        )))
        .unwrap()
    }
    fn cleanup(parent: PrivateDirectory) {
        let path = parent.path().to_owned();
        parent.check().unwrap();
        drop(parent);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn windows_trust_root_case_alias_and_missing_profile_share_namespace_without_creation() {
        let parent = fresh();
        let path = parent.path().join("Profile");
        let first = namespace(&path, "default").unwrap();
        assert_eq!(
            first,
            namespace(&parent.path().join("pROFILE"), "default").unwrap()
        );
        assert!(!path.exists());
        assert!(!first.0.exists());
        assert_ne!(
            first,
            namespace(&path, &uuid::Uuid::new_v4().to_string()).unwrap()
        );
        assert!(namespace(&path, "../foreign").is_err());
        cleanup(parent);
    }
    #[test]
    fn windows_trust_root_lock_is_exclusive_and_pins_root_until_drop() {
        let parent = fresh();
        let path = parent.path().join("trust");
        let root = NativeRoot::create(&path).unwrap();
        let lock = root.lock().unwrap();
        let second = NativeRoot::inspect(&path).unwrap();
        assert_eq!(second.lock().err().unwrap(), Error::TrustBusy);
        assert!(fs::rename(&path, parent.path().join("moved")).is_err());
        assert!(matches!(NativeRoot::create(&path), Err(Error::TrustExists)));
        drop(lock);
        let another = second.lock().unwrap();
        drop(another);
        drop(second);
        drop(root);
        cleanup(parent);
    }
    #[test]
    fn windows_trust_root_read_refuses_writer_hardlink_empty_and_oversized_records() {
        let parent = fresh();
        let path = parent.path().join("trust");
        let root = NativeRoot::create(&path).unwrap();
        drop(
            root.directory
                .write_new_record("one.json", b"synthetic generation", MAX_RECORD as usize)
                .unwrap(),
        );
        assert_eq!(root.read("one.json").unwrap(), b"synthetic generation");
        let writer = fs::OpenOptions::new()
            .write(true)
            .open(path.join("one.json"))
            .unwrap();
        assert_eq!(root.read("one.json"), Err(Error::TrustInvalid));
        drop(writer);
        fs::hard_link(path.join("one.json"), path.join("alias.json")).unwrap();
        assert_eq!(root.read("one.json"), Err(Error::TrustInvalid));
        drop(
            root.directory
                .write_new_record("empty.json", b"", MAX_RECORD as usize)
                .unwrap(),
        );
        assert_eq!(root.read("empty.json"), Err(Error::TrustInvalid));
        let file = root.directory.create_record("large.json").unwrap();
        file.set_len(MAX_RECORD + 1).unwrap();
        drop(file);
        assert_eq!(root.read("large.json"), Err(Error::TrustInvalid));
        assert_eq!(
            fs::read(path.join("one.json")).unwrap(),
            b"synthetic generation"
        );
        drop(root);
        cleanup(parent);
    }
    #[test]
    fn windows_trust_root_acl_change_is_refused_without_adoption() {
        let parent = fresh();
        let path = parent.path().join("trust");
        let root = NativeRoot::create(&path).unwrap();
        drop(
            root.directory
                .write_new_record("witness", b"preserve", 1024)
                .unwrap(),
        );
        let status = crate::process_window::background_command("icacls.exe")
            .arg(&path)
            .args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(root.check(), Err(Error::TrustInvalid));
        assert!(NativeRoot::inspect(&path).is_err());
        assert_eq!(fs::read(path.join("witness")).unwrap(), b"preserve");
        drop(root);
        cleanup(parent);
    }
    #[test]
    fn windows_trust_root_pending_partial_bytes_do_not_become_committed_records() {
        let parent = fresh();
        let path = parent.path().join("trust");
        let root = NativeRoot::create(&path).unwrap();
        let name = format!("pending-{}.json", uuid::Uuid::new_v4());
        drop(
            root.directory
                .write_new_record(&name, b"", MAX_RECORD as usize)
                .unwrap(),
        );
        root.validate_pending(&name).unwrap();
        assert_eq!(root.read(&name), Err(Error::TrustInvalid));
        assert!(root.validate_pending("pending-invalid.json").is_err());
        let file = fs::OpenOptions::new()
            .write(true)
            .open(path.join(&name))
            .unwrap();
        file.set_len(MAX_RECORD + 1).unwrap();
        drop(file);
        assert!(root.validate_pending(&name).is_err());
        assert_eq!(
            fs::metadata(path.join(&name)).unwrap().len(),
            MAX_RECORD + 1
        );
        drop(root);
        cleanup(parent);
    }
}
