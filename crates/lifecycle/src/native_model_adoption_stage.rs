// SPDX-License-Identifier: Apache-2.0
//! Reserve and transfer fixed adoption leaves under the Manager-pinned root.

use crate::{Result, err};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, Metadata},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::fs::MetadataExt,
    },
};
use uuid::Uuid;

const MAX_REQUEST_BYTES: u64 = 1024;
const MAX_MODEL_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    root_device: String,
    root_inode: String,
    operation_id: String,
    role: Role,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Original,
    Display,
}

impl Role {
    fn leaf(&self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Display => "display",
        }
    }
}

fn invalid() -> crate::LifecycleError {
    err("NATIVE_MODEL_ADOPTION_STAGE_INVALID")
}

fn metadata(file: &File) -> Result<Metadata> {
    file.metadata().map_err(|_| invalid())
}

pub(crate) fn private_directory(value: &Metadata) -> bool {
    value.is_dir()
        && value.uid() == unsafe { libc::geteuid() }
        && value.mode() & 0o7777 == 0o700
        && value.ino() != 0
}

fn private_leaf(value: &Metadata, bytes: u64) -> bool {
    value.is_file()
        && value.uid() == unsafe { libc::geteuid() }
        && value.mode() & 0o7777 == 0o600
        && value.ino() != 0
        && value.nlink() == 1
        && value.len() == bytes
}

fn same_inode(first: &Metadata, second: &Metadata) -> bool {
    first.dev() == second.dev() && first.ino() == second.ino()
}

fn same_observation(first: &Metadata, second: &Metadata) -> bool {
    same_inode(first, second)
        && (
            first.mtime(),
            first.mtime_nsec(),
            first.ctime(),
            first.ctime_nsec(),
        ) == (
            second.mtime(),
            second.mtime_nsec(),
            second.ctime(),
            second.ctime_nsec(),
        )
}

