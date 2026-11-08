// SPDX-License-Identifier: Apache-2.0
//! Offline refresh of an already exported local publication.
use crate::native_payload::{Payload, StaticRefreshRuntimeFile};
use crate::{Result, digest, err, hash_valid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString, OsStr},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt, process::CommandExt},
    },
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const MANIFEST: &str = "static-export-manifest.json";
const INDEX: &str = "static-publication-index.json";
const SCRIPT: &str = "platform/scripts/refresh-static-publication.mjs";
const REQUEST_LIMIT: usize = 64 * 1024;
const MANIFEST_LIMIT: u64 = 2 * 1024 * 1024;
const FILE_LIMIT: u64 = 25 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 512 * 1024 * 1024;
const FILES_LIMIT: usize = 20_000;
const ENTRIES_LIMIT: usize = 40_000;
const DEPTH_LIMIT: usize = 32;

type PublicationPin = (&'static str, &'static str, &'static str, &'static str);
type RuntimePin = (&'static str, &'static str, &'static str);

// Each independently reviewed offline publication is paired with one exact
// validation runtime. These pins do not admit an authoring profile or release.
const SOURCE_PUBLICATION: PublicationPin = (
    "625618bf-8b63-448c-abd6-08f1bb7dd68e",
    "34024d16fdce3c1fe44a550217352df6545f6821d26fcaf0893350c70a5311f2",
    "896f66289538e47f9bc182a0458089b5308c4535babbd998e84cdeaa7b34a1c9",
    "40197a09b809e8a46fc45ef1eb2fb9f701e05dd4b5273499aac0b8b083604a0a",
);

// Independently verified Platform PR #415 validation package.
const RUNTIME: RuntimePin = (
    "b9f8a1d716db67f2820b3ca552b781092d12ef3d",
    "ee5a94b613031e2cea918dead21c885c42fce4d585a03e55a2f3799af4e6991c",
    "5efbca58c9ca7f081746d30d3c56d7c8992b415f5f23a9420d4b03ecdb901f6b",
);
const HAYES_R5_PUBLICATION: PublicationPin = (
    "01ccb3ba-9951-4ce5-97bf-68c8079b8ea0",
    "af46cc20d7aae7189f99d939f6a1d2d21ae6e6057ffcc03ad9115fc10a42331e",
    "78d4837e048dd8ad79270000a00d62bcf16c6be185bda600c9b3839112eaa226",
    "bc2d637914b33a010f95eab8ea0bd3a8964bf51e6901ee7fde2f9e229d98d8ce",
);
const HAYES_ROTATED_PUBLICATION: PublicationPin = (
    "ddb30826-98b2-43e3-a997-5e08281ead46",
    "4c8b7ce8a01011b9ce1ae55eeec6629d017357c4f604d8fe807141b0ba3c0a19",
    "d736114b558e7578c4edac5ddba44bc8778740b27b78e6f74769fb261f9db997",
    "a7aaf885534b62c98df436cea6d6a0b7f51c25add58d9652b3572981e52d6762",
);
const HAYES_EDITOR_CONTENT: [&str; 4] = [
    "furnish-catalog/backless-oak-bench/1/LICENSE-NOTICE.txt",
    "furnish-catalog/backless-oak-bench/1/model.glb",
    "material-catalogue-thumbnails/fine-gallery-concrete/1/preview-82b907f0df15.png",
    "material-catalogue-thumbnails/quiet-oak-herringbone/1/preview-4423a1fc11dc.png",
];
const HAYES_KNOWN_MEMORY_RUNTIME: RuntimePin = (
    "bf3b4929aab96baf4ec896ee23e318ea70e6c78b",
    "2627954c7ac6c083bed01470db1e3ccec94e857ea2858babca1b4ed1e740adad",
    "29e0c0685bd279fce60268ff9a1d99299ce223dbecff3930106d39def8c8202f",
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation_id: String,
    source_dir: PathBuf,
    source_manifest_sha256: String,
    index_sha256: String,
    publication_id: String,
    revision_sha256: String,
    target_manifest_sha256: String,
    output_dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChildRequest<'a> {
    source_dir: &'a Path,
    source_manifest_sha256: &'a str,
    index_sha256: &'a str,
    publication_id: &'a str,
    revision_sha256: &'a str,
    target_runtime_dir: &'a Path,
    target_manifest_sha256: &'a str,
    staged_output_dir: &'a Path,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChildReceipt {
    publication_id: String,
    revision_sha256: String,
    index_sha256: String,
    manifest_sha256: String,
    files: u64,
    bytes: u64,
    output: PathBuf,
    stage_status: String,
    source_manifest_sha256: String,
    target_manifest_sha256: String,
    stage_directory: PathBuf,
    stage_cleanup: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    operation_id: String,
    publication_id: String,
    revision_sha256: String,
    index_sha256: String,
    manifest_sha256: String,
    files: u64,
    bytes: u64,
    output: PathBuf,
    source_manifest_sha256: String,
    target_manifest_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitRecord {
    candidate_device: u64,
    candidate_inode: u64,
    receipt: Receipt,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    publication_id: String,
    revision_sha256: String,
    index_sha256: String,
    files: Vec<Entry>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: u64,
    sha256: String,
}

fn invalid() -> crate::LifecycleError {
    err("NATIVE_STATIC_REFRESH_INVALID")
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(err("NATIVE_STATIC_REFRESH_CANCELLED"));
    }
    Ok(())
}

fn c_name(name: &OsStr) -> Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| invalid())
}

fn open_at(parent: &File, name: &OsStr, flags: libc::c_int) -> Result<File> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(invalid());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_directory(path: &Path) -> Result<File> {
    if !path.is_absolute() || path.to_str().is_none() || path.as_os_str().len() > 4096 {
        return Err(invalid());
    }
    let mut held = File::open("/").map_err(|_| invalid())?;
    let mut normalized = PathBuf::from("/");
    for part in path.components().skip(1) {
        let Component::Normal(part) = part else {
            return Err(invalid());
        };
        held = open_at(&held, part, libc::O_RDONLY | libc::O_DIRECTORY)?;
        normalized.push(part);
    }
    if normalized.as_os_str() != path.as_os_str() {
        return Err(invalid());
    }
    Ok(held)
}

fn identity(file: &File) -> Result<(u64, u64)> {
    let stat = file.metadata().map_err(|_| invalid())?;
    Ok((stat.dev(), stat.ino()))
}

