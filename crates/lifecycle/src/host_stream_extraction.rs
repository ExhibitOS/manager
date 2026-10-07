// SPDX-License-Identifier: Apache-2.0
//! Authenticated chunk sink; its private staging tree is never a published proof.
use super::*;
use std::io::Write;

pub(super) struct Extraction {
    profile: PathBuf,
    recovered: PathBuf,
    prefix: Vec<u8>,
    manifest_len: Option<usize>,
    manifest: Vec<u8>,
    inventory: Option<Inventory>,
    next: usize,
    output: Option<File>,
    remaining: u64,
    hash: Sha256,
    require_reference_clones: bool,
}
impl Extraction {
    pub(super) fn new(profile: &Path, recovered: &Path) -> Self {
        Self {
            profile: profile.into(),
            recovered: recovered.into(),
            prefix: Vec::new(),
            manifest_len: None,
            manifest: Vec::new(),
            inventory: None,
            next: 0,
            output: None,
            remaining: 0,
            hash: Sha256::new(),
            require_reference_clones: false,
        }
    }
    pub(super) fn require_reference_clones(&mut self) {
        self.require_reference_clones = true;
    }
    fn advance(&mut self) -> Result<()> {
        let m = self.inventory.as_ref().ok_or_else(fail)?;
        loop {
            let Some(e) = m.items.get(self.next) else {
                return Ok(());
            };
            if e.kind == "directory" {
                installations::new_directory(&self.recovered.join(&e.path))?;
                self.next += 1;
                continue;
            }
            if let Some(original) = m.content_references.get(&e.path) {
                let original = self.recovered.join(original);
                let target = self.recovered.join(&e.path);
                let mut input = file(&original)?;
                let before = input.metadata().map_err(|_| fail())?;
                let copied =
                    copy_reference(&mut input, &target, e.bytes, self.require_reference_clones)?;
                if copied != e.bytes
                    || !unchanged(&before, &input.metadata().map_err(|_| fail())?)
                    || !unchanged(
                        &before,
                        &fs::symlink_metadata(&original).map_err(|_| fail())?,
                    )
                    || hash(&target)? != (e.bytes, e.sha256.clone().ok_or_else(fail)?)
                {
                    return Err(fail());
                }
                self.next += 1;
                continue;
            }
            if self.output.is_none() {
                self.output = Some(private_new(&self.recovered.join(&e.path))?);
                self.remaining = e.bytes;
                self.hash = Sha256::new();
            }
            if self.remaining != 0 {
                return Ok(());
            }
            if Some(format!("{:x}", self.hash.clone().finalize())) != e.sha256 {
                return Err(fail());
            }
            self.output
                .take()
                .ok_or_else(fail)?
                .sync_all()
                .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
            self.next += 1;
        }
    }
    fn accept(&mut self, mut bytes: &[u8]) -> Result<()> {
        if self.prefix.len() < 8 {
            let n = bytes.len().min(8 - self.prefix.len());
            self.prefix.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.prefix.len() < 8 {
                return Ok(());
            }
            let len = u64::from_be_bytes(self.prefix.as_slice().try_into().map_err(|_| fail())?);
            if len == 0 || len > MANIFEST_LIMIT {
                return Err(fail());
            }
            self.manifest_len = Some(len as usize);
        }
        if self.inventory.is_none() {
            let len = self.manifest_len.ok_or_else(fail)?;
            let n = bytes.len().min(len - self.manifest.len());
            self.manifest.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.manifest.len() < len {
                return Ok(());
            }
            let m: Inventory = serde_json::from_slice(&self.manifest).map_err(|_| fail())?;
            validate_inventory(&m, &self.profile)?;
            installations::new_directory(&self.recovered)?;
            self.inventory = Some(m);
            self.advance()?;
        }
        while !bytes.is_empty() {
            let n = bytes
                .len()
                .min(usize::try_from(self.remaining).map_err(|_| fail())?);
            if n == 0 {
                return Err(fail());
            }
            self.output
                .as_mut()
                .ok_or_else(fail)?
                .write_all(&bytes[..n])
                .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
            self.hash.update(&bytes[..n]);
            self.remaining -= n as u64;
            bytes = &bytes[n..];
            self.advance()?;
        }
        Ok(())
    }
    pub(super) fn finish(mut self) -> Result<(Inventory, Vec<u8>)> {
        self.advance()?;
        let m = self.inventory.take().ok_or_else(fail)?;
        if self.next != m.items.len() || self.remaining != 0 || self.output.is_some() {
            return Err(fail());
        }
        Ok((m, self.manifest))
    }
}
impl Write for Extraction {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.accept(bytes)
            .map_err(|_| std::io::Error::other("HOST_CHECKPOINT_INVALID"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A reference is still independently hash-checked by the authenticated sink.
/// macOS may share immutable extents, never an inode or a mutable file handle.
/// Unsupported filesystems retain the bounded dense path and expanded budget.
fn copy_reference(input: &mut File, target: &Path, bytes: u64, require_clone: bool) -> Result<u64> {
    #[cfg(target_os = "macos")]
    if clone_reference(input, target)? {
        return input.metadata().map(|m| m.len()).map_err(|_| fail());
    }
    dense_reference(input, target, bytes, require_clone)
}

fn dense_reference(
    input: &mut File,
    target: &Path,
    bytes: u64,
    require_clone: bool,
) -> Result<u64> {
    if require_clone {
        return Err(err("HOST_REFERENCE_CLONE_REQUIRED"));
    }
    let mut output = private_new(target)?;
    let copied = std::io::copy(&mut input.take(bytes + 1), &mut output)
        .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    output.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    Ok(copied)
}

#[cfg(target_os = "macos")]
fn clone_reference(input: &File, target: &Path) -> Result<bool> {
    use std::os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    let parent = target.parent().ok_or_else(fail)?;
    canonical_private(parent)?;
    let before = fs::symlink_metadata(parent).map_err(|_| fail())?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(parent)
        .map_err(|_| fail())?;
    if !unchanged(&before, &directory.metadata().map_err(|_| fail())?) {
        return Err(fail());
    }
    let name = std::ffi::CString::new(target.file_name().ok_or_else(fail)?.as_bytes())
        .map_err(|_| fail())?;
    // SDK sys/clonefile.h: CLONE_ACL | CLONE_NOFOLLOW_ANY. The source is the
    // already opened private file, the destination is relative to its pinned parent.
    let result = unsafe {
        libc::fclonefileat(
            input.as_raw_fd(),
            directory.as_raw_fd(),
            name.as_ptr(),
            0x0004 | 0x0008,
        )
    };
    if result != 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOTSUP | libc::EXDEV | libc::ENOSYS) => Ok(false),
            _ => Err(err("HOST_WRITE_UNCERTAIN")),
        };
    }
    if !publication_identity(&before, &fs::symlink_metadata(parent).map_err(|_| fail())?) {
        return Err(fail());
    }
    file(target)?
        .sync_all()
        .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    directory
        .sync_all()
        .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    Ok(true)
}

