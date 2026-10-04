// SPDX-License-Identifier: Apache-2.0
//! Private bounded staging for later OCI validation/import. Never executes bytes.
use super::{Error, VerifiedRelease};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct StagedArtifact {
    file: File,
    path: PathBuf,
    identity: Metadata,
}
impl StagedArtifact {
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Recheck the retained read-only handle and path. Does not recheck trust policy.
    pub fn reverify(&mut self, release: &mut VerifiedRelease, now: u64) -> Result<(), Error> {
        release.artifact_verified = false;
        valid_time(release, now)?;
        if !same(
            &self.identity,
            &self
                .file
                .metadata()
                .map_err(|_| Error::ArtifactUnavailable)?,
        ) || !same(
            &self.identity,
            &fs::symlink_metadata(&self.path).map_err(|_| Error::ArtifactUnavailable)?,
        ) {
            return Err(Error::ArtifactMismatch);
        }
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::ArtifactUnavailable)?;
        release.verify_artifact(&mut self.file)?;
        if !same(
            &self.identity,
            &self
                .file
                .metadata()
                .map_err(|_| Error::ArtifactUnavailable)?,
        ) || !same(
            &self.identity,
            &fs::symlink_metadata(&self.path).map_err(|_| Error::ArtifactUnavailable)?,
        ) {
            release.artifact_verified = false;
            return Err(Error::ArtifactMismatch);
        }
        self.file.seek(SeekFrom::Start(0)).map_err(|_| {
            release.artifact_verified = false;
            Error::ArtifactUnavailable
        })?;
        Ok(())
    }
}
fn valid_time(v: &VerifiedRelease, now: u64) -> Result<(), Error> {
    if now < v.release.issued_at || now >= v.release.expires_at {
        Err(Error::Expired)
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn same(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.is_file()
        && b.is_file()
        && (
            a.dev(),
            a.ino(),
            a.len(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
            a.nlink(),
            a.mode(),
            a.uid(),
            a.gid(),
        ) == (
            b.dev(),
            b.ino(),
            b.len(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
            b.nlink(),
            b.mode(),
            b.uid(),
            b.gid(),
        )
}
#[cfg(not(unix))]
fn same(_a: &Metadata, _b: &Metadata) -> bool {
    false
}
/// Produces a new private retained copy, verified using one stable read-only handle.
/// Failed partial copies are retained. No Engine/API/trust journal is modified.
#[cfg(unix)]
pub fn stage(
    v: &mut VerifiedRelease,
    source: &Path,
    parent: &Path,
    now: u64,
) -> Result<StagedArtifact, Error> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    v.artifact_verified = false;
    valid_time(v, now)?;
    if !parent.is_absolute()
        || parent
            .canonicalize()
            .map_err(|_| Error::ArtifactUnavailable)?
            != parent
    {
        return Err(Error::ArtifactUnavailable);
    }
    let pm = fs::symlink_metadata(parent).map_err(|_| Error::ArtifactUnavailable)?;
    if !pm.is_dir() || pm.uid() != unsafe { libc::geteuid() } || pm.mode() & 0o777 != 0o700 {
        return Err(Error::ArtifactUnavailable);
    }
    if source.file_name().and_then(|n| n.to_str()) != Some(v.release.artifact.name.as_str()) {
        return Err(Error::ArtifactMismatch);
    }
    // Development staging stays within the qualified archive and host disk bounds.
    if v.release.artifact.bytes > 2 * 1024 * 1024 * 1024
        || fs2::available_space(parent).map_err(|_| Error::ArtifactUnavailable)?
            < v.release
                .artifact
                .bytes
                .saturating_add(2 * 1024 * 1024 * 1024)
    {
        return Err(Error::ArtifactUnavailable);
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source)
        .map_err(|_| Error::ArtifactUnavailable)?;
    let before = input.metadata().map_err(|_| Error::ArtifactUnavailable)?;
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != unsafe { libc::geteuid() }
        || before.mode() & 0o077 != 0
        || before.len() != v.release.artifact.bytes
    {
        return Err(Error::ArtifactMismatch);
    }
    let directory = parent.join(format!("runtime-stage-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|_| Error::ArtifactUnavailable)?;
    let parent_now = fs::symlink_metadata(parent).map_err(|_| Error::ArtifactUnavailable)?;
    if !parent_now.is_dir()
        || (pm.dev(), pm.ino(), pm.uid(), pm.mode())
            != (
                parent_now.dev(),
                parent_now.ino(),
                parent_now.uid(),
                parent_now.mode(),
            )
    {
        return Err(Error::ArtifactUnavailable);
    }
    let path = directory.join(&v.release.artifact.name);
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| Error::ArtifactUnavailable)?;
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = input
            .read(&mut buffer)
            .map_err(|_| Error::ArtifactUnavailable)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|n| *n <= v.release.artifact.bytes)
            .ok_or(Error::ArtifactMismatch)?;
        out.write_all(&buffer[..n])
            .map_err(|_| Error::ArtifactUnavailable)?;
    }
    if count != v.release.artifact.bytes
        || !same(
            &before,
            &input.metadata().map_err(|_| Error::ArtifactUnavailable)?,
        )
        || !same(
            &before,
            &fs::symlink_metadata(source).map_err(|_| Error::ArtifactUnavailable)?,
        )
    {
        return Err(Error::ArtifactMismatch);
    }
    out.sync_all().map_err(|_| Error::ArtifactUnavailable)?;
    out.set_permissions(fs::Permissions::from_mode(0o400))
        .map_err(|_| Error::ArtifactUnavailable)?;
    out.sync_all().map_err(|_| Error::ArtifactUnavailable)?;
    drop(out);
    File::open(&directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::ArtifactUnavailable)?;
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::ArtifactUnavailable)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| Error::ArtifactUnavailable)?;
    let identity = file.metadata().map_err(|_| Error::ArtifactUnavailable)?;
    if !identity.is_file()
        || identity.nlink() != 1
        || identity.uid() != unsafe { libc::geteuid() }
        || identity.mode() & 0o777 != 0o400
    {
        return Err(Error::ArtifactMismatch);
    }
    let parent_now = fs::symlink_metadata(parent).map_err(|_| Error::ArtifactUnavailable)?;
    let directory_now = fs::symlink_metadata(&directory).map_err(|_| Error::ArtifactUnavailable)?;
    if !parent_now.is_dir()
        || (pm.dev(), pm.ino(), pm.uid(), pm.mode())
            != (
                parent_now.dev(),
                parent_now.ino(),
                parent_now.uid(),
                parent_now.mode(),
            )
        || !directory_now.is_dir()
        || directory_now.uid() != unsafe { libc::geteuid() }
        || directory_now.mode() & 0o777 != 0o700
    {
        return Err(Error::ArtifactUnavailable);
    }
    let mut staged = StagedArtifact {
        file,
        path,
        identity,
    };
    staged.reverify(v, now)?;
    Ok(staged)
}
#[cfg(not(unix))]
pub fn stage(
    v: &mut VerifiedRelease,
    _source: &Path,
    _parent: &Path,
    _now: u64,
) -> Result<StagedArtifact, Error> {
    v.artifact_verified = false;
    Err(Error::TrustPlatformUnverified)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::signed_release::{Artifact, Release};
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    fn fixture() -> (VerifiedRelease, PathBuf, PathBuf) {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-stage-test-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let source = root.join("runtime.tar");
        let bytes = vec![13u8; 131073];
        fs::write(&source, &bytes).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let artifact = Artifact {
            name: "runtime.tar".into(),
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            runtime_image_sha256: "b".repeat(64),
            schema_sha256: "c".repeat(64),
        };
        let release = Release {
            format: 1,
            product: "ExhibitOS/runtime".into(),
            channel: "development".into(),
            target: "linux-arm64".into(),
            version: "0.1.1-dev.1".into(),
            sequence: 41,
            issued_at: 100,
            expires_at: 200,
            protocol_version: 1,
            source_schemas: vec!["a".repeat(64)],
            artifact,
        };
        let v = VerifiedRelease {
            release,
            key_id: "d".repeat(64),
            payload_sha256: "e".repeat(64),
            source_schema_sha256: "a".repeat(64),
            artifact_verified: true,
        };
        (v, source, root)
    }
    #[test]
    fn bounded_private_copy_rechecks_and_preserves_original() {
        let (mut v, source, root) = fixture();
        let before = fs::read(&source).unwrap();
        let mut staged = stage(&mut v, &source, &root, 150).unwrap();
        assert_eq!(fs::read(staged.path()).unwrap(), before);
        assert_eq!(fs::metadata(staged.path()).unwrap().mode() & 0o777, 0o400);
        assert_ne!(
            fs::metadata(&source).unwrap().ino(),
            fs::metadata(staged.path()).unwrap().ino()
        );
        staged.reverify(&mut v, 151).unwrap();
        assert!(v.receipt().artifact_verified);
        assert!(!v.receipt().activated);
        assert_eq!(fs::read(source).unwrap(), before);
        assert!(staged.reverify(&mut v, 200).is_err());
        assert!(!v.receipt().artifact_verified);
    }
    #[test]
    fn links_permissions_and_changed_bytes_clear_old_proof() {
        let (mut v, source, root) = fixture();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(stage(&mut v, &source, &root, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let alias = root.join("alias");
        fs::hard_link(&source, &alias).unwrap();
        assert!(stage(&mut v, &source, &root, 150).is_err());
        fs::remove_file(alias).unwrap();
        let bytes = vec![14u8; 131073];
        fs::write(&source, bytes).unwrap();
        assert!(stage(&mut v, &source, &root, 150).is_err());
        assert!(!v.receipt().artifact_verified);
    }
    #[test]
    fn path_replacement_and_content_change_are_refused() {
        let (mut v, source, root) = fixture();
        let mut staged = stage(&mut v, &source, &root, 150).unwrap();
        fs::rename(staged.path(), root.join("retained-original")).unwrap();
        fs::copy(&source, staged.path()).unwrap();
        assert!(staged.reverify(&mut v, 151).is_err());
        assert!(!v.receipt().artifact_verified);
        let mut next = stage(&mut v, &source, &root, 150).unwrap();
        fs::set_permissions(next.path(), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(next.path(), vec![0; 131073]).unwrap();
        assert!(next.reverify(&mut v, 151).is_err());
        assert!(!v.receipt().artifact_verified);
    }
}
