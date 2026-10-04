// SPDX-License-Identifier: Apache-2.0
//! Native protected JSON I/O; individual handle-based replacement, not whole recovery.
use super::*;
use fs2::FileExt;

const JSON_LIMIT: usize = 4 * 1024 * 1024;
const PUBLICATION_LOCK: &str = ".exhibitos-json-publication.lock";

struct PublicationLock(File);
impl Drop for PublicationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
fn publication_lock(root: &PrivateDirectory) -> Result<PublicationLock> {
    root.check()?;
    let file = match root.create_record(PUBLICATION_LOCK) {
        Ok(file) => file,
        Err(_) => {
            let path = wide(&root.path.join(PUBLICATION_LOCK))?;
            let handle = unsafe {
                fsapi::CreateFileW(
                    path.as_ptr(),
                    0x80000000 | 0x40000000 | fsapi::READ_CONTROL,
                    fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE,
                    ptr::null(),
                    fsapi::OPEN_EXISTING,
                    fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(err("WINDOWS_PROFILE_PUBLICATION_LOCK_INVALID"));
            }
            unsafe { File::from_raw_handle(handle.cast()) }
        }
    };
    root.check_record(&file, PUBLICATION_LOCK)?;
    file.try_lock_exclusive()
        .map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
    root.check_record(&file, PUBLICATION_LOCK)?;
    Ok(PublicationLock(file))
}
fn read_public(path: &Path) -> Result<Vec<u8>> {
    let parent = path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
    let _ancestors = pin_ancestors(parent)?;
    let sid = Sid::current()?;
    let path_wide = wide(path)?;
    let handle = unsafe {
        fsapi::CreateFileW(
            path_wide.as_ptr(),
            0x80000000 | fsapi::READ_CONTROL,
            fsapi::FILE_SHARE_READ,
            ptr::null(),
            fsapi::OPEN_EXISTING,
            fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(err("WINDOWS_PROFILE_RECORD_OPEN_REFUSED"));
    }
    let mut file = unsafe { File::from_raw_handle(handle.cast()) };
    public_acl(&file, &sid)?;
    let before = identity(&file, false)?;
    if identity(&open(path, false)?, false)? != before {
        return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
    }
    let len = file
        .metadata()
        .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?
        .len();
    if len > JSON_LIMIT as u64 {
        return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(JSON_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
    public_acl(&file, &sid)?;
    if bytes.len() as u64 != len
        || identity(&file, false)? != before
        || identity(&open(path, false)?, false)? != before
    {
        return Err(err("WINDOWS_PROFILE_RECORD_CHANGED"));
    }
    Ok(bytes)
}
pub(crate) fn read_json_path(path: &Path) -> Result<Vec<u8>> {
    let parent = path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
    // Bundle manifest is redistributable input; private journals/installed state are not.
    if name == "manifest.json" && parent.file_name().is_some_and(|v| v == "bundle") {
        return read_public(path);
    }
    let root = PrivateDirectory::inspect(parent)?;
    root.read_record(name)?.read_bounded(JSON_LIMIT)
}
/// Check root, old destination and new file; rename the retained source handle.
/// Successful file sync is not proof of host-powerloss/directory durability.
pub(crate) fn write_json_root(path: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    valid_name(name)?;
    if name == PUBLICATION_LOCK {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    if bytes.len() > JSON_LIMIT {
        return Err(err("JOB_HISTORY_FULL"));
    }
    let root = PrivateDirectory::inspect(path)?;
    let _lock = publication_lock(&root)?;
    let destination = root.path.join(name);
    let replacing = match std::fs::symlink_metadata(&destination) {
        Ok(_) => {
            let verified_old = root.read_record(name)?.read_bounded(JSON_LIMIT)?;
            serde_json::from_slice::<serde_json::Value>(&verified_old)
                .map_err(|_| err("STATE_INVALID"))?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(err("WINDOWS_PROFILE_RECORD_OPEN_REFUSED")),
    };
    let temporary = format!(".exhibitos-json-{}.tmp", uuid::Uuid::new_v4());
    let mut file = root.create_record_access(&temporary, fsapi::FILE_SHARE_READ, fsapi::DELETE)?;
    file.try_lock_exclusive()
        .map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
    root.check_record(&file, &temporary)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
    root.check_record(&file, &temporary)?;
    let expected = identity(&file, false)?;
    let filename = wide(&destination)?;
    if filename.len() > 32768 {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    let size = (mem::offset_of!(fsapi::FILE_RENAME_INFO, FileName) + filename.len() * 2)
        .max(mem::size_of::<fsapi::FILE_RENAME_INFO>());
    let mut buffer = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<fsapi::FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replacing;
        (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = ((filename.len() - 1) * 2) as u32;
        ptr::copy_nonoverlapping(
            filename.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            filename.len(),
        );
    }
    root.check()?;
    if unsafe {
        fsapi::SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            fsapi::FileRenameInfo,
            buffer.as_ptr().cast(),
            size as u32,
        )
    } == 0
    {
        return Err(err("WINDOWS_PROFILE_PUBLICATION_REFUSED"));
    }
    // From here a failure means replacement may already be visible: never retry blindly.
    let verify = (|| {
        file.sync_all()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        root.check_record(&file, name)?;
        if identity(&file, false)? != expected {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    })();
    FileExt::unlock(&file).map_err(|_| err("WINDOWS_PROFILE_PUBLICATION_UNCERTAIN"))?;
    verify.map_err(|_| err("WINDOWS_PROFILE_PUBLICATION_UNCERTAIN"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fresh() -> PrivateDirectory {
        let parent = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        PrivateDirectory::create(
            &parent.join(format!("exhibitos-json-test-{}", uuid::Uuid::new_v4())),
        )
        .unwrap()
    }
    fn cleanup(root: PrivateDirectory) {
        root.check().unwrap();
        let path = root.path().to_owned();
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("exhibitos-json-test-")
        );
        let entries: Vec<_> = std::fs::read_dir(&path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        for file in &entries {
            assert!(
                std::fs::symlink_metadata(file)
                    .unwrap()
                    .file_type()
                    .is_file()
            );
        }
        drop(root);
        for file in entries {
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn native_core_json_write_read_and_replace_preserve_other_records() {
        let root = fresh();
        let original = root
            .write_new_record("unrelated", b"synthetic-only", 1024)
            .unwrap();
        drop(original);
        crate::write_json(root.path(), "job.json", &serde_json::json!({"version":1})).unwrap();
        assert_eq!(
            crate::read_json::<serde_json::Value>(&root.path().join("job.json")).unwrap()["version"],
            1
        );
        crate::write_json(root.path(), "job.json", &serde_json::json!({"version":2})).unwrap();
        assert_eq!(
            crate::read_json::<serde_json::Value>(&root.path().join("job.json")).unwrap()["version"],
            2
        );
        assert_eq!(
            root.read_record("unrelated")
                .unwrap()
                .read_bounded(1024)
                .unwrap(),
            b"synthetic-only"
        );
        cleanup(root);
    }
    #[test]
    fn active_read_handle_refuses_replace_and_preserves_previous_bytes() {
        let root = fresh();
        crate::write_json(root.path(), "job.json", &1).unwrap();
        let mut held = root.read_record("job.json").unwrap();
        assert_eq!(
            crate::write_json(root.path(), "job.json", &2)
                .unwrap_err()
                .code,
            "WINDOWS_PROFILE_PUBLICATION_REFUSED"
        );
        assert_eq!(held.read_bounded(1024).unwrap(), b"1");
        drop(held);
        assert_eq!(
            crate::read_json::<u32>(&root.path().join("job.json")).unwrap(),
            1
        );
        cleanup(root);
    }
    #[test]
    fn shared_pointer_and_busy_lock_fail_closed_without_overwrite() {
        let root = fresh();
        crate::write_json(root.path(), "job.json", &1).unwrap();
        let lock = publication_lock(&root).unwrap();
        assert_eq!(
            crate::write_json(root.path(), "job.json", &2)
                .unwrap_err()
                .code,
            "WINDOWS_PROFILE_BUSY"
        );
        drop(lock);
        let status = crate::process_window::background_command("icacls.exe")
            .arg(root.path().join("job.json"))
            .args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            crate::write_json(root.path(), "job.json", &3)
                .unwrap_err()
                .code,
            "WINDOWS_PROFILE_ACL_INVALID"
        );
        assert!(crate::read_json::<u32>(&root.path().join("job.json")).is_err());
        assert_eq!(std::fs::read(root.path().join("job.json")).unwrap(), b"1");
        cleanup(root);
    }
    #[test]
    fn malformed_private_json_and_unsafe_destination_are_not_accepted() {
        let root = fresh();
        let record = root.write_new_record("bad.json", b"{", 1024).unwrap();
        drop(record);
        assert_eq!(
            crate::read_json::<serde_json::Value>(&root.path().join("bad.json"))
                .unwrap_err()
                .code,
            "STATE_INVALID"
        );
        assert_eq!(
            crate::write_json(root.path(), "bad.json", &1)
                .unwrap_err()
                .code,
            "STATE_INVALID"
        );
        assert_eq!(std::fs::read(root.path().join("bad.json")).unwrap(), b"{");
        assert!(crate::write_json(root.path(), "../outside.json", &1).is_err());
        assert!(!root.path().parent().unwrap().join("outside.json").exists());
        cleanup(root);
    }
    #[test]
    fn public_bundle_manifest_remains_readable_without_private_acl_adoption() {
        let root = fresh();
        let bundle = root.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        std::fs::write(bundle.join("manifest.json"), b"{\"synthetic\":true}").unwrap();
        assert_eq!(
            crate::read_json::<serde_json::Value>(&bundle.join("manifest.json")).unwrap()["synthetic"],
            true
        );
        assert!(PrivateDirectory::inspect(&bundle).is_err());
        // Public ownership compatibility must not allow unrelated writers.
        let status = crate::process_window::background_command("icacls.exe")
            .arg(bundle.join("manifest.json"))
            .args(["/grant", "*S-1-1-0:W"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(crate::read_json::<serde_json::Value>(&bundle.join("manifest.json")).is_err());
        assert_eq!(
            std::fs::read(bundle.join("manifest.json")).unwrap(),
            b"{\"synthetic\":true}"
        );
        std::fs::remove_file(bundle.join("manifest.json")).unwrap();
        std::fs::remove_dir(bundle).unwrap();
        cleanup(root);
    }
}
