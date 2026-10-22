// SPDX-License-Identifier: Apache-2.0
//! Fixed bundled verifier interpreter. No renderer paths, host Python lookup or provisioning.
use exhibitos_lifecycle::LifecycleError;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    io::Read,
    path::{Component, Path, PathBuf},
};
const FORMAT: &str = "exhibitos-manager-update-runtime-v1";
const ARCHIVE: &str = "4e2b20716a080e9141ea80029dabbdc9457e20760f3fd3b16a17abd48ea6bf4a";
const EXECUTABLE: &str = "python/bin/python3.11";
const MAX_FILES: usize = 10_000;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
type FileTree = BTreeMap<String, Metadata>;
type DirectoryTree = BTreeMap<PathBuf, Metadata>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Source {
    release: String,
    archive_name: String,
    url: String,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sha256: String,
    bytes: u64,
    mode: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Manifest {
    format: String,
    version: u32,
    platform: String,
    python_version: String,
    executable: String,
    selection: String,
    source: Source,
    redistribution: String,
    files: BTreeMap<String, Entry>,
}

fn qualified_distribution(manifest: &Manifest) -> bool {
    if manifest.redistribution != "pinned-notices-qualified-v1" {
        return false;
    }
    let Ok(pins) = serde_json::from_str::<BTreeMap<String, Entry>>(include_str!(
        "../../scripts/update-runtime-notices/distribution-files.json"
    )) else {
        return false;
    };
    let actual: BTreeSet<_> = manifest
        .files
        .keys()
        .filter(|n| n.starts_with("notices/"))
        .collect();
    let expected: BTreeSet<_> = pins.keys().filter(|n| n.starts_with("notices/")).collect();
    actual == expected
        && pins.iter().all(|(name, pin)| {
            manifest.files.get(name).is_some_and(|entry| {
                entry.sha256 == pin.sha256 && entry.bytes == pin.bytes && entry.mode == pin.mode
            })
        })
}

fn failure(code: &str) -> LifecycleError {
    LifecycleError { code: code.into(), guidance: "이 설치 파일의 업데이트 검증 도구를 확인하지 못했어요. 현재 전시와 백업은 유지됩니다. 검증된 Manager 설치 파일을 사용해 주세요.".into() }
}
/// Opaque native resource identity. Only the main executable descriptor remains open;
/// thousands of stdlib resources use metadata snapshots, not retained descriptors.
pub struct VerifiedPython {
    root: PathBuf,
    python: PathBuf,
    sha256: String,
    executable: File,
    main: Metadata,
    files: BTreeMap<String, Metadata>,
    directories: BTreeMap<PathBuf, Metadata>,
    parents: Vec<(PathBuf, Metadata)>,
}
impl VerifiedPython {
    pub fn python(&self) -> &Path {
        &self.python
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn check(&self) -> Result<(), LifecycleError> {
        #[cfg(target_os = "macos")]
        {
            self.check_inner()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(failure("UPDATE_RUNTIME_UNAVAILABLE"))
        }
    }
}
pub fn verified(resources: &Path) -> Result<VerifiedPython, LifecycleError> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        verify_inner(resources)
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        let _ = resources;
        Err(failure("UPDATE_RUNTIME_UNAVAILABLE"))
    }
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 4096
        && !name.contains('\\')
        && !name.contains('\0')
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && (name.starts_with("python/")
            || name.starts_with("metadata/")
            || name.starts_with("notices/"))
}
#[cfg(target_os = "macos")]
fn same(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.mode() == b.mode()
        && a.nlink() == b.nlink()
        && a.uid() == b.uid()
        && a.gid() == b.gid()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
#[cfg(target_os = "macos")]
fn metadata(path: &Path, directory: bool) -> Result<Metadata, LifecycleError> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path).map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?;
    if m.file_type().is_symlink()
        || (if directory {
            !m.is_dir()
        } else {
            !m.is_file() || m.nlink() != 1
        })
        || m.mode() & 0o6022 != 0
        || (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() })
    {
        return Err(failure("UPDATE_RUNTIME_UNSAFE"));
    }
    Ok(m)
}
#[cfg(target_os = "macos")]
fn open_checked(path: &Path, limit: u64) -> Result<(File, Metadata), LifecycleError> {
    use std::os::unix::fs::OpenOptionsExt;
    let before = metadata(path, false)?;
    if before.len() > limit {
        return Err(failure("UPDATE_RUNTIME_UNSAFE"));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| failure("UPDATE_RUNTIME_UNAVAILABLE"))?;
    if !same(
        &before,
        &file
            .metadata()
            .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?,
    ) {
        return Err(failure("UPDATE_RUNTIME_CHANGED"));
    }
    Ok((file, before))
}
#[cfg(target_os = "macos")]
fn tree(root: &Path) -> Result<(FileTree, DirectoryTree), LifecycleError> {
    let mut files = BTreeMap::new();
    let mut directories = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut count = 0;
    while let Some((path, depth)) = stack.pop() {
        if depth > 32 || count > MAX_FILES * 2 {
            return Err(failure("UPDATE_RUNTIME_UNSAFE"));
        }
        let before = metadata(&path, true)?;
        for entry in fs::read_dir(&path).map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))? {
            count += 1;
            if count > MAX_FILES * 2 {
                return Err(failure("UPDATE_RUNTIME_UNSAFE"));
            }
            let entry = entry.map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?;
            let p = entry.path();
            let m = fs::symlink_metadata(&p).map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?;
            if m.is_dir() && !m.file_type().is_symlink() {
                stack.push((p, depth + 1));
            } else {
                let name = p
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::to_str)
                    .ok_or_else(|| failure("UPDATE_RUNTIME_UNSAFE"))?;
                if name != "manifest.json" && !safe_name(name) {
                    return Err(failure("UPDATE_RUNTIME_UNSAFE"));
                }
                if files.insert(name.into(), metadata(&p, false)?).is_some()
                    || files.len() > MAX_FILES
                {
                    return Err(failure("UPDATE_RUNTIME_UNSAFE"));
                }
            }
        }
        if !same(&before, &metadata(&path, true)?) {
            return Err(failure("UPDATE_RUNTIME_CHANGED"));
        }
        directories.insert(path, before);
    }
    Ok((files, directories))
}
#[cfg(target_os = "macos")]
impl VerifiedPython {
    fn check_inner(&self) -> Result<(), LifecycleError> {
        for (path, m) in &self.parents {
            if !same(m, &metadata(path, true)?) {
                return Err(failure("UPDATE_RUNTIME_CHANGED"));
            }
        }
        if !same(
            &self.main,
            &self
                .executable
                .metadata()
                .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?,
        ) {
            return Err(failure("UPDATE_RUNTIME_CHANGED"));
        }
        let (files, dirs) = tree(&self.root)?;
        if files.len() != self.files.len()
            || dirs.len() != self.directories.len()
            || self
                .files
                .iter()
                .any(|(n, m)| files.get(n).is_none_or(|v| !same(m, v)))
            || self
                .directories
                .iter()
                .any(|(n, m)| dirs.get(n).is_none_or(|v| !same(m, v)))
        {
            return Err(failure("UPDATE_RUNTIME_CHANGED"));
        }
        Ok(())
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn verify_inner(resources: &Path) -> Result<VerifiedPython, LifecycleError> {
    let compiled = include_str!(concat!(env!("OUT_DIR"), "/update-runtime-manifest.json"));
    verify_manifest(resources, compiled)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn verify_manifest(resources: &Path, compiled: &str) -> Result<VerifiedPython, LifecycleError> {
    use std::os::unix::fs::MetadataExt;
    if compiled.is_empty() || compiled.len() > 4 * 1024 * 1024 {
        return Err(failure("UPDATE_RUNTIME_UNAVAILABLE"));
    }
    if !resources.is_absolute() || fs::canonicalize(resources).ok().as_deref() != Some(resources) {
        return Err(failure("UPDATE_RUNTIME_UNSAFE"));
    }
    let root = resources.join("update-runtime/generated");
    if !root
        .try_exists()
        .map_err(|_| failure("UPDATE_RUNTIME_UNAVAILABLE"))?
    {
        return Err(failure("UPDATE_RUNTIME_UNAVAILABLE"));
    }
    let parents = [resources.to_path_buf(), resources.join("update-runtime")]
        .into_iter()
        .map(|p| metadata(&p, true).map(|m| (p, m)))
        .collect::<Result<Vec<_>, _>>()?;
    let (mut descriptor, disk_metadata) =
        open_checked(&root.join("manifest.json"), 4 * 1024 * 1024)?;
    let mut disk = Vec::new();
    descriptor
        .by_ref()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut disk)
        .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?;
    if disk != compiled.as_bytes()
        || !same(
            &disk_metadata,
            &descriptor
                .metadata()
                .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?,
        )
    {
        return Err(failure("UPDATE_RUNTIME_CHANGED"));
    }
    drop(descriptor);
    let manifest: Manifest =
        serde_json::from_slice(&disk).map_err(|_| failure("UPDATE_RUNTIME_UNAVAILABLE"))?;
    if manifest.format != FORMAT
        || manifest.version != 1
        || manifest.platform != "darwin-arm64"
        || manifest.python_version != "3.11.17"
        || manifest.executable != EXECUTABLE
        || manifest.selection != "full-stdlib-no-static-embedding-v1"
        || !(manifest.redistribution == "private-validation-only"
            || qualified_distribution(&manifest))
        || manifest.source.release != "20261013"
        || manifest.source.sha256 != ARCHIVE
        || manifest.source.bytes != 42_796_019
        || manifest.source.archive_name
            != "cpython-3.11.17+20261013-aarch64-apple-darwin-pgo+lto-full.tar.zst"
        || manifest.source.url
            != "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.11.17%2B20261013-aarch64-apple-darwin-pgo%2Blto-full.tar.zst"
        || manifest.files.len() > MAX_FILES
        || manifest.files.is_empty()
    {
        return Err(failure("UPDATE_RUNTIME_UNAVAILABLE"));
    }
    for name in [
        EXECUTABLE,
        "python/lib/python3.11/encodings/__init__.py",
        "metadata/PYTHON.json",
        "metadata/license-discrepancy.json",
        "notices/LICENSE.cpython.txt",
        "notices/LICENSE.zlib.txt",
    ] {
        if !manifest.files.contains_key(name) {
            return Err(failure("UPDATE_RUNTIME_UNAVAILABLE"));
        }
    }
    let mut expected_directories = BTreeSet::from([root.clone()]);
    for name in manifest.files.keys() {
        if !safe_name(name) {
            return Err(failure("UPDATE_RUNTIME_UNSAFE"));
        }
        let mut parent = Path::new(name).parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            expected_directories.insert(root.join(path));
            parent = path.parent();
        }
    }
    let (files, directories) = tree(&root)?;
    if directories.len() != expected_directories.len()
        || directories
            .keys()
            .any(|path| !expected_directories.contains(path))
    {
        return Err(failure("UPDATE_RUNTIME_CHANGED"));
    }
    if files.len() != manifest.files.len() + 1 || !files.contains_key("manifest.json") {
        return Err(failure("UPDATE_RUNTIME_CHANGED"));
    }
    if files
        .get("manifest.json")
        .is_none_or(|m| !same(m, &disk_metadata) || m.mode() & 0o777 != 0o644)
    {
        return Err(failure("UPDATE_RUNTIME_CHANGED"));
    }
    let mut total = 0u64;
    let mut main = None;
    for (name, entry) in &manifest.files {
        if !safe_name(name)
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || ![0o644, 0o755].contains(&entry.mode)
        {
            return Err(failure("UPDATE_RUNTIME_UNSAFE"));
        }
        total = total
            .checked_add(entry.bytes)
            .filter(|v| *v <= MAX_BYTES)
            .ok_or_else(|| failure("UPDATE_RUNTIME_UNSAFE"))?;
        let observed = files
            .get(name)
            .ok_or_else(|| failure("UPDATE_RUNTIME_CHANGED"))?;
        if observed.len() != entry.bytes || observed.mode() & 0o777 != entry.mode {
            return Err(failure("UPDATE_RUNTIME_CHANGED"));
        }
        let path = root.join(name);
        let (mut file, identity) = open_checked(&path, MAX_BYTES)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut count = 0u64;
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > entry.bytes {
                return Err(failure("UPDATE_RUNTIME_CHANGED"));
            }
            hash.update(&buffer[..n]);
        }
        if count != entry.bytes
            || format!("{:x}", hash.finalize()) != entry.sha256
            || !same(observed, &identity)
            || !same(
                &identity,
                &file
                    .metadata()
                    .map_err(|_| failure("UPDATE_RUNTIME_CHANGED"))?,
            )
            || !same(&identity, &metadata(&path, false)?)
        {
            return Err(failure("UPDATE_RUNTIME_CHANGED"));
        }
        if name == EXECUTABLE {
            if entry.mode != 0o755 {
                return Err(failure("UPDATE_RUNTIME_UNSAFE"));
            }
            main = Some((file, identity, entry.sha256.clone()));
        }
    }
    let (executable, main, sha256) = main.ok_or_else(|| failure("UPDATE_RUNTIME_UNAVAILABLE"))?;
    let verified = VerifiedPython {
        python: root.join(EXECUTABLE),
        root,
        sha256,
        executable,
        main,
        files,
        directories,
        parents,
    };
    verified.check()?;
    Ok(verified)
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture {
        resources: PathBuf,
        compiled: String,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .resources
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exhibitos-python-resource-test-"))
            {
                let _ = fs::remove_dir_all(&self.resources);
            }
        }
    }
    fn fixture() -> Fixture {
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let resources = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-python-resource-test-{}-{nonce}-{sequence}",
                std::process::id()
            ));
        fs::create_dir(&resources).unwrap();
        fs::set_permissions(&resources, fs::Permissions::from_mode(0o700)).unwrap();
        let root = resources.join("update-runtime/generated");
        fs::create_dir_all(&root).unwrap();
        let mut files = serde_json::Map::new();
        // Synthetic metadata/file identity fixture only; never an executable Python claim.
        for name in [
            EXECUTABLE,
            "python/lib/python3.11/encodings/__init__.py",
            "metadata/PYTHON.json",
            "metadata/license-discrepancy.json",
            "notices/LICENSE.cpython.txt",
            "notices/LICENSE.zlib.txt",
        ] {
            let data = b"synthetic identity fixture";
            let p = root.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, data).unwrap();
            let mode = if name == EXECUTABLE { 0o755 } else { 0o644 };
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
            files.insert(name.into(),serde_json::json!({"bytes":data.len(),"sha256":format!("{:x}",Sha256::digest(data)),"mode":mode}));
        }
        let value = serde_json::json!({"format":FORMAT,"version":1,"platform":"darwin-arm64","pythonVersion":"3.11.17","executable":EXECUTABLE,"selection":"full-stdlib-no-static-embedding-v1","redistribution":"private-validation-only","source":{"release":"20261013","archiveName":"cpython-3.11.17+20261013-aarch64-apple-darwin-pgo+lto-full.tar.zst","url":"https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.11.17%2B20261013-aarch64-apple-darwin-pgo%2Blto-full.tar.zst","sha256":ARCHIVE,"bytes":42796019},"files":files});
        let compiled = serde_json::to_string(&value).unwrap();
        fs::write(root.join("manifest.json"), &compiled).unwrap();
        fs::set_permissions(
            root.join("manifest.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        Fixture {
            resources,
            compiled,
        }
    }
    #[test]
    fn qualified_distribution_requires_every_pinned_notice_and_original_metadata() {
        let f = fixture();
        let mut value: serde_json::Value = serde_json::from_str(&f.compiled).unwrap();
        value["redistribution"] = serde_json::json!("pinned-notices-qualified-v1");
        let pins: serde_json::Value = serde_json::from_str(include_str!(
            "../../scripts/update-runtime-notices/distribution-files.json"
        ))
        .unwrap();
        for (name, entry) in pins.as_object().unwrap() {
            value["files"][name] = entry.clone();
        }
        let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
        assert!(qualified_distribution(&manifest));
        for name in [
            "notices/LICENSE.zlib-ng.txt",
            "metadata/PYTHON.json",
            "notices/DISTRIBUTION.txt",
        ] {
            let mut changed = value.clone();
            changed["files"].as_object_mut().unwrap().remove(name);
            assert!(!qualified_distribution(
                &serde_json::from_value(changed).unwrap()
            ));
            let mut changed = value.clone();
            changed["files"][name]["sha256"] = serde_json::json!("0".repeat(64));
            assert!(!qualified_distribution(
                &serde_json::from_value(changed).unwrap()
            ));
        }
        value["redistribution"] = serde_json::json!("public");
        assert!(!qualified_distribution(
            &serde_json::from_value(value).unwrap()
        ));
    }
    #[test]
    fn exact_initial_tree_hash_and_later_metadata_are_bound() {
        let f = fixture();
        let runtime = verify_manifest(&f.resources, &f.compiled).unwrap();
        runtime.check().unwrap();
        assert!(runtime.python().ends_with(EXECUTABLE));
        assert_eq!(runtime.sha256().len(), 64);
        fs::write(
            f.resources
                .join("update-runtime/generated/python/lib/python3.11/encodings/__init__.py"),
            b"SYNTHETIC identity fixture",
        )
        .unwrap();
        assert!(runtime.check().is_err());
        assert!(verify_manifest(&f.resources, &f.compiled).is_err());
    }
    #[test]
    fn added_bytecode_and_replaced_directories_refuse_phase_check() {
        let f = fixture();
        fs::create_dir(
            f.resources
                .join("update-runtime/generated/unexpected-empty-directory"),
        )
        .unwrap();
        assert!(verify_manifest(&f.resources, &f.compiled).is_err());
        let f = fixture();
        let runtime = verify_manifest(&f.resources, &f.compiled).unwrap();
        fs::write(
            f.resources.join("update-runtime/generated/python/new.pyc"),
            b"extra",
        )
        .unwrap();
        assert!(runtime.check().is_err());
        let f = fixture();
        let runtime = verify_manifest(&f.resources, &f.compiled).unwrap();
        let p = f.resources.join("update-runtime/generated/notices");
        fs::rename(&p, p.with_file_name("retained-notices")).unwrap();
        fs::create_dir(&p).unwrap();
        assert!(runtime.check().is_err());
    }
    #[test]
    fn linked_or_group_writable_stdlib_and_changed_manifest_are_refused() {
        let f = fixture();
        let p = f
            .resources
            .join("update-runtime/generated/python/lib/python3.11/encodings/__init__.py");
        fs::remove_file(&p).unwrap();
        symlink(
            f.resources
                .join("update-runtime/generated/metadata/PYTHON.json"),
            &p,
        )
        .unwrap();
        assert!(verify_manifest(&f.resources, &f.compiled).is_err());
        let f = fixture();
        let p = f
            .resources
            .join("update-runtime/generated/python/lib/python3.11/encodings/__init__.py");
        fs::set_permissions(p, fs::Permissions::from_mode(0o664)).unwrap();
        assert!(verify_manifest(&f.resources, &f.compiled).is_err());
        let f = fixture();
        fs::write(
            f.resources.join("update-runtime/generated/manifest.json"),
            b"{}",
        )
        .unwrap();
        assert!(verify_manifest(&f.resources, &f.compiled).is_err());
    }
}
