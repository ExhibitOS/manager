// SPDX-License-Identifier: Apache-2.0
//! Existing hosted/SSH resource file identity fences, shared with native publication.
use crate::LifecycleError;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
fn refused(code: &str) -> LifecycleError {
    LifecycleError { code: code.into(), guidance: "서버 작업을 완료하지 못했습니다. 기존 데이터·작업을 보존하고 연결·인증 정보·실제 상태를 확인하세요.".into() }
}
pub struct Pin {
    path: PathBuf,
    file: std::fs::File,
    metadata: std::fs::Metadata,
}
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mode() == b.mode()
            && a.nlink() == b.nlink()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}
impl Pin {
    pub(crate) fn duplicate_file(&self) -> Result<std::fs::File, LifecycleError> {
        self.check()?;
        self.file.try_clone().map_err(|_| refused("REMOTE_RUNTIME_UNAVAILABLE"))
    }
    pub fn check(&self) -> Result<(), LifecycleError> {
        let named =
            std::fs::symlink_metadata(&self.path).map_err(|_| refused("REMOTE_RUNTIME_CHANGED"))?;
        let opened = self
            .file
            .metadata()
            .map_err(|_| refused("REMOTE_RUNTIME_CHANGED"))?;
        if named.file_type().is_symlink()
            || !same(&self.metadata, &named)
            || !same(&self.metadata, &opened)
        {
            return Err(refused("REMOTE_RUNTIME_CHANGED"));
        }
        Ok(())
    }
}
pub fn bounded(path: &Path, max: u64) -> Result<(Vec<u8>, Pin), LifecycleError> {
    let before =
        std::fs::symlink_metadata(path).map_err(|_| refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > max {
        return Err(refused("REMOTE_RUNTIME_UNSAFE"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.nlink() != 1 || before.mode() & 0o022 != 0 {
            return Err(refused("REMOTE_RUNTIME_UNSAFE"));
        }
    }
    let mut file = std::fs::File::open(path).map_err(|_| refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
    let metadata = file
        .metadata()
        .map_err(|_| refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
    if !same(&before, &metadata) {
        return Err(refused("REMOTE_RUNTIME_CHANGED"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused("REMOTE_RUNTIME_UNAVAILABLE"))?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max {
        return Err(refused("REMOTE_RUNTIME_UNSAFE"));
    }
    let pin = Pin {
        path: path.into(),
        file,
        metadata,
    };
    pin.check()?;
    Ok((bytes, pin))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("exhibitos-hosted-pin-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn bounded_read_refuses_oversized_linked_and_shared_files_without_rewriting() {
        let root = Fixture::new();
        let file = root.file("input", b"source");
        assert_eq!(bounded(&file, 6).unwrap().0, b"source");
        assert!(bounded(&file, 5).is_err());
        let link = root.0.join("alias");
        symlink("input", &link).unwrap();
        assert!(bounded(&link, 6).is_err());
        std::fs::hard_link(&file, root.0.join("hard-link")).unwrap();
        assert!(bounded(&file, 6).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"source");
    }
    #[test]
    fn retained_pin_detects_same_bytes_replaced_at_the_name() {
        let root = Fixture::new();
        let file = root.file("input", b"unchanged bytes");
        let (_, pin) = bounded(&file, 64).unwrap();
        pin.check().unwrap();
        let replacement = root.file("replacement", b"unchanged bytes");
        std::fs::rename(replacement, &file).unwrap();
        assert_eq!(pin.check().err().unwrap().code, "REMOTE_RUNTIME_CHANGED");
        assert_eq!(std::fs::read(file).unwrap(), b"unchanged bytes");
    }
}