fn open_at(directory: &File, name: &str, flags: i32) -> Result<File> {
    let name = CString::new(name).map_err(|_| invalid())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600 as libc::c_int,
        )
    };
    if fd < 0 {
        return Err(invalid());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// FD 3 is the private staging root passed by the authenticated local runtime.
/// Output remains empty on validation failure. The caller must also verify
/// the bytes it ingests against the grant and wait for a successful exit.
pub fn read_inherited_stage(input: impl Read, mut output: impl Write) -> Result<()> {
    let mut wire = Vec::new();
    input
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut wire)
        .map_err(|_| invalid())?;
    if wire.len() as u64 > MAX_REQUEST_BYTES {
        return Err(invalid());
    }
    let request = parse_request(&wire)?;
    let (root, directory, directory_meta) = inherited_stage(&request)?;
    let name = request.role.leaf();
    let leaf = open_at(&directory, name, libc::O_RDONLY)?;
    let before = metadata(&leaf)?;
    if !private_leaf(&before, request.bytes) || before.dev() != directory_meta.dev() {
        return Err(invalid());
    }

    let mut reader = &leaf;
    let mut verified = vec![0; request.bytes as usize];
    reader.read_exact(&mut verified).map_err(|_| invalid())?;
    if reader.read(&mut [0]).map_err(|_| invalid())? != 0
        || format!("{:x}", Sha256::digest(&verified)) != request.sha256
    {
        return Err(invalid());
    }
    let after = metadata(&leaf)?;
    let directory_after = metadata(&open_at(&root, &request.operation_id, libc::O_DIRECTORY)?)?;
    let named_after = metadata(&open_at(&directory, name, libc::O_RDONLY)?)?;
    if !private_directory(&metadata(&root)?)
        || !private_directory(&directory_after)
        || !same_inode(&directory_meta, &directory_after)
        || !private_leaf(&after, request.bytes)
        || !private_leaf(&named_after, request.bytes)
        || !same_observation(&before, &after)
        || !same_observation(&after, &named_after)
    {
        return Err(invalid());
    }

    output
        .write_all(&verified)
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

fn parse_request(wire: &[u8]) -> Result<Request> {
    let request: Request = serde_json::from_slice(wire).map_err(|_| invalid())?;
    if request.version != 1
        || !Uuid::parse_str(&request.operation_id).is_ok_and(|id| {
            id.get_variant() == uuid::Variant::RFC4122
                && (1..=8).contains(&id.get_version_num())
                && id.to_string() == request.operation_id
        })
        || !(28..=MAX_MODEL_BYTES).contains(&request.bytes)
        || !crate::hash_valid(&request.sha256)
    {
        return Err(invalid());
    }

    Ok(request)
}

fn inherited_stage(request: &Request) -> Result<(File, File, Metadata)> {
    let root_fd = unsafe { libc::fcntl(3, libc::F_DUPFD_CLOEXEC, 10) };
    if root_fd < 0 {
        return Err(invalid());
    }
    let root = unsafe { File::from_raw_fd(root_fd) };
    let root_meta = metadata(&root)?;
    if !private_directory(&root_meta)
        || root_meta.dev().to_string() != request.root_device
        || root_meta.ino().to_string() != request.root_inode
    {
        return Err(invalid());
    }

    let directory = open_at(&root, &request.operation_id, libc::O_DIRECTORY)?;
    let directory_meta = metadata(&directory)?;
    if !private_directory(&directory_meta) || directory_meta.dev() != root_meta.dev() {
        return Err(invalid());
    }
    Ok((root, directory, directory_meta))
}

const WRITTEN: &[u8] = b"{\"version\":1,\"outcome\":\"written\"}\n";

/// Stage only an empty reserved leaf. A write or close failure retains it and
/// emits no acknowledgment. A second writer must never overwrite it.
pub fn write_inherited_stage(mut input: impl Read, mut output: impl Write) -> Result<()> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix).map_err(|_| invalid())?;
    let size = u64::from(u32::from_be_bytes(prefix));
    if !(1..=MAX_REQUEST_BYTES).contains(&size) {
        return Err(invalid());
    }
    let mut wire = vec![0; size as usize];
    input.read_exact(&mut wire).map_err(|_| invalid())?;
    let request = parse_request(&wire)?;
    let (root, directory, directory_meta) = inherited_stage(&request)?;
    let mut bytes = vec![0; request.bytes as usize];
    input.read_exact(&mut bytes).map_err(|_| invalid())?;
    if input.read(&mut [0]).map_err(|_| invalid())? != 0
        || format!("{:x}", Sha256::digest(&bytes)) != request.sha256
    {
        return Err(invalid());
    }
    let name = request.role.leaf();
    let mut leaf = open_at(&directory, name, libc::O_WRONLY)?;
    if unsafe { libc::flock(leaf.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(invalid());
    }
    let prior = metadata(&leaf)?;
    if !private_leaf(&prior, 0) || prior.dev() != directory_meta.dev() {
        return Err(invalid());
    }
    check_name(
        &root,
        &directory_meta,
        &request.operation_id,
        &directory,
        name,
        &prior,
        0,
    )?;
    leaf.write_all(&bytes)
        .and_then(|_| leaf.sync_all())
        .map_err(|_| invalid())?;
    let written = metadata(&leaf)?;
    if !same_inode(&prior, &written) {
        return Err(invalid());
    }
    check_name(
        &root,
        &directory_meta,
        &request.operation_id,
        &directory,
        name,
        &written,
        request.bytes,
    )?;
    directory.sync_all().map_err(|_| invalid())?;
    let fd = leaf.into_raw_fd();
    if unsafe { libc::close(fd) } != 0 {
        return Err(invalid());
    }
    check_name(
        &root,
        &directory_meta,
        &request.operation_id,
        &directory,
        name,
        &written,
        request.bytes,
    )?;
    output
        .write_all(WRITTEN)
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

fn check_name(
    root: &File,
    expected_directory: &Metadata,
    operation_id: &str,
    directory: &File,
    leaf: &str,
    expected_leaf: &Metadata,
    bytes: u64,
) -> Result<()> {
    let current_directory = metadata(&open_at(root, operation_id, libc::O_DIRECTORY)?)?;
    let named = metadata(&open_at(directory, leaf, libc::O_RDONLY)?)?;
    if !private_directory(&metadata(root)?)
        || !private_directory(&current_directory)
        || !same_inode(expected_directory, &current_directory)
        || !private_leaf(expected_leaf, bytes)
        || !private_leaf(&named, bytes)
        || !same_observation(expected_leaf, &named)
    {
        return Err(invalid());
    }
    Ok(())
}

/// The caller retains custody. An incomplete reservation is preserved and
/// cannot be adopted by another attempt.
pub(crate) struct Reservation {
    pub(crate) operation: Uuid,
    root: File,
    directory: File,
    leaves: [File; 2],
}

impl Reservation {
    pub(crate) fn create(root: File, operation: Uuid) -> Result<Self> {
        let root_meta = metadata(&root)?;
        if !private_directory(&root_meta)
            || operation.get_version_num() != 4
            || operation.get_variant() != uuid::Variant::RFC4122
        {
            return Err(invalid());
        }
        let id = operation.to_string();
        let name = CString::new(id.as_str()).map_err(|_| invalid())?;
        if unsafe { libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(invalid());
        }
        let directory = open_at(&root, &id, libc::O_DIRECTORY)?;
        let directory_meta = metadata(&directory)?;
        if !private_directory(&directory_meta) || directory_meta.dev() != root_meta.dev() {
            return Err(invalid());
        }
        let create = |role: Role| -> Result<File> {
            let file = open_at(
                &directory,
                role.leaf(),
                libc::O_RDONLY | libc::O_CREAT | libc::O_EXCL,
            )?;
            let stat = metadata(&file)?;
            if stat.dev() != directory_meta.dev() {
                return Err(invalid());
            }
            check_name(
                &root,
                &directory_meta,
                &id,
                &directory,
                role.leaf(),
                &stat,
                0,
            )?;
            file.sync_all().map_err(|_| invalid())?;
            Ok(file)
        };
        let leaves = [create(Role::Original)?, create(Role::Display)?];
        directory
            .sync_all()
            .and_then(|_| root.sync_all())
            .map_err(|_| invalid())?;
        Ok(Self {
            operation,
            root,
            directory,
            leaves,
        })
    }

    pub(crate) fn matches_root(&self, root: &File) -> Result<bool> {
        let original = metadata(&self.root)?;
        let current = metadata(root)?;
        Ok(private_directory(&original)
            && private_directory(&current)
            && same_inode(&original, &current))
    }

    pub(crate) fn verify(&self, original: (u64, &str), display: (u64, &str)) -> Result<()> {
        let directory_meta = metadata(&self.directory)?;
        let id = self.operation.to_string();
        for ((role, file), (size, hash)) in [Role::Original, Role::Display]
            .into_iter()
            .zip(&self.leaves)
            .zip([original, display])
        {
            use std::os::unix::fs::FileExt;
            if !(28..=MAX_MODEL_BYTES).contains(&size) || !crate::hash_valid(hash) {
                return Err(invalid());
            }
            let prior = metadata(file)?;
            check_name(
                &self.root,
                &directory_meta,
                &id,
                &self.directory,
                role.leaf(),
                &prior,
                size,
            )?;
            let mut digest = Sha256::new();
            let mut offset = 0;
            let mut block = [0; 65536];
            while offset < size {
                let count = (size - offset).min(block.len() as u64) as usize;
                file.read_exact_at(&mut block[..count], offset)
                    .map_err(|_| invalid())?;
                digest.update(&block[..count]);
                offset += count as u64;
            }
            if format!("{:x}", digest.finalize()) != hash
                || !same_observation(&prior, &metadata(file)?)
            {
                return Err(invalid());
            }
            check_name(
                &self.root,
                &directory_meta,
                &id,
                &self.directory,
                role.leaf(),
                &prior,
                size,
            )?;
        }
        Ok(())
    }

    /// Only the exact target's confirmed, durable settlement may call this.
    /// Uncertainty never authorizes a repeated unlink.
    pub(crate) fn retire(&self, original: (u64, &str), display: (u64, &str)) -> Result<()> {
        self.verify(original, display)?;
        let directory = metadata(&self.directory)?;
        let operation = self.operation.to_string();
        for ((role, file), (bytes, _)) in [Role::Original, Role::Display]
            .into_iter()
            .zip(&self.leaves)
            .zip([original, display])
        {
            let before = metadata(file)?;
            check_name(
                &self.root,
                &directory,
                &operation,
                &self.directory,
                role.leaf(),
                &before,
                bytes,
            )?;
            let name = CString::new(role.leaf()).map_err(|_| invalid())?;
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                return Err(invalid());
            }
            let after = metadata(file)?;
            if !same_inode(&before, &after)
                || after.nlink() != 0
                || after.len() != bytes
                || (before.mtime(), before.mtime_nsec()) != (after.mtime(), after.mtime_nsec())
            {
                return Err(invalid());
            }
        }
        self.directory.sync_all().map_err(|_| invalid())?;
        let current = metadata(&open_at(&self.root, &operation, libc::O_DIRECTORY)?)?;
        if !private_directory(&current) || !same_inode(&directory, &current) {
            return Err(invalid());
        }
        let name = CString::new(operation).map_err(|_| invalid())?;
        if unsafe { libc::unlinkat(self.root.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
        {
            return Err(invalid());
        }
        self.root.sync_all().map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    #[test]
    fn exact_settled_pair_can_retire_without_touching_another_operation() {
        let root_path = std::env::temp_dir().join(format!("adoption-retire-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root_path)
            .unwrap();
        let root = File::open(&root_path).unwrap();
        let operation = Uuid::new_v4();
        let stage = Reservation::create(root.try_clone().unwrap(), operation).unwrap();
        let other = Reservation::create(root, Uuid::new_v4()).unwrap();
        let path = root_path.join(operation.to_string());
        let bytes = vec![b'z'; 32];
        let hash = format!("{:x}", Sha256::digest(&bytes));
        for role in ["original", "display"] {
            fs::write(path.join(role), &bytes).unwrap();
        }
        assert!(stage.retire((32, &hash), (32, &"a".repeat(64))).is_err());
        assert!(path.join("original").exists() && path.join("display").exists());
        stage.retire((32, &hash), (32, &hash)).unwrap();
        assert!(!path.exists());
        assert!(
            root_path
                .join(other.operation.to_string())
                .join("original")
                .exists()
        );
        assert!(stage.retire((32, &hash), (32, &hash)).is_err());
        drop(stage);
        drop(other);
        fs::remove_dir_all(root_path).unwrap();
    }

    #[test]
    fn reservation_retains_exact_leaves_and_never_reopens_a_prior_operation() {
        let root_path = std::env::temp_dir().join(format!("adoption-reserve-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root_path)
            .unwrap();
        let root = File::open(&root_path).unwrap();
        let operation = Uuid::new_v4();
        let stage = Reservation::create(root.try_clone().unwrap(), operation).unwrap();
        assert!(stage.matches_root(&root).unwrap());
        assert!(Reservation::create(root, operation).is_err());
        let path = root_path.join(operation.to_string());
        let original = vec![b'o'; 32];
        let display = vec![b'd'; 48];
        fs::write(path.join("original"), &original).unwrap();
        fs::write(path.join("display"), &display).unwrap();
        let identity = |bytes: &[u8]| (bytes.len() as u64, format!("{:x}", Sha256::digest(bytes)));
        let (size, hash) = identity(&original);
        let (display_size, display_hash) = identity(&display);
        assert!(
            stage
                .verify((size, &hash), (display_size, &display_hash))
                .is_ok()
        );
        fs::rename(path.join("original"), path.join("old")).unwrap();
        let mut replacement = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path.join("original"))
            .unwrap();
        replacement.write_all(&original).unwrap();
        assert!(
            stage
                .verify((size, &hash), (display_size, &display_hash))
                .is_err()
        );
        drop(stage);
        assert!(path.join("display").exists());
        fs::remove_dir_all(root_path).unwrap();
    }
}
