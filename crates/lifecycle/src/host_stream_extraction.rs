// SPDX-License-Identifier: Apache-2.0
//! Authenticated chunk sink; its private staging tree is never a published proof.
use super::*;
#[cfg(windows)]
use crate::windows_private::{HostDirectory, HostFile};
use std::io::Write;
#[cfg(not(windows))]
type ExtractionFile = File;
#[cfg(windows)]
type ExtractionFile = HostFile;
#[cfg(windows)]
type WindowsExtraction = (Inventory, Vec<u8>, BTreeMap<String, HostDirectory>, BTreeMap<String, HostFile>);

pub(super) struct Extraction {
    profile: PathBuf,
    #[cfg(not(windows))]
    recovered: PathBuf,
    prefix: Vec<u8>,
    manifest_len: Option<usize>,
    manifest: Vec<u8>,
    inventory: Option<Inventory>,
    next: usize,
    output: Option<ExtractionFile>,
    #[cfg(windows)]
    directories: BTreeMap<String, HostDirectory>,
    #[cfg(windows)]
    files: BTreeMap<String, HostFile>,
    remaining: u64,
    hash: Sha256,
    require_reference_clones: bool,
}
impl Extraction {
    pub(super) fn new(profile: &Path, recovered: &Path) -> Self {
        #[cfg(windows)]
        let _ = recovered;
        Self {
            profile: profile.into(),
            #[cfg(not(windows))]
            recovered: recovered.into(),
            prefix: Vec::new(),
            manifest_len: None,
            manifest: Vec::new(),
            inventory: None,
            next: 0,
            output: None,
            #[cfg(windows)]
            directories: BTreeMap::new(),
            #[cfg(windows)]
            files: BTreeMap::new(),
            remaining: 0,
            hash: Sha256::new(),
            require_reference_clones: false,
        }
    }
    #[cfg(windows)]
    pub(super) fn new_windows(profile: &Path, recovered: HostDirectory) -> Self {
        let mut sink = Self::new(profile, recovered.path());
        sink.directories.insert(String::new(), recovered);
        sink
    }
    #[cfg(any(not(windows), test))]
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
                #[cfg(not(windows))]
                installations::new_directory(&self.recovered.join(&e.path))?;
                #[cfg(windows)]
                {
                    let (parent, name) = windows_parent(&self.directories, &e.path)?;
                    let directory = parent.create_directory(name)?;
                    self.directories.insert(e.path.clone(), directory);
                }
                self.next += 1;
                continue;
            }
            if let Some(original) = m.content_references.get(&e.path) {
                #[cfg(not(windows))]
                {
                    let original = self.recovered.join(original);
                    let target = self.recovered.join(&e.path);
                    let mut input = file(&original)?;
                    let before = input.metadata().map_err(|_| fail())?;
                    let copied = copy_reference(
                        &mut input,
                        &target,
                        e.bytes,
                        self.require_reference_clones,
                    )?;
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
                }
                #[cfg(windows)]
                {
                    let input = self.files.get_mut(original).ok_or_else(fail)?;
                    input
                        .seek(std::io::SeekFrom::Start(0))
                        .map_err(|_| fail())?;
                    let (target_parent, target_name) = windows_parent(&self.directories, &e.path)?;
                    let output = windows_dense_reference(
                        input,
                        target_parent,
                        target_name,
                        e,
                        self.require_reference_clones,
                    )?;
                    self.files.insert(e.path.clone(), output);
                }
                self.next += 1;
                continue;
            }
            if self.output.is_none() {
                #[cfg(not(windows))]
                {
                    self.output = Some(private_new(&self.recovered.join(&e.path))?);
                }
                #[cfg(windows)]
                {
                    let (parent, name) = windows_parent(&self.directories, &e.path)?;
                    self.output = Some(parent.create_file(name)?);
                }
                self.remaining = e.bytes;
                self.hash = Sha256::new();
            }
            if self.remaining != 0 {
                return Ok(());
            }
            if Some(format!("{:x}", self.hash.clone().finalize())) != e.sha256 {
                return Err(fail());
            }
            let output = self.output.take().ok_or_else(fail)?;
            output.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
            #[cfg(windows)]
            {
                output.check()?;
                // Retain NEW authority; native permissions are restored only after
                // authenticated EOF and after every parent has its original ACL.
                self.files.insert(e.path.clone(), output);
            }
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
            #[cfg(not(windows))]
            installations::new_directory(&self.recovered)?;
            #[cfg(windows)]
            self.directories.get("").ok_or_else(fail)?.check()?;
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
    #[cfg(not(windows))]
    pub(super) fn finish(mut self) -> Result<(Inventory, Vec<u8>)> {
        self.advance()?;
        let m = self.inventory.take().ok_or_else(fail)?;
        if self.next != m.items.len() || self.remaining != 0 || self.output.is_some() {
            return Err(fail());
        }
        Ok((m, self.manifest))
    }
    #[cfg(windows)]
    pub(super) fn finish_windows(
        mut self,
    ) -> Result<WindowsExtraction> {
        self.advance()?;
        let m = self.inventory.take().ok_or_else(fail)?;
        if self.next != m.items.len() || self.remaining != 0 || self.output.is_some() {
            return Err(fail());
        }
        // Called only after the authenticated stream's final frame and EOF.
        // Restore inherited ACLs from root to descendants, then files. Retain
        // every NEW object authority for the caller's full final census.
        let directory = self.directories.get("").ok_or_else(fail)?;
        let permissions = m.root_windows_permissions.as_ref().ok_or_else(fail)?;
        directory.apply_permissions(permissions)?;
        if directory.permissions()? != *permissions {
            return Err(fail());
        }
        // The validated sorted paths place every parent before its descendants.
        for entry in m.items.iter().filter(|item| item.kind == "directory") {
            let directory = self.directories.get(&entry.path).ok_or_else(fail)?;
            let permissions = entry.windows_permissions.as_ref().ok_or_else(fail)?;
            directory.apply_permissions(permissions)?;
            if directory.permissions()? != *permissions {
                return Err(fail());
            }
        }
        for entry in m.items.iter().filter(|item| item.kind == "file") {
            let file = self.files.get(&entry.path).ok_or_else(fail)?;
            let permissions = entry.windows_permissions.as_ref().ok_or_else(fail)?;
            file.apply_permissions(permissions)?;
            if file.permissions()? != *permissions {
                return Err(fail());
            }
        }
        Ok((m, self.manifest, self.directories, self.files))
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
#[cfg(not(windows))]
fn copy_reference(input: &mut File, target: &Path, bytes: u64, require_clone: bool) -> Result<u64> {
    #[cfg(target_os = "macos")]
    if clone_reference(input, target)? {
        return input.metadata().map(|m| m.len()).map_err(|_| fail());
    }
    dense_reference(input, target, bytes, require_clone)
}

#[cfg(not(windows))]
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

#[cfg(windows)]
fn windows_parent<'a>(
    directories: &'a BTreeMap<String, HostDirectory>,
    relative: &'a str,
) -> Result<(&'a HostDirectory, &'a str)> {
    if !safe_path(relative) {
        return Err(fail());
    }
    let (parent, name) = relative.rsplit_once('/').unwrap_or(("", relative));
    Ok((directories.get(parent).ok_or_else(fail)?, name))
}