fn verify_named(path: &Path, held: &File) -> Result<()> {
    if identity(&open_directory(path)?)? != identity(held)? {
        return Err(invalid());
    }
    Ok(())
}

fn private_child(parent: &File, name: &str) -> Result<File> {
    let c = c_name(OsStr::new(name))?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
        return Err(invalid());
    }
    open_at(parent, OsStr::new(name), libc::O_RDONLY | libc::O_DIRECTORY)
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

fn list(directory: &File, remaining: usize) -> Result<Vec<String>> {
    let duplicate = open_at(
        directory,
        OsStr::new("."),
        libc::O_RDONLY | libc::O_DIRECTORY,
    )?;
    let fd = duplicate.as_raw_fd();
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        return Err(invalid());
    }
    let _ = duplicate.into_raw_fd(); // fdopendir owns it on success
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        let next = unsafe {
            *libc::__error() = 0;
            libc::readdir(stream.0)
        };
        if next.is_null() {
            return if unsafe { *libc::__error() } == 0 {
                Ok(names)
            } else {
                Err(invalid())
            };
        }
        let name = unsafe { CStr::from_ptr((*next).d_name.as_ptr()) }
            .to_str()
            .map_err(|_| invalid())?;
        if matches!(name, "." | "..") {
            continue;
        }
        if names.len() >= remaining {
            return Err(invalid());
        }
        names.push(name.to_owned());
    }
}

fn safe_path(name: &str) -> bool {
    name.len() <= 240
        && name.split('/').all(|part| {
            !part.ends_with('.')
                && part
                    .bytes()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        })
}

fn regular(file: &File, maximum: u64) -> Result<fs::Metadata> {
    let info = file.metadata().map_err(|_| invalid())?;
    if !info.is_file() || info.nlink() != 1 || info.len() > maximum {
        return Err(invalid());
    }
    Ok(info)
}

fn stamp(m: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

fn read_manifest(root: &File, expected: &str, request: &Request) -> Result<(Manifest, Vec<u8>)> {
    let file = open_at(root, OsStr::new(MANIFEST), libc::O_RDONLY)?;
    let before = regular(&file, MANIFEST_LIMIT)?;
    let mut bytes = Vec::new();
    (&file)
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != before.len()
        || stamp(&regular(&file, MANIFEST_LIMIT)?) != stamp(&before)
        || digest(&bytes) != expected
    {
        return Err(invalid());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if manifest.format != "exhibitos-static-publication-v1"
        || manifest.publication_id != request.publication_id
        || manifest.revision_sha256 != request.revision_sha256
        || manifest.index_sha256 != request.index_sha256
        || manifest.files.is_empty()
        || manifest.files.len() > FILES_LIMIT
    {
        return Err(invalid());
    }
    Ok((manifest, bytes))
}

fn index_entries(manifest: &Manifest) -> Result<BTreeMap<&str, &Entry>> {
    let mut entries = BTreeMap::new();
    let mut bytes = 0;
    for entry in &manifest.files {
        let name = entry.path.as_str();
        if name == MANIFEST
            || !safe_path(name)
            || !hash_valid(&entry.sha256)
            || entry.bytes > FILE_LIMIT
            || entries.insert(name, entry).is_some()
        {
            return Err(invalid());
        }
        bytes += entry.bytes;
        if bytes > TOTAL_LIMIT {
            return Err(invalid());
        }
    }
    if entries.get(INDEX).map(|entry| entry.sha256.as_str()) != Some(&manifest.index_sha256)
        || !entries.contains_key("index.html")
    {
        return Err(invalid());
    }
    Ok(entries)
}

fn verify_file(
    mut source: File,
    mut target: Option<File>,
    entry: &Entry,
    stop: &AtomicBool,
) -> Result<()> {
    let before = regular(&source, entry.bytes)?;
    if before.len() != entry.bytes {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut remaining = entry.bytes;
    let mut chunk = [0; 64 * 1024];
    while remaining != 0 {
        check_cancelled(stop)?;
        let limit = remaining.min(chunk.len() as u64) as usize;
        source
            .read_exact(&mut chunk[..limit])
            .map_err(|_| invalid())?;
        hash.update(&chunk[..limit]);
        if let Some(file) = &mut target {
            file.write_all(&chunk[..limit]).map_err(|_| invalid())?;
        }
        remaining -= limit as u64;
    }
    if source.read(&mut chunk[..1]).map_err(|_| invalid())? != 0
        || stamp(&regular(&source, entry.bytes)?) != stamp(&before)
        || format!("{:x}", hash.finalize()) != entry.sha256
    {
        return Err(invalid());
    }
    if let Some(file) = target {
        file.sync_all().map_err(|_| invalid())?;
    }
    Ok(())
}

struct Walk<'a> {
    entries: BTreeMap<&'a str, &'a Entry>,
    seen: BTreeSet<String>,
    directories: BTreeSet<&'a str>,
    durable: bool,
    visited: usize,
    stop: &'a AtomicBool,
}

impl Walk<'_> {
    fn tree(
        &mut self,
        source: &File,
        target: Option<&File>,
        prefix: &str,
        depth: usize,
    ) -> Result<()> {
        check_cancelled(self.stop)?;
        if depth > DEPTH_LIMIT {
            return Err(invalid());
        }
        let names = list(source, ENTRIES_LIMIT - self.visited)?;
        self.visited += names.len();
        for name in names {
            let path = format!("{prefix}{name}");
            if !safe_path(&path) {
                return Err(invalid());
            }
            if path == MANIFEST {
                if self.durable {
                    open_at(source, OsStr::new(MANIFEST), libc::O_RDONLY)?
                        .sync_all()
                        .map_err(|_| invalid())?;
                }
                continue;
            }
            let leaf = OsStr::new(&name);
            let file = open_at(source, leaf, libc::O_RDONLY)?;
            if file.metadata().map_err(|_| invalid())?.is_dir() {
                if !self.directories.contains(path.as_str()) {
                    return Err(invalid());
                }
                let child = target
                    .map(|parent| private_child(parent, &name))
                    .transpose()?;
                self.tree(&file, child.as_ref(), &format!("{path}/"), depth + 1)?;
                if let Some(child) = child {
                    child.sync_all().map_err(|_| invalid())?;
                }
            } else {
                let entry = self.entries.get(path.as_str()).ok_or_else(invalid)?;
                let copy = target
                    .map(|parent| {
                        open_at(parent, leaf, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
                    })
                    .transpose()?;
                if self.durable {
                    file.sync_all().map_err(|_| invalid())?;
                }
                verify_file(file, copy, entry, self.stop)?;
                if !self.seen.insert(path) {
                    return Err(invalid());
                }
            }
        }
        if self.durable {
            source.sync_all().map_err(|_| invalid())?;
        }
        Ok(())
    }
}

fn verify_tree(
    source: &File,
    snapshot: Option<&File>,
    manifest: &Manifest,
    stop: &AtomicBool,
    durable: bool,
) -> Result<()> {
    let entries = index_entries(manifest)?;
    let directories = entries
        .keys()
        .flat_map(|path| path.match_indices('/').map(|(i, _)| &path[..i]))
        .collect();
    let mut walk = Walk {
        entries,
        directories,
        durable,
        seen: BTreeSet::new(),
        visited: 0,
        stop,
    };
    walk.tree(source, snapshot, "", 0)?;
    if walk.seen.len() != walk.entries.len() {
        return Err(invalid());
    }
    Ok(())
}

fn unchanged_content(before: &Manifest, after: &Manifest) -> Result<()> {
    let hayes_editor = [HAYES_R5_PUBLICATION.0, HAYES_ROTATED_PUBLICATION.0]
        .contains(&before.publication_id.as_str());
    let content = |path: &str| {
        path == INDEX
            || path.starts_with("data/")
            || path.starts_with("floor-textures/")
            || path == "floor-texture-notices.txt"
            || path.starts_with("surface-material-textures/")
            || path.starts_with("surface-material-notices/")
            || (hayes_editor && HAYES_EDITOR_CONTENT.contains(&path))
    };
    let shell = |path: &str| {
        matches!(
            path,
            "index.html"
                | "studio.html"
                | "THIRD_PARTY_NOTICES.txt"
                | "viewer-favicon.svg"
                | "voice-pcm-worklet.js"
        ) || ["assets/", "basis/"].iter().any(|prefix| {
            path.strip_prefix(prefix)
                .is_some_and(|name| !name.contains('/'))
        })
    };
    if before
        .files
        .iter()
        .chain(&after.files)
        .any(|entry| !content(&entry.path) && !shell(&entry.path))
    {
        return Err(err("NATIVE_STATIC_REFRESH_CONTENT_CHANGED"));
    }
    let before = before
        .files
        .iter()
        .filter(|entry| content(&entry.path))
        .map(|entry| (&entry.path, entry))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .files
        .iter()
        .filter(|entry| content(&entry.path))
        .map(|entry| (&entry.path, entry))
        .collect::<BTreeMap<_, _>>();
    if before != after {
        return Err(err("NATIVE_STATIC_REFRESH_CONTENT_CHANGED"));
    }
    Ok(())
}

pub fn read_request(fd: RawFd, stop: &AtomicBool) -> Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        check_cancelled(stop)?;
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(invalid());
        }
        if ready == 0 {
            continue;
        }
        let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count == 0 {
            return Ok(bytes);
        }
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(invalid());
        }
        if bytes.len() + count as usize > REQUEST_LIMIT {
            return Err(invalid());
        }
        bytes.extend_from_slice(&buffer[..count as usize]);
    }
}

