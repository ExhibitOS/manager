// SPDX-License-Identifier: Apache-2.0
//! Bounded current Engine image export, not a caller-supplied archive proof.
use crate::*;
use std::io::Write;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageArchiveProof {
    pub reference: String,
    pub content_id: String,
    pub archive: String,
    pub bytes: u64,
    pub sha256: String,
}
fn bounded(
    mut input: impl Read,
    mut file: File,
    limit: u64,
) -> Result<crate::backup_creation::FileHash> {
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    let mut sha = Sha256::new();
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > limit {
            return Err(err("STORAGE_QUOTA"));
        }
        file.write_all(&buffer[..count])
            .map_err(|_| err("STORAGE_UNAVAILABLE"))?;
        sha.update(&buffer[..count]);
    }
    if bytes == 0 {
        return Err(err("UPDATE_SOURCE_IMAGES_INVALID"));
    }
    file.sync_all().map_err(|_| err("STORAGE_UNAVAILABLE"))?;
    Ok(crate::backup_creation::FileHash {
        bytes,
        sha256: format!("{:x}", sha.finalize()),
    })
}
fn save(image: &str, path: &Path, limit: u64) -> Result<crate::backup_creation::FileHash> {
    let file = private_options()
        .open(path)
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    let binary = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
    let mut child = process_window::background_command(binary)
        .args(["image", "save", image])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| err("ENGINE_PERMISSION"))?;
    let pipe = child
        .stdout
        .take()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let _ = tx.send(bounded(pipe, file, limit));
    });
    let result = rx.recv_timeout(Duration::from_secs(600));
    let result = match result {
        Ok(Ok(proof)) => Ok(proof),
        Ok(Err(e)) => {
            let _ = child.kill();
            Err(e)
        }
        Err(_) => {
            let _ = child.kill();
            Err(err("ENGINE_TIMEOUT"))
        }
    };
    let exited = child.wait().map_err(|_| err("ENGINE_OPERATION_FAILED"))?;
    let _ = reader.join();
    let proof = result?;
    if !exited.success() {
        return Err(err("ENGINE_OPERATION_FAILED"));
    }
    let repeated = crate::backup_creation::hash_file(path)?;
    if proof.bytes != repeated.bytes || proof.sha256 != repeated.sha256 {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    Ok(proof)
}
pub(super) fn export(
    work: &Path,
    expected: &[crate::restoration::PreservedImage],
) -> Result<Vec<ImageArchiveProof>> {
    let mut bytes = 0u64;
    for image in expected {
        bytes = bytes
            .checked_add(image.bytes)
            .ok_or_else(|| err("STORAGE_QUOTA"))?;
    }
    if bytes > 2 * 1024 * 1024 * 1024
        || fs2::available_space(work).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < bytes + 2 * 1024 * 1024 * 1024
    {
        return Err(err("STORAGE_QUOTA"));
    }
    let mut proofs = Vec::new();
    for (i, image) in expected.iter().enumerate() {
        let id = crate::backup_creation::local_image("docker", &image.reference)?;
        if id != image.content_id {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
        let path = work.join(format!("image-{i}.tar"));
        let saved = save(&id, &path, image.bytes)?;
        if saved.bytes != image.bytes
            || saved.sha256 != image.sha256
            || crate::backup_creation::local_image("docker", &image.reference)? != id
        {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
        proofs.push(ImageArchiveProof {
            reference: image.reference.clone(),
            content_id: id,
            archive: image.archive.clone(),
            bytes: saved.bytes,
            sha256: saved.sha256,
        });
    }
    Ok(proofs)
}
/// Retire only newly exported exact image files after the encompassing verifier
/// rechecks source/root fences. No historical scan or archive/baseline cleanup.
#[cfg(unix)]
pub(super) fn retire_verified(work: &Path, proofs: &[ImageArchiveProof]) -> Result<()> {
    use std::ffi::CString;
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let failure = || err("UPDATE_SOURCE_IMAGES_RETIRE_UNCERTAIN");
    if proofs.is_empty()
        || proofs.len() > 16
        || fs::canonicalize(work).ok().as_deref() != Some(work)
    {
        return Err(failure());
    }
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(work)
        .map_err(|_| failure())?;
    let root = directory.metadata().map_err(|_| failure())?;
    let same = |a: &fs::Metadata, b: &fs::Metadata| {
        (
            a.dev(),
            a.ino(),
            a.len(),
            a.mode(),
            a.uid(),
            a.nlink(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
        ) == (
            b.dev(),
            b.ino(),
            b.len(),
            b.mode(),
            b.uid(),
            b.nlink(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
        )
    };
    if root.uid() != unsafe { libc::geteuid() } || root.mode() & 0o7777 != 0o700 {
        return Err(failure());
    }
    let names: std::collections::BTreeSet<_> = (0..proofs.len())
        .map(|i| format!("image-{i}.tar"))
        .collect();
    let found: std::collections::BTreeSet<_> = fs::read_dir(work)
        .map_err(|_| failure())?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()
        .map_err(|_| failure())?;
    if found != names {
        return Err(failure());
    }
    let mut retained = Vec::new();
    for (i, proof) in proofs.iter().enumerate() {
        let name = CString::new(format!("image-{i}.tar")).map_err(|_| failure())?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(failure());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let before = file.metadata().map_err(|_| failure())?;
        if !before.is_file()
            || before.uid() != unsafe { libc::geteuid() }
            || before.nlink() != 1
            || before.mode() & 0o7777 != 0o600
            || before.len() != proof.bytes
            || proof.bytes > 2 * 1024 * 1024 * 1024
        {
            return Err(failure());
        }
        let mut sha = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(|_| failure())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > proof.bytes {
                return Err(failure());
            }
            sha.update(&buffer[..n]);
        }
        if count != proof.bytes
            || format!("{:x}", sha.finalize()) != proof.sha256
            || !same(&before, &file.metadata().map_err(|_| failure())?)
        {
            return Err(failure());
        }
        retained.push((name, file, before));
    }
    if !same(&root, &fs::symlink_metadata(work).map_err(|_| failure())?) {
        return Err(failure());
    }
    // Persist the exact verification/reproduction inventory before any unlink.
    let marker = CString::new("verified-images.json").map_err(|_| failure())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            marker.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(failure());
    }
    let mut marker = unsafe { File::from_raw_fd(fd) };
    marker.write_all(&serde_json::to_vec(&serde_json::json!({"format":1,"images":proofs,"policy":"retire_successful_observation_only","recoveryArchive":false})).map_err(|_| failure())?)
        .and_then(|_| marker.sync_all()).map_err(|_| failure())?;
    directory.sync_all().map_err(|_| failure())?;
    for (name, file, before) in retained {
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(failure());
        }
        let current = unsafe { File::from_raw_fd(fd) };
        if !same(&before, &current.metadata().map_err(|_| failure())?)
            || !same(&before, &file.metadata().map_err(|_| failure())?)
        {
            return Err(failure());
        }
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(failure());
        }
    }
    directory.sync_all().map_err(|_| failure())?;
    // Detect path replacement without ever deleting through the replacement path.
    let after = fs::symlink_metadata(work).map_err(|_| failure())?;
    if (root.dev(), root.ino(), root.uid(), root.mode())
        != (after.dev(), after.ino(), after.uid(), after.mode())
    {
        return Err(failure());
    }
    Ok(())
}
#[cfg(not(unix))]
pub(super) fn retire_verified(_work: &Path, _proofs: &[ImageArchiveProof]) -> Result<()> {
    Err(err("UPDATE_SOURCE_IMAGES_RETIRE_UNVERIFIED"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn fixture() -> (PathBuf, Vec<ImageArchiveProof>) {
        use std::os::unix::fs::PermissionsExt;
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("exhibitos-export-retention-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let mut file = private_options().open(root.join("image-0.tar")).unwrap();
        file.write_all(b"original").unwrap();
        let proof = ImageArchiveProof {
            reference: "synthetic".into(),
            content_id: "a".repeat(64),
            archive: "image-0.tar".into(),
            bytes: 8,
            sha256: digest(b"original"),
        };
        (root, vec![proof])
    }
    #[cfg(unix)]
    #[test]
    fn successful_export_retirement_keeps_exact_small_inventory() {
        let (root, proofs) = fixture();
        retire_verified(&root, &proofs).unwrap();
        assert!(!root.join("image-0.tar").exists());
        let receipt: Value =
            serde_json::from_slice(&fs::read(root.join("verified-images.json")).unwrap()).unwrap();
        assert_eq!(receipt["images"][0]["sha256"], proofs[0].sha256);
        assert_eq!(receipt["recoveryArchive"], false);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn changed_export_and_unknown_files_preserve_all_candidates() {
        let (root, mut proofs) = fixture();
        proofs[0].sha256 = "b".repeat(64);
        assert!(retire_verified(&root, &proofs).is_err());
        assert_eq!(fs::read(root.join("image-0.tar")).unwrap(), b"original");
        proofs[0].sha256 = digest(b"original");
        fs::write(root.join("unexpected"), b"keep").unwrap();
        assert!(retire_verified(&root, &proofs).is_err());
        assert!(!root.join("verified-images.json").exists());
        assert_eq!(fs::read(root.join("unexpected")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn linked_export_and_replaced_scope_never_retire_foreign_bytes() {
        use std::os::unix::fs::symlink;
        let (root, proofs) = fixture();
        let outside = root.with_extension("outside");
        fs::hard_link(root.join("image-0.tar"), &outside).unwrap();
        assert!(retire_verified(&root, &proofs).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"original");
        fs::remove_file(&outside).unwrap();
        fs::rename(root.join("image-0.tar"), &outside).unwrap();
        symlink(&outside, root.join("image-0.tar")).unwrap();
        assert!(retire_verified(&root, &proofs).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"original");
        let alias = root.with_extension("alias");
        symlink(&root, &alias).unwrap();
        assert!(retire_verified(&alias, &proofs).is_err());
        fs::remove_file(alias).unwrap();
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn oversized_image_stream_never_writes_beyond_authenticated_budget() {
        let path = std::env::temp_dir().join(format!("exhibitos-image-stream-{}", Uuid::new_v4()));
        let file = private_options().open(&path).unwrap();
        let error = bounded(&b"four"[..], file, 3).unwrap_err();
        assert_eq!(error.code, "STORAGE_QUOTA");
        assert!(fs::metadata(path).unwrap().len() <= 3);
    }
}