#[cfg(windows)]
fn windows_dense_reference(
    input: &mut HostFile,
    parent: &HostDirectory,
    name: &str,
    entry: &Item,
    require_clone: bool,
) -> Result<HostFile> {
    if require_clone {
        return Err(err("HOST_REFERENCE_CLONE_REQUIRED"));
    }
    input.check()?;
    let before = input.metadata()?;
    if before.len() != entry.bytes {
        return Err(fail());
    }
    let mut output = parent.create_file(name)?;
    let copied = std::io::copy(
        &mut std::io::Read::by_ref(input).take(entry.bytes + 1),
        &mut output,
    )
    .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    output.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    input.check()?;
    if copied != entry.bytes || !unchanged(&before, &input.metadata()?) {
        return Err(fail());
    }
    // Read the independently created NEW file through its owned handle. Closing
    // and reopening would discard the only authority allowed to restore its ACL.
    output
        .seek(std::io::SeekFrom::Start(0))
        .map_err(|_| fail())?;
    let metadata = output.metadata()?;
    let mut hasher = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        output.check()?;
        let length = output.read(&mut buffer).map_err(|_| fail())?;
        if length == 0 {
            break;
        }
        count += length as u64;
        if count > entry.bytes {
            return Err(fail());
        }
        hasher.update(&buffer[..length]);
    }
    output.check()?;
    if count != entry.bytes
        || !unchanged(&metadata, &output.metadata()?)
        || Some(format!("{:x}", hasher.finalize())) != entry.sha256
    {
        return Err(fail());
    }
    Ok(output)
}

