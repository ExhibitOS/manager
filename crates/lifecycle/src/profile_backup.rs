// SPDX-License-Identifier: Apache-2.0
//! App-closed, authenticated profile metadata recovery. Runtime data is never copied or erased.
use super::installation_backup::source_bytes;
use super::installations::{self, Registry};
use super::*;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
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
pub(crate) fn session_lock(profile: &Path, exclusive: bool) -> Result<File> {
    lock_file(profile, "profile-session.lock", exclusive)
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
    let mut file = private_options()
        .open(path)
        .map_err(|_| err("PROFILE_DESTINATION_EXISTS"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| err("PROFILE_WRITE_UNCERTAIN"))?;
    sync(path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?)
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
    acknowledgement(ack)?;
    canonical_private(profile)?;
    // Reject an empty/wrong source before creating any profile lock files.
    if installations::load(profile)?.is_none() {
        return Err(err("PROFILE_FORMAT_INVALID"));
    }

    let parent = archive_parent(profile, archive, key)?;
    let key = external_key(profile, key)?;
    // Caller never gets a partially written destination: a retained private temporary is published by no-replace link.
    if fs::symlink_metadata(archive).is_ok() {
        return Err(err("PROFILE_DESTINATION_EXISTS"));
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
    let temporary = parent.join(format!(".profile-backup-{}.pending", s.id));
    write_new(&temporary, &bytes)?;
    // Verify authentication and closed schema before publishing; failed encrypted pending files remain for inspection.
    let opened = decrypt(&bytes, &key)?;
    validate(&opened)?;
    fs::hard_link(&temporary, archive).map_err(|_| err("PROFILE_DESTINATION_EXISTS"))?;
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
