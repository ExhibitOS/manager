// SPDX-License-Identifier: Apache-2.0
//! App-closed local trust journal, outside replaceable profile data. It does not
//! resist the trusted OS account deleting/rolling back this entire journal.
use super::*;
use crate::{installations, profile_backup};
use fs2::FileExt;
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_RECORD: u64 = 24 * 1024;
const MAX_RECORDS: usize = 4096;
const MAX_REVOKED: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Accepted {
    sequence: u64,
    issued_at: u64,
    key_id: String,
    payload_sha256: String,
    artifact_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    format: u8,
    scope: String,
    generation: u64,
    previous_sha256: String,
    policy_generation: u64,
    observed_at: u64,
    policy: Policy,
    revoked_keys: Vec<String>,
    acceptance: Option<Accepted>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustReceipt {
    pub generation: u64,
    pub policy_generation: u64,
    pub minimum_sequence: u64,
    pub minimum_issued_at: u64,
    pub observed_at: u64,
    pub activated: bool,
    pub write_uncertain: bool,
}
/// Holds the stable profile fence exclusively through verification/publication.
/// Open never bootstraps missing/corrupt trust. The caller selects default or a
/// registered installation UUID and must preserve that scope through recovery.
pub struct Store {
    root: PathBuf,
    scope: String,
    root_identity: Metadata,
    _anchor: File,
    _lock: File,
    current: Record,
    current_sha256: String,
    uncertain: bool,
}
fn invalid() -> Error {
    Error::TrustInvalid
}
fn identity(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (a.dev(), a.ino()) == (b.dev(), b.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
fn private_file(path: &Path, create: bool) -> Result<File, Error> {
    let mut o = OpenOptions::new();
    o.read(true).write(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    if create {
        o.create_new(true);
    }
    let f = o.open(path).map_err(|_| invalid())?;
    let m = f.metadata().map_err(|_| invalid())?;
    let p = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !m.is_file() || p.is_symlink() || !identity(&m, &p) {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 || m.mode() & 0o7777 != 0o600 {
            return Err(invalid());
        }
    }
    Ok(f)
}
fn read_record(path: &Path) -> Result<Vec<u8>, Error> {
    let mut f = private_file(path, false)?;
    let before = f.metadata().map_err(|_| invalid())?;
    if before.len() == 0 || before.len() > MAX_RECORD {
        return Err(invalid());
    }
    let mut b = Vec::new();
    Read::by_ref(&mut f)
        .take(MAX_RECORD + 1)
        .read_to_end(&mut b)
        .map_err(|_| invalid())?;
    let after = f.metadata().map_err(|_| invalid())?;
    let current = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if b.len() as u64 != before.len()
        || !identity(&before, &after)
        || !identity(&after, &current)
        || current.is_symlink()
    {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let state = |m: &Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
                m.uid(),
                m.mode(),
                m.nlink(),
            )
        };
        if state(&before) != state(&after) || state(&after) != state(&current) {
            return Err(invalid());
        }
    }
    Ok(b)
}
fn scope(profile: &Path, installation: &str) -> Result<(PathBuf, String, File), Error> {
    if !profile.is_absolute() {
        return Err(invalid());
    }
    if installation != "default" && !installations::uuid(installation) {
        return Err(invalid());
    }
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err(Error::TrustPlatformUnverified);
    }
    let (profile, anchor) = profile_backup::anchor_lock(profile, true).map_err(|e| {
        if e.code == "PROFILE_BUSY" {
            Error::TrustBusy
        } else {
            invalid()
        }
    })?;
    let path = profile.to_str().ok_or_else(invalid)?;
    let id = hash(format!("ExhibitOS-release-trust-v1\0{path}\0{installation}").as_bytes());
    let root = profile
        .parent()
        .ok_or_else(invalid)?
        .join(format!(".exhibitos-release-trust-{id}"));
    Ok((root, id, anchor))
}
fn keys(p: &Policy) -> Result<Vec<String>, Error> {
    Ok(p.keys()?.into_iter().map(|(id, _)| id).collect())
}
fn valid_record(r: &Record, scope: &str) -> Result<(), Error> {
    let ids = keys(&r.policy)?;
    if r.format != 1
        || r.scope != scope
        || r.generation == 0
        || r.policy_generation == 0
        || r.policy_generation > r.generation
        || hex::<32>(&r.previous_sha256).is_none()
        || r.revoked_keys.len() > MAX_REVOKED
        || r.revoked_keys
            .iter()
            .any(|k| hex::<32>(k).is_none() || ids.contains(k))
        || r.revoked_keys.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(invalid());
    }
    if let Some(a) = &r.acceptance
        && (a.sequence != r.policy.minimum_sequence
            || a.issued_at > r.policy.minimum_issued_at
            || a.issued_at > r.observed_at
            || !ids.contains(&a.key_id)
            || hex::<32>(&a.payload_sha256).is_none()
            || hex::<32>(&a.artifact_sha256).is_none())
    {
        return Err(invalid());
    }
    Ok(())
}
fn transition(old: &Record, next: &Record) -> Result<(), Error> {
    if next.generation != old.generation.checked_add(1).ok_or_else(invalid)?
        || next.observed_at < old.observed_at
        || next.policy.channel != old.policy.channel
        || next.policy.target != old.policy.target
        || next.policy.protocol_version != old.policy.protocol_version
        || next.policy.minimum_sequence < old.policy.minimum_sequence
        || next.policy.minimum_issued_at < old.policy.minimum_issued_at
        || old
            .revoked_keys
            .iter()
            .any(|id| !next.revoked_keys.contains(id))
    {
        return Err(Error::TrustRollback);
    }
    if next.policy_generation == old.policy_generation {
        let a = next.acceptance.as_ref().ok_or_else(invalid)?;
        let mut expected = old.policy.clone();
        expected.minimum_sequence = a.sequence;
        expected.minimum_issued_at = expected.minimum_issued_at.max(a.issued_at);
        if a.sequence <= old.policy.minimum_sequence
            || a.issued_at < old.policy.minimum_issued_at
            || next.revoked_keys != old.revoked_keys
            || serde_json::to_vec(&expected).map_err(|_| invalid())?
                != serde_json::to_vec(&next.policy).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
    } else {
        if next.policy_generation != old.policy_generation.checked_add(1).ok_or_else(invalid)?
            || next.acceptance.is_some()
        {
            return Err(invalid());
        }
        let mut revoked = old.revoked_keys.clone();
        let nextkeys = keys(&next.policy)?;
        for k in keys(&old.policy)? {
            if !nextkeys.contains(&k) {
                revoked.push(k);
            }
        }
        revoked.sort();
        revoked.dedup();
        if revoked != next.revoked_keys {
            return Err(invalid());
        }
    }
    Ok(())
}
fn lock(root: &Path) -> Result<File, Error> {
    installations::private_directory(root).map_err(|_| invalid())?;
    let p = root.join("trust.lock");
    let f = match private_file(&p, true) {
        Ok(f) => f,
        Err(_) => private_file(&p, false)?,
    };
    // Read-only descriptor supports exclusive flock on qualified Unix platforms.
    f.try_lock_exclusive().map_err(|_| Error::TrustBusy)?;
    Ok(f)
}
fn sync_dir(root: &Path) -> Result<(), Error> {
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::TrustWriteUncertain)
}
fn publish(from: &Path, to: &Path) -> Result<(), Error> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(|_| invalid())?;
        let b = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(|_| invalid())?;
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(Error::TrustWriteUncertain);
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (from, to);
        Err(Error::TrustPlatformUnverified)
    }
}
fn write(root: &Path, r: &Record) -> Result<String, Error> {
    let bytes = serde_json::to_vec(r).map_err(|_| invalid())?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err(Error::TrustLimit);
    }
    let p = root.join(format!("pending-{}.json", uuid::Uuid::new_v4()));
    let mut f = private_file(&p, true)?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| Error::TrustWriteUncertain)?;
    drop(f);
    publish(&p, &root.join(format!("{:020}.json", r.generation)))?;
    sync_dir(root)?;
    Ok(hash(&bytes))
}
impl Store {
    /// Explicit trusted administrator bootstrap. Missing stores never bootstrap
    /// on verify/open. Partial initialization remains quarantined for inspection.
    pub fn provision(
        profile: &Path,
        installation: &str,
        policy: Policy,
        now: u64,
    ) -> Result<Self, Error> {
        keys(&policy)?;
        let (root, scope, anchor) = scope(profile, installation)?;
        let mut d = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            d.mode(0o700);
        }
        d.create(&root).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::TrustExists
            } else {
                invalid()
            }
        })?;
        let lock = lock(&root)?;
        let r = Record {
            format: 1,
            scope: scope.clone(),
            generation: 1,
            previous_sha256: "0".repeat(64),
            policy_generation: 1,
            observed_at: now,
            policy,
            revoked_keys: vec![],
            acceptance: None,
        };
        valid_record(&r, &scope)?;
        let h = write(&root, &r)?;
        sync_dir(root.parent().ok_or_else(invalid)?)?;
        let root_identity = fs::symlink_metadata(&root).map_err(|_| invalid())?;
        Ok(Self {
            root,
            scope,
            root_identity,
            _anchor: anchor,
            _lock: lock,
            current: r,
            current_sha256: h,
            uncertain: false,
        })
    }
    pub fn open(profile: &Path, installation: &str) -> Result<Self, Error> {
        let (root, scope, anchor) = scope(profile, installation)?;
        if !root.exists() {
            return Err(Error::TrustMissing);
        }
        let lock = lock(&root)?;
        let root_identity = fs::symlink_metadata(&root).map_err(|_| invalid())?;
        let mut paths = Vec::new();
        let mut entries = 0;
        for entry in fs::read_dir(&root).map_err(|_| invalid())? {
            entries += 1;
            if entries > MAX_RECORDS + 65 {
                return Err(Error::TrustLimit);
            }
            let entry = entry.map_err(|_| invalid())?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(invalid)?;
            if name == "trust.lock" {
                continue;
            }
            if let Some(id) = name
                .strip_prefix("pending-")
                .and_then(|n| n.strip_suffix(".json"))
            {
                if !installations::uuid(id) {
                    return Err(invalid());
                }
                // Incomplete, private uncommitted writes do not advance floors.
                let _ = private_file(&entry.path(), false)?;
                continue;
            }
            if name.len() != 25
                || !name.ends_with(".json")
                || !name.as_bytes()[..20]
                    .iter()
                    .copied()
                    .all(|b| b.is_ascii_digit())
            {
                return Err(invalid());
            }
            paths.push(entry.path());
            if paths.len() > MAX_RECORDS {
                return Err(Error::TrustLimit);
            }
        }
        paths.sort();
        let mut previous = None;
        let mut h = "0".repeat(64);
        for (index, p) in paths.iter().enumerate() {
            if p.file_name().and_then(|s| s.to_str())
                != Some(format!("{:020}.json", index + 1).as_str())
            {
                return Err(invalid());
            }
            let b = read_record(p)?;
            let r: Record = serde_json::from_slice(&b).map_err(|_| invalid())?;
            valid_record(&r, &scope)?;
            if r.generation != index as u64 + 1 || r.previous_sha256 != h {
                return Err(invalid());
            }
            if let Some(old) = &previous {
                transition(old, &r)?;
            } else if r.policy_generation != 1
                || !r.revoked_keys.is_empty()
                || r.acceptance.is_some()
            {
                return Err(invalid());
            }
            h = hash(&b);
            previous = Some(r);
        }
        let current = previous.ok_or_else(invalid)?;
        let s = Self {
            root,
            scope,
            root_identity,
            _anchor: anchor,
            _lock: lock,
            current,
            current_sha256: h,
            uncertain: false,
        };
        s.check_root()?;
        Ok(s)
    }
    pub fn policy(&self) -> &Policy {
        &self.current.policy
    }
    pub fn receipt(&self) -> TrustReceipt {
        TrustReceipt {
            generation: self.current.generation,
            policy_generation: self.current.policy_generation,
            minimum_sequence: self.current.policy.minimum_sequence,
            minimum_issued_at: self.current.policy.minimum_issued_at,
            observed_at: self.current.observed_at,
            activated: false,
            write_uncertain: self.uncertain,
        }
    }
    fn check_root(&self) -> Result<(), Error> {
        if self.uncertain {
            return Err(Error::TrustWriteUncertain);
        }
        installations::private_directory(&self.root).map_err(|_| invalid())?;
        if !identity(
            &self.root_identity,
            &fs::symlink_metadata(&self.root).map_err(|_| invalid())?,
        ) {
            return Err(invalid());
        }
        Ok(())
    }
    fn commit(&mut self, mut next: Record, now: u64) -> Result<TrustReceipt, Error> {
        if now < self.current.observed_at {
            return Err(Error::TrustClockRollback);
        }
        if self.current.generation >= MAX_RECORDS as u64 {
            return Err(Error::TrustLimit);
        }
        self.check_root()?;
        next.generation = self.current.generation + 1;
        next.previous_sha256 = self.current_sha256.clone();
        next.observed_at = now;
        valid_record(&next, &self.scope)?;
        transition(&self.current, &next)?;
        self.uncertain = true;
        let h = write(&self.root, &next)?;
        installations::private_directory(&self.root).map_err(|_| Error::TrustWriteUncertain)?;
        if !identity(
            &self.root_identity,
            &fs::symlink_metadata(&self.root).map_err(|_| Error::TrustWriteUncertain)?,
        ) {
            return Err(Error::TrustWriteUncertain);
        }
        self.current = next;
        self.current_sha256 = h;
        self.uncertain = false;
        Ok(self.receipt())
    }
    /// Authorized OS administrator action, never an instruction from a feed.
    /// Channel/target/protocol and floors cannot decrease; removed keys remain revoked.
    pub fn replace_policy(
        &mut self,
        policy: Policy,
        expected_generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        if expected_generation != self.current.policy_generation {
            return Err(Error::TrustStalePolicy);
        }
        let newkeys = keys(&policy)?;
        let mut next = self.current.clone();
        next.acceptance = None;
        next.policy_generation = next.policy_generation.checked_add(1).ok_or_else(invalid)?;
        for k in keys(&next.policy)? {
            if !newkeys.contains(&k) {
                next.revoked_keys.push(k);
            }
        }
        next.revoked_keys.sort();
        next.revoked_keys.dedup();
        next.policy = policy;
        self.commit(next, now)
    }
    pub fn verify(&self, envelope: &[u8], now: u64) -> Result<VerifiedRelease, Error> {
        self.check_root()?;
        if now < self.current.observed_at {
            return Err(Error::TrustClockRollback);
        }
        super::verify(envelope, self.policy(), now)
    }
    /// Complete signature+artifact proof required; revalidate pinned policy/time
    /// and exact payload before durably consuming the generation. No activation.
    pub fn accept(
        &mut self,
        envelope: &[u8],
        verified: &VerifiedRelease,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        let fresh = self.verify(envelope, now)?;
        if !verified.artifact_verified
            || fresh.payload_sha256 != verified.payload_sha256
            || fresh.key_id != verified.key_id
            || fresh.source_schema_sha256 != verified.source_schema_sha256
        {
            return Err(Error::PlanMismatch);
        }
        let r = fresh.release();
        let mut next = self.current.clone();
        next.policy.minimum_sequence = r.sequence;
        next.policy.minimum_issued_at = next.policy.minimum_issued_at.max(r.issued_at);
        next.acceptance = Some(Accepted {
            sequence: r.sequence,
            issued_at: r.issued_at,
            key_id: fresh.key_id.clone(),
            payload_sha256: fresh.payload_sha256.clone(),
            artifact_sha256: r.artifact.sha256.clone(),
        });
        self.commit(next, now)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (PathBuf, SigningKey, Policy, Release) {
        let parent =
            std::env::temp_dir().join(format!("exhibitos-release-trust-{}", uuid::Uuid::new_v4()));
        let mut d = fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        d.mode(0o700);
        d.create(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let profile = parent.join("profile");
        d.create(&profile).unwrap();
        let key = SigningKey::from_bytes(&[31; 32]);
        let policy = Policy {
            format: 1,
            channel: "development".into(),
            target: "linux-arm64".into(),
            protocol_version: 1,
            source_schema_sha256: "a".repeat(64),
            minimum_sequence: 1,
            minimum_issued_at: 10,
            public_keys: vec![
                key.verifying_key()
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            ],
        };
        let release = Release {
            format: 1,
            product: "ExhibitOS/runtime".into(),
            channel: policy.channel.clone(),
            target: policy.target.clone(),
            version: "0.2.0-dev.1".into(),
            sequence: 2,
            issued_at: 10,
            expires_at: 100,
            protocol_version: 1,
            source_schemas: vec![policy.source_schema_sha256.clone()],
            artifact: Artifact {
                name: "runtime.tar".into(),
                bytes: 7,
                sha256: hash(b"fixture"),
                runtime_image_sha256: "b".repeat(64),
                schema_sha256: "c".repeat(64),
            },
        };
        (profile, key, policy, release)
    }
    fn seal(key: &SigningKey, r: &Release) -> Vec<u8> {
        let payload = serde_json::to_string(r).unwrap();
        serde_json::to_vec(&Envelope {
            format: 1,
            algorithm: "ed25519".into(),
            key_id: hash(key.verifying_key().as_bytes()),
            signature: key
                .sign(&message(&payload))
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            payload,
        })
        .unwrap()
    }
    #[test]
    fn durable_acceptance_reopens_and_refuses_replay_without_activation() {
        let (p, k, policy, r) = fixture();
        let e = seal(&k, &r);
        assert!(matches!(
            Store::open(&p, "default"),
            Err(Error::TrustMissing)
        ));
        let mut s = Store::provision(&p, "default", policy.clone(), 10).unwrap();
        assert!(matches!(Store::open(&p, "default"), Err(Error::TrustBusy)));
        let mut v = s.verify(&e, 20).unwrap();
        assert_eq!(s.accept(&e, &v, 20).unwrap_err(), Error::PlanMismatch);
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let a = s.accept(&e, &v, 20).unwrap();
        assert_eq!(a.generation, 2);
        assert_eq!(a.minimum_sequence, 2);
        assert!(!a.activated);
        drop(s);
        let s = Store::open(&p, "default").unwrap();
        assert_eq!(s.verify(&e, 21).unwrap_err(), Error::Replay);
        assert_eq!(s.verify(&e, 19).unwrap_err(), Error::TrustClockRollback);
        drop(s);
        assert!(matches!(
            Store::provision(&p, "default", policy, 21),
            Err(Error::TrustExists)
        ));
    }
    #[test]
    fn profile_replacement_and_absence_do_not_reset_security_floors() {
        let (p, k, policy, r) = fixture();
        let e = seal(&k, &r);
        let mut s = Store::provision(&p, "default", policy, 10).unwrap();
        let mut v = s.verify(&e, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        s.accept(&e, &v, 20).unwrap();
        let root = s.root.clone();
        drop(s);
        fs::rename(&p, p.with_file_name("retained-original-profile")).unwrap();
        let s = Store::open(&p, "default").unwrap();
        assert_eq!(s.verify(&e, 21).unwrap_err(), Error::Replay);
        drop(s);
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        let s = Store::open(&p, "default").unwrap();
        assert_eq!(s.root, root);
        assert_eq!(s.receipt().minimum_sequence, 2);
        assert_eq!(fs::read_dir(&p).unwrap().count(), 0);
    }
    #[test]
    fn admin_rotation_preserves_revocation_and_monotonic_policy() {
        let (p, k, policy, r) = fixture();
        let mut s = Store::provision(&p, "default", policy.clone(), 10).unwrap();
        let second = SigningKey::from_bytes(&[32; 32]);
        let mut rotated = policy.clone();
        rotated.public_keys = vec![
            second
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ];
        assert_eq!(
            s.replace_policy(rotated.clone(), 2, 20).unwrap_err(),
            Error::TrustStalePolicy
        );
        s.replace_policy(rotated.clone(), 1, 20).unwrap();
        assert_eq!(
            s.verify(&seal(&k, &r), 20).unwrap_err(),
            Error::UntrustedKey
        );
        assert_eq!(
            s.replace_policy(policy, 2, 21).unwrap_err(),
            Error::TrustInvalid
        );
        rotated.minimum_sequence = 0;
        assert_eq!(
            s.replace_policy(rotated.clone(), 2, 21).unwrap_err(),
            Error::TrustRollback
        );
        rotated.minimum_sequence = 1;
        rotated.channel = "stable".into();
        assert_eq!(
            s.replace_policy(rotated, 2, 21).unwrap_err(),
            Error::TrustRollback
        );
        drop(s);
        let s = Store::open(&p, "default").unwrap();
        assert_eq!(s.receipt().generation, 2);
        assert_eq!(s.receipt().policy_generation, 2);
    }
    #[test]
    fn failed_artifact_signature_and_late_expiry_do_not_consume_floor() {
        let (p, k, policy, r) = fixture();
        let e = seal(&k, &r);
        let mut s = Store::provision(&p, "default", policy, 10).unwrap();
        let mut v = s.verify(&e, 20).unwrap();
        assert_eq!(
            v.verify_artifact(&mut b"changed".as_slice()).unwrap_err(),
            Error::ArtifactMismatch
        );
        assert_eq!(s.accept(&e, &v, 20).unwrap_err(), Error::PlanMismatch);
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        assert_eq!(s.accept(&e, &v, 100).unwrap_err(), Error::Expired);
        let mut altered = e.clone();
        altered[20] ^= 1;
        assert!(s.verify(&altered, 20).is_err());
        assert_eq!(s.receipt().generation, 1);
        drop(s);
        assert_eq!(
            Store::open(&p, "default")
                .unwrap()
                .receipt()
                .minimum_sequence,
            1
        );
    }
    #[test]
    fn partial_writes_retained_and_publication_never_overwrites_a_record() {
        let (p, k, policy, r) = fixture();
        let mut s = Store::provision(&p, "default", policy, 10).unwrap();
        let pending = s
            .root
            .join(format!("pending-{}.json", uuid::Uuid::new_v4()));
        private_file(&pending, true)
            .unwrap()
            .write_all(b"partial")
            .unwrap();
        let collision = s.root.join(format!("{:020}.json", 2));
        private_file(&collision, true)
            .unwrap()
            .write_all(b"retained collision")
            .unwrap();
        let e = seal(&k, &r);
        let mut v = s.verify(&e, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        assert_eq!(
            s.accept(&e, &v, 20).unwrap_err(),
            Error::TrustWriteUncertain
        );
        assert!(s.receipt().write_uncertain);
        assert_eq!(s.verify(&e, 21).unwrap_err(), Error::TrustWriteUncertain);
        assert_eq!(fs::read(&collision).unwrap(), b"retained collision");
        assert_eq!(fs::read(&pending).unwrap(), b"partial");
        drop(s);
        assert!(matches!(
            Store::open(&p, "default"),
            Err(Error::TrustInvalid)
        ));
    }
    #[test]
    fn changed_hash_chain_gap_unknown_fields_and_unicode_names_refuse() {
        for kind in ["changed", "gap", "unknown", "unicode"] {
            let (p, _, policy, _) = fixture();
            let s = Store::provision(&p, "default", policy, 10).unwrap();
            let root = s.root.clone();
            drop(s);
            let path = root.join(format!("{:020}.json", 1));
            match kind {
                "changed" => {
                    let mut v: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    v["previousSha256"] = serde_json::json!("a".repeat(64));
                    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
                }
                "gap" => fs::rename(&path, root.join(format!("{:020}.json", 2))).unwrap(),
                "unknown" => {
                    let mut v: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    v["extra"] = serde_json::json!(true);
                    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
                }
                _ => {
                    private_file(&root.join("éééééééééé.json"), true).unwrap();
                }
            }
            assert!(matches!(
                Store::open(&p, "default"),
                Err(Error::TrustInvalid)
            ));
        }
    }
    #[test]
    fn private_files_modes_links_and_scopes_are_enforced() {
        for kind in ["mode", "hardlink", "symlink"] {
            let (p, _, policy, _) = fixture();
            let s = Store::provision(&p, "default", policy, 10).unwrap();
            let root = s.root.clone();
            drop(s);
            let path = root.join(format!("{:020}.json", 1));
            match kind {
                "mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o622)).unwrap(),
                "hardlink" => {
                    fs::hard_link(&path, p.parent().unwrap().join("retained-link")).unwrap()
                }
                _ => {
                    let outside = p.parent().unwrap().join("retained-source");
                    fs::rename(&path, &outside).unwrap();
                    symlink(&outside, &path).unwrap();
                }
            }
            assert!(matches!(
                Store::open(&p, "default"),
                Err(Error::TrustInvalid)
            ));
        }
        let (p, _, policy, _) = fixture();
        assert!(matches!(
            Store::provision(&p, "../escape", policy.clone(), 10),
            Err(Error::TrustInvalid)
        ));
        let s = Store::provision(&p, "default", policy, 10).unwrap();
        drop(s);
        assert!(matches!(
            Store::open(&p, "5c3ac1e0-27ea-407d-b0ae-15d074bf4c9c"),
            Err(Error::TrustMissing)
        ));
    }
}
