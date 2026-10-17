// SPDX-License-Identifier: Apache-2.0
//! Export only a completed job's encrypted archive. Never copies keys or decrypted workspaces.
use super::*;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReceipt {
    pub job_id: String,
    pub backup_id: String,
    pub path: String,
    pub files: u64,
    pub bytes: u64,
}
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:"백업 사본을 보관하지 못했어요. 원래 백업과 키는 유지됩니다. 설치·백업 폴더 밖의 다른 위치나 남은 공간을 확인하고 이 백업의 보관만 다시 시도하세요. 실패한 부분 사본은 보존되며 완료 사본이 아닙니다.".into()}
}
#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{MetadataExt, OpenOptionsExt},
        },
    };
    fn c(value: &std::ffi::OsStr) -> Result<std::ffi::CString> {
        std::ffi::CString::new(value.as_bytes()).map_err(|_| failure("BACKUP_EXPORT_PATH_INVALID"))
    }
    fn openat(parent: &File, name: &str, directory: bool, write: bool) -> Result<File> {
        let name = c(std::ffi::OsStr::new(name))?;
        let flags = libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if write {
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
            } else {
                libc::O_RDONLY | libc::O_NONBLOCK
            }
            | if directory { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(failure("BACKUP_EXPORT_IO"));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    pub(super) fn bounded_hash(file: &mut File, expected: u64) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut bytes = [0u8; 65536];
        loop {
            let limit =
                usize::try_from((expected - count).saturating_add(1).min(bytes.len() as u64))
                    .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
            let n = file
                .read(&mut bytes[..limit])
                .map_err(|_| failure("BACKUP_EXPORT_IO"))?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > expected {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
            hash.update(&bytes[..n]);
        }
        if count != expected {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        Ok(hash.finalize().into())
    }
    fn check(directory: &File, path: &Path) -> Result<()> {
        let mut actual = [0u8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_GETPATH, actual.as_mut_ptr()) } < 0 {
            return Err(failure("BACKUP_EXPORT_PATH_CHANGED"));
        }
        let len = actual
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| failure("BACKUP_EXPORT_PATH_CHANGED"))?;
        let actual = PathBuf::from(std::ffi::OsString::from_vec(actual[..len].to_vec()));
        let a = directory
            .metadata()
            .map_err(|_| failure("BACKUP_EXPORT_PATH_CHANGED"))?;
        let b = fs::symlink_metadata(path).map_err(|_| failure("BACKUP_EXPORT_PATH_CHANGED"))?;
        if actual != path
            || a.uid() != unsafe { libc::geteuid() }
            || b.file_type().is_symlink()
            || a.dev() != b.dev()
            || a.ino() != b.ino()
            || fs::canonicalize(path).ok().as_deref() != Some(path)
        {
            return Err(failure("BACKUP_EXPORT_PATH_CHANGED"));
        }
        Ok(())
    }
    fn allowed(parent: &Path, profile: &Path, root: &Path) -> Result<()> {
        let text = parent
            .to_str()
            .ok_or_else(|| failure("BACKUP_EXPORT_PATH_INVALID"))?;
        if !parent.is_absolute()
            || text.len() > 1900
            || text.chars().any(|c| c.is_control() || c == ',')
            || parent.starts_with(profile)
            || parent.starts_with(root)
            || fs::canonicalize(parent).ok().as_deref() != Some(parent)
        {
            return Err(failure("BACKUP_EXPORT_PATH_INVALID"));
        }
        for a in parent.ancestors() {
            if a.join("manifest.gcm").exists()
                || (a.join("runtime.env").exists() && a.join("installed.json").exists())
            {
                return Err(failure("BACKUP_EXPORT_PATH_INVALID"));
            }
        }
        Ok(())
    }
    fn regular(file: &File) -> Result<fs::Metadata> {
        let m = file
            .metadata()
            .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
        if !m.is_file()
            || m.nlink() != 1
            || m.mode() & 0o7777 != 0o600
            || m.uid() != unsafe { libc::geteuid() }
            || m.len() == 0
            || m.len() > 8 * 1024 * 1024 * 1024
        {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        Ok(m)
    }
    fn mkdir(parent: &File, name: &str) -> Result<File> {
        let n = c(std::ffi::OsStr::new(name))?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), n.as_ptr(), 0o700) } != 0 {
            return Err(failure("BACKUP_EXPORT_IO"));
        }
        let directory = openat(parent, name, true, false)?;
        let metadata = directory
            .metadata()
            .map_err(|_| failure("BACKUP_EXPORT_IO"))?;
        if metadata.mode() & 0o7777 != 0o700 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(failure("BACKUP_EXPORT_PRIVATE_PERMISSIONS"));
        }
        Ok(directory)
    }
    pub(super) fn copy(
        root: &Path,
        profile: &Path,
        parent: &Path,
        receipt: &backup_creation::BackupCreationReceipt,
        before_open: impl FnOnce(),
    ) -> Result<ExportReceipt> {
        allowed(parent, profile, root)?;
        before_open();
        let output = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)
            .map_err(|_| failure("BACKUP_EXPORT_PATH_INVALID"))?;
        check(&output, parent)?;
        allowed(parent, profile, root)?;
        let source_path = root.join(format!("backup-creation-{}/archive", receipt.id));
        let source = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&source_path)
            .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
        check(&source, &source_path)?;
        let encrypted = openat(&source, "files", true, false)?;
        check(&encrypted, &source_path.join("files"))?;
        if source
            .metadata()
            .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
            .mode()
            & 0o077
            != 0
            || encrypted
                .metadata()
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
                .mode()
                & 0o077
                != 0
        {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        let mut names = Vec::new();
        for e in fs::read_dir(&source_path).map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))? {
            let n = e
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
                .file_name()
                .into_string()
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
            if n == "files" {
                continue;
            }
            if !["complete.json", "writing.json", "manifest.gcm"].contains(&n.as_str()) {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
            names.push((false, n));
        }
        if names.len() != 3 {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        for e in fs::read_dir(source_path.join("files"))
            .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
        {
            let n = e
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
                .file_name()
                .into_string()
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
            if n.len() != 12
                || !n.ends_with(".gcm")
                || !n.as_bytes()[..8].iter().all(u8::is_ascii_digit)
            {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
            names.push((true, n));
        }
        if names.len() != receipt.files as usize + 3 || names.len() > 10003 {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        names.sort();
        let mut total = 0u64;
        for (in_files, name) in &names {
            let f = openat(
                if *in_files { &encrypted } else { &source },
                name,
                false,
                false,
            )?;
            total = total
                .checked_add(regular(&f)?.len())
                .ok_or_else(|| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
        }
        let manifest: BundleManifest = read_json(&root.join("bundle/manifest.json"))?;
        let required = total
            .checked_add(manifest.minimum_free_bytes)
            .ok_or_else(|| failure("STORAGE_INSUFFICIENT"))?;
        if fs2::available_space(parent).map_err(|_| failure("STORAGE_UNAVAILABLE"))? < required {
            return Err(failure("STORAGE_INSUFFICIENT"));
        }
        let name = format!("ExhibitOS-backup-{}-{}", receipt.backup_id, Uuid::new_v4());
        let stage = format!(".{name}.partial");
        let folder = mkdir(&output, &stage)?;
        let files = mkdir(&folder, "files")?;
        let mut count = 0u64;
        let mut digests = std::collections::BTreeMap::new();
        for (in_files, name) in &names {
            check(&output, parent)?;
            allowed(parent, profile, root)?;
            check(&source, &source_path)?;
            check(&encrypted, &source_path.join("files"))?;
            let mut input = openat(
                if *in_files { &encrypted } else { &source },
                name,
                false,
                false,
            )?;
            let before = regular(&input)?;
            let mut target = openat(if *in_files { &files } else { &folder }, name, false, true)?;
            let created = target.metadata().map_err(|_| failure("BACKUP_EXPORT_IO"))?;
            if created.mode() & 0o7777 != 0o600
                || created.nlink() != 1
                || created.uid() != unsafe { libc::geteuid() }
            {
                return Err(failure("BACKUP_EXPORT_PRIVATE_PERMISSIONS"));
            }
            check(&folder, &parent.join(&stage))?;
            check(&files, &parent.join(&stage).join("files"))?;
            let mut hash = Sha256::new();
            let mut copied = 0u64;
            let mut bytes = [0u8; 65536];
            loop {
                let n = input
                    .read(&mut bytes)
                    .map_err(|_| failure("BACKUP_EXPORT_IO"))?;
                if n == 0 {
                    break;
                }
                copied += n as u64;
                if copied > before.len() {
                    return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
                }
                target
                    .write_all(&bytes[..n])
                    .map_err(|_| failure("BACKUP_EXPORT_IO"))?;
                hash.update(&bytes[..n]);
            }
            target.sync_all().map_err(|_| failure("BACKUP_EXPORT_IO"))?;
            let after = regular(&input)?;
            if copied != before.len()
                || before.dev() != after.dev()
                || before.ino() != after.ino()
                || before.mtime() != after.mtime()
                || before.mtime_nsec() != after.mtime_nsec()
                || before.ctime() != after.ctime()
                || before.ctime_nsec() != after.ctime_nsec()
            {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
            let mut verify = openat(if *in_files { &files } else { &folder }, name, false, false)?;
            let expected: [u8; 32] = hash.finalize().into();
            if expected != bounded_hash(&mut verify, copied)? {
                return Err(failure("BACKUP_EXPORT_COPY_CHANGED"));
            }
            digests.insert((*in_files, name.clone()), (copied, expected));
            count += copied;
        }
        // Recheck every source byte after the whole copy, including files copied first.
        let mut current = Vec::new();
        for e in fs::read_dir(&source_path).map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))? {
            let name = e
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
                .file_name()
                .into_string()
                .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?;
            if name != "files" {
                current.push((false, name));
            }
        }
        for e in fs::read_dir(source_path.join("files"))
            .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
        {
            current.push((
                true,
                e.map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?
                    .file_name()
                    .into_string()
                    .map_err(|_| failure("BACKUP_EXPORT_SOURCE_CHANGED"))?,
            ));
        }
        current.sort();
        if current != names {
            return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
        }
        for ((in_files, name), (size, expected)) in digests {
            let mut f = openat(
                if in_files { &encrypted } else { &source },
                &name,
                false,
                false,
            )?;
            if regular(&f)?.len() != size {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
            if bounded_hash(&mut f, size)? != expected {
                return Err(failure("BACKUP_EXPORT_SOURCE_CHANGED"));
            }
        }
        files
            .sync_all()
            .and_then(|()| folder.sync_all())
            .map_err(|_| failure("BACKUP_EXPORT_IO"))?;
        check(&output, parent)?;
        allowed(parent, profile, root)?;
        check(&folder, &parent.join(&stage))?;
        check(&files, &parent.join(&stage).join("files"))?;
        check(&source, &source_path)?;
        let a = c(std::ffi::OsStr::new(&stage))?;
        let b = c(std::ffi::OsStr::new(&name))?;
        if unsafe {
            libc::renameatx_np(
                output.as_raw_fd(),
                a.as_ptr(),
                output.as_raw_fd(),
                b.as_ptr(),
                libc::RENAME_EXCL,
            )
        } != 0
        {
            return Err(failure("BACKUP_EXPORT_IO"));
        }
        output.sync_all().map_err(|_| failure("BACKUP_EXPORT_IO"))?;
        check(&output, parent)?;
        check(&folder, &parent.join(&name))?;
        Ok(ExportReceipt {
            job_id: receipt.id.clone(),
            backup_id: receipt.backup_id.clone(),
            path: parent
                .join(name)
                .to_str()
                .ok_or_else(|| failure("BACKUP_EXPORT_PATH_INVALID"))?
                .into(),
            files: receipt.files + 3,
            bytes: count,
        })
    }
    #[cfg(test)]
    pub(super) fn test_copy(
        root: &Path,
        profile: &Path,
        parent: &Path,
        receipt: &backup_creation::BackupCreationReceipt,
        hook: impl FnOnce(),
    ) -> Result<ExportReceipt> {
        copy(root, profile, parent, receipt, hook)
    }
}
impl LifecycleService {
    pub(crate) fn export_backup(
        &self,
        profile: &Path,
        id: &str,
        parent: &Path,
    ) -> Result<ExportReceipt> {
        let _operation = self.lock()?;
        if Uuid::parse_str(id).ok().is_none_or(|v| v.to_string() != id) {
            return Err(failure("BACKUP_EXPORT_JOB_INVALID"));
        }
        let workspace = self.root.join(format!("backup-creation-{id}"));
        let job: backup_creation::BackupCreationJob = read_json(&workspace.join("job.json"))?;
        let receipt: backup_creation::BackupCreationReceipt =
            read_json(&workspace.join("receipt.json"))?;
        if job.id != id
            || job.state != "completed"
            || job.stage != "complete"
            || job.error_code.is_some()
            || receipt.id != id
            || receipt.operation != "created-and-authenticated"
            || !receipt.writers_paused
            || Uuid::parse_str(&receipt.backup_id).is_err()
            || receipt.files == 0
            || receipt.files > 10000
        {
            return Err(failure("BACKUP_EXPORT_JOB_INVALID"));
        }
        #[cfg(target_os = "macos")]
        return native::copy(&self.root, profile, parent, &receipt, || {});
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, parent, receipt);
            Err(failure("BACKUP_EXPORT_PLATFORM_UNAVAILABLE"))
        }
    }
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (
        PathBuf,
        PathBuf,
        PathBuf,
        backup_creation::BackupCreationReceipt,
    ) {
        let base = std::env::temp_dir().join(format!("backup-export-test-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let base = fs::canonicalize(base).unwrap();
        let root = base.join("profile");
        let parent = base.join("external");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&parent).unwrap();
        let id = Uuid::new_v4().to_string();
        let receipt = backup_creation::BackupCreationReceipt {
            id: id.clone(),
            backup_id: Uuid::new_v4().to_string(),
            operation: "created-and-authenticated".into(),
            files: 1,
            image: format!("sha256:{}", "a".repeat(64)),
            authenticated_manifest_sha256: "b".repeat(64),
            writers_paused: true,
            at: 1,
        };
        let source = root.join(format!("backup-creation-{id}/archive"));
        fs::create_dir_all(root.join("bundle")).unwrap();
        fs::write(root.join("bundle/manifest.json"),serde_json::to_vec(&serde_json::json!({"schemaVersion":"1.0.0-draft.1","bundleId":Uuid::new_v4().to_string(),"version":"synthetic","protocolVersion":"1","composeSha256":"a".repeat(64),"projectName":"synthetic","services":["platform","database"],"images":[],"ports":[13200],"openUrl":"http://127.0.0.1:13200","readinessUrl":"http://127.0.0.1:13200/api/v1/readiness","minimumFreeBytes":0})).unwrap()).unwrap();
        fs::create_dir_all(source.join("files")).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(source.join("files"), fs::Permissions::from_mode(0o700)).unwrap();
        for (name, bytes) in [
            ("complete.json", b"synthetic-complete".as_slice()),
            ("writing.json", b"synthetic-writing"),
            ("manifest.gcm", b"opaque-encrypted-manifest"),
            ("files/00000000.gcm", b"opaque-encrypted-file"),
        ] {
            fs::write(source.join(name), bytes).unwrap();
            fs::set_permissions(source.join(name), fs::Permissions::from_mode(0o600)).unwrap()
        }
        (base, root, parent, receipt)
    }
    #[test]
    fn encrypted_copy_is_exact_private_and_repeat_never_overwrites() {
        let (base, root, parent, r) = fixture();
        let a = native::test_copy(&root, &root, &parent, &r, || {}).unwrap();
        let b = native::test_copy(&root, &root, &parent, &r, || {}).unwrap();
        assert_ne!(a.path, b.path);
        assert_eq!(a.files, 4);
        for name in [
            "complete.json",
            "writing.json",
            "manifest.gcm",
            "files/00000000.gcm",
        ] {
            let original = root.join(format!("backup-creation-{}/archive/{name}", r.id));
            for output in [&a.path, &b.path] {
                let path = Path::new(output).join(name);
                assert_eq!(fs::read(&original).unwrap(), fs::read(&path).unwrap());
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o7777,
                    0o600
                )
            }
        }
        assert_eq!(
            fs::metadata(&a.path).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        fs::remove_dir_all(base).unwrap()
    }
    #[test]
    fn protected_or_raced_destination_cannot_receive_ciphertext() {
        let (base, root, parent, r) = fixture();
        assert!(native::test_copy(&root, &root, &root, &r, || {}).is_err());
        let moved = base.join("moved");
        assert!(
            native::test_copy(&root, &root, &parent, &r, || {
                fs::rename(&parent, &moved).unwrap();
                symlink(&root, &parent).unwrap()
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(base).unwrap()
    }
    #[test]
    fn unexpected_or_linked_file_never_exports_decrypted_data() {
        let (base, root, parent, r) = fixture();
        let path = root.join(format!(
            "backup-creation-{}/archive/files/00000000.gcm",
            r.id
        ));
        fs::remove_file(&path).unwrap();
        let secret = base.join("private.key");
        fs::write(&secret, b"secret must stay outside output").unwrap();
        symlink(&secret, &path).unwrap();
        assert!(native::test_copy(&root, &root, &parent, &r, || {}).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        assert_eq!(
            fs::read(secret).unwrap(),
            b"secret must stay outside output"
        );
        fs::remove_dir_all(base).unwrap()
    }
    #[test]
    fn failed_or_missing_job_never_exports_an_archive() {
        let (base, root, parent, r) = fixture();
        let workspace = root.join(format!("backup-creation-{}", r.id));
        let job = backup_creation::BackupCreationJob {
            id: r.id.clone(),
            operation: "create".into(),
            state: "failed".into(),
            stage: "copying-authenticated-archive".into(),
            error_code: Some("HELPER_FAILED".into()),
            created_at: 1,
            updated_at: 1,
        };
        write_json(&workspace, "job.json", &job).unwrap();
        write_json(&workspace, "receipt.json", &r).unwrap();
        let service = LifecycleService { root: root.clone() };
        assert!(service.export_backup(&root, &r.id, &parent).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap()
    }
    #[test]
    fn fifo_is_rejected_before_any_blocking_read_or_output() {
        let (base, root, parent, r) = fixture();
        let fifo = root.join(format!(
            "backup-creation-{}/archive/files/00000000.gcm",
            r.id
        ));
        fs::remove_file(&fifo).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(native::test_copy(&root, &root, &parent, &r, || {}).is_err());
        assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn rehash_refuses_growth_or_shortening_at_expected_boundary() {
        let (base, root, _, r) = fixture();
        let file = root.join(format!("backup-creation-{}/archive/complete.json", r.id));
        let size = fs::metadata(&file).unwrap().len();
        assert!(native::bounded_hash(&mut File::open(&file).unwrap(), size - 1).is_err());
        assert!(native::bounded_hash(&mut File::open(&file).unwrap(), size + 1).is_err());
        assert!(native::bounded_hash(&mut File::open(&file).unwrap(), size).is_ok());
        fs::remove_dir_all(base).unwrap();
    }
}