#[cfg(all(test, windows))]
mod windows_stream_tests {
    use super::*;

    fn fixture() -> (HostDirectory, Inventory, Vec<u8>, Vec<u8>) {
        let path = std::env::temp_dir().join(format!("host-stream-{}", Uuid::new_v4()));
        let initial = crate::windows_private::PrivateDirectory::create(&path).unwrap();
        let path = initial.path().to_owned();
        drop(initial);
        let base = HostDirectory::inspect(&path).unwrap();
        let source = base.create_directory("source").unwrap();
        let assets = source.create_directory("assets").unwrap();
        let registry = b"synthetic installation registry";
        let mut registry_file = source.create_file("installation-selection.json").unwrap();
        registry_file.write_all(registry).unwrap();
        registry_file.sync_all().unwrap();
        let registry_permissions = registry_file.permissions().unwrap();
        drop(registry_file);
        let bytes = vec![0x57; 65537];
        let mut master = assets.create_file("master.bin").unwrap();
        master.write_all(&bytes).unwrap();
        master.sync_all().unwrap();
        drop(master);
        drop(assets);
        drop(source);
        for (target, arguments) in [
            (
                base.path().join("source/assets"),
                vec!["/grant", "*S-1-1-0:(OI)(CI)R"],
            ),
            (
                base.path().join("source/assets/master.bin"),
                vec!["/inheritance:e"],
            ),
        ] {
            let status = crate::process_window::background_command("icacls.exe")
                .arg(target)
                .args(arguments)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
        }
        let source = base.child_directory("source").unwrap();
        let assets = source.child_directory("assets").unwrap();
        let inherited = assets.child_file("master.bin").unwrap();
        let file_permissions = inherited.permissions().unwrap();
        assert!(
            file_permissions.security_descriptor.contains("S-1-1-0")
                || file_permissions.security_descriptor.contains("WD")
        );
        drop(inherited);
        let manifest = Inventory {
            format: 4,
            id: Uuid::new_v4().to_string(),
            scope: "host-profile".into(),
            source_profile_sha256: digest(source.path().to_string_lossy().as_bytes()),
            registry_sha256: digest(registry),
            total_bytes: 2 * bytes.len() as u64 + registry.len() as u64,
            external_volumes_saved: false,
            host_writer_quiescence: "operator-acknowledged".into(),
            excluded_locks: Vec::new(),
            items: vec![
                Item {
                    path: "assets".into(),
                    kind: "directory".into(),
                    mode: 0,
                    bytes: 0,
                    sha256: None,
                    windows_permissions: Some(assets.permissions().unwrap()),
                },
                Item {
                    path: "assets/master.bin".into(),
                    kind: "file".into(),
                    mode: 0,
                    bytes: bytes.len() as u64,
                    sha256: Some(digest(&bytes)),
                    windows_permissions: Some(file_permissions.clone()),
                },
                Item {
                    path: "assets/reference.bin".into(),
                    kind: "file".into(),
                    mode: 0,
                    bytes: bytes.len() as u64,
                    sha256: Some(digest(&bytes)),
                    windows_permissions: Some(file_permissions),
                },
                Item {
                    path: "installation-selection.json".into(),
                    kind: "file".into(),
                    mode: 0,
                    bytes: registry.len() as u64,
                    sha256: Some(digest(registry)),
                    windows_permissions: Some(registry_permissions),
                },
            ],
            content_references: BTreeMap::from([(
                "assets/reference.bin".into(),
                "assets/master.bin".into(),
            )]),
            root_windows_permissions: Some(source.permissions().unwrap()),
        };
        let encoded = serde_json::to_vec(&manifest).unwrap();
        let mut payload = (encoded.len() as u64).to_be_bytes().to_vec();
        payload.extend_from_slice(&encoded);
        payload.extend_from_slice(&bytes);
        payload.extend_from_slice(registry);
        (base, manifest, payload, bytes)
    }

