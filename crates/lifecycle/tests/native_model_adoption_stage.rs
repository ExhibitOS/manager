// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use uuid::Uuid;

const ORIGINAL: &[u8] = b"synthetic original role bytes for adoption";
const DISPLAY: &[u8] = b"synthetic display role bytes for adoption!";

struct Stage {
    root: PathBuf,
    operation: Uuid,
    directory: File,
}

impl Stage {
    fn new() -> Self {
        Self::fixture(false)
    }

    fn empty() -> Self {
        Self::fixture(true)
    }

    fn fixture(empty: bool) -> Self {
        let root = std::env::temp_dir().join(format!("manager-adoption-{}", Uuid::new_v4()));
        let operation = Uuid::new_v4();
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        builder.create(&root).unwrap();
        let stage = root.join(operation.to_string());
        builder.create(&stage).unwrap();
        for (role, bytes) in [("original", ORIGINAL), ("display", DISPLAY)] {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(stage.join(role))
                .unwrap()
                .write_all(if empty { &[] } else { bytes })
                .unwrap();
        }
        let directory = File::open(&root).unwrap();
        Self {
            root,
            operation,
            directory,
        }
    }

    fn leaf(&self, role: &str) -> PathBuf {
        self.root.join(self.operation.to_string()).join(role)
    }

    fn request(&self, role: &str, bytes: &[u8]) -> Value {
        let stat = self.directory.metadata().unwrap();
        json!({
            "version": 1,
            "rootDevice": stat.dev().to_string(),
            "rootInode": stat.ino().to_string(),
            "operationId": self.operation,
            "role": role,
            "bytes": bytes.len(),
            "sha256": format!("{:x}", Sha256::digest(bytes)),
        })
    }

    fn invoke(&self, request: &Value) -> Output {
        self.invoke_wire(&serde_json::to_vec(request).unwrap())
    }

    fn invoke_wire(&self, request: &[u8]) -> Output {
        self.run("--model-adoption-stage-read-v1", request)
    }

    fn write(&self, request: &Value, body: &[u8]) -> Output {
        let request = serde_json::to_vec(request).unwrap();
        let mut wire = (request.len() as u32).to_be_bytes().to_vec();
        wire.extend(request);
        wire.extend(body);
        self.run("--model-adoption-stage-write-v1", &wire)
    }

    fn run(&self, operation: &str, request: &[u8]) -> Output {
        let root_fd = self.directory.as_raw_fd();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_exhibitos-native"));
        cmd.arg(operation)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(root_fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(request).unwrap();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn refused(output: Output) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"NATIVE_MODEL_ADOPTION_STAGE_INVALID\n");
}

#[test]
fn emits_exact_bytes_for_each_fixed_role() {
    let stage = Stage::new();
    for (role, bytes) in [("original", ORIGINAL), ("display", DISPLAY)] {
        let output = stage.invoke(&stage.request(role, bytes));
        assert!(output.status.success());
        assert_eq!(output.stdout, bytes);
        assert!(output.stderr.is_empty());
        assert_eq!(fs::read(stage.leaf(role)).unwrap(), bytes);
    }
}

#[test]
fn rejects_unbound_or_malformed_requests_without_output() {
    let stage = Stage::new();
    let valid = stage.request("original", ORIGINAL);
    for (key, value) in [
        ("rootDevice", json!("0")),
        ("rootInode", json!("0")),
        ("operationId", json!("../")),
        ("role", json!("../display")),
        ("bytes", json!(ORIGINAL.len() + 1)),
        ("sha256", json!("a".repeat(64))),
        ("path", json!(stage.leaf("original"))),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        refused(stage.invoke(&invalid));
    }
    refused(stage.invoke_wire(&[b' '; 1025]));
}

#[test]
fn rejects_unsafe_directory_and_leaf_without_output() {
    let stage = Stage::new();
    let request = stage.request("original", ORIGINAL);
    let original = stage.leaf("original");
    let display = stage.leaf("display");
    fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
    refused(stage.invoke(&request));
    fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();

    let link = stage.leaf("extra-link");
    fs::hard_link(&original, &link).unwrap();
    refused(stage.invoke(&request));
    fs::remove_file(&link).unwrap();

    fs::rename(&original, &link).unwrap();
    symlink(&link, &original).unwrap();
    refused(stage.invoke(&request));
    fs::remove_file(&original).unwrap();
    fs::rename(&link, &original).unwrap();
    assert_eq!(fs::read(display).unwrap(), DISPLAY);

    let op_directory = original.parent().unwrap();
    let moved = stage.root.join("other");
    fs::rename(op_directory, &moved).unwrap();
    symlink(Path::new("other"), op_directory).unwrap();
    refused(stage.invoke(&request));
}

#[test]
fn writer_accepts_only_exact_empty_roles_and_reader_observes_them() {
    let stage = Stage::empty();
    for (role, bytes) in [("original", ORIGINAL), ("display", DISPLAY)] {
        let request = stage.request(role, bytes);
        let output = stage.write(&request, bytes);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"{\"version\":1,\"outcome\":\"written\"}\n");
        assert!(output.stderr.is_empty());
        assert_eq!(stage.invoke(&request).stdout, bytes);
        refused(stage.write(&request, bytes));
        assert_eq!(fs::read(stage.leaf(role)).unwrap(), bytes);
    }
}

#[test]
fn writer_rejects_truncated_tampered_extra_and_unreserved_bytes() {
    let stage = Stage::empty();
    let request = stage.request("original", ORIGINAL);
    let mut extra = ORIGINAL.to_vec();
    extra.push(0);
    for bytes in [&ORIGINAL[1..], &vec![0; ORIGINAL.len()], &extra] {
        refused(stage.write(&request, bytes));
        assert_eq!(fs::metadata(stage.leaf("original")).unwrap().len(), 0);
    }
    fs::remove_file(stage.leaf("original")).unwrap();
    refused(stage.write(&request, ORIGINAL));
    assert!(!stage.leaf("original").exists());
    symlink(stage.leaf("display"), stage.leaf("original")).unwrap();
    refused(stage.write(&request, ORIGINAL));
    assert_eq!(fs::metadata(stage.leaf("display")).unwrap().len(), 0);
}
