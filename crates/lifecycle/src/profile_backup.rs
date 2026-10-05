// SPDX-License-Identifier: Apache-2.0
//! App-closed, authenticated profile metadata recovery. Runtime data is never copied or erased.
#[path = "host_checkpoint.rs"]
mod host_checkpoint;
use super::installation_backup::source_bytes;
use super::installations::{self, Registry};
use super::*;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
pub use host_checkpoint::{HostReceipt, checkpoint_host, extract_host};
pub(crate) use host_checkpoint::{checkpoint_host_anchored, checkpoint_host_borrowed};
use std::collections::BTreeMap;
const MAGIC: &[u8] = b"ExhibitOS-profile-v1\0";
const LIMIT: u64 = 64 * 1024 * 1024;
const MAX_HISTORY: usize = 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    format: u32,
    id: String,
    at: u64,
    registry: Vec<u8>,
    #[serde(deserialize_with = "unique_history")]
    history: BTreeMap<String, Vec<u8>>,
    spaces: Vec<Space>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Space {
    id: String,
    kind: String,
    relative: String,
    available: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReceipt {
    pub id: String,
    pub operation: String,
    pub spaces: usize,
    pub history: usize,
    pub archive_sha256: String,
    pub at: u64,
}
fn unique_history<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, Vec<u8>>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = BTreeMap<String, Vec<u8>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded unique history files")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((name, bytes)) = map.next_entry::<String, Vec<u8>>()? {
                if result.len() >= MAX_HISTORY
                    || bytes.len() > 64 * 1024
                    || result.insert(name, bytes).is_some()
                {
                    return Err(serde::de::Error::custom("history inventory invalid"));
                }
            }
            Ok(result)
        }
    }
    d.deserialize_map(V)
}
fn canonical_private(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    installations::private_directory(path).map_err(|_| err("PROFILE_PATH_INVALID"))?;
    Ok(path.into())
}
fn lock_file(root: &Path, name: &str, exclusive: bool) -> Result<File> {
    #[cfg(windows)]
    {
        let directory = super::windows_private::PrivateDirectory::inspect(root)?;
        let file = directory.lock_record(name)?;
        if exclusive {
            file.try_lock_exclusive()
        } else {
            FileExt::try_lock_shared(&file)
        }
        .map_err(|_| err("PROFILE_BUSY"))?;
        directory.check_record(&file, name)?;
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        let path = root.join(name);
        let file = match private_options().open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut o = OpenOptions::new();
                o.read(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                o.open(&path).map_err(|_| err("PROFILE_LOCK_INVALID"))?
            }
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        let m = file.metadata().map_err(|_| err("PROFILE_LOCK_INVALID"))?;
        if !m.is_file() {
            return Err(err("PROFILE_LOCK_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::symlink_metadata(&path).map_err(|_| err("PROFILE_LOCK_INVALID"))?;
            if m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.mode() & 0o7777 != 0o600
                || current.is_symlink()
                || (m.dev(), m.ino()) != (current.dev(), current.ino())
            {
                return Err(err("PROFILE_LOCK_INVALID"));
            }
        }
        if exclusive {
            file.try_lock_exclusive()
        } else {
            FileExt::try_lock_shared(&file)
        }
        .map_err(|_| err("PROFILE_BUSY"))?;
        Ok(file)
    }
}
/// Owns the exact locked handle and parent fence. A borrowed Windows clone
/// shares this owner through Arc and never duplicates/closes the native handle.
pub(crate) struct ProfileAnchor {
    #[cfg(not(windows))]
    file: File,
    #[cfg(windows)]
    native: std::sync::Arc<WindowsAnchor>,
}
#[cfg(windows)]
struct WindowsAnchor {
    file: File,
    parent: super::windows_private::ParentDirectory,
    profile_name: String,
    lock_name: String,
    exclusive: bool,
}
impl ProfileAnchor {
    pub(crate) fn try_clone(&self) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                native: std::sync::Arc::clone(&self.native),
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                file: self.file.try_clone()?,
            })
        }
    }
    #[cfg(windows)]
    fn check(&self, profile: &Path, exclusive: bool) -> Result<()> {
        self.native
            .parent
            .check_record(&self.native.file, &self.native.lock_name)?;
        let name = profile
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        if self.native.exclusive != exclusive
            || !self.native.profile_name.eq_ignore_ascii_case(name)
            || profile
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .as_deref()
                != Some(self.native.parent.path())
        {
            return Err(err("PROFILE_LOCK_INVALID"));
        }
        Ok(())
    }
}
/// Hold the pathname fence as well as the legacy inode fence. The anchor is
/// outside the replaceable profile and must never be unlinked by maintenance.
pub(crate) struct ProfileSession {
    profile: PathBuf,
    #[cfg(not(windows))]
    identity: fs::Metadata,
    #[cfg(windows)]
    root_guard: super::windows_private::PrivateDirectory,
    exclusive: bool,
    _anchor: ProfileAnchor,
    _legacy: File,
}
impl ProfileSession {
    pub(crate) fn check_exclusive(&self, profile: &Path) -> Result<()> {
        canonical_private(profile)?;
        #[cfg(unix)]
        let current = fs::symlink_metadata(profile).map_err(|_| err("PROFILE_PATH_INVALID"))?;
        #[cfg(windows)]
        {
            self.root_guard.check()?;
            self._anchor.check(profile, self.exclusive)?;
            self.root_guard
                .check_record(&self._legacy, "profile-session.lock")?;
        }
        if !self.exclusive || self.profile != profile {
            return Err(err("PROFILE_LOCK_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (current.dev(), current.ino()) != (self.identity.dev(), self.identity.ino()) {
                return Err(err("PROFILE_LOCK_INVALID"));
            }
        }
        Ok(())
    }
}
/// Resolve a stable logical profile path without creating that profile. A
/// controller must acquire this guard before creating/opening a replacement.
pub(crate) fn anchor_lock(profile: &Path, exclusive: bool) -> Result<(PathBuf, ProfileAnchor)> {
    #[cfg(windows)]
    {
        let parent = super::windows_private::ParentDirectory::inspect(
            profile
                .parent()
                .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
        )?;
        let profile_name = profile
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
            .to_owned();
        let target = parent.path().join(&profile_name);
        let lock_name = format!(
            ".exhibitos-profile-session-{}.lock",
            parent.logical_child_key(&profile_name)?
        );
        let file = parent.lock_record(&lock_name)?;
        if exclusive {
            file.try_lock_exclusive()
        } else {
            FileExt::try_lock_shared(&file)
        }
        .map_err(|_| err("PROFILE_BUSY"))?;
        parent.check_record(&file, &lock_name)?;
        Ok((
            target,
            ProfileAnchor {
                native: std::sync::Arc::new(WindowsAnchor {
                    file,
                    parent,
                    profile_name,
                    lock_name,
                    exclusive,
                }),
            },
        ))
    }
    #[cfg(not(windows))]
    {
        let parent = fs::canonicalize(
            profile
                .parent()
                .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
        )
        .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let name = profile
            .file_name()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let target = parent.join(name);
        let text = target.to_str().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let metadata = fs::symlink_metadata(&parent).map_err(|_| err("PROFILE_PATH_INVALID"))?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // Private/owner-writable parent, or sticky shared temp parent. Sticky
            // protects this uid-owned anchor against other users' unlink/rename.
            let private_parent =
                metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0;
            let sticky_parent = metadata.mode() & 0o1000 != 0
                && (metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() });
            if !private_parent && !sticky_parent {
                return Err(err("PROFILE_PATH_INVALID"));
            }
        }
        if !cfg!(unix) {
            return Err(err("PROFILE_PLATFORM_UNVERIFIED"));
        }
        let filename = format!(
            ".exhibitos-profile-session-{}.lock",
            digest(text.as_bytes())
        );
        let anchor = lock_file(&parent, &filename, exclusive)?;
        // lock_file checks file identity, private ownership/mode and no hardlinks.
        // Recheck the parent mapping after obtaining the guard; unsupported external
        // rename/edit of the parent is never silently treated as the same profile.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current_parent =
                fs::symlink_metadata(&parent).map_err(|_| err("PROFILE_PATH_INVALID"))?;
            if (metadata.dev(), metadata.ino()) != (current_parent.dev(), current_parent.ino())
                || current_parent.is_symlink()
            {
                return Err(err("PROFILE_PATH_INVALID"));
            }
        }
        if fs::canonicalize(profile.parent().unwrap()).ok().as_ref() != Some(&parent) {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        Ok((target, ProfileAnchor { file: anchor }))
    }
}
pub(crate) fn anchored_session(
    profile: &Path,
    anchor: ProfileAnchor,
    exclusive: bool,
) -> Result<ProfileSession> {
    canonical_private(profile)?;
    #[cfg(windows)]
    let root_guard = super::windows_private::PrivateDirectory::inspect(profile)?;
    #[cfg(windows)]
    anchor.check(profile, exclusive)?;
    Ok(ProfileSession {
        profile: profile.into(),
        #[cfg(windows)]
        root_guard,
        #[cfg(not(windows))]
        identity: fs::symlink_metadata(profile).map_err(|_| err("PROFILE_PATH_INVALID"))?,
        exclusive,
        _anchor: anchor,
        _legacy: lock_file(profile, "profile-session.lock", exclusive)?,
    })
}
pub(crate) fn session_lock(profile: &Path, exclusive: bool) -> Result<ProfileSession> {
    let (target, anchor) = anchor_lock(profile, exclusive)?;
    if target != profile {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    anchored_session(profile, anchor, exclusive)
}
fn relative(id: &str, kind: &str) -> String {
    if kind == "default" {
        "local-runtime".into()
    } else {
        format!("installations/{id}")
    }
}
fn space_available(profile: &Path, path: &str) -> Result<bool> {
    if path.starts_with("installations/") {
        match fs::symlink_metadata(profile.join("installations")) {
            Ok(_) => installations::private_directory(&profile.join("installations"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(err("PROFILE_PATH_INVALID")),
        }
    }
    match fs::symlink_metadata(profile.join(path)) {
        Ok(_) => {
            installations::private_directory(&profile.join(path))?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(err("PROFILE_PATH_INVALID")),
    }
}
fn registry(bytes: &[u8]) -> Result<Registry> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(err("PROFILE_FORMAT_INVALID"));
    }
    let r: Registry = serde_json::from_slice(bytes).map_err(|_| err("PROFILE_FORMAT_INVALID"))?;
    installations::valid(&r).map_err(|_| err("PROFILE_FORMAT_INVALID"))?;
    Ok(r)
}
fn history_valid(name: &str, bytes: &[u8]) -> Result<()> {
    let valid_name = (|| -> Option<()> {
        let stem = name.strip_suffix(".json")?;
        let (prefix, h) = stem.rsplit_once('-')?;
        let (time, id) = prefix.split_once('-')?;
        if time.parse::<u64>().ok()? > 8_640_000_000_000_000
            || !installations::uuid(id)
            || !hash_valid(h)
            || digest(bytes) != h
        {
            return None;
        }
        Some(())
    })();
    if valid_name.is_none() {
        return Err(err("PROFILE_HISTORY_INVALID"));
    }
    registry(bytes)?;
    Ok(())
}
fn validate(s: &Snapshot) -> Result<Registry> {
    if s.format != 1
        || !installations::uuid(&s.id)
        || s.at > 8_640_000_000_000_000
        || s.history.len() > MAX_HISTORY
    {
        return Err(err("PROFILE_FORMAT_INVALID"));
    }
    let r = registry(&s.registry)?;
    if s.spaces.len() != r.installations.len() {
        return Err(err("PROFILE_FORMAT_INVALID"));
    }
    for (space, entry) in s.spaces.iter().zip(&r.installations) {
        if space.id != entry.id
            || space.kind != entry.kind
            || space.relative != relative(&entry.id, &entry.kind)
        {
            return Err(err("PROFILE_FORMAT_INVALID"));
        }
    }
    for (name, bytes) in &s.history {
        history_valid(name, bytes)?;
    }
    Ok(r)
}
fn capture(profile: &Path) -> Result<Snapshot> {
    let bytes = source_bytes(profile, "installation-selection.json", 64 * 1024, true)?;
    let r = registry(&bytes)?;
    let mut history = BTreeMap::new();
    let dir = profile.join("selection-history");
    match fs::symlink_metadata(&dir) {
        Ok(_) => {
            installations::private_directory(&dir)?;
            for file in fs::read_dir(&dir).map_err(|_| err("STATE_UNAVAILABLE"))? {
                if history.len() >= MAX_HISTORY {
                    return Err(err("PROFILE_QUOTA"));
                }
                let name = file
                    .map_err(|_| err("STATE_UNAVAILABLE"))?
                    .file_name()
                    .into_string()
                    .map_err(|_| err("PROFILE_HISTORY_INVALID"))?;
                let value = source_bytes(&dir, &name, 64 * 1024, true)?;
                history_valid(&name, &value)?;
                history.insert(name, value);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(err("STATE_UNAVAILABLE")),
    }
    let mut spaces = Vec::new();
    for entry in &r.installations {
        let path = relative(&entry.id, &entry.kind);
        let available = space_available(profile, &path)?;
        spaces.push(Space {
            id: entry.id.clone(),
            kind: entry.kind.clone(),
            relative: path,
            available,
        });
    }
    Ok(Snapshot {
        format: 1,
        id: Uuid::new_v4().to_string(),
        at: now(),
        registry: bytes,
        history,
        spaces,
    })
}
fn root_locks(profile: &Path, spaces: &[Space]) -> Result<Vec<File>> {
    let mut guards = Vec::new();
    let mut paths: Vec<_> = spaces.iter().map(|s| s.relative.clone()).collect();
    paths.sort();
    paths.dedup();
    for p in paths {
        if space_available(profile, &p)? {
            guards.push(lock_file(&profile.join(p), "operation.lock", true)?);
        }
    }
    Ok(guards)
}
fn external_key(profile: &Path, key: &Path) -> Result<Vec<u8>> {
    let parent = key.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
    canonical_private(parent)?;
    if key.starts_with(profile) || fs::canonicalize(key).ok().as_deref() != Some(key) {
        return Err(err("PROFILE_KEY_INVALID"));
    }
    let bytes = source_bytes(
        parent,
        key.file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_KEY_INVALID"))?,
        32,
        true,
    )?;
    if bytes.len() != 32 {
        return Err(err("PROFILE_KEY_INVALID"));
    }
    Ok(bytes)
}
fn archive_parent(profile: &Path, archive: &Path, key: &Path) -> Result<PathBuf> {
    let parent = canonical_private(
        archive
            .parent()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
    )?;
    if archive.starts_with(profile)
        || archive == key
        || archive.file_name().and_then(|v| v.to_str()).is_none()
    {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    Ok(parent)
}
fn sync(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = private_options().open(path).map_err(|e| {
        err(if e.kind() == std::io::ErrorKind::AlreadyExists {
            "PROFILE_DESTINATION_EXISTS"
        } else {
            "PROFILE_DESTINATION_UNAVAILABLE"
        })
    })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))?;
    sync(path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?)
}
fn publish(temporary: &Path, archive: &Path) -> Result<()> {
    fs::hard_link(temporary, archive).map_err(|e| {
        err(if e.kind() == std::io::ErrorKind::AlreadyExists {
            "PROFILE_DESTINATION_EXISTS"
        } else {
            // The encrypted pending already exists; retain it and never imply completion.
            "PROFILE_WRITE_UNCERTAIN"
        })
    })
}
fn acknowledgement(ack: bool) -> Result<()> {
    if !cfg!(unix) {
        return Err(err("PROFILE_PLATFORM_UNVERIFIED"));
    }
    if !ack {
        return Err(err("PROFILE_ACK_REQUIRED"));
    }
    Ok(())
}
pub fn backup(profile: &Path, key: &Path, archive: &Path, ack: bool) -> Result<ProfileReceipt> {
    backup_mode(profile, key, archive, ack, false)
}
/// Opt-in authenticated stream envelope. Profile payload/quota and lock scope
/// are unchanged; this does not yet capture maintenance journals or candidates.
pub fn backup_stream(
    profile: &Path,
    key: &Path,
    archive: &Path,
    ack: bool,
) -> Result<ProfileReceipt> {
    backup_mode(profile, key, archive, ack, true)
}
fn backup_mode(
    profile: &Path,
    key: &Path,
    archive: &Path,
    ack: bool,
    stream: bool,
) -> Result<ProfileReceipt> {
    acknowledgement(ack)?;
    canonical_private(profile)?;
    // Reject an empty/wrong source before creating any profile lock files.
    if installations::load(profile)?.is_none() {
        return Err(err("PROFILE_FORMAT_INVALID"));
    }

    let parent = archive_parent(profile, archive, key)?;
    let key = external_key(profile, key)?;
    // Caller never gets a partially written destination: a retained private temporary is published by no-replace link.
    match fs::symlink_metadata(archive) {
        Ok(_) => return Err(err("PROFILE_DESTINATION_EXISTS")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(err("PROFILE_DESTINATION_UNAVAILABLE")),
    }
    let _session = session_lock(profile, true)?;
    let _profile = lock_file(profile, "operation.lock", true)?;
    let s = capture(profile)?;
    let _roots = root_locks(profile, &s.spaces)?;
    let second = capture(profile)?;
    if s.registry != second.registry
        || s.history != second.history
        || serde_json::to_vec(&s.spaces).ok() != serde_json::to_vec(&second.spaces).ok()
    {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    let plain = serde_json::to_vec(&s).map_err(|_| err("PROFILE_FORMAT_INVALID"))?;
    if plain.len() as u64 > LIMIT - 128 {
        return Err(err("PROFILE_QUOTA"));
    }
    let bytes = if stream {
        let mut bytes = Vec::new();
        super::maintenance_stream::seal(
            &mut plain.as_slice(),
            &mut bytes,
            &key,
            MAGIC,
            LIMIT - 128,
        )?;
        bytes
    } else {
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| err("PROFILE_KEY_INVALID"))?;
        let mut nonce = [0u8; 12];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| err("PROFILE_RANDOM_UNAVAILABLE"))?;
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: MAGIC,
                },
            )
            .map_err(|_| err("PROFILE_ENCRYPTION_FAILED"))?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(nonce);
        bytes.extend(encrypted);
        bytes
    };
    if bytes.len() as u64 > LIMIT {
        return Err(err("PROFILE_QUOTA"));
    }
    let temporary = parent.join(format!(".profile-backup-{}.pending", s.id));
    write_new(&temporary, &bytes)?;
    // Verify authentication and closed schema before publishing; failed encrypted pending files remain for inspection.
    let opened = decrypt(&bytes, &key)?;
    validate(&opened)?;
    publish(&temporary, archive)?;
    fs::remove_file(&temporary).map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))?;
    sync(&parent)?;
    Ok(ProfileReceipt {
        id: s.id,
        operation: "profile-backup".into(),
        spaces: s.spaces.len(),
        history: s.history.len(),
        archive_sha256: digest(&bytes),
        at: now(),
    })
}
fn decrypt(bytes: &[u8], key: &[u8]) -> Result<Snapshot> {
    if bytes.starts_with(super::maintenance_stream::MAGIC) {
        if bytes.len() as u64 > LIMIT {
            return Err(err("PROFILE_AUTHENTICATION_FAILED"));
        }
        let mut plain = Vec::new();
        super::maintenance_stream::open(&mut &bytes[..], &mut plain, key, MAGIC, LIMIT - 128)
            .map_err(|_| err("PROFILE_AUTHENTICATION_FAILED"))?;
        return serde_json::from_slice(&plain).map_err(|_| err("PROFILE_FORMAT_INVALID"));
    }
    if bytes.len() < MAGIC.len() + 12 + 16
        || bytes.len() as u64 > LIMIT
        || !bytes.starts_with(MAGIC)
    {
        return Err(err("PROFILE_AUTHENTICATION_FAILED"));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| err("PROFILE_KEY_INVALID"))?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&bytes[MAGIC.len()..MAGIC.len() + 12]),
            Payload {
                msg: &bytes[MAGIC.len() + 12..],
                aad: MAGIC,
            },
        )
        .map_err(|_| err("PROFILE_AUTHENTICATION_FAILED"))?;
    serde_json::from_slice(&plain).map_err(|_| err("PROFILE_FORMAT_INVALID"))
}
fn current_registry(profile: &Path) -> Result<Option<Vec<u8>>> {
    let path = profile.join("installation-selection.json");
    let before = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(err("PROFILE_PATH_INVALID")),
    };
    if before.len() != 0 {
        return source_bytes(profile, "installation-selection.json", 64 * 1024, true).map(Some);
    }
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut f = o.open(&path).map_err(|_| err("PROFILE_PATH_INVALID"))?;
    let m = f.metadata().map_err(|_| err("PROFILE_PATH_INVALID"))?;
    if !m.is_file() || m.len() != 0 || before.is_symlink() {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o7777 != 0o600
            || (m.dev(), m.ino()) != (before.dev(), before.ino())
        {
            return Err(err("PROFILE_PATH_INVALID"));
        }
    }
    let mut probe = [0u8; 1];
    if f.read(&mut probe)
        .map_err(|_| err("BACKUP_SOURCE_CHANGED"))?
        != 0
    {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    let after = fs::symlink_metadata(path).map_err(|_| err("BACKUP_SOURCE_CHANGED"))?;
    if after.len() != 0 || after.is_symlink() {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (
            m.dev(),
            m.ino(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.mode(),
            m.nlink(),
        ) != (
            after.dev(),
            after.ino(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
            after.mode(),
            after.nlink(),
        ) {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
    }
    Ok(Some(Vec::new()))
}
pub fn restore(profile: &Path, key: &Path, archive: &Path, ack: bool) -> Result<ProfileReceipt> {
    acknowledgement(ack)?;
    canonical_private(profile)?;
    let parent = archive_parent(profile, archive, key)?;
    let key = external_key(profile, key)?;
    let bytes = source_bytes(
        &parent,
        archive
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
        LIMIT,
        true,
    )?;
    let s = decrypt(&bytes, &key)?;
    validate(&s)?;
    let _session = session_lock(profile, true)?;
    let _profile = lock_file(profile, "operation.lock", true)?;
    let _roots = root_locks(profile, &s.spaces)?;
    let current = current_registry(profile)?;
    // Preserve the old pointer and lock current-only roots on an older-point restore.
    let _current_roots = if let Some(ref b) = current {
        if let Ok(r) = registry(b) {
            let spaces = r
                .installations
                .iter()
                .map(|e| Space {
                    id: e.id.clone(),
                    kind: e.kind.clone(),
                    relative: relative(&e.id, &e.kind),
                    available: false,
                })
                .filter(|v| !s.spaces.iter().any(|e| e.relative == v.relative))
                .collect::<Vec<_>>();
            root_locks(profile, &spaces)?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let history = profile.join("selection-history");
    match fs::symlink_metadata(&history) {
        Ok(_) => installations::private_directory(&history)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(err("PROFILE_PATH_INVALID")),
    }
    // Preflight every filename collision before preserving/writing any registry bytes.
    for (name, value) in &s.history {
        match fs::symlink_metadata(history.join(name)) {
            Ok(_) => {
                if source_bytes(&history, name, 64 * 1024, true)? != *value {
                    return Err(err("PROFILE_HISTORY_CONFLICT"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(err("PROFILE_HISTORY_CONFLICT")),
        }
    }
    let retained = profile.join(format!("profile-restore-{}", Uuid::new_v4()));
    installations::new_directory(&retained)?;
    if let Some(ref b) = current {
        write_new(&retained.join("previous-registry.bin"), b)?;
    }
    write_new(&retained.join("restored-registry.bin"), &s.registry)?;
    if !history.exists() {
        installations::new_directory(&history)?;
    }
    // Valid pre-restore pointers also enter typed history, so the next encrypted
    // profile checkpoint preserves later-only registrations. Corrupt bytes stay diagnostic.
    if let Some(ref previous) = current
        && previous != &s.registry
        && registry(previous).is_ok()
    {
        let name = format!("{}-{}-{}.json", now(), Uuid::new_v4(), digest(previous));
        write_new(&history.join(name), previous)?;
    }
    for (name, value) in &s.history {
        if !history.join(name).exists() {
            write_new(&history.join(name), value)?;
        }
    }
    let staging = profile.join(format!(".profile-registry-{}.pending", Uuid::new_v4()));
    write_new(&staging, &s.registry)?;
    // Refuse an external writer changing the original pointer while guards are held.
    let latest = current_registry(profile)?;
    if latest != current {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    fs::rename(staging, profile.join("installation-selection.json"))
        .map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))?;
    sync(profile)?;
    let receipt = ProfileReceipt {
        id: s.id,
        operation: "profile-restore".into(),
        spaces: s.spaces.len(),
        history: s.history.len(),
        archive_sha256: digest(&bytes),
        at: now(),
    };
    write_json(&retained, "receipt.json", &receipt).map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))?;
    sync(&retained)?;
    Ok(receipt)
}
#[cfg(all(test, unix))]
mod tests {
    use super::installations::InstallationController;
    use super::*;
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;
    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("profile-proof-{}", Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let p = parent.join("profile");
        let c = InstallationController::new(p.clone(), None).unwrap();
        let cx = c.context().unwrap();
        c.create(&cx.selection_token, true).unwrap();
        drop(c);
        let k = parent.join("key.bin");
        write_new(&k, &[7u8; 32]).unwrap();
        let a = parent.join("profile.exb");
        (p, k, a)
    }
    #[test]
    fn unsafe_existing_profile_does_not_create_sibling_anchor() {
        let (p, _, _) = fixture();
        let unsafe_profile = p.with_file_name("unsafe-profile");
        installations::new_directory(&unsafe_profile).unwrap();
        fs::set_permissions(&unsafe_profile, fs::Permissions::from_mode(0o755)).unwrap();
        let names = || {
            fs::read_dir(p.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<BTreeSet<_>>()
        };
        let before = names();
        assert_eq!(
            InstallationController::new(unsafe_profile.clone(), None)
                .err()
                .unwrap()
                .code,
            "INSTALLATION_ROOT_UNAVAILABLE"
        );
        assert_eq!(names(), before);
        assert_eq!(fs::read_dir(&unsafe_profile).unwrap().count(), 0);
    }
    #[test]
    fn offline_anchor_survives_root_move_and_blocks_app_before_profile_creation() {
        let (p, _, _) = fixture();
        let bytes = fs::read(p.join("installation-selection.json")).unwrap();
        let held = session_lock(&p, true).unwrap();
        let retained = p.with_file_name("retained-anchor-source");
        fs::rename(&p, &retained).unwrap();
        assert_eq!(
            InstallationController::new(p.clone(), None)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        assert!(!p.exists());
        installations::new_directory(&p).unwrap();
        assert_eq!(
            InstallationController::new(p.clone(), None)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        assert_eq!(fs::read_dir(&p).unwrap().count(), 0);
        assert_eq!(
            fs::read(retained.join("installation-selection.json")).unwrap(),
            bytes
        );
        drop(held);
        fs::rename(&p, p.with_file_name("retained-empty-replacement")).unwrap();
        fs::rename(&retained, &p).unwrap();
        assert!(InstallationController::new(p, None).is_ok());
    }
    #[test]
    fn legacy_lock_and_canonical_parent_alias_keep_both_fences() {
        let (p, k, a) = fixture();
        let legacy = lock_file(&p, "profile-session.lock", true).unwrap();
        assert_eq!(
            backup_stream(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_BUSY"
        );
        assert_eq!(
            InstallationController::new(p.clone(), None)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        drop(legacy);
        let held = session_lock(&p, true).unwrap();
        let alias = p
            .parent()
            .unwrap()
            .with_file_name(format!("profile-parent-alias-{}", Uuid::new_v4()));
        std::os::unix::fs::symlink(p.parent().unwrap(), &alias).unwrap();
        assert_eq!(
            InstallationController::new(alias.join("profile"), None)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        drop(held);
        assert!(InstallationController::new(p, None).is_ok());
    }
    #[test]
    fn anchor_symlink_hardlink_permissions_and_unsafe_parent_refuse_without_pointer_changes() {
        let (p, k, a) = fixture();
        let pointer = fs::read(p.join("installation-selection.json")).unwrap();
        let name = format!(
            ".exhibitos-profile-session-{}.lock",
            digest(p.to_string_lossy().as_bytes())
        );
        let anchor = p.parent().unwrap().join(name);
        fs::set_permissions(&anchor, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            backup(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_LOCK_INVALID"
        );
        fs::set_permissions(&anchor, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&anchor, anchor.with_extension("retained")).unwrap();
        std::os::unix::fs::symlink(anchor.with_extension("retained"), &anchor).unwrap();
        assert_eq!(
            backup(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_LOCK_INVALID"
        );
        fs::rename(&anchor, anchor.with_extension("retained-symlink")).unwrap();
        fs::hard_link(anchor.with_extension("retained"), &anchor).unwrap();
        assert_eq!(
            backup(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_LOCK_INVALID"
        );
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            pointer
        );
        assert!(!a.exists());
        let (other, _, _) = fixture();
        let parent = other.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            anchor_lock(&other, true).err().unwrap().code,
            "PROFILE_PATH_INVALID"
        );
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn opt_in_stream_profile_restores_same_registry_and_rejects_truncated_prefix() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let receipt = backup_stream(&p, &k, &a, true).unwrap();
        let bytes = fs::read(&a).unwrap();
        assert!(bytes.starts_with(super::super::maintenance_stream::MAGIC));
        assert_eq!(restore(&p, &k, &a, true).unwrap().id, receipt.id);
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
        let bad = a.with_file_name("truncated-stream.exb");
        write_new(&bad, &bytes[..bytes.len() - 1]).unwrap();
        assert_eq!(
            restore(&p, &k, &bad, true).unwrap_err().code,
            "PROFILE_AUTHENTICATION_FAILED"
        );
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
        assert_eq!(fs::read(&a).unwrap(), bytes);
    }
    #[test]
    fn destination_io_failures_are_not_collisions_and_never_remove_pending() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let absent_parent = k.with_file_name("absent-parent").join("archive.exb");
        assert_eq!(
            write_new(&absent_parent, b"private").unwrap_err().code,
            "PROFILE_DESTINATION_UNAVAILABLE"
        );
        let pending = k.with_file_name("retained.pending");
        write_new(&pending, b"ciphertext witness").unwrap();
        assert_eq!(
            publish(&pending, &absent_parent).unwrap_err().code,
            "PROFILE_WRITE_UNCERTAIN"
        );
        assert_eq!(fs::read(&pending).unwrap(), b"ciphertext witness");
        write_new(&a, b"existing archive").unwrap();
        assert_eq!(
            publish(&pending, &a).unwrap_err().code,
            "PROFILE_DESTINATION_EXISTS"
        );
        assert_eq!(
            write_new(&a, b"replacement").unwrap_err().code,
            "PROFILE_DESTINATION_EXISTS"
        );
        assert_eq!(fs::read(&a).unwrap(), b"existing archive");
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
    }
    #[test]
    fn inaccessible_destination_refuses_before_profile_locks_or_pending_files() {
        let (p, k, _) = fixture();
        let profile_before: BTreeSet<_> = fs::read_dir(&p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let too_long = k.with_file_name("x".repeat(300));
        let before: BTreeSet<_> = fs::read_dir(k.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            backup(&p, &k, &too_long, true).unwrap_err().code,
            "PROFILE_DESTINATION_UNAVAILABLE"
        );
        let after: BTreeSet<_> = fs::read_dir(k.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
        let profile_after: BTreeSet<_> = fs::read_dir(&p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(profile_before, profile_after);
    }
    #[test]
    fn live_windows_and_offline_lock_exclude_each_other() {
        let (p, k, a) = fixture();
        let c1 = InstallationController::new(p.clone(), None).unwrap();
        let c2 = InstallationController::new(p.clone(), None).unwrap();
        assert_eq!(backup(&p, &k, &a, true).unwrap_err().code, "PROFILE_BUSY");
        drop(c1);
        assert_eq!(backup(&p, &k, &a, true).unwrap_err().code, "PROFILE_BUSY");
        drop(c2);
        let _offline = session_lock(&p, true).unwrap();
        assert!(InstallationController::new(p, None).is_err());
        assert!(!a.exists());
    }
    #[test]
    fn authenticated_old_pointer_restore_preserves_every_space_and_old_pointer() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let original = registry(&before).unwrap();
        for e in &original.installations {
            write_new(&installations::root(&p, e).join("witness"), e.id.as_bytes()).unwrap();
        }
        let saved = backup(&p, &k, &a, true).unwrap();
        let encrypted = fs::read(&a).unwrap();
        assert!(!encrypted.windows(before.len()).any(|v| v == before));
        let c = InstallationController::new(p.clone(), None).unwrap();
        let cx = c.context().unwrap();
        let later = c.create(&cx.selection_token, true).unwrap();
        drop(c);
        let later_bytes = fs::read(p.join("installation-selection.json")).unwrap();
        let r = restore(&p, &k, &a, true).unwrap();
        assert_eq!(saved.id, r.id);
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
        assert!(p.join("installations").join(later.active_id).exists());
        for e in &original.installations {
            assert_eq!(
                fs::read(installations::root(&p, e).join("witness")).unwrap(),
                e.id.as_bytes()
            );
        }
        assert!(fs::read_dir(&p).unwrap().filter_map(|v| v.ok()).any(|v| {
            v.file_name()
                .to_string_lossy()
                .starts_with("profile-restore-")
                && fs::read(v.path().join("previous-registry.bin"))
                    .ok()
                    .as_deref()
                    == Some(later_bytes.as_slice())
        }));
        let next = a.with_file_name("next-profile-checkpoint.exb");
        backup(&p, &k, &next, true).unwrap();
        let next_snapshot = decrypt(&fs::read(&next).unwrap(), &[7u8; 32]).unwrap();
        assert!(next_snapshot.history.values().any(|v| v == &later_bytes));
        assert_eq!(fs::read(&a).unwrap(), encrypted);
        assert_eq!(
            InstallationController::new(p, None)
                .unwrap()
                .context()
                .unwrap()
                .active_id,
            original.active_id
        );
    }
    #[test]
    fn wrong_key_tamper_truncation_ack_and_overwrite_preserve_registry() {
        let (p, k, a) = fixture();
        assert_eq!(
            backup(&p, &k, &a, false).unwrap_err().code,
            "PROFILE_ACK_REQUIRED"
        );
        backup(&p, &k, &a, true).unwrap();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let bytes = fs::read(&a).unwrap();
        assert_eq!(
            backup(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_DESTINATION_EXISTS"
        );
        let other = k.with_file_name("wrong-key.bin");
        write_new(&other, &[9u8; 32]).unwrap();
        assert_eq!(
            restore(&p, &other, &a, true).unwrap_err().code,
            "PROFILE_AUTHENTICATION_FAILED"
        );
        for mut bad in [bytes[..bytes.len() - 1].to_vec(), bytes.clone()] {
            let last = bad.len() - 1;
            bad[last] ^= 1;
            let path = k.with_file_name(format!("bad-{}.exb", Uuid::new_v4()));
            write_new(&path, &bad).unwrap();
            assert_eq!(
                restore(&p, &k, &path, true).unwrap_err().code,
                "PROFILE_AUTHENTICATION_FAILED"
            );
            assert_eq!(
                fs::read(p.join("installation-selection.json")).unwrap(),
                before
            );
        }
        assert_eq!(fs::read(&a).unwrap(), bytes);
    }
    #[test]
    fn original_profile_unavailable_new_target_keeps_missing_roots_missing() {
        let (p, k, a) = fixture();
        let original = fs::read(p.join("installation-selection.json")).unwrap();
        backup(&p, &k, &a, true).unwrap();
        fs::rename(&p, p.with_file_name("retained-original-profile")).unwrap();
        let target = p.with_file_name("new-profile");
        installations::new_directory(&target).unwrap();
        restore(&target, &k, &a, true).unwrap();
        assert_eq!(
            fs::read(target.join("installation-selection.json")).unwrap(),
            original
        );
        let c = InstallationController::new(target.clone(), None).unwrap();
        assert!(c.context().unwrap().error_code.is_some());
        assert!(!target.join("local-runtime").exists());
        assert!(!target.join("installations").exists());
    }
    #[test]
    fn corrupt_current_registry_is_retained_before_authenticated_replacement() {
        let (p, k, a) = fixture();
        let original = fs::read(p.join("installation-selection.json")).unwrap();
        backup(&p, &k, &a, true).unwrap();
        fs::write(
            p.join("installation-selection.json"),
            b"synthetic corruption",
        )
        .unwrap();
        restore(&p, &k, &a, true).unwrap();
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            original
        );
        fs::write(p.join("installation-selection.json"), b"").unwrap();
        restore(&p, &k, &a, true).unwrap();
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            original
        );
        assert!(fs::read_dir(p).unwrap().filter_map(|v| v.ok()).any(|v| {
            v.file_name()
                .to_string_lossy()
                .starts_with("profile-restore-")
                && fs::read(v.path().join("previous-registry.bin"))
                    .ok()
                    .as_deref()
                    == Some(b"synthetic corruption")
        }));
    }
    #[test]
    fn unsafe_key_archive_root_lock_and_history_fail_closed() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let alias = k.with_file_name("alias-key");
        std::os::unix::fs::symlink(&k, &alias).unwrap();
        assert!(backup(&p, &alias, &a, true).is_err());
        fs::set_permissions(&k, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(backup(&p, &k, &a, true).is_err());
        fs::set_permissions(&k, fs::Permissions::from_mode(0o600)).unwrap();
        let lock = p.join("local-runtime/operation.lock");
        let _held = lock_file(&p.join("local-runtime"), "operation.lock", true).unwrap();
        assert_eq!(backup(&p, &k, &a, true).unwrap_err().code, "PROFILE_BUSY");
        drop(_held);
        fs::hard_link(&lock, p.join("lock-alias")).unwrap();
        assert_eq!(
            backup(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_LOCK_INVALID"
        );
        assert!(!a.exists());
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
    }
    #[test]
    fn corrupted_history_and_authenticated_unknown_payload_cannot_replace_pointer() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let hist = fs::read_dir(p.join("selection-history"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(hist, b"{}").unwrap();
        assert!(backup(&p, &k, &a, true).is_err());
        assert!(!a.exists());
        let nonce = [4u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&[7u8; 32]).unwrap();
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: b"{\"format\":2}",
                    aad: MAGIC,
                },
            )
            .unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend(nonce);
        bytes.extend(encrypted);
        write_new(&a, &bytes).unwrap();
        assert_eq!(
            restore(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_FORMAT_INVALID"
        );
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
    }
    #[test]
    fn authenticated_duplicate_history_inventory_is_rejected_before_restore() {
        let (p, k, a) = fixture();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let snapshot = capture(&p).unwrap();
        let (name, bytes) = snapshot.history.first_key_value().unwrap();
        let item = format!(
            "{}:{}",
            serde_json::to_string(name).unwrap(),
            serde_json::to_string(bytes).unwrap()
        );
        let old = serde_json::to_string(&snapshot.history).unwrap();
        let bad = format!("{{{item},{item}}}");
        let plain = serde_json::to_string(&snapshot)
            .unwrap()
            .replace(&old, &bad);
        let nonce = [3u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&[7u8; 32]).unwrap();
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain.as_bytes(),
                    aad: MAGIC,
                },
            )
            .unwrap();
        let mut archive = MAGIC.to_vec();
        archive.extend(nonce);
        archive.extend(encrypted);
        write_new(&a, &archive).unwrap();
        assert_eq!(
            restore(&p, &k, &a, true).unwrap_err().code,
            "PROFILE_FORMAT_INVALID"
        );
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
    }
    #[test]
    fn invalid_backup_source_never_occupies_an_empty_fresh_runtime_root() {
        let (p, k, a) = fixture();
        let empty = p.with_file_name("empty-runtime-root");
        installations::new_directory(&empty).unwrap();
        assert_eq!(
            backup(&empty, &k, &a, true).unwrap_err().code,
            "PROFILE_FORMAT_INVALID"
        );
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
        assert!(!a.exists());
        assert!(
            LifecycleService::new(empty)
                .unwrap()
                .restoration_context()
                .unwrap()
                .fresh
        );
    }
}