#[cfg(all(test, target_os = "macos"))]
mod native_clone_tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn native_reference_is_private_distinct_and_write_independent() {
        let (_scope, profile, _, _) = super::super::tests::fixture();
        let source = profile.join("clone-source");
        let target = profile.join("clone-target");
        let bytes = vec![73; 2 * 1024 * 1024];
        write_new(&source, &bytes).unwrap();
        let input = file(&source).unwrap();
        assert!(clone_reference(&input, &target).unwrap());
        let original = input.metadata().unwrap();
        let cloned = fs::metadata(&target).unwrap();
        assert_ne!(original.ino(), cloned.ino());
        assert_eq!((original.nlink(), cloned.nlink()), (1, 1));
        assert_eq!(
            (original.mode(), original.uid(), original.gid()),
            (cloned.mode(), cloned.uid(), cloned.gid())
        );
        assert_eq!(fs::read(&target).unwrap(), bytes);
        fs::write(&target, b"only this private candidate changes").unwrap();
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }

    #[test]
    fn native_reference_collision_and_symlink_parent_preserve_existing_bytes() {
        let (_scope, profile, _, _) = super::super::tests::fixture();
        let source = profile.join("clone-source");
        let target = profile.join("clone-target");
        write_new(&source, b"original private bytes").unwrap();
        write_new(&target, b"preserved existing bytes").unwrap();
        let input = file(&source).unwrap();
        assert!(clone_reference(&input, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"preserved existing bytes");
        let alias = profile.join("alias");
        std::os::unix::fs::symlink(&profile, &alias).unwrap();
        assert!(clone_reference(&input, &alias.join("new-file")).is_err());
        assert!(!profile.join("new-file").exists());
        assert_eq!(fs::read(&source).unwrap(), b"original private bytes");
    }
}

#[cfg(all(test, unix))]
mod required_clone_tests {
    use super::*;
    #[test]
    fn unsupported_clone_never_creates_dense_destination_or_consumes_input() {
        let (_scope, profile, _, _) = super::super::tests::fixture();
        let source = profile.join("refusal-source");
        let target = profile.join("refusal-target");
        write_new(&source, b"preserved authenticated source").unwrap();
        let mut input = file(&source).unwrap();
        assert_eq!(
            dense_reference(&mut input, &target, 29, true)
                .unwrap_err()
                .code,
            "HOST_REFERENCE_CLONE_REQUIRED"
        );
        assert!(!target.exists());
        assert_eq!(input.stream_position().unwrap(), 0);
        assert_eq!(
            fs::read(&source).unwrap(),
            b"preserved authenticated source"
        );
    }
}