fn promote(from: &File, source: &OsStr, to: &File, destination: &OsStr) -> Result<()> {
    let source = c_name(source)?;
    let destination = c_name(destination)?;
    if unsafe {
        libc::renameatx_np(
            from.as_raw_fd(),
            source.as_ptr(),
            to.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err(
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                err("NATIVE_STATIC_REFRESH_OUTPUT_EXISTS")
            } else {
                invalid()
            },
        );
    }
    Ok(())
}

fn clear_tree(directory: &File, depth: usize, visited: &mut usize) -> Result<()> {
    if depth > DEPTH_LIMIT {
        return Err(invalid());
    }
    let mut names = list(directory, 200_000 - *visited)?;
    *visited += names.len();
    if depth == 0 {
        names.sort_by_key(|name| name == "commit.json");
    }
    for name in names {
        let raw = c_name(OsStr::new(&name))?;
        let flags = if let Ok(child) = open_at(
            directory,
            OsStr::new(&name),
            libc::O_RDONLY | libc::O_DIRECTORY,
        ) {
            clear_tree(&child, depth + 1, visited)?;
            let current = open_at(
                directory,
                OsStr::new(&name),
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
            if identity(&child)? != identity(&current)? {
                return Err(invalid());
            }
            libc::AT_REMOVEDIR
        } else {
            0
        };
        if unsafe { libc::unlinkat(directory.as_raw_fd(), raw.as_ptr(), flags) } != 0 {
            return Err(invalid());
        }
    }
    Ok(())
}

struct Operation {
    path: PathBuf,
    parent_path: PathBuf,
    parent: File,
    root: File,
    drained: bool,
}

impl Operation {
    fn clean(&mut self) -> bool {
        if !self.drained
            || verify_named(&self.parent_path, &self.parent).is_err()
            || verify_named(&self.path, &self.root).is_err()
        {
            return false;
        }
        self.drained = false; // never attempt a second deletion through the name
        let cleanup = || -> Result<()> {
            clear_tree(&self.root, 0, &mut 0)?;
            verify_named(&self.path, &self.root)?;
            let name = c_name(self.path.file_name().ok_or_else(invalid)?)?;
            if unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) }
                != 0
            {
                return Err(invalid());
            }
            self.parent.sync_all().map_err(|_| invalid())
        };
        cleanup().is_ok()
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.clean();
    }
}