    fn seal(payload: &[u8]) -> Vec<u8> {
        let mut ciphertext = Vec::new();
        crate::maintenance_stream::seal_compact(
            &mut Cursor::new(payload),
            &mut ciphertext,
            &[37; 32],
            CONTEXT,
            DATA_LIMIT,
        )
        .unwrap();
        ciphertext
    }

    #[test]
    fn windows_authenticated_sink_restores_dense_independent_bytes_and_permissions() {
        let (base, expected, payload, original) = fixture();
        let source = base.path().join("source");
        let recovered = base.create_directory("recovered").unwrap();
        let mut sink = Extraction::new_windows(&source, recovered);
        let ciphertext = seal(&payload);
        crate::maintenance_stream::open(
            &mut Cursor::new(&ciphertext),
            &mut sink,
            &[37; 32],
            CONTEXT,
            DATA_LIMIT,
        )
        .unwrap();
        let (actual, encoded, guards, mut files) = sink.finish_windows().unwrap();
        assert_eq!(actual.items, expected.items);
        assert_eq!(encoded, serde_json::to_vec(&expected).unwrap());
        assert_eq!(
            guards[""].permissions().unwrap(),
            expected.root_windows_permissions.unwrap()
        );
        let assets = &guards["assets"];
        for name in ["master.bin", "reference.bin"] {
            let file = files.get_mut(&format!("assets/{name}")).unwrap();
            file.seek(std::io::SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, original);
            assert_eq!(
                file.permissions().unwrap(),
                expected.items[1].windows_permissions.clone().unwrap()
            );
        }
        // Final byte/permission census completed; release NEW writer/delete handles.
        drop(files);
        fs::write(
            assets.path().join("reference.bin"),
            b"independent candidate change",
        )
        .unwrap();
        assert_eq!(
            fs::read(assets.path().join("master.bin")).unwrap(),
            original
        );
        assert_eq!(
            fs::read(source.join("assets/master.bin")).unwrap(),
            original
        );
    }

    #[test]
    fn windows_authenticated_sink_rejects_trailing_ciphertext_and_preserves_staging() {
        let (base, _, payload, original) = fixture();
        let source = base.path().join("source");
        let recovered = base.create_directory("failed-recovered").unwrap();
        let path = recovered.path().to_owned();
        let mut sink = Extraction::new_windows(&source, recovered);
        let mut ciphertext = seal(&payload);
        ciphertext.push(0x41);
        assert!(
            crate::maintenance_stream::open(
                &mut Cursor::new(&ciphertext),
                &mut sink,
                &[37; 32],
                CONTEXT,
                DATA_LIMIT,
            )
            .is_err()
        );
        drop(sink);
        assert!(path.join("assets/master.bin").is_file());
        assert_eq!(
            fs::read(source.join("assets/master.bin")).unwrap(),
            original
        );
        assert!(!base.path().join("published").exists());
    }

    #[test]
    fn windows_sink_refuses_existing_destination_and_required_clone_without_replacement() {
        let (base, _, payload, original) = fixture();
        let source = base.path().join("source");
        let recovered = base.create_directory("collision-recovered").unwrap();
        let existing = recovered.create_directory("assets").unwrap();
        let mut sentinel = existing.create_file("sentinel.bin").unwrap();
        sentinel.write_all(b"preserved existing candidate").unwrap();
        sentinel.sync_all().unwrap();
        drop(sentinel);
        let mut sink = Extraction::new_windows(&source, recovered);
        assert!(sink.write_all(&payload).is_err());
        assert_eq!(
            fs::read(existing.path().join("sentinel.bin")).unwrap(),
            b"preserved existing candidate"
        );
        drop(sink);
        let recovered = base.create_directory("clone-required-recovered").unwrap();
        let path = recovered.path().to_owned();
        let mut sink = Extraction::new_windows(&source, recovered);
        sink.require_reference_clones();
        assert!(sink.write_all(&payload).is_err());
        assert!(path.join("assets/master.bin").exists());
        assert!(!path.join("assets/reference.bin").exists());
        assert_eq!(
            fs::read(source.join("assets/master.bin")).unwrap(),
            original
        );
    }
}
