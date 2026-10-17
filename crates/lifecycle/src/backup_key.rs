// SPDX-License-Identifier: Apache-2.0
//! User-initiated external service-backup key creation. Never returns key bytes.
use super::*;
fn failure(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:"새 백업 키를 저장하지 못했어요. 설치·백업 폴더 밖의 다른 새 파일을 선택하세요. 기존 키는 바꾸지 않습니다. 저장 오류 후 파일이 생겼다면 보존하고 상태를 확인하세요.".into()}
}
#[cfg(target_os = "macos")]
pub(super) fn create(path: &Path, profile: &Path, installation: &Path) -> Result<()> {
    create_with_hook(path, profile, installation, || {})
}
#[cfg(target_os = "macos")]
fn create_with_hook(
    path: &Path,
    profile: &Path,
    installation: &Path,
    before_open: impl FnOnce(),
) -> Result<()> {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
    };
    let parent = path
        .parent()
        .ok_or_else(|| failure("BACKUP_KEY_PATH_INVALID"))?;
    let text = path
        .to_str()
        .ok_or_else(|| failure("BACKUP_KEY_PATH_INVALID"))?;
    if !path.is_absolute()
        || text.len() > 2048
        || text.chars().any(|c| c.is_control() || c == ',')
        || path.starts_with(profile)
        || path.starts_with(installation)
        || fs::canonicalize(parent).ok().as_deref() != Some(parent)
    {
        return Err(failure("BACKUP_KEY_PATH_INVALID"));
    }
    // Keep a key outside both local installations and externally copied archive trees.
    for ancestor in parent.ancestors() {
        if ancestor
            .join("manifest.gcm")
            .try_exists()
            .map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?
            || (ancestor.join("runtime.env").exists() && ancestor.join("installed.json").exists())
        {
            return Err(failure("BACKUP_KEY_PATH_INVALID"));
        }
    }
    let name = std::ffi::CString::new(
        path.file_name()
            .ok_or_else(|| failure("BACKUP_KEY_PATH_INVALID"))?
            .as_bytes(),
    )
    .map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?;
    before_open();
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)
        .map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?;
    let before = directory
        .metadata()
        .map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?;
    if before.uid() != unsafe { libc::geteuid() } {
        return Err(failure("BACKUP_KEY_PATH_INVALID"));
    }
    // Resolve the opened descriptor itself, not just the textual parent after a raced open.
    let mut actual = [0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_GETPATH, actual.as_mut_ptr()) } < 0 {
        return Err(failure("BACKUP_KEY_PATH_INVALID"));
    }
    let length = actual
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| failure("BACKUP_KEY_PATH_INVALID"))?;
    let actual = PathBuf::from(std::ffi::OsString::from_vec(actual[..length].to_vec()));
    if actual != parent || actual.starts_with(profile) || actual.starts_with(installation) {
        return Err(failure("BACKUP_KEY_PATH_INVALID"));
    }
    for ancestor in actual.ancestors() {
        if ancestor
            .join("manifest.gcm")
            .try_exists()
            .map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?
            || (ancestor.join("runtime.env").exists() && ancestor.join("installed.json").exists())
        {
            return Err(failure("BACKUP_KEY_PATH_INVALID"));
        }
    }
    let opened = fs::symlink_metadata(parent).map_err(|_| failure("BACKUP_KEY_PATH_INVALID"))?;
    if opened.file_type().is_symlink()
        || opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || fs::canonicalize(parent).ok().as_deref() != Some(parent)
    {
        return Err(failure("BACKUP_KEY_PATH_INVALID"));
    }
    let mut bytes = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| failure("BACKUP_KEY_RANDOM_UNAVAILABLE"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        bytes.fill(0);
        return Err(failure("BACKUP_KEY_NEW_FILE_REQUIRED"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = file.write_all(&bytes).and_then(|()| file.sync_all());
    bytes.fill(0);
    result.map_err(|_| failure("BACKUP_KEY_WRITE_UNCERTAIN"))?;
    let current =
        fs::symlink_metadata(parent).map_err(|_| failure("BACKUP_KEY_WRITE_UNCERTAIN"))?;
    if current.dev() != before.dev()
        || current.ino() != before.ino()
        || fs::canonicalize(parent).ok().as_deref() != Some(parent)
    {
        return Err(failure("BACKUP_KEY_WRITE_UNCERTAIN"));
    }
    directory
        .sync_all()
        .map_err(|_| failure("BACKUP_KEY_WRITE_UNCERTAIN"))?;
    super::maintenance::input_path(path, false).map_err(|_| failure("BACKUP_KEY_WRITE_UNCERTAIN"))
}
#[cfg(not(target_os = "macos"))]
pub(super) fn create(_path: &Path, _profile: &Path, _installation: &Path) -> Result<()> {
    Err(failure("BACKUP_KEY_PLATFORM_UNAVAILABLE"))
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn raced_parent_symlink_never_writes_a_key_into_protected_profile() {
        let root = std::env::temp_dir().join(format!("exhibitos-key-race-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root=fs::canonicalize(root).unwrap();
        let profile = root.join("profile");
        fs::create_dir(&profile).unwrap();
        let external = root.join("external");
        fs::create_dir(&external).unwrap();
        let moved = root.join("moved");
        assert!(
            create_with_hook(&external.join("new.key"), &profile, &profile, || {
                fs::rename(&external, &moved).unwrap();
                symlink(&profile, &external).unwrap();
            })
            .is_err()
        );
        assert!(!profile.join("new.key").exists());
        assert!(!moved.join("new.key").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn explicit_new_key_is_private_and_never_replaces_existing_or_archive_data() {
        let root = std::env::temp_dir().join(format!("exhibitos-key-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root=fs::canonicalize(root).unwrap();
        let profile = root.join("profile");
        fs::create_dir(&profile).unwrap();
        let external = root.join("external");
        fs::create_dir(&external).unwrap();
        let key = external.join("new.key");
        create(&key, &profile, &profile).unwrap();
        let bytes = fs::read(&key).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(
            fs::metadata(&key).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        assert!(create(&key, &profile, &profile).is_err());
        assert!(fs::read(&key).unwrap() == bytes);
        assert!(create(&profile.join("bad.key"), &profile, &profile).is_err());
        let alias = root.join("alias");
        symlink(&external, &alias).unwrap();
        assert!(create(&alias.join("bad.key"), &profile, &profile).is_err());
        let linked = external.join("link.key");
        symlink(&key, &linked).unwrap();
        assert!(create(&linked, &profile, &profile).is_err());
        assert!(fs::read(&key).unwrap() == bytes);
        fs::write(external.join("manifest.gcm"), b"synthetic archive witness").unwrap();
        assert!(create(&external.join("bad.key"), &profile, &profile).is_err());
        assert!(!external.join("bad.key").exists());
        assert_eq!(
            fs::read(external.join("manifest.gcm")).unwrap(),
            b"synthetic archive witness"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
