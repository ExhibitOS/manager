// SPDX-License-Identifier: Apache-2.0
//! Private bounded staging for later OCI validation/import. Never executes bytes.
use super::{Error, VerifiedRelease};
use std::{
    fs::{self, File, Metadata},
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Verify an existing Windows artifact through one retained NTFS read guard.
/// This is diagnostic input verification only: no staging, import or activation.
#[cfg(windows)]
pub fn verify_public_input(v: &mut VerifiedRelease, source: &Path, now: u64) -> Result<(), Error> {
    v.artifact_verified = false;
    valid_time(v, now)?;
    if source.file_name().and_then(|name| name.to_str()) != Some(v.release.artifact.name.as_str()) {
        return Err(Error::ArtifactMismatch);
    }
    let mut input = crate::windows_private::PublicRecord::open(source)
        .map_err(|_| Error::ArtifactUnavailable)?;
    let before = input
        .checked_metadata()
        .map_err(|_| Error::ArtifactUnavailable)?;
    if before.len() != v.release.artifact.bytes {
        return Err(Error::ArtifactMismatch);
    }
    let modified = before.modified().map_err(|_| Error::ArtifactUnavailable)?;
    v.verify_artifact(&mut input)?;
    let final_check = (|| {
        let after = input
            .checked_metadata()
            .map_err(|_| Error::ArtifactUnavailable)?;
        if after.len() != before.len()
            || after.modified().map_err(|_| Error::ArtifactUnavailable)? != modified
        {
            return Err(Error::ArtifactMismatch);
        }
        Ok(())
    })();
    if final_check.is_err() {
        v.artifact_verified = false;
    }
    final_check
}

#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(any(unix, windows))]
use std::io::{Read, Write};

#[cfg_attr(not(windows), derive(Debug))]
pub struct StagedArtifact {
    file: File,
    path: PathBuf,
    identity: Metadata,
    #[cfg(windows)]
    directory: crate::windows_private::PrivateDirectory,
}
#[cfg(windows)]
impl std::fmt::Debug for StagedArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedArtifact").finish_non_exhaustive()
    }
}
impl StagedArtifact {
    fn check(&self) -> Result<(), Error> {
        #[cfg(windows)]
        self.directory
            .check_record(
                &self.file,
                self.path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or(Error::ArtifactUnavailable)?,
            )
            .map_err(|_| Error::ArtifactUnavailable)?;
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
        Ok(())
    }
    pub(crate) fn retained_input(&mut self) -> Result<File, Error> {
        self.check()?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::ArtifactUnavailable)?;
        self.file
            .try_clone()
            .map_err(|_| Error::ArtifactUnavailable)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Recheck the retained read-only handle and path. Does not recheck trust policy.
    pub fn reverify(&mut self, release: &mut VerifiedRelease, now: u64) -> Result<(), Error> {
        release.artifact_verified = false;
        valid_time(release, now)?;
        self.check()?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::ArtifactUnavailable)?;
        release.verify_artifact(&mut self.file)?;
        if let Err(error) = self.check() {
            release.artifact_verified = false;
            return Err(error);
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
#[cfg(windows)]
fn same(a: &Metadata, b: &Metadata) -> bool {
    // Native identity/ACL/single-link checks are supplied by the retained
    // PrivateDirectory fence. Metadata supplements them, never replaces them.
    a.is_file()
        && b.is_file()
        && a.len() == b.len()
        && matches!((a.modified(), b.modified()), (Ok(x), Ok(y)) if x == y)
}
#[cfg(not(any(unix, windows)))]
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
/// Windows owner-only staging with retained NTFS input and output fences.
/// Failed fresh candidates remain for diagnosis; no Engine/trust mutation.
#[cfg(windows)]
pub fn stage(
    v: &mut VerifiedRelease,
    source: &Path,
    parent: &Path,
    now: u64,
) -> Result<StagedArtifact, Error> {
    use crate::windows_private::{PrivateDirectory, PublicRecord};
    v.artifact_verified = false;
    let result = (|| {
        valid_time(v, now)?;
        if source.file_name().and_then(|n| n.to_str()) != Some(v.release.artifact.name.as_str()) {
            return Err(Error::ArtifactMismatch);
        }
        let parent_guard =
            PrivateDirectory::inspect(parent).map_err(|_| Error::ArtifactUnavailable)?;
        if v.release.artifact.bytes > 2 * 1024 * 1024 * 1024
            || fs2::available_space(parent_guard.path()).map_err(|_| Error::ArtifactUnavailable)?
                < v.release
                    .artifact
                    .bytes
                    .saturating_add(2 * 1024 * 1024 * 1024)
        {
            return Err(Error::ArtifactUnavailable);
        }
        let mut input = PublicRecord::open(source).map_err(|_| Error::ArtifactUnavailable)?;
        let before = input
            .checked_metadata()
            .map_err(|_| Error::ArtifactUnavailable)?;
        if before.len() != v.release.artifact.bytes {
            return Err(Error::ArtifactMismatch);
        }
        let directory = PrivateDirectory::create(
            &parent_guard
                .path()
                .join(format!("runtime-stage-{}", uuid::Uuid::new_v4())),
        )
        .map_err(|_| Error::ArtifactUnavailable)?;
        parent_guard
            .check()
            .map_err(|_| Error::ArtifactUnavailable)?;
        let name = v.release.artifact.name.clone();
        let path = directory.path().join(&name);
        let mut out = directory
            .create_record(&name)
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
                &input
                    .checked_metadata()
                    .map_err(|_| Error::ArtifactUnavailable)?,
            )
        {
            return Err(Error::ArtifactMismatch);
        }
        directory
            .check_record(&out, &name)
            .map_err(|_| Error::ArtifactUnavailable)?;
        out.sync_all().map_err(|_| Error::ArtifactUnavailable)?;
        drop(out);
        // Drop the write-capable handle before opening the immutable read/share
        // fence. Failed reopen/hash checks never authorize the candidate.
        let file = directory
            .retained_read_file(&name)
            .map_err(|_| Error::ArtifactUnavailable)?;
        let identity = file.metadata().map_err(|_| Error::ArtifactUnavailable)?;
        let mut staged = StagedArtifact {
            file,
            path,
            identity,
            directory,
        };
        staged.reverify(v, now)?;
        parent_guard
            .check()
            .map_err(|_| Error::ArtifactUnavailable)?;
        if !same(
            &before,
            &input
                .checked_metadata()
                .map_err(|_| Error::ArtifactUnavailable)?,
        ) {
            return Err(Error::ArtifactMismatch);
        }
        Ok(staged)
    })();
    if result.is_err() {
        v.artifact_verified = false;
    }
    result
}
#[cfg(not(any(unix, windows)))]
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

#[cfg(all(test, windows))]
mod windows_artifact_inputs {
    use super::*;
    use crate::{
        signed_release::{Artifact, Release},
        windows_private::PrivateDirectory,
    };
    use sha2::{Digest, Sha256};
    use std::fs::OpenOptions;
    fn fixture(size: usize) -> (VerifiedRelease, PathBuf) {
        let parent = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        let root = PrivateDirectory::create(
            &parent.join(format!("exhibitos-artifact-input-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
        let source = root.path().join("runtime.tar");
        let bytes = vec![13u8; size];
        fs::write(&source, &bytes).unwrap();
        // Synthetic verification state isolates native reader semantics; this
        // fixture does not claim cryptographic signature or OCI verification.
        let release = Release {
            format: 1,
            product: "ExhibitOS/runtime".into(),
            channel: "development".into(),
            target: "linux-amd64".into(),
            version: "0.1.1-dev.1".into(),
            sequence: 41,
            issued_at: 100,
            expires_at: 200,
            protocol_version: 1,
            source_schemas: vec!["a".repeat(64)],
            artifact: Artifact {
                name: "runtime.tar".into(),
                bytes: size as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                runtime_image_sha256: "b".repeat(64),
                schema_sha256: "c".repeat(64),
            },
        };
        (
            VerifiedRelease {
                release,
                key_id: "d".repeat(64),
                payload_sha256: "e".repeat(64),
                source_schema_sha256: "a".repeat(64),
                artifact_verified: true,
            },
            source,
        )
    }
    #[test]
    fn windows_artifact_inputs_stream_above_small_document_limit_without_activation() {
        let (mut v, source) = fixture(17 * 1024 * 1024 + 1);
        verify_public_input(&mut v, &source, 150).unwrap();
        assert!(v.receipt().artifact_verified);
        assert!(!v.receipt().activated);
        assert_eq!(fs::metadata(source).unwrap().len(), 17 * 1024 * 1024 + 1);
    }
    #[test]
    fn windows_artifact_inputs_busy_writer_and_hardlink_clear_old_proof() {
        let (mut v, source) = fixture(131073);
        let writer = OpenOptions::new().write(true).open(&source).unwrap();
        assert!(verify_public_input(&mut v, &source, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        drop(writer);
        verify_public_input(&mut v, &source, 150).unwrap();
        fs::hard_link(&source, source.with_file_name("alias.tar")).unwrap();
        assert!(verify_public_input(&mut v, &source, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        assert_eq!(fs::read(source).unwrap(), vec![13u8; 131073]);
    }
    #[test]
    fn windows_artifact_inputs_changed_size_hash_and_expiry_clear_old_proof() {
        let (mut v, source) = fixture(131073);
        verify_public_input(&mut v, &source, 150).unwrap();
        fs::write(&source, vec![14u8; 131073]).unwrap();
        assert_eq!(
            verify_public_input(&mut v, &source, 150),
            Err(Error::ArtifactMismatch)
        );
        assert!(!v.receipt().artifact_verified);
        fs::write(&source, b"short").unwrap();
        assert_eq!(
            verify_public_input(&mut v, &source, 150),
            Err(Error::ArtifactMismatch)
        );
        v.artifact_verified = true;
        assert_eq!(
            verify_public_input(&mut v, &source, 200),
            Err(Error::Expired)
        );
        assert!(!v.receipt().artifact_verified);
    }
    #[test]
    fn windows_artifact_inputs_wrong_name_and_missing_input_clear_old_proof() {
        let (mut v, source) = fixture(7);
        let other = source.with_file_name("other.tar");
        assert_eq!(
            verify_public_input(&mut v, &other, 150),
            Err(Error::ArtifactMismatch)
        );
        assert!(!v.receipt().artifact_verified);
        let moved = source.with_file_name("preserved.tar");
        fs::rename(&source, &moved).unwrap();
        assert!(verify_public_input(&mut v, &source, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        assert_eq!(fs::read(moved).unwrap(), vec![13u8; 7]);
    }
    #[test]
    fn windows_artifact_staging_retains_exact_read_only_copy_without_activation() {
        let (mut v, source) = fixture(131073);
        let root = source.parent().unwrap().to_path_buf();
        let before = fs::read(&source).unwrap();
        let mut staged = stage(&mut v, &source, &root, 150).unwrap();
        assert_ne!(staged.path(), source);
        assert_eq!(fs::read(staged.path()).unwrap(), before);
        assert!(OpenOptions::new().write(true).open(staged.path()).is_err());
        assert!(fs::rename(staged.path(), root.join("moved.tar")).is_err());
        assert!(fs::rename(staged.path().parent().unwrap(), root.join("moved-stage")).is_err());
        let mut retained = staged.retained_input().unwrap();
        assert!(retained.write_all(b"refuse write").is_err());
        let mut copied = Vec::new();
        retained.read_to_end(&mut copied).unwrap();
        assert_eq!(copied, before);
        staged.reverify(&mut v, 151).unwrap();
        assert!(v.receipt().artifact_verified);
        assert!(!v.receipt().activated);
        assert_eq!(fs::read(&source).unwrap(), before);
        assert_eq!(staged.reverify(&mut v, 200), Err(Error::Expired));
        assert!(!v.receipt().artifact_verified);
        drop(retained);
        drop(staged);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_artifact_staging_busy_source_and_hardlink_do_not_publish() {
        let (mut v, source) = fixture(131073);
        let root = source.parent().unwrap().to_path_buf();
        let writer = OpenOptions::new().write(true).open(&source).unwrap();
        assert!(stage(&mut v, &source, &root, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        drop(writer);
        fs::hard_link(&source, root.join("alias.tar")).unwrap();
        assert!(stage(&mut v, &source, &root, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        assert_eq!(fs::read(&source).unwrap(), vec![13u8; 131073]);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_artifact_staging_hash_failure_retains_candidate_and_bounds_clear_proof() {
        let (mut v, source) = fixture(131073);
        let root = source.parent().unwrap().to_path_buf();
        fs::write(&source, vec![14u8; 131073]).unwrap();
        assert!(matches!(
            stage(&mut v, &source, &root, 150),
            Err(Error::ArtifactMismatch)
        ));
        assert!(!v.receipt().artifact_verified);
        let directories: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.is_dir())
            .collect();
        assert_eq!(directories.len(), 1);
        assert_eq!(
            fs::read(directories[0].join("runtime.tar")).unwrap(),
            vec![14u8; 131073]
        );
        assert_eq!(fs::read(&source).unwrap(), vec![14u8; 131073]);
        v.artifact_verified = true;
        v.release.artifact.bytes = 2 * 1024 * 1024 * 1024 + 1;
        assert!(stage(&mut v, &source, &root, 150).is_err());
        assert!(!v.receipt().artifact_verified);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert!(matches!(
            stage(&mut v, &source, &root, 200),
            Err(Error::Expired)
        ));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_artifact_staging_acl_change_refuses_without_repair_or_source_mutation() {
        let (mut v, source) = fixture(131073);
        let root = source.parent().unwrap().to_path_buf();
        let mut staged = stage(&mut v, &source, &root, 150).unwrap();
        let status = crate::process_window::background_command("icacls.exe")
            .arg(staged.path())
            .args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(staged.reverify(&mut v, 151).is_err());
        assert!(!v.receipt().artifact_verified);
        assert!(staged.retained_input().is_err());
        assert!(
            PrivateDirectory::inspect(staged.path().parent().unwrap())
                .unwrap()
                .retained_read_file("runtime.tar")
                .is_err()
        );
        assert_eq!(fs::read(staged.path()).unwrap(), vec![13u8; 131073]);
        assert_eq!(fs::read(&source).unwrap(), vec![13u8; 131073]);
        drop(staged);
        // Only the new synthetic fixture is retired after preservation assertions.
        fs::remove_dir_all(root).unwrap();
    }
}
