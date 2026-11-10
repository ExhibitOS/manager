// SPDX-License-Identifier: Apache-2.0
//! Read one fixed staging leaf through the directory inherited from Manager.

use crate::{Result, err};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, Metadata},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
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

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
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

fn private_directory(value: &Metadata) -> bool {
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

fn open_leaf(directory: &File, name: &str, flags: i32) -> Result<File> {
    let name = CString::new(name).map_err(|_| invalid())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
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
    let request: Request = serde_json::from_slice(&wire).map_err(|_| invalid())?;
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

    let directory = open_leaf(&root, &request.operation_id, libc::O_DIRECTORY)?;
    let directory_meta = metadata(&directory)?;
    if !private_directory(&directory_meta) || directory_meta.dev() != root_meta.dev() {
        return Err(invalid());
    }
    let name = request.role.leaf();
    let leaf = open_leaf(&directory, name, 0)?;
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
    let directory_after = metadata(&open_leaf(&root, &request.operation_id, libc::O_DIRECTORY)?)?;
    let named_after = metadata(&open_leaf(&directory, name, 0)?)?;
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