fn run_child(
    payload: &Payload,
    runtime: &Path,
    script: &Path,
    worker: &Path,
    snapshot: &Path,
    op: &mut Operation,
    request: &[u8],
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    let file = |name, flags| open_at(&op.root, OsStr::new(name), flags);
    let mut input = file(
        "request.json",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    input.write_all(request).map_err(|_| invalid())?;
    drop(input);
    let input = file("request.json", libc::O_RDONLY)?;
    let stdout = file(
        "receipt.json",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    let stderr = file("error.txt", libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
    let output = file("receipt.json", libc::O_RDONLY)?;
    let errors = file("error.txt", libc::O_RDONLY)?;
    check_cancelled(stop)?;
    if [runtime, worker, snapshot].iter().any(|path| {
        path.as_os_str()
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b'*' | b','))
    }) {
        return Err(invalid());
    }
    let mut command = Command::new(payload.node());
    command
        .arg("--permission")
        .arg("--no-addons")
        .arg(format!("--allow-fs-read={}", runtime.display()))
        .arg(format!("--allow-fs-read={}", snapshot.display()))
        .arg(format!("--allow-fs-read={}", worker.display()))
        .arg(format!("--allow-fs-write={}", worker.display()))
        .arg(script)
        .current_dir(worker)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .env("TMPDIR", worker.join("tmp"))
        .process_group(0)
        .stdin(input)
        .stdout(stdout)
        .stderr(stderr);
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: FILE_LIMIT,
                rlim_max: FILE_LIMIT,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|_| err("NATIVE_STATIC_REFRESH_CHILD_FAILED"))?;
    op.drained = false;
    let group = -(child.id() as libc::pid_t);
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut stopping = None;
    let mut forced = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                unsafe {
                    libc::kill(group, libc::SIGKILL);
                }
                let _ = child.wait();
                return Err(err("NATIVE_STATIC_REFRESH_DRAIN_UNCERTAIN"));
            }
        }
        let overflow = [&output, &errors].iter().any(|file| {
            file.metadata()
                .map_or(true, |m| m.len() > REQUEST_LIMIT as u64)
        });
        if stopping.is_none()
            && (stop.load(Ordering::SeqCst) || overflow || Instant::now() >= deadline)
        {
            unsafe {
                libc::kill(group, libc::SIGTERM);
            }
            stopping = Some(Instant::now() + Duration::from_secs(5));
        }
        if forced.is_none() && stopping.is_some_and(|until| Instant::now() >= until) {
            unsafe {
                libc::kill(group, libc::SIGKILL);
            }
            forced = Some(Instant::now() + Duration::from_secs(5));
        }
        if forced.is_some_and(|until| Instant::now() >= until) {
            return Err(err("NATIVE_STATIC_REFRESH_DRAIN_UNCERTAIN"));
        }
        thread::sleep(Duration::from_millis(20));
    };
    // The fixed child must not leave a descendant owning this private stage.
    if unsafe { libc::kill(group, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        unsafe {
            libc::kill(group, libc::SIGKILL);
        }
        return Err(err("NATIVE_STATIC_REFRESH_DRAIN_UNCERTAIN"));
    }
    op.drained = true;
    check_cancelled(stop)?;
    if stopping.is_some() {
        return Err(err("NATIVE_STATIC_REFRESH_CHILD_FAILED"));
    }
    if !status.success() {
        let mut error = String::new();
        errors
            .take(129)
            .read_to_string(&mut error)
            .map_err(|_| invalid())?;
        let code = match error.strip_suffix('\n') {
            Some("REFRESH_CONTENT_CHANGED" | "REFRESH_CONTENT_UNCLASSIFIED") => {
                "NATIVE_STATIC_REFRESH_CONTENT_CHANGED"
            }
            Some("REFRESH_SOURCE_INVALID") => "NATIVE_STATIC_REFRESH_SOURCE_INVALID",
            Some("REFRESH_RUNTIME_INVALID") => "NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED",
            _ => "NATIVE_STATIC_REFRESH_CHILD_FAILED",
        };
        return Err(err(code));
    }
    let before = regular(&output, REQUEST_LIMIT as u64)?;
    let mut reply = Vec::new();
    output
        .take(REQUEST_LIMIT as u64 + 1)
        .read_to_end(&mut reply)
        .map_err(|_| invalid())?;
    if reply.len() as u64 != before.len() {
        return Err(invalid());
    }
    Ok(reply)
}

fn runtime_parent(root: &File, path: &Path, create: bool) -> Result<File> {
    let mut directory = root.try_clone().map_err(|_| invalid())?;
    for part in path.components() {
        let Component::Normal(name) = part else {
            return Err(invalid());
        };
        if create {
            let name = c_name(name)?;
            if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
            {
                return Err(invalid());
            }
        }
        directory = open_at(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    }
    Ok(directory)
}

fn copy_runtime_file(
    mut input: File,
    output_parent: &File,
    name: &OsStr,
    bytes: u64,
    executable: bool,
    stop: &AtomicBool,
) -> Result<()> {
    let before = regular(&input, bytes)?;
    if before.len() != bytes {
        return Err(invalid());
    }
    let raw = c_name(name)?;
    // The same held-FD APFS primitive used for native stream staging.
    let cloned = unsafe {
        libc::fclonefileat(
            input.as_raw_fd(),
            output_parent.as_raw_fd(),
            raw.as_ptr(),
            0x0004 | 0x0008,
        )
    };
    if cloned != 0 {
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOTSUP | libc::EXDEV | libc::ENOSYS) => {}
            _ => return Err(invalid()),
        }
        let mut output = open_at(
            output_parent,
            name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let mut left = bytes;
        let mut buffer = [0; 64 * 1024];
        while left != 0 {
            check_cancelled(stop)?;
            let count = left.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..count])
                .map_err(|_| invalid())?;
            output.write_all(&buffer[..count]).map_err(|_| invalid())?;
            left -= count as u64;
        }
    }
    let copy = open_at(output_parent, name, libc::O_RDONLY)?;
    let mode = if executable { 0o500 } else { 0o400 };
    if unsafe { libc::fchmod(copy.as_raw_fd(), mode) } != 0
        || stamp(&regular(&input, bytes)?) != stamp(&before)
    {
        return Err(invalid());
    }
    Ok(())
}

