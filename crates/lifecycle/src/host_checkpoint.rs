// SPDX-License-Identifier: Apache-2.0
//! Full host-profile bytes, not engine-volume backup or live root activation.
//! Failed encrypted/plaintext staging remains private and unpublished.
use super::*;
use std::collections::BTreeSet;
use std::io::Cursor;
const CONTEXT: &[u8] = b"ExhibitOS-host-checkpoint-v1\0";
const DATA_LIMIT: u64 = 64 * 1024 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 8 * 1024 * 1024;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostReceipt {
    pub id: String,
    pub operation: String,
    pub files: usize,
    pub bytes: u64,
    pub manifest_sha256: String,
    pub external_volumes_saved: bool,
    pub host_writer_quiescence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    path: String,
    kind: String,
    mode: u32,
    bytes: u64,
    sha256: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Inventory {
    format: u32,
    id: String,
    scope: String,
    source_profile_sha256: String,
    registry_sha256: String,
    total_bytes: u64,
    external_volumes_saved: bool,
    host_writer_quiescence: String,
    excluded_locks: Vec<String>,
    items: Vec<Item>,
}
fn fail() -> LifecycleError {
    err("HOST_CHECKPOINT_INVALID")
}
fn safe_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 2048
        && !s.contains(['\\', ':'])
        && !s.chars().any(char::is_control)
        && s.split('/').count() <= 32
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}
fn mode(m: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        0
    }
}
fn trusted(m: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.uid() == unsafe { libc::geteuid() } && (!m.is_file() || m.nlink() == 1)
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        false
    }
}
fn safe_mode(m: u32) -> bool {
    m & 0o7000 == 0 && m & 0o022 == 0 && m & 0o400 != 0
}
fn file(path: &Path) -> Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = o.open(path).map_err(|_| fail())?;
    let m = f.metadata().map_err(|_| fail())?;
    if !m.is_file()
        || !trusted(&m)
        || !safe_mode(mode(&m))
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(fail());
    }
    Ok(f)
}
fn unchanged(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (
            before.dev(),
            before.ino(),
            before.len(),
            before.mode(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
            before.nlink(),
        ) == (
            after.dev(),
            after.ino(),
            after.len(),
            after.mode(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
            after.nlink(),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (before, after);
        false
    }
}
fn hash(path: &Path) -> Result<(u64, String)> {
    let mut f = file(path)?;
    let before = f.metadata().map_err(|_| fail())?;
    let mut count = 0u64;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b).map_err(|_| fail())?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|n| *n <= DATA_LIMIT)
            .ok_or_else(|| err("HOST_CHECKPOINT_QUOTA"))?;
        h.update(&b[..n]);
    }
    let after = f.metadata().map_err(|_| fail())?;
    let current = fs::symlink_metadata(path).map_err(|_| fail())?;
    if count != before.len() || !unchanged(&before, &after) || !unchanged(&after, &current) {
        return Err(err("HOST_SOURCE_CHANGED"));
    }
    Ok((count, format!("{:x}", h.finalize())))
}
fn space(parent: &Path, needed: u64) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let p = std::ffi::CString::new(parent.as_os_str().as_bytes()).map_err(|_| fail())?;
        let mut v = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(p.as_ptr(), v.as_mut_ptr()) } != 0 {
            return Err(err("HOST_STORAGE_UNVERIFIED"));
        }
        let v = unsafe { v.assume_init() };
        if u128::from(v.f_bavail) * u128::from(v.f_frsize) < u128::from(needed) + 256 * 1024 * 1024
        {
            return Err(err("HOST_STORAGE_INSUFFICIENT"));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (parent, needed);
        Err(err("HOST_PLATFORM_UNVERIFIED"))
    }
}
fn walk(
    root: &Path,
    relative: &str,
    excluded: &BTreeSet<String>,
    items: &mut Vec<Item>,
    budget: &mut u64,
) -> Result<()> {
    let dir = root.join(relative);
    let m = fs::symlink_metadata(&dir).map_err(|_| fail())?;
    if !m.is_dir()
        || !trusted(&m)
        || !safe_mode(mode(&m))
        || fs::canonicalize(&dir).ok().as_deref() != Some(dir.as_path())
    {
        return Err(fail());
    }
    let mut names = fs::read_dir(&dir)
        .map_err(|_| fail())?
        .map(|e| e.map(|e| e.file_name()).map_err(|_| fail()))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        let name = name.to_str().ok_or_else(fail)?;
        let p = if relative.is_empty() {
            name.to_string()
        } else {
            format!("{relative}/{name}")
        };
        if !safe_path(&p) {
            return Err(fail());
        }
        if excluded.contains(&p) {
            continue;
        }
        *budget = budget
            .checked_add(2 * p.len() as u64 + 320)
            .filter(|v| *v <= MANIFEST_LIMIT - 4096)
            .ok_or_else(|| err("HOST_CHECKPOINT_QUOTA"))?;
        if items.len() >= 100000 {
            return Err(err("HOST_CHECKPOINT_QUOTA"));
        }
        let path = root.join(&p);
        let meta = fs::symlink_metadata(&path).map_err(|_| fail())?;
        if !trusted(&meta) || !safe_mode(mode(&meta)) {
            return Err(fail());
        }
        if meta.is_dir() && !meta.is_symlink() {
            items.push(Item {
                path: p.clone(),
                kind: "directory".into(),
                mode: mode(&meta),
                bytes: 0,
                sha256: None,
            });
            walk(root, &p, excluded, items, budget)?;
        } else if meta.is_file() && !meta.is_symlink() {
            let (bytes, h) = hash(&path)?;
            items.push(Item {
                path: p,
                kind: "file".into(),
                mode: mode(&meta),
                bytes,
                sha256: Some(h),
            });
        } else {
            return Err(fail());
        }
    }
    Ok(())
}
fn inventory(profile: &Path, excluded: &BTreeSet<String>, id: &str) -> Result<Inventory> {
    let mut items = Vec::new();
    walk(profile, "", excluded, &mut items, &mut 0)?;
    items.sort_by(|a, b| a.path.cmp(&b.path));
    let total = items
        .iter()
        .try_fold(0u64, |n, e| n.checked_add(e.bytes))
        .filter(|n| *n <= DATA_LIMIT - MANIFEST_LIMIT)
        .ok_or_else(|| err("HOST_CHECKPOINT_QUOTA"))?;
    let registry = items
        .iter()
        .find(|e| e.path == "installation-selection.json")
        .and_then(|e| e.sha256.clone())
        .ok_or_else(fail)?;
    Ok(Inventory {
        format: 1,
        id: id.into(),
        scope: "host-profile".into(),
        source_profile_sha256: digest(profile.to_string_lossy().as_bytes()),
        registry_sha256: registry,
        total_bytes: total,
        external_volumes_saved: false,
        host_writer_quiescence: "operator-acknowledged".into(),
        excluded_locks: excluded.iter().cloned().collect(),
        items,
    })
}
fn validate_inventory(m: &Inventory, profile: &Path) -> Result<()> {
    if m.format != 1
        || !installations::uuid(&m.id)
        || m.scope != "host-profile"
        || m.external_volumes_saved
        || m.host_writer_quiescence != "operator-acknowledged"
        || m.source_profile_sha256 != digest(profile.to_string_lossy().as_bytes())
        || !hash_valid(&m.registry_sha256)
        || m.items.len() > 100000
        || m.items.windows(2).any(|p| p[0].path >= p[1].path)
        || m.excluded_locks.windows(2).any(|p| p[0] >= p[1])
    {
        return Err(fail());
    }
    let mut dirs = BTreeSet::new();
    let mut sum = 0u64;
    for e in &m.items {
        if !safe_path(&e.path) || !safe_mode(e.mode) || m.excluded_locks.contains(&e.path) {
            return Err(fail());
        }
        if let Some((parent, _)) = e.path.rsplit_once('/')
            && !dirs.contains(parent)
        {
            return Err(fail());
        }
        match e.kind.as_str() {
            "directory" if e.bytes == 0 && e.sha256.is_none() => {
                dirs.insert(e.path.as_str());
            }
            "file" if e.bytes <= DATA_LIMIT && e.sha256.as_deref().is_some_and(hash_valid) => {}
            _ => return Err(fail()),
        };
        sum = sum
            .checked_add(e.bytes)
            .filter(|v| *v <= DATA_LIMIT - MANIFEST_LIMIT)
            .ok_or_else(fail)?;
    }
    if sum != m.total_bytes
        || m.items
            .iter()
            .find(|e| e.path == "installation-selection.json")
            .and_then(|e| e.sha256.as_deref())
            != Some(m.registry_sha256.as_str())
    {
        return Err(fail());
    }
    for p in &m.excluded_locks {
        if !safe_path(p)
            || !(p == "profile-session.lock"
                || p == "operation.lock"
                || p == "local-runtime/operation.lock"
                || p.strip_prefix("installations/")
                    .and_then(|s| s.strip_suffix("/operation.lock"))
                    .is_some_and(installations::uuid))
        {
            return Err(fail());
        }
    }
    Ok(())
}
struct Body {
    prefix: Cursor<Vec<u8>>,
    root: PathBuf,
    items: Vec<Item>,
    next: usize,
    current: Option<(File, Item, fs::Metadata, u64, Sha256)>,
}
impl Read for Body {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        let n = self.prefix.read(b)?;
        if n > 0 {
            return Ok(n);
        }
        loop {
            if let Some((f, e, before, remaining, h)) = &mut self.current {
                if *remaining > 0 {
                    let cap = b.len().min(*remaining as usize);
                    let n = f.read(&mut b[..cap])?;
                    if n == 0 {
                        return Err(std::io::Error::other("HOST_SOURCE_CHANGED"));
                    }
                    h.update(&b[..n]);
                    *remaining -= n as u64;
                    return Ok(n);
                }
                let mut extra = [0u8; 1];
                if f.read(&mut extra)? != 0
                    || !unchanged(before, &f.metadata()?)
                    || !unchanged(before, &fs::symlink_metadata(self.root.join(&e.path))?)
                    || format!("{:x}", h.clone().finalize()) != e.sha256.as_deref().unwrap_or("")
                {
                    return Err(std::io::Error::other("HOST_SOURCE_CHANGED"));
                }
                self.current = None;
            }
            let Some(e) = self.items.get(self.next).cloned() else {
                return Ok(0);
            };
            self.next += 1;
            if e.kind != "file" {
                continue;
            }
            let f = file(&self.root.join(&e.path))
                .map_err(|_| std::io::Error::other("HOST_SOURCE_CHANGED"))?;
            let meta = f.metadata()?;
            if meta.len() != e.bytes || mode(&meta) != e.mode {
                return Err(std::io::Error::other("HOST_SOURCE_CHANGED"));
            }
            self.current = Some((f, e.clone(), meta, e.bytes, Sha256::new()));
        }
    }
}
fn no_destination(p: &Path) -> Result<()> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(err("PROFILE_DESTINATION_EXISTS")),
        Err(_) => Err(err("PROFILE_DESTINATION_UNAVAILABLE")),
    }
}
fn private_new(path: &Path) -> Result<File> {
    private_options()
        .open(path)
        .map_err(|_| err("HOST_WRITE_UNCERTAIN"))
}
fn receipt(m: &Inventory, operation: &str, encoded: &[u8]) -> HostReceipt {
    HostReceipt {
        id: m.id.clone(),
        operation: operation.into(),
        files: m.items.iter().filter(|e| e.kind == "file").count(),
        bytes: m.total_bytes,
        manifest_sha256: digest(encoded),
        external_volumes_saved: false,
        host_writer_quiescence: m.host_writer_quiescence.clone(),
    }
}
pub fn checkpoint_host(
    profile: &Path,
    key: &Path,
    archive: &Path,
    apps_closed: bool,
    writers_stopped: bool,
) -> Result<HostReceipt> {
    checkpoint_host_guarded(
        profile,
        key,
        archive,
        apps_closed,
        writers_stopped,
        None,
        None,
        || Ok(()),
        || Ok(()),
    )
    .map(|(receipt, (), ())| receipt)
}
pub(crate) fn checkpoint_host_anchored<T>(
    profile: &Path,
    key: &Path,
    archive: &Path,
    writers_stopped: bool,
    anchor: ProfileAnchor,
    after: impl FnOnce() -> Result<T>,
) -> Result<(HostReceipt, T)> {
    checkpoint_host_guarded(
        profile,
        key,
        archive,
        true,
        writers_stopped,
        Some(anchor),
        None,
        || Ok(()),
        after,
    )
    .map(|(receipt, (), after)| (receipt, after))
}
pub(crate) fn checkpoint_host_borrowed<B, T>(
    profile: &Path,
    key: &Path,
    archive: &Path,
    session: &ProfileSession,
    before: impl FnOnce() -> Result<B>,
    after: impl FnOnce() -> Result<T>,
) -> Result<(HostReceipt, B, T)> {
    checkpoint_host_guarded(
        profile,
        key,
        archive,
        true,
        true,
        None,
        Some(session),
        before,
        after,
    )
}
#[allow(clippy::too_many_arguments)]
fn checkpoint_host_guarded<B, T>(
    profile: &Path,
    key: &Path,
    archive: &Path,
    apps_closed: bool,
    writers_stopped: bool,
    anchor: Option<ProfileAnchor>,
    borrowed: Option<&ProfileSession>,
    before: impl FnOnce() -> Result<B>,
    after: impl FnOnce() -> Result<T>,
) -> Result<(HostReceipt, B, T)> {
    acknowledgement(apps_closed)?;
    if !writers_stopped {
        return Err(err("HOST_WRITER_ACK_REQUIRED"));
    }
    canonical_private(profile)?;
    let parent = archive_parent(profile, archive, key)?;
    let key = external_key(profile, key)?;
    no_destination(archive)?;
    let _session = if let Some(session) = borrowed {
        if anchor.is_some() {
            return Err(err("PROFILE_LOCK_INVALID"));
        }
        session.check_exclusive(profile)?;
        None
    } else {
        Some(match anchor {
            Some(anchor) => anchored_session(profile, anchor, true)?,
            None => session_lock(profile, true)?,
        })
    };
    let _profile = lock_file(profile, "operation.lock", true)?;
    let s = capture(profile)?;
    let _roots = root_locks(profile, &s.spaces)?;
    let mut excluded = BTreeSet::from(["operation.lock".into(), "profile-session.lock".into()]);
    for r in &s.spaces {
        if r.available {
            excluded.insert(format!("{}/operation.lock", r.relative));
        }
    }
    let prepared = before()?;
    if let Some(session) = borrowed {
        session.check_exclusive(profile)?;
    }
    let id = Uuid::new_v4().to_string();
    let m = inventory(profile, &excluded, &id)?;
    validate_inventory(&m, profile)?;
    let encoded = serde_json::to_vec(&m).map_err(|_| fail())?;
    if encoded.len() as u64 > MANIFEST_LIMIT {
        return Err(err("HOST_CHECKPOINT_QUOTA"));
    }
    let mut prefix = (encoded.len() as u64).to_be_bytes().to_vec();
    prefix.extend(&encoded);
    let needed = m.total_bytes + prefix.len() as u64 + 1024 * 1024;
    space(&parent, needed)?;
    let pending = parent.join(format!(".host-checkpoint-{id}.pending"));
    let mut output = private_new(&pending)?;
    let mut body = Body {
        prefix: Cursor::new(prefix),
        root: profile.into(),
        items: m.items.clone(),
        next: 0,
        current: None,
    };
    super::super::maintenance_stream::seal(&mut body, &mut output, &key, CONTEXT, DATA_LIMIT)
        .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    output.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    drop(output);
    let paired = after()?;
    let second = inventory(profile, &excluded, &id)?;
    if m.items != second.items || m.registry_sha256 != second.registry_sha256 {
        return Err(err("HOST_SOURCE_CHANGED"));
    }
    let mut verify = file(&pending)?;
    super::super::maintenance_stream::open(
        &mut verify,
        &mut std::io::sink(),
        &key,
        CONTEXT,
        DATA_LIMIT,
    )
    .map_err(|_| fail())?;
    publish(&pending, archive)?;
    fs::remove_file(&pending).map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    sync(&parent)?;
    if let Some(session) = borrowed {
        session.check_exclusive(profile)?;
    }
    Ok((
        receipt(&m, "host-profile-checkpoint", &encoded),
        prepared,
        paired,
    ))
}
fn publish_directory(source: &Path, target: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(|_| fail())?;
        let b = std::ffi::CString::new(target.as_os_str().as_bytes()).map_err(|_| fail())?;
        #[cfg(target_os = "macos")]
        let n = unsafe {
            libc::renameatx_np(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(target_os = "linux")]
        let n = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if n != 0 {
            return Err(err("HOST_WRITE_UNCERTAIN"));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (source, target);
        Err(err("HOST_PLATFORM_UNVERIFIED"))
    }
}
// Delete only this fresh, fully authenticated intermediate, after the archive
// and external key were rechecked. Replaced/changed candidates are preserved.
fn retire_plaintext(path: &Path, plain: File, expected: &fs::Metadata) -> Result<()> {
    let held = plain.metadata().map_err(|_| fail())?;
    let current = fs::symlink_metadata(path).map_err(|_| fail())?;
    if !current.is_file() || !unchanged(expected, &held) || !unchanged(expected, &current) {
        return Err(fail());
    }
    fs::remove_file(path).map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    drop(plain);
    Ok(())
}

pub fn extract_host(
    profile: &Path,
    key: &Path,
    archive: &Path,
    destination: &Path,
    ack: bool,
) -> Result<HostReceipt> {
    acknowledgement(ack)?;
    let expected_parent =
        fs::canonicalize(profile.parent().ok_or_else(fail)?).map_err(|_| fail())?;
    if !profile.is_absolute()
        || expected_parent.join(profile.file_name().ok_or_else(fail)?) != profile
    {
        return Err(fail());
    }
    let archive_dir = archive_parent(profile, archive, key)?;
    let key_path = key.to_path_buf();
    let key = external_key(profile, key)?;
    let parent = canonical_private(destination.parent().ok_or_else(fail)?)?;
    if destination.starts_with(profile)
        || profile.starts_with(destination)
        || archive.starts_with(destination)
        || key_path.starts_with(destination)
    {
        return Err(fail());
    }
    no_destination(destination)?;
    let archive_name = archive
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or_else(fail)?;
    let canonical_archive = archive_dir.join(archive_name);
    let mut source = file(&canonical_archive)?;
    let meta = source.metadata().map_err(|_| fail())?;
    if mode(&meta) != 0o600 || meta.len() > DATA_LIMIT + 16 * 1024 * 1024 {
        return Err(fail());
    }
    space(&parent, meta.len().checked_mul(2).ok_or_else(fail)?)?;
    let stage = parent.join(format!(".host-extract-{}.pending", Uuid::new_v4()));
    installations::new_directory(&stage)?;
    let payload = stage.join("payload.pending");
    let mut plain = private_new(&payload)?;
    super::super::maintenance_stream::open(&mut source, &mut plain, &key, CONTEXT, DATA_LIMIT)
        .map_err(|_| fail())?;
    plain.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
    // Reopen a read-only handle: writer was deliberately created without read access.
    drop(plain);
    let mut plain = file(&payload)?;
    let plain_identity = plain.metadata().map_err(|_| fail())?;
    let mut size = [0u8; 8];
    plain.read_exact(&mut size).map_err(|_| fail())?;
    let len = u64::from_be_bytes(size);
    if len == 0 || len > MANIFEST_LIMIT {
        return Err(fail());
    }
    let mut encoded = vec![0u8; len as usize];
    plain.read_exact(&mut encoded).map_err(|_| fail())?;
    let m: Inventory = serde_json::from_slice(&encoded).map_err(|_| fail())?;
    validate_inventory(&m, profile)?;
    let recovered = stage.join("profile");
    installations::new_directory(&recovered)?;
    for e in &m.items {
        let path = recovered.join(&e.path);
        if e.kind == "directory" {
            installations::new_directory(&path)?;
        } else {
            let mut out = private_new(&path)?;
            let mut limited = Read::by_ref(&mut plain).take(e.bytes);
            let mut h = Sha256::new();
            let mut n = 0u64;
            let mut b = [0u8; 65536];
            loop {
                let got = limited.read(&mut b).map_err(|_| fail())?;
                if got == 0 {
                    break;
                }
                out.write_all(&b[..got])
                    .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
                h.update(&b[..got]);
                n += got as u64;
            }
            if n != e.bytes || Some(format!("{:x}", h.finalize())) != e.sha256 {
                return Err(fail());
            }
            out.sync_all().map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
        }
    }
    let mut extra = [0u8; 1];
    if plain.read(&mut extra).map_err(|_| fail())? != 0 {
        return Err(fail());
    }
    let r = capture(&recovered)?;
    if digest(&r.registry) != m.registry_sha256 {
        return Err(fail());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in m.items.iter().rev() {
            let p = recovered.join(&e.path);
            fs::set_permissions(&p, fs::Permissions::from_mode(e.mode)).map_err(|_| fail())?;
            if e.kind == "directory" {
                sync(&p)?;
            }
        }
    }
    write_new(&stage.join("manifest.json"), &encoded)?;
    write_new(
        &stage.join("verified.json"),
        &serde_json::to_vec(&receipt(
            &m,
            "host-profile-extracted-not-activated",
            &encoded,
        ))
        .map_err(|_| fail())?,
    )?;
    sync(&recovered)?;
    sync(&stage)?;
    // Every reconstructed file/hash/mode and receipt is complete. Preserve the
    // original encrypted archive/key and refuse changed input before removing
    // only the fresh redundant plaintext. Failure staging/profile roots remain.
    if !unchanged(&meta, &source.metadata().map_err(|_| fail())?)
        || !unchanged(
            &meta,
            &fs::symlink_metadata(&canonical_archive).map_err(|_| fail())?,
        )
        || external_key(profile, &key_path)? != key
    {
        return Err(fail());
    }
    retire_plaintext(&payload, plain, &plain_identity)?;
    sync(&stage)?;
    // Nothing here registers, activates or overwrites profile roots.
    publish_directory(&stage, destination)?;
    sync(&parent)?;
    Ok(receipt(
        &m,
        "host-profile-extracted-not-activated",
        &encoded,
    ))
}

fn publication_identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (a.dev(), a.ino(), a.uid()) == (b.dev(), b.ino(), b.uid())
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
/// Activate only a freshly authenticated extraction into an absent original
/// namespace. Caller holds the external profile anchor and retained trust fence.
pub(crate) fn activate_missing_host(
    profile: &Path,
    extracted: &Path,
    expected: &HostReceipt,
) -> Result<()> {
    no_destination(profile)?;
    let parent = canonical_private(profile.parent().ok_or_else(fail)?)?;
    let parent_before = fs::symlink_metadata(&parent).map_err(|_| fail())?;
    canonical_private(extracted)?;
    let encoded_path = extracted.join("manifest.json");
    let mut f = file(&encoded_path)?;
    if f.metadata().map_err(|_| fail())?.len() > MANIFEST_LIMIT {
        return Err(fail());
    }
    let mut encoded = Vec::new();
    f.read_to_end(&mut encoded).map_err(|_| fail())?;
    if digest(&encoded) != expected.manifest_sha256 {
        return Err(fail());
    }
    let manifest: Inventory = serde_json::from_slice(&encoded).map_err(|_| fail())?;
    validate_inventory(&manifest, profile)?;
    let candidate = extracted.join("profile");
    canonical_private(&candidate)?;
    let candidate_before = fs::symlink_metadata(&candidate).map_err(|_| fail())?;
    let inventory = inventory(&candidate, &BTreeSet::new(), &manifest.id)?;
    if inventory.items != manifest.items
        || inventory.total_bytes != manifest.total_bytes
        || inventory.registry_sha256 != manifest.registry_sha256
        || expected.id != manifest.id
        || expected.bytes != manifest.total_bytes
        || expected.files != manifest.items.iter().filter(|i| i.kind == "file").count()
        || digest(&capture(&candidate)?.registry) != manifest.registry_sha256
    {
        return Err(fail());
    }
    if !unchanged(
        &candidate_before,
        &fs::symlink_metadata(&candidate).map_err(|_| fail())?,
    ) || !unchanged(
        &parent_before,
        &fs::symlink_metadata(&parent).map_err(|_| fail())?,
    ) {
        return Err(fail());
    }
    no_destination(profile)?;
    publish_directory(&candidate, profile)?;
    // Publication can have happened if a later durability/identity check fails.
    // Preserve the restored tree and the external receipt; never retry overwrite.
    sync(&parent).map_err(|_| err("HOST_RESTORE_UNCERTAIN"))?;
    let after = fs::symlink_metadata(profile).map_err(|_| err("HOST_RESTORE_UNCERTAIN"))?;
    if !after.is_dir()
        || after.is_symlink()
        || mode(&after) != mode(&candidate_before)
        || !publication_identity(&candidate_before, &after)
    {
        return Err(err("HOST_RESTORE_UNCERTAIN"));
    }
    Ok(())
}

// These fixtures exercise the supported Unix host archive/activation boundary.
// Native Windows is tested separately for explicit unsupported refusal below.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::installations::InstallationController;
    // Own only the fresh synthetic root. Successful tests retire it; a panic or
    // changed root preserves the fixture for diagnosis. Never scan old temp data.
    struct FixtureRoot {
        path: PathBuf,
        held: File,
    }
    impl FixtureRoot {
        fn new() -> Self {
            let path = fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("host-checkpoint-{}", Uuid::new_v4()));
            installations::new_directory(&path).unwrap();
            use std::os::unix::fs::OpenOptionsExt;
            let held = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
                .open(&path)
                .unwrap();
            Self { path, held }
        }
    }
    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            use std::os::unix::fs::MetadataExt;
            let Ok(original) = self.held.metadata() else {
                return;
            };
            let Ok(current) = fs::symlink_metadata(&self.path) else {
                return;
            };
            if current.is_dir()
                && current.dev() == original.dev()
                && current.ino() == original.ino()
                && current.uid() == original.uid()
                && current.mode() & 0o077 == 0
            {
                // std's removal does not follow child symlinks. A failure leaves
                // remaining synthetic bytes; it never expands cleanup scope.
                if fs::remove_dir_all(&self.path).is_err() {
                    eprintln!("synthetic host fixture cleanup incomplete");
                }
            }
        }
    }
    fn fixture() -> (FixtureRoot, PathBuf, PathBuf, PathBuf) {
        let root = FixtureRoot::new();
        let p = root.path.join("source");
        let c = InstallationController::new(p.clone(), None).unwrap();
        drop(c);
        let k = root.path.join("key.bin");
        write_new(&k, &[17; 32]).unwrap();
        let a = root.path.join("archive.exb");
        (root, p, k, a)
    }
    #[test]
    fn successful_fixture_cleanup_never_follows_external_symlink() {
        let outside = FixtureRoot::new();
        let witness = outside.path.join("witness");
        write_new(&witness, b"preserve outside bytes").unwrap();
        let root = FixtureRoot::new();
        let path = root.path.clone();
        std::os::unix::fs::symlink(&outside.path, path.join("alias")).unwrap();
        drop(root);
        assert!(!path.exists());
        assert_eq!(fs::read(witness).unwrap(), b"preserve outside bytes");
    }
    #[test]
    fn failed_fixture_cleanup_preserves_diagnostic_bytes() {
        let path = std::cell::RefCell::new(None);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let root = FixtureRoot::new();
            *path.borrow_mut() = Some(root.path.clone());
            write_new(&root.path.join("failure"), b"synthetic failure bytes").unwrap();
            panic!("synthetic failure for retention check");
        }));
        assert!(result.is_err());
        let path = path.into_inner().unwrap();
        assert_eq!(
            fs::read(path.join("failure")).unwrap(),
            b"synthetic failure bytes"
        );
        // Only this deliberately induced and verified test failure is retired.
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn replaced_fixture_root_is_preserved_without_touching_either_directory() {
        let root = FixtureRoot::new();
        let path = root.path.clone();
        let moved = path.with_extension("retained");
        fs::rename(&path, &moved).unwrap();
        installations::new_directory(&path).unwrap();
        write_new(&path.join("replacement"), b"preserve replacement").unwrap();
        drop(root);
        assert!(moved.exists());
        assert_eq!(
            fs::read(path.join("replacement")).unwrap(),
            b"preserve replacement"
        );
        // Both names were created by this test; assertions completed first.
        fs::remove_dir_all(path).unwrap();
        fs::remove_dir_all(moved).unwrap();
    }
    #[test]
    fn borrowed_fence_spans_callbacks_and_captures_prepared_bytes() {
        let (_root, p, k, a) = fixture();
        let session = session_lock(&p, true).unwrap();
        let witness = p.join("local-runtime/prepared-witness");
        let (receipt, (), ()) = checkpoint_host_borrowed(
            &p,
            &k,
            &a,
            &session,
            || {
                assert!(session_lock(&p, false).is_err());
                assert!(lock_file(&p, "operation.lock", true).is_err());
                assert!(lock_file(&p.join("local-runtime"), "operation.lock", true).is_err());
                write_new(&witness, b"prepared-under-fence")
            },
            || {
                assert!(session_lock(&p, true).is_err());
                assert!(lock_file(&p.join("local-runtime"), "operation.lock", true).is_err());
                Ok(())
            },
        )
        .unwrap();
        drop(session);
        let target = k.with_file_name("extracted");
        let opened = extract_host(&p, &k, &a, &target, true).unwrap();
        assert_eq!(receipt.manifest_sha256, opened.manifest_sha256);
        assert_eq!(
            fs::read(target.join("profile/local-runtime/prepared-witness")).unwrap(),
            b"prepared-under-fence"
        );
    }
    #[test]
    fn borrowed_fence_rejects_wrong_scope_shared_guard_and_post_archive_changes() {
        let (_root, p, k, a) = fixture();
        let (_other_root, other, _, _) = fixture();
        let wrong = session_lock(&other, true).unwrap();
        assert_eq!(
            checkpoint_host_borrowed(&p, &k, &a, &wrong, || Ok(()), || Ok(()))
                .unwrap_err()
                .code,
            "PROFILE_LOCK_INVALID"
        );
        let shared = session_lock(&p, false).unwrap();
        assert_eq!(
            checkpoint_host_borrowed(&p, &k, &a, &shared, || Ok(()), || Ok(()))
                .unwrap_err()
                .code,
            "PROFILE_LOCK_INVALID"
        );
        drop(shared);
        let held = session_lock(&p, true).unwrap();
        assert_eq!(
            checkpoint_host_borrowed(
                &p,
                &k,
                &a,
                &held,
                || Ok(()),
                || write_new(&p.join("local-runtime/late-writer"), b"late")
            )
            .unwrap_err()
            .code,
            "HOST_SOURCE_CHANGED"
        );
        assert!(!a.exists());
    }
    #[test]
    fn callback_failure_never_publishes_borrowed_archive() {
        let (_root, p, k, a) = fixture();
        let held = session_lock(&p, true).unwrap();
        assert_eq!(
            checkpoint_host_borrowed(
                &p,
                &k,
                &a,
                &held,
                || Err::<(), _>(err("UPDATE_SOURCE_CHANGED")),
                || Ok(())
            )
            .unwrap_err()
            .code,
            "UPDATE_SOURCE_CHANGED"
        );
        assert!(!a.exists());
        assert_eq!(
            checkpoint_host_borrowed(
                &p,
                &k,
                &a,
                &held,
                || Ok(()),
                || Err::<(), _>(err("UPDATE_SOURCE_CHANGED"))
            )
            .unwrap_err()
            .code,
            "UPDATE_SOURCE_CHANGED"
        );
        assert!(!a.exists());
    }
    #[test]
    fn host_bytes_above_profile_envelope_limit_extract_exact_without_activation() {
        let (_root, p, k, a) = fixture();
        let dir = p.join("local-runtime/restore-candidate");
        installations::new_directory(&dir).unwrap();
        let data = dir.join("candidate.bin");
        let f = private_new(&data).unwrap();
        f.set_len(65 * 1024 * 1024).unwrap();
        f.sync_all().unwrap();
        drop(f);
        write_new(&dir.join("empty-witness"), b"").unwrap();
        let before = hash(&data).unwrap();
        let pointer = fs::read(p.join("installation-selection.json")).unwrap();
        let saved = checkpoint_host(&p, &k, &a, true, true).unwrap();
        assert!(saved.bytes > 64 * 1024 * 1024);
        assert!(!saved.external_volumes_saved);
        assert_eq!(saved.host_writer_quiescence, "operator-acknowledged");
        let archive_before = hash(&a).unwrap();
        let key_before = hash(&k).unwrap();
        let target = k.with_file_name("recovered");
        let opened = extract_host(&p, &k, &a, &target, true).unwrap();
        assert_eq!(saved.id, opened.id);
        assert_eq!(saved.manifest_sha256, opened.manifest_sha256);
        assert_eq!(
            hash(&target.join("profile/local-runtime/restore-candidate/candidate.bin")).unwrap(),
            before
        );
        assert!(
            target
                .join("profile/local-runtime/restore-candidate/empty-witness")
                .exists()
        );
        assert!(!target.join("payload.pending").exists());
        assert_eq!(hash(&a).unwrap(), archive_before);
        assert_eq!(hash(&k).unwrap(), key_before);
        assert!(!target.join("profile/operation.lock").exists());
        assert!(!target.join("profile/local-runtime/operation.lock").exists());
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            pointer
        );
        assert_eq!(hash(&data).unwrap(), before);
        assert_eq!(
            checkpoint_host(&p, &k, &a, true, true).unwrap_err().code,
            "PROFILE_DESTINATION_EXISTS"
        );
        assert_eq!(
            extract_host(&p, &k, &a, &target, true).unwrap_err().code,
            "PROFILE_DESTINATION_EXISTS"
        );
    }
    #[test]
    fn closed_profile_and_writer_ack_precede_copy() {
        let (_root, p, k, a) = fixture();
        assert_eq!(
            checkpoint_host(&p, &k, &a, true, false).unwrap_err().code,
            "HOST_WRITER_ACK_REQUIRED"
        );
        let c = InstallationController::new(p.clone(), None).unwrap();
        assert_eq!(
            checkpoint_host(&p, &k, &a, true, true).unwrap_err().code,
            "PROFILE_BUSY"
        );
        drop(c);
        assert!(!a.exists());
    }
    #[cfg(unix)]
    #[test]
    fn links_unsafe_paths_and_permissions_never_publish() {
        let (_root, p, k, a) = fixture();
        std::os::unix::fs::symlink(&k, p.join("alias")).unwrap();
        assert!(checkpoint_host(&p, &k, &a, true, true).is_err());
        assert!(!a.exists());
        let (_root, p, k, a) = fixture();
        let bad = p.join("unsafe-mode");
        write_new(&bad, b"synthetic").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o622)).unwrap();
        assert!(checkpoint_host(&p, &k, &a, true, true).is_err());
        assert!(!a.exists());
        let (_root, p, k, a) = fixture();
        let first = p.join("hardlink-source");
        write_new(&first, b"synthetic").unwrap();
        fs::hard_link(&first, p.join("hardlink-alias")).unwrap();
        assert!(checkpoint_host(&p, &k, &a, true, true).is_err());
        assert!(!a.exists());
        for bad in ["../secret", "/absolute", "a//b", "a/./b", "a\\b", "a:b"] {
            assert!(!safe_path(bad));
        }
    }
    #[test]
    fn tamper_wrong_domain_and_wrong_source_namespace_leave_target_unpublished() {
        let (_root, p, k, a) = fixture();
        checkpoint_host(&p, &k, &a, true, true).unwrap();
        let bytes = fs::read(&a).unwrap();
        let bad = k.with_file_name("truncated.exb");
        write_new(&bad, &bytes[..bytes.len() - 1]).unwrap();
        let target = k.with_file_name("bad-extract");
        assert!(extract_host(&p, &k, &bad, &target, true).is_err());
        assert!(!target.exists());
        let other = p.with_file_name("wrong-source");
        assert!(extract_host(&other, &k, &a, &target, true).is_err());
        assert!(!target.exists());
        let profile_archive = k.with_file_name("metadata.exb");
        backup_stream(&p, &k, &profile_archive, true).unwrap();
        assert!(extract_host(&p, &k, &profile_archive, &target, true).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(&a).unwrap(), bytes);
    }
    #[test]
    fn replaced_plaintext_is_preserved_without_removing_either_file() {
        let (_root, _, key, _) = fixture();
        let path = key.with_file_name("payload.pending");
        write_new(&path, b"authenticated synthetic bytes").unwrap();
        let held = file(&path).unwrap();
        let before = held.metadata().unwrap();
        let retained = key.with_file_name("retained-plaintext");
        fs::rename(&path, &retained).unwrap();
        write_new(&path, b"changed synthetic bytes").unwrap();
        assert!(retire_plaintext(&path, held, &before).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"changed synthetic bytes");
        assert_eq!(
            fs::read(&retained).unwrap(),
            b"authenticated synthetic bytes"
        );
    }

    #[test]
    fn duplicate_or_unparented_manifest_entries_refuse() {
        let (_root, p, _, _) = fixture();
        let id = Uuid::new_v4().to_string();
        let mut m = inventory(&p, &BTreeSet::new(), &id).unwrap();
        validate_inventory(&m, &p).unwrap();
        m.items.push(m.items[0].clone());
        assert!(validate_inventory(&m, &p).is_err());
        let mut m = inventory(&p, &BTreeSet::new(), &id).unwrap();
        m.items.push(Item {
            path: "z-missing-parent/child".into(),
            kind: "file".into(),
            mode: 0o600,
            bytes: 0,
            sha256: Some(digest(b"")),
        });
        m.items.sort_by(|a, b| a.path.cmp(&b.path));
        assert!(validate_inventory(&m, &p).is_err());
    }
    #[test]
    fn changing_source_and_publication_collision_refuse() {
        let (_root, p, k, _) = fixture();
        let data = p.join("changing-source");
        write_new(&data, b"initial").unwrap();
        let m = inventory(&p, &BTreeSet::new(), &Uuid::new_v4().to_string()).unwrap();
        let mut body = Body {
            prefix: Cursor::new(Vec::new()),
            root: p,
            items: m.items,
            next: 0,
            current: None,
        };
        fs::write(&data, b"changed").unwrap();
        assert!(body.read_to_end(&mut Vec::new()).is_err());
        let stage = k.with_file_name("publish-stage");
        let target = k.with_file_name("occupied-target");
        installations::new_directory(&stage).unwrap();
        installations::new_directory(&target).unwrap();
        write_new(&target.join("witness"), b"preserve").unwrap();
        assert!(publish_directory(&stage, &target).is_err());
        assert!(stage.exists());
        assert_eq!(fs::read(target.join("witness")).unwrap(), b"preserve");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    fn fixture() -> (crate::windows_private::PrivateDirectory, PathBuf, PathBuf) {
        let parent = fs::canonicalize(std::env::var_os("LOCALAPPDATA").unwrap()).unwrap();
        let root = crate::windows_private::PrivateDirectory::create(
            &parent.join(format!("exhibitos-host-gate-{}", Uuid::new_v4())),
        )
        .unwrap();
        write_private_new(root.path(), "witness", b"synthetic-host-preserved", 1024).unwrap();
        write_private_new(root.path(), "key.bin", &[17; 32], 32).unwrap();
        let key = root.path().join("key.bin");
        let archive = root.path().join("archive.bin");
        (root, key, archive)
    }
    fn preserved(root: crate::windows_private::PrivateDirectory, archive: &Path) {
        assert!(!archive.exists());
        assert_eq!(
            fs::read(root.path().join("witness")).unwrap(),
            b"synthetic-host-preserved"
        );
        assert_eq!(fs::read(root.path().join("key.bin")).unwrap(), [17; 32]);
        let path = root.path().to_owned();
        drop(root);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn unsupported_host_checkpoint_never_creates_archive_or_reads_missing_source() {
        let (root, key, archive) = fixture();
        for (apps, writers) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                checkpoint_host(
                    &root.path().join("missing-profile"),
                    &key,
                    &archive,
                    apps,
                    writers
                )
                .unwrap_err()
                .code,
                "PROFILE_PLATFORM_UNVERIFIED"
            );
        }
        assert!(!root.path().join("missing-profile").exists());
        preserved(root, &archive);
    }
    #[test]
    fn unsupported_host_extraction_never_creates_destination_or_rewrites_input() {
        let (root, key, archive) = fixture();
        let destination = root.path().join("recovered");
        for ack in [false, true] {
            assert_eq!(
                extract_host(
                    &root.path().join("missing-profile"),
                    &key,
                    &archive,
                    &destination,
                    ack
                )
                .unwrap_err()
                .code,
                "PROFILE_PLATFORM_UNVERIFIED"
            );
        }
        assert!(!destination.exists());
        preserved(root, &archive);
    }
    #[test]
    fn unsupported_borrowed_host_checkpoint_never_calls_writer_callbacks() {
        let (root, key, archive) = fixture();
        let profile =
            crate::windows_private::PrivateDirectory::create(&root.path().join("profile")).unwrap();
        let session = session_lock(profile.path(), true).unwrap();
        let calls = std::cell::Cell::new(0);
        assert_eq!(
            checkpoint_host_borrowed(
                profile.path(),
                &key,
                &archive,
                &session,
                || {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                || {
                    calls.set(calls.get() + 1);
                    Ok(())
                }
            )
            .unwrap_err()
            .code,
            "PROFILE_PLATFORM_UNVERIFIED"
        );
        assert_eq!(calls.get(), 0);
        drop(session);
        drop(profile);
        preserved(root, &archive);
    }
}