fn verify_runtime_link(
    directory: &File,
    relative: &Path,
    target: &str,
    declared_links: &BTreeMap<&Path, &str>,
) -> Result<()> {
    let parent = relative.parent().ok_or_else(invalid)?;
    let mut name = relative.file_name().ok_or_else(invalid)?.to_owned();
    for _ in 0..DEPTH_LIMIT {
        let raw = c_name(&name)?;
        let mut buffer = [0u8; 2049];
        let count = unsafe {
            libc::readlinkat(
                directory.as_raw_fd(),
                raw.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if count <= 0 || count as usize >= buffer.len() {
            return Err(invalid());
        }
        let link = &buffer[..count as usize];
        if link == target.as_bytes() {
            return Ok(());
        }
        // The producer records canonical targets. A sibling alias can lead
        // there if the manifest also declares that alias and the same target.
        let hop = Path::new(OsStr::from_bytes(link));
        if hop.file_name() != Some(hop.as_os_str())
            || declared_links.get(parent.join(hop).as_path()) != Some(&target)
        {
            return Err(invalid());
        }
        name = hop.as_os_str().to_owned();
    }
    Err(invalid())
}

fn snapshot_runtime(
    root: &File,
    target: &File,
    tuple: (&str, &str, &str),
    stop: &AtomicBool,
) -> Result<()> {
    let manifest = open_at(root, OsStr::new("manifest.json"), libc::O_RDONLY)?;
    let maximum = 16 * 1024 * 1024;
    regular(&manifest, maximum)?;
    let mut bytes = Vec::new();
    manifest
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > maximum {
        return Err(invalid());
    }
    let inventory = Payload::static_refresh_inventory(&bytes, tuple)?;
    let declared_links = inventory
        .iter()
        .filter_map(|entry| match entry {
            StaticRefreshRuntimeFile::Link { path, target } => {
                Some((Path::new(path), target.as_str()))
            }
            StaticRefreshRuntimeFile::Regular { .. } => None,
        })
        .collect();
    for entry in &inventory {
        check_cancelled(stop)?;
        let relative = match entry {
            StaticRefreshRuntimeFile::Regular { path, .. }
            | StaticRefreshRuntimeFile::Link { path, .. } => Path::new(path),
        };
        let name = relative.file_name().ok_or_else(invalid)?;
        let prefix = relative.parent().ok_or_else(invalid)?;
        let from = runtime_parent(root, prefix, false)?;
        let to = runtime_parent(target, prefix, true)?;
        match entry {
            StaticRefreshRuntimeFile::Regular {
                bytes, executable, ..
            } => {
                copy_runtime_file(
                    open_at(&from, name, libc::O_RDONLY)?,
                    &to,
                    name,
                    *bytes,
                    *executable,
                    stop,
                )?;
            }
            StaticRefreshRuntimeFile::Link { target, .. } => {
                verify_runtime_link(&from, relative, target, &declared_links)?;
                let name = c_name(name)?;
                let target = c_name(OsStr::new(target))?;
                if unsafe { libc::symlinkat(target.as_ptr(), to.as_raw_fd(), name.as_ptr()) } != 0 {
                    return Err(invalid());
                }
            }
        }
    }
    let mut manifest = open_at(
        target,
        OsStr::new("manifest.json"),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    manifest.write_all(&bytes).map_err(|_| invalid())
}

fn validate_request(input: &[u8]) -> Result<Request> {
    if input.len() > REQUEST_LIMIT {
        return Err(invalid());
    }
    let request: Request = serde_json::from_slice(input).map_err(|_| invalid())?;
    if [&request.publication_id, &request.operation_id]
        .iter()
        .any(|value| {
            !Uuid::parse_str(value).is_ok_and(|id| id.to_string().as_str() == value.as_str())
        })
        || [
            &request.source_manifest_sha256,
            &request.index_sha256,
            &request.revision_sha256,
            &request.target_manifest_sha256,
        ]
        .iter()
        .any(|hash| !hash_valid(hash))
        || !request.output_dir.is_absolute()
        || request
            .output_dir
            .file_name()
            .and_then(OsStr::to_str)
            .is_none_or(|name| !safe_path(name) || name.len() > 200)
    {
        return Err(invalid());
    }
    Ok(request)
}

fn reviewed_runtime(request: &Request) -> Result<RuntimePin> {
    let source = (
        request.publication_id.as_str(),
        request.revision_sha256.as_str(),
        request.index_sha256.as_str(),
        request.source_manifest_sha256.as_str(),
    );
    let runtime = match source {
        SOURCE_PUBLICATION => RUNTIME,
        HAYES_R5_PUBLICATION | HAYES_ROTATED_PUBLICATION => HAYES_KNOWN_MEMORY_RUNTIME,
        _ => return Err(err("NATIVE_STATIC_REFRESH_SOURCE_UNQUALIFIED")),
    };
    if request.target_manifest_sha256 != runtime.2 {
        return Err(err("NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED"));
    }
    Ok(runtime)
}

pub fn run(input: &[u8], runtime: &Path, stop: &AtomicBool, output: &mut dyn Write) -> Result<()> {
    let request = validate_request(input)?;
    check_cancelled(stop)?;
    let (source_commit, producer, manifest_digest) = reviewed_runtime(&request)?;
    let runtime_root = open_directory(runtime)?;
    let source = open_directory(&request.source_dir)?;
    let parent_path = request.output_dir.parent().ok_or_else(invalid)?;
    let name = request.output_dir.file_name().ok_or_else(invalid)?;
    if parent_path.join(name).as_os_str() != request.output_dir.as_os_str() {
        return Err(invalid());
    }
    let parent = open_directory(parent_path)?;
    let parent_info = parent.metadata().map_err(|_| invalid())?;
    if parent_info.uid() != unsafe { libc::geteuid() }
        || parent_info.mode() & 0o022 != 0
        || request.output_dir.starts_with(&request.source_dir)
        || request.source_dir.starts_with(&request.output_dir)
        || request.output_dir.starts_with(runtime)
        || runtime.starts_with(&request.output_dir)
    {
        return Err(invalid());
    }
    let (source_manifest, source_bytes) =
        read_manifest(&source, &request.source_manifest_sha256, &request)?;
    index_entries(&source_manifest)?;
    let stage_name = format!(".exhibitos-native-refresh-{}", request.operation_id);
    let mut op = Operation {
        path: parent_path.join(&stage_name),
        parent_path: parent_path.into(),
        root: private_child(&parent, &stage_name)?,
        parent,
        drained: true,
    };
    let result = (|| -> Result<()> {
        let runtime_copy = op.path.join("runtime");
        let copied = private_child(&op.root, "runtime")?;
        snapshot_runtime(
            &runtime_root,
            &copied,
            (source_commit, producer, manifest_digest),
            stop,
        )?;
        verify_named(runtime, &runtime_root)?;
        verify_named(&runtime_copy, &copied)?;
        let payload = Payload::open(&runtime_copy)?;
        let context = payload.context();
        if context.source_commit != source_commit
            || payload.producer_sha256() != producer
            || context.manifest_sha256 != manifest_digest
        {
            return Err(err("NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED"));
        }
        let script = payload.static_refresh_script(SCRIPT)?;
        let worker = private_child(&op.root, "worker")?;
        private_child(&worker, "tmp")?;
        let worker_path = op.path.join("worker");
        let snapshot = private_child(&op.root, "snapshot")?;
        verify_tree(&source, Some(&snapshot), &source_manifest, stop, false)?;
        verify_named(&request.source_dir, &source)?;
        let (_, current) = read_manifest(&source, &request.source_manifest_sha256, &request)?;
        if current != source_bytes {
            return Err(invalid());
        }
        let mut manifest_file = open_at(
            &snapshot,
            OsStr::new(MANIFEST),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        manifest_file
            .write_all(&source_bytes)
            .and_then(|_| manifest_file.sync_all())
            .map_err(|_| invalid())?;
        snapshot.sync_all().map_err(|_| invalid())?;
        let source_dir = op.path.join("snapshot");
        let candidate = worker_path.join("candidate");
        let wire = serde_json::to_vec(&ChildRequest {
            source_dir: &source_dir,
            source_manifest_sha256: &request.source_manifest_sha256,
            index_sha256: &request.index_sha256,
            publication_id: &request.publication_id,
            revision_sha256: &request.revision_sha256,
            target_runtime_dir: &runtime_copy,
            target_manifest_sha256: &request.target_manifest_sha256,
            staged_output_dir: &candidate,
        })
        .map_err(|_| invalid())?;
        verify_named(&op.path, &op.root)?;
        verify_named(&runtime_copy, &copied)?;
        verify_named(&worker_path, &worker)?;
        verify_named(&source_dir, &snapshot)?;
        if wire.len() > REQUEST_LIMIT {
            return Err(invalid());
        }
        let reply = run_child(
            &payload,
            &runtime_copy,
            &script,
            &worker_path,
            &source_dir,
            &mut op,
            &wire,
            stop,
        )?;
        let child: ChildReceipt = serde_json::from_slice(&reply).map_err(|_| invalid())?;
        let candidate_fd = open_at(
            &worker,
            OsStr::new("candidate"),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let (output_manifest, output_bytes) =
            read_manifest(&candidate_fd, &child.manifest_sha256, &request)?;
        verify_tree(&candidate_fd, None, &output_manifest, stop, true)?;
        unchanged_content(&source_manifest, &output_manifest)?;
        let bytes = output_manifest
            .files
            .iter()
            .map(|entry| entry.bytes)
            .sum::<u64>()
            + output_bytes.len() as u64;
        let files = output_manifest.files.len() as u64 + 1;
        if child.output != candidate
            || child.stage_status != "verified"
            || child.stage_directory.parent() != Some(worker_path.as_path())
            || !child
                .stage_directory
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| {
                    name.starts_with(".exhibitos-refresh-") && safe_path(&name[1..])
                })
            || !matches!(child.stage_cleanup.as_str(), "complete" | "pending")
            || child.publication_id != request.publication_id
            || child.revision_sha256 != request.revision_sha256
            || child.index_sha256 != request.index_sha256
            || child.source_manifest_sha256 != request.source_manifest_sha256
            || child.target_manifest_sha256 != request.target_manifest_sha256
            || child.bytes != bytes
            || child.files != files
        {
            return Err(invalid());
        }
        verify_named(&runtime_copy, &copied)?;
        payload.recheck()?;
        verify_named(&request.source_dir, &source)?;
        read_manifest(&source, &request.source_manifest_sha256, &request)?;
        verify_tree(&source, None, &source_manifest, stop, false)?;
        verify_named(&op.parent_path, &op.parent)?;
        verify_named(&op.path, &op.root)?;
        verify_named(&worker_path, &worker)?;
        verify_named(&candidate, &candidate_fd)?;
        read_manifest(&candidate_fd, &child.manifest_sha256, &request)?;
        let receipt = Receipt {
            operation_id: request.operation_id.clone(),
            publication_id: child.publication_id,
            revision_sha256: child.revision_sha256,
            index_sha256: child.index_sha256,
            manifest_sha256: child.manifest_sha256,
            files,
            bytes,
            output: request.output_dir.clone(),
            source_manifest_sha256: child.source_manifest_sha256,
            target_manifest_sha256: child.target_manifest_sha256,
        };
        let (candidate_device, candidate_inode) = identity(&candidate_fd)?;
        let commit = CommitRecord {
            candidate_device,
            candidate_inode,
            receipt,
        };
        let mut journal = open_at(
            &op.root,
            OsStr::new("commit.json"),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        serde_json::to_writer(&mut journal, &commit).map_err(|_| invalid())?;
        journal
            .sync_all()
            .and_then(|_| op.root.sync_all())
            .and_then(|_| op.parent.sync_all())
            .map_err(|_| invalid())?;
        check_cancelled(stop)?;
        promote(&worker, OsStr::new("candidate"), &op.parent, name)?;
        let uncertain = || err("NATIVE_STATIC_REFRESH_COMMIT_UNCERTAIN");
        op.parent
            .sync_all()
            .and_then(|_| worker.sync_all())
            .map_err(|_| uncertain())?;
        verify_named(&commit.receipt.output, &candidate_fd).map_err(|_| uncertain())?;
        write_receipt(&commit.receipt, output).map_err(|_| uncertain())?;
        Ok(())
    })();
    if result.as_ref().is_err_and(|error| {
        matches!(
            error.code.as_str(),
            "NATIVE_STATIC_REFRESH_DRAIN_UNCERTAIN" | "NATIVE_STATIC_REFRESH_COMMIT_UNCERTAIN"
        )
    }) {
        op.drained = false; // preserve operation-owned state for read-only recovery
        return result;
    }
    if !op.clean() {
        return Err(err(if result.is_ok() {
            "NATIVE_STATIC_REFRESH_COMMITTED_CLEANUP_PENDING"
        } else {
            "NATIVE_STATIC_REFRESH_CLEANUP_PENDING"
        }));
    }
    result
}

fn write_receipt(receipt: &impl Serialize, output: &mut dyn Write) -> Result<()> {
    let bytes = serde_json::to_vec(receipt).map_err(|_| invalid())?;
    output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

/// Read only. A retained commit record and the final directory's original
/// inode must both match; an unrelated preexisting output is never adopted.
pub fn recover(input: &[u8], stop: &AtomicBool, output: &mut dyn Write) -> Result<()> {
    let request = validate_request(input)?;
    let missing = || err("NATIVE_STATIC_REFRESH_COMMIT_UNCERTAIN");
    let parent = request.output_dir.parent().ok_or_else(invalid)?;
    let stage = parent.join(format!(
        ".exhibitos-native-refresh-{}",
        request.operation_id
    ));
    let stage = open_directory(&stage).map_err(|_| missing())?;
    let info = stage.metadata().map_err(|_| missing())?;
    if info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o077 != 0 {
        return Err(missing());
    }
    let journal =
        open_at(&stage, OsStr::new("commit.json"), libc::O_RDONLY).map_err(|_| missing())?;
    regular(&journal, REQUEST_LIMIT as u64)?;
    let mut wire = Vec::new();
    journal
        .take(REQUEST_LIMIT as u64 + 1)
        .read_to_end(&mut wire)
        .map_err(|_| missing())?;
    if wire.len() > REQUEST_LIMIT {
        return Err(missing());
    }
    let record: CommitRecord = serde_json::from_slice(&wire).map_err(|_| missing())?;
    let receipt = record.receipt;
    if reviewed_runtime(&request).is_err() || receipt.operation_id != request.operation_id
        || receipt.output != request.output_dir
        || receipt.source_manifest_sha256 != request.source_manifest_sha256
        || receipt.target_manifest_sha256 != request.target_manifest_sha256
        || receipt.publication_id != request.publication_id
        || receipt.index_sha256 != request.index_sha256
        || receipt.revision_sha256 != request.revision_sha256
    {
        return Err(missing());
    }
    let target = open_directory(&request.output_dir).map_err(|_| missing())?;
    if identity(&target)? != (record.candidate_device, record.candidate_inode) {
        return Err(missing());
    }
    let (manifest, manifest_bytes) = read_manifest(&target, &receipt.manifest_sha256, &request)?;
    verify_tree(&target, None, &manifest, stop, false)?;
    let bytes =
        manifest.files.iter().map(|entry| entry.bytes).sum::<u64>() + manifest_bytes.len() as u64;
    if manifest.files.len() as u64 + 1 != receipt.files || bytes != receipt.bytes {
        return Err(missing());
    }
    verify_named(&request.output_dir, &target)?;
    check_cancelled(stop)?;
    #[derive(Serialize)]
    struct ObservedCommit<'a> {
        state: &'static str,
        durability: &'static str,
        receipt: &'a Receipt,
    }
    write_receipt(
        &ObservedCommit {
            state: "observed",
            durability: "unknown",
            receipt: &receipt,
        },
        output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn static_refresh_admits_only_reviewed_pairs() {
        let request = |source: PublicationPin, runtime: RuntimePin| Request {
            operation_id: Uuid::new_v4().to_string(),
            source_dir: "/unused/source".into(),
            source_manifest_sha256: source.3.into(),
            index_sha256: source.2.into(),
            publication_id: source.0.into(),
            revision_sha256: source.1.into(),
            target_manifest_sha256: runtime.2.into(),
            output_dir: "/unused/output".into(),
        };
        assert_eq!(reviewed_runtime(&request(SOURCE_PUBLICATION, RUNTIME)).unwrap(), RUNTIME);
        assert_eq!(
            reviewed_runtime(&request(HAYES_R5_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME)).unwrap(),
            HAYES_KNOWN_MEMORY_RUNTIME,
        );
        assert_eq!(
            reviewed_runtime(&request(HAYES_ROTATED_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME)).unwrap(),
            HAYES_KNOWN_MEMORY_RUNTIME,
        );
        let mut mixed_hayes = request(HAYES_ROTATED_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME);
        mixed_hayes.source_manifest_sha256 = HAYES_R5_PUBLICATION.3.into();
        assert_eq!(
            reviewed_runtime(&mixed_hayes).unwrap_err().code,
            "NATIVE_STATIC_REFRESH_SOURCE_UNQUALIFIED"
        );
        for (source, runtime) in [
            (SOURCE_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME),
            (HAYES_R5_PUBLICATION, RUNTIME),
            (HAYES_ROTATED_PUBLICATION, RUNTIME),
        ] {
            assert_eq!(reviewed_runtime(&request(source, runtime)).unwrap_err().code, "NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED");
        }
        let mut obsolete = request(HAYES_R5_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME);
        obsolete.target_manifest_sha256 =
            "c5cff38ec91c2f4429880a99421d5dcd3225e160e587223e9e2b1ea2d08990f5".into();
        assert_eq!(
            reviewed_runtime(&obsolete).unwrap_err().code,
            "NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED"
        );
        let mut changed = request(HAYES_R5_PUBLICATION, HAYES_KNOWN_MEMORY_RUNTIME);
        changed.index_sha256 = "a".repeat(64);
        assert_eq!(reviewed_runtime(&changed).unwrap_err().code, "NATIVE_STATIC_REFRESH_SOURCE_UNQUALIFIED");
    }

    #[test]
    #[ignore = "requires the sealed Hayes static refresh runtime directory"]
    fn opens_sealed_hayes_runtime_and_refresh_entry_offline() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_REFRESH_RUNTIME").unwrap());
        let bytes = fs::read(root.join("manifest.json")).unwrap();
        assert!(!Payload::static_refresh_inventory(&bytes, HAYES_KNOWN_MEMORY_RUNTIME).unwrap().is_empty());
        let payload = Payload::open(&root).unwrap();
        let context = payload.context();
        assert_eq!((context.source_commit.as_str(), payload.producer_sha256(), context.manifest_sha256.as_str()),
            HAYES_KNOWN_MEMORY_RUNTIME);
        assert_eq!(payload.static_refresh_script(SCRIPT).unwrap(), root.join(SCRIPT));
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("static-refresh-test-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn manifest() -> Manifest {
        let entry = |path: &str, bytes: &[u8]| Entry {
            path: path.into(),
            bytes: bytes.len() as u64,
            sha256: digest(bytes),
        };
        Manifest {
            format: "exhibitos-static-publication-v1".into(),
            publication_id: Uuid::new_v4().to_string(),
            revision_sha256: "a".repeat(64),
            index_sha256: digest(b"index"),
            files: vec![
                entry(INDEX, b"index"),
                entry("data/asset.bin", b"content"),
                entry("index.html", b"shell"),
            ],
        }
    }

    #[test]
    fn snapshot_rejects_links_unlisted_data_changed_hash_and_cancel() {
        let root = Fixture::new();
        let source = root.0.join("source");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("data")).unwrap();
        fs::write(source.join(INDEX), b"index").unwrap();
        fs::write(source.join("index.html"), b"shell").unwrap();
        let asset = source.join("data/asset.bin");
        fs::write(&asset, b"content").unwrap();
        let base = open_directory(&root.0).unwrap();
        let snapshot = private_child(&base, "snapshot").unwrap();
        let manifest = manifest();
        let cancelled = AtomicBool::new(false);
        let held = open_directory(&source).unwrap();
        verify_tree(&held, Some(&snapshot), &manifest, &cancelled, false).unwrap();
        assert_eq!(
            fs::read(root.0.join("snapshot/data/asset.bin")).unwrap(),
            b"content"
        );
        cancelled.store(true, Ordering::SeqCst);
        assert!(verify_tree(&held, None, &manifest, &cancelled, true).is_err());
        cancelled.store(false, Ordering::SeqCst);
        fs::write(&asset, b"changed").unwrap();
        assert!(verify_tree(&held, None, &manifest, &cancelled, true).is_err());
        fs::remove_file(&asset).unwrap();
        symlink(root.0.join("snapshot/data/asset.bin"), &asset).unwrap();
        assert!(verify_tree(&held, None, &manifest, &cancelled, true).is_err());
        fs::remove_file(&asset).unwrap();
        fs::write(&asset, b"content").unwrap();
        fs::create_dir(root.0.join("snapshot/empty")).unwrap();
        assert!(verify_tree(&snapshot, None, &manifest, &cancelled, true).is_err());
        fs::write(source.join("unlisted"), b"extra").unwrap();
        assert!(verify_tree(&held, None, &manifest, &cancelled, true).is_err());
        symlink(&source, root.0.join("alias")).unwrap();
        assert!(open_directory(&root.0.join("alias/data")).is_err());
    }

    #[test]
    fn only_classified_shell_bytes_can_change() {
        let before = manifest();
        let mut after = manifest();
        after.files[2].sha256 = digest(b"new shell");
        unchanged_content(&before, &after).unwrap();
        after.files[1].sha256 = digest(b"changed content");
        assert!(unchanged_content(&before, &after).is_err());
        let mut after = manifest();
        after.files[2].path = "unknown.txt".into();
        assert!(unchanged_content(&before, &after).is_err());
    }

    #[test]
    fn hayes_r5_editor_catalogue_is_immutable_and_scoped() {
        for path in HAYES_EDITOR_CONTENT {
            let mut before = manifest();
            let mut after = manifest();
            for value in [&mut before, &mut after] {
                value.publication_id = HAYES_R5_PUBLICATION.0.into();
                value.files.push(Entry {
                    path: path.into(),
                    bytes: 7,
                    sha256: digest(b"content"),
                });
            }
            unchanged_content(&before, &after).unwrap();
            let entry = after.files.last_mut().unwrap();
            entry.sha256 = digest(b"changed");
            assert!(unchanged_content(&before, &after).is_err());
            let entry = after.files.pop().unwrap();
            assert!(unchanged_content(&before, &after).is_err());
            after.files.push(Entry {
                path: "furnish-catalog/other/1/model.glb".into(),
                ..entry
            });
            assert!(unchanged_content(&before, &after).is_err());
            before.publication_id = SOURCE_PUBLICATION.0.into();
            assert!(unchanged_content(&before, &before).is_err());
        }
    }

    #[test]
    fn runtime_copy_uses_exact_manifest_and_is_independent_of_bundle_paths() {
        let fixture = Fixture::new();
        let bundled = fixture.0.join("bundle");
        fs::create_dir(&bundled).unwrap();
        fs::create_dir(bundled.join("bin")).unwrap();
        fs::write(bundled.join("bin/node"), b"verified").unwrap();
        symlink("intermediate", bundled.join("bin/alias")).unwrap();
        symlink("node", bundled.join("bin/intermediate")).unwrap();
        let source_commit = "a".repeat(40);
        let producer = "b".repeat(64);
        let manifest = serde_json::to_vec(&serde_json::json!({
            "format": "exhibitos-native-runtime-v1", "mode": "local-validation",
            "platform": "darwin-arm64", "nodeVersion": "24.21.0", "postgresVersion": "18.6",
            "platformVersion": "0.1.0", "protocolVersion": "1", "sourceCommit": source_commit,
            "producerSHA256": producer, "officialSigned": false, "redistributionQualified": false,
            "remaining": "", "entry": {"node": "bin/node", "script": "platform/scripts/local-runtime.mjs",
                "postgres": "postgres/bin/postgres", "initdb": "postgres/bin/initdb"},
            "files": [
                {"path": "bin/node", "bytes": 8, "sha256": digest(b"verified"), "executable": true},
                {"path": "bin/alias", "link": "node"},
                {"path": "bin/intermediate", "link": "node"}
            ]
        })).unwrap();
        fs::write(bundled.join("manifest.json"), &manifest).unwrap();
        let hash = digest(&manifest);
        let root = open_directory(&fixture.0).unwrap();
        let source = open_directory(&bundled).unwrap();
        let copied = private_child(&root, "copied").unwrap();
        snapshot_runtime(
            &source,
            &copied,
            (&source_commit, &producer, &hash),
            &AtomicBool::new(false),
        )
        .unwrap();
        fs::write(bundled.join("bin/node"), b"replaced").unwrap();
        assert_eq!(
            fs::read(fixture.0.join("copied/bin/node")).unwrap(),
            b"verified"
        );
        assert_eq!(
            fs::read_link(fixture.0.join("copied/bin/alias")).unwrap(),
            Path::new("node")
        );
        assert_eq!(
            fs::metadata(fixture.0.join("copied/bin/node"))
                .unwrap()
                .mode()
                & 0o777,
            0o500
        );
    }

    #[test]
    fn runtime_link_rejects_undeclared_hops_escape_and_cycles() {
        let fixture = Fixture::new();
        let parent = open_directory(&fixture.0).unwrap();
        let alias = Path::new("alias");
        let intermediate = Path::new("intermediate");
        let declared = BTreeMap::from([(alias, "node"), (intermediate, "node")]);
        symlink("alias", fixture.0.join(intermediate)).unwrap();
        for hop in ["unlisted", "../outside", "intermediate"] {
            symlink(hop, fixture.0.join(alias)).unwrap();
            assert!(verify_runtime_link(&parent, alias, "node", &declared).is_err());
            fs::remove_file(fixture.0.join(alias)).unwrap();
        }
    }

    #[test]
    fn exclusive_promotion_preserves_competing_destination() {
        let root = Fixture::new();
        let parent = open_directory(&root.0).unwrap();
        let stage = private_child(&parent, "stage").unwrap();
        let candidate = private_child(&stage, "candidate").unwrap();
        private_child(&parent, "result").unwrap();
        let from = OsStr::new("candidate");
        let to = OsStr::new("result");
        assert_eq!(
            promote(&stage, from, &parent, to).unwrap_err().code,
            "NATIVE_STATIC_REFRESH_OUTPUT_EXISTS"
        );
        verify_named(&root.0.join("stage/candidate"), &candidate).unwrap();
        fs::remove_dir(root.0.join("result")).unwrap();
        promote(&stage, from, &parent, to).unwrap();
        verify_named(&root.0.join("result"), &candidate).unwrap();
    }
}
