// SPDX-License-Identifier: Apache-2.0
//! Manager's attempt-bound retirement observation. The Platform must still
//! prove ACK acceptance, finalization, decoder drain and prior Node shutdown.
use super::*;
use crate::maintenance_source::DirectoryPin;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;

const LIMIT: usize = 32 * 1024;
pub(super) const FORMAT: &str = "1.0.0-draft.12";

fn invalid() -> LifecycleError {
    err("NATIVE_OEX_RECOVERY_REQUIRED")
}

fn valid_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| {
        id.to_string() == value
            && id.get_variant() == uuid::Variant::RFC4122
            && (1..=8).contains(&id.get_version_num())
    })
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirectoryIdentity {
    root: PathBuf,
    device: String,
    inode: String,
}

impl DirectoryIdentity {
    fn read(pin: &DirectoryPin, path: PathBuf) -> Result<Self> {
        pin.check_path(&path)?;
        let metadata = pin.try_clone_file()?.metadata().map_err(|_| invalid())?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7777 != 0o700 {
            return Err(invalid());
        }
        Ok(Self {
            root: path,
            device: metadata.dev().to_string(),
            inode: metadata.ino().to_string(),
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SaveIdentity {
    pub(super) exhibition_id: String,
    pub(super) etag: String,
    pub(super) request_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Witness {
    version: u32,
    kind: String,
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
    owner_uid: u32,
    job_id: String,
    attempt_id: String,
    import_request_id: String,
    source_archive_sha256: String,
    runtime_generation: String,
    claim_sha256: String,
    provider_sha256: String,
    journal: DirectoryIdentity,
    coordinator: DirectoryIdentity,
    database_system_id: String,
    database_oid: String,
    stage_id: String,
    settlement_receipt_sha256: String,
    save: SaveIdentity,
}

impl Witness {
    fn check(&self) -> Result<()> {
        if self.version != 1
            || self.kind != "workspace-oex-manager-retired"
            || self.owner_uid != unsafe { libc::geteuid() }
            || ![
                &self.workspace_id,
                &self.tenant_id,
                &self.principal_id,
                &self.job_id,
                &self.attempt_id,
                &self.import_request_id,
                &self.runtime_generation,
                &self.stage_id,
                &self.save.exhibition_id,
                &self.save.request_id,
            ]
            .into_iter()
            .all(|id| valid_id(id))
            || ![
                &self.source_archive_sha256,
                &self.claim_sha256,
                &self.provider_sha256,
                &self.settlement_receipt_sha256,
            ]
            .into_iter()
            .all(|sha| hash_valid(sha))
            || !self
                .save
                .etag
                .strip_prefix("\"studio-r2-")
                .and_then(|tag| tag.strip_suffix('"'))
                .is_some_and(hash_valid)
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn name(&self) -> String {
        witness_name(&self.job_id)
    }
}

fn witness_name(job: &str) -> String {
    format!("native-oex-recovery-{job}.json")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Claim {
    version: u32,
    kind: String,
    operation: String,
    purpose: String,
    tenant_id: String,
    resource_id: String,
    attempt_id: String,
    runtime_generation: String,
    user_id: String,
    request_id: String,
    source_digest: String,
    stage_id: String,
    owner: DiskOwner,
    journal: DirectoryIdentity,
    coordinator: DirectoryIdentity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiskOwner {
    database_system_id: String,
    database_oid: String,
    store_root: PathBuf,
    store_device: String,
    store_inode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Provider {
    version: u32,
    kind: String,
    journal: DirectoryIdentity,
    coordinator: DirectoryIdentity,
}

fn open_record(dir: &File, name: &str, flags: i32) -> std::io::Result<File> {
    let name = std::ffi::CString::new(name)?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[derive(PartialEq, Eq)]
struct RecordStamp {
    device: u64,
    inode: u64,
    bytes: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

fn record_stamp(file: &File, links: u64) -> Result<RecordStamp> {
    let m = file.metadata().map_err(|_| invalid())?;
    if !m.is_file()
        || m.nlink() != links
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o600
        || m.len() == 0
        || m.len() > LIMIT as u64
    {
        return Err(invalid());
    }
    Ok(RecordStamp {
        device: m.dev(),
        inode: m.ino(),
        bytes: m.len(),
        mtime: (m.mtime(), m.mtime_nsec()),
        ctime: (m.ctime(), m.ctime_nsec()),
    })
}

fn read_record(dir: &File, name: &str, links: u64) -> Result<(Vec<u8>, RecordStamp)> {
    let open = || open_record(dir, name, libc::O_RDONLY | libc::O_NONBLOCK).map_err(|_| invalid());
    let mut file = open()?;
    let stamp = record_stamp(&file, links)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != stamp.bytes
        || record_stamp(&file, links)? != stamp
        || record_stamp(&open()?, links)? != stamp
    {
        return Err(invalid());
    }
    Ok((bytes, stamp))
}

fn require_absent(dir: &File, name: &str) -> Result<()> {
    let name = std::ffi::CString::new(name).map_err(|_| invalid())?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        return Ok(());
    }
    Err(invalid())
}

struct PriorDirectories {
    journal: DirectoryPin,
    coordinator: DirectoryPin,
    store: DirectoryPin,
    journal_path: PathBuf,
    coordinator_path: PathBuf,
    store_path: PathBuf,
}

impl PriorDirectories {
    fn open(root: &Path, generation: &str) -> Result<Self> {
        if !valid_id(generation) {
            return Err(invalid());
        }
        let journal_path = root.join(format!("native-oex-journal-{generation}"));
        let coordinator_path = root.join(format!("native-oex-budget-{generation}"));
        let store_path = root.join("blobs");
        Ok(Self {
            journal: DirectoryPin::open(&journal_path)?,
            coordinator: DirectoryPin::open(&coordinator_path)?,
            store: DirectoryPin::open(&store_path)?,
            journal_path,
            coordinator_path,
            store_path,
        })
    }

    fn identities(&self) -> Result<(DirectoryIdentity, DirectoryIdentity, DirectoryIdentity)> {
        Ok((
            DirectoryIdentity::read(&self.journal, self.journal_path.clone())?,
            DirectoryIdentity::read(&self.coordinator, self.coordinator_path.clone())?,
            DirectoryIdentity::read(&self.store, self.store_path.clone())?,
        ))
    }

    fn check_records(&self, witness: &Witness, live: bool) -> Result<()> {
        let identities = self.identities()?;
        let (journal, coordinator, store) = &identities;
        let dir = self.journal.try_clone_file()?;
        let alias = format!("{}.claim.json", witness.attempt_id);
        // IDs were validated as canonical UUIDs. This is the Platform's
        // canonical resource key, sorted by key and with no whitespace.
        let scope = format!(
            r#"{{"resourceId":"{}","tenantId":"{}"}}"#,
            witness.workspace_id, witness.tenant_id
        );
        let anchor = format!("resource-{}.claim.json", digest(scope.as_bytes()));
        let (claim_bytes, alias_stamp) = read_record(&dir, &alias, if live { 2 } else { 1 })?;
        if live {
            let (_, anchor_stamp) = read_record(&dir, &anchor, 2)?;
            if anchor_stamp != alias_stamp {
                return Err(invalid());
            }
        } else {
            require_absent(&dir, &anchor)?;
            dir.sync_all().map_err(|_| invalid())?;
            require_absent(&dir, &anchor)?;
        }
        let (provider_bytes, provider_stamp) = read_record(&dir, "claim-provider.json", 1)?;
        if digest(&claim_bytes) != witness.claim_sha256
            || digest(&provider_bytes) != witness.provider_sha256
        {
            return Err(invalid());
        }

        let claim: Claim = serde_json::from_slice(&claim_bytes).map_err(|_| invalid())?;
        let provider: Provider = serde_json::from_slice(&provider_bytes).map_err(|_| invalid())?;
        if claim.version != 1
            || claim.kind != "resource-claim"
            || claim.operation != "oex-import"
            || claim.purpose != "import"
            || claim.tenant_id != witness.tenant_id
            || claim.resource_id != witness.workspace_id
            || claim.attempt_id != witness.attempt_id
            || claim.runtime_generation != witness.runtime_generation
            || claim.user_id != witness.principal_id
            || claim.request_id != witness.import_request_id
            || claim.source_digest != witness.source_archive_sha256
            || claim.stage_id != witness.stage_id
            || &claim.journal != journal
            || &claim.coordinator != coordinator
            || provider.version != 1
            || provider.kind != "resource-claim-provider"
            || &provider.journal != journal
            || &provider.coordinator != coordinator
            || &witness.journal != journal
            || &witness.coordinator != coordinator
            || claim.owner.database_system_id != witness.database_system_id
            || claim.owner.database_oid != witness.database_oid
            || claim.owner.store_root != store.root
            || claim.owner.store_device != store.device
            || claim.owner.store_inode != store.inode
            || record_stamp(
                &open_record(&dir, &alias, libc::O_RDONLY | libc::O_NONBLOCK)
                    .map_err(|_| invalid())?,
                if live { 2 } else { 1 },
            )? != alias_stamp
            || record_stamp(
                &open_record(
                    &dir,
                    "claim-provider.json",
                    libc::O_RDONLY | libc::O_NONBLOCK,
                )
                .map_err(|_| invalid())?,
                1,
            )? != provider_stamp
            || self.identities()? != identities
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn write_witness(root: &DirectoryPin, witness: &Witness) -> Result<()> {
    let dir = root.try_clone_file()?;
    let bytes = serde_json::to_vec(witness).map_err(|_| invalid())?;
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    let mut file = open_record(
        &dir,
        &witness.name(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )
    .map_err(|_| invalid())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| dir.sync_all())
        .map_err(|_| invalid())?;
    let (retained, stamp) = read_record(&dir, &witness.name(), 1)?;
    if retained != bytes || stamp != record_stamp(&file, 1)? {
        return Err(invalid());
    }
    root.check()
}

/// Call only after the live Session has checked its typed reaped children,
/// retired stage, closed FDs and exact settlement receipt. A write failure
/// leaves any partial file in place and must prevent sending the ACK.
pub(super) fn persist(
    session: &Session,
    owner: &oex_owner::Owner,
    challenge: &owner_claim_frames::ClaimChallenge,
    ready: &owner_claim_frames::ClaimReady,
    save: &owner_claim_frames::SaveFrameV3,
    saved_etag: &str,
    settlement_receipt: &[u8],
) -> Result<()> {
    session.check_native_observation_fence()?;
    if ready.format != FORMAT
        || ready.runtime_generation != challenge.runtime_generation
        || session.job_id.as_ref() != Some(&ready.runtime_generation)
    {
        return Err(invalid());
    }
    let dirs = PriorDirectories::open(&session.service.root, &ready.runtime_generation)?;
    let (journal, coordinator, store) = dirs.identities()?;
    let (store_path, device, inode) = owner.store_identity();
    if store.root != store_path
        || store.device != device.to_string()
        || store.inode != inode.to_string()
    {
        return Err(invalid());
    }
    let (provider, _) = read_record(&dirs.journal.try_clone_file()?, "claim-provider.json", 1)?;
    let witness = Witness {
        version: 1,
        kind: "workspace-oex-manager-retired".into(),
        workspace_id: ready.workspace_id.clone(),
        tenant_id: ready.tenant_id.clone(),
        principal_id: ready.user_id.clone(),
        owner_uid: unsafe { libc::geteuid() },
        job_id: ready.job_id.clone(),
        attempt_id: ready.attempt_id.clone(),
        import_request_id: ready.request_id.clone(),
        source_archive_sha256: ready.archive_sha256.clone(),
        runtime_generation: ready.runtime_generation.clone(),
        claim_sha256: challenge.claim_digest.clone(),
        provider_sha256: digest(&provider),
        journal,
        coordinator,
        database_system_id: owner.system_id.clone(),
        database_oid: owner.database_oid.clone(),
        stage_id: ready.stage_id.clone(),
        settlement_receipt_sha256: digest(settlement_receipt),
        save: SaveIdentity {
            exhibition_id: save.exhibition_id.clone(),
            etag: saved_etag.into(),
            request_id: save.request_id.clone(),
        },
    };
    persist_witness(&session.observation_root, &dirs, &witness)?;
    session.check_native_observation_fence()
}

fn persist_witness(root: &DirectoryPin, dirs: &PriorDirectories, witness: &Witness) -> Result<()> {
    witness.check()?;
    dirs.check_records(witness, true)?;
    write_witness(root, witness)?;
    dirs.check_records(witness, true)
}

/// Inputs must come from the independently held pending transfer and authenticated
/// recovery peer. The persisted witness cannot supply its own expected scope.
#[allow(dead_code)]
pub(super) struct Restart<'a> {
    pub(super) transfer: &'a crate::installations::native_oex_import::Transfer,
    pub(super) attempt_id: &'a str,
    pub(super) runtime_generation: &'a str,
    pub(super) claim_sha256: &'a str,
    pub(super) save: &'a SaveIdentity,
}

#[allow(dead_code)]
pub(super) struct PinnedRecovery {
    root: DirectoryPin,
    root_path: PathBuf,
    dirs: PriorDirectories,
    witness: Witness,
    stamp: RecordStamp,
    bytes: Vec<u8>,
}

impl PinnedRecovery {
    /// Capture the prior-generation pins before replacing the current run
    /// pointer, while holding the selected workspace's root operation fence.
    #[allow(dead_code)]
    pub(super) fn capture(session: &Session, expected: Restart<'_>) -> Result<Self> {
        session.check_native_observation_fence()?;
        if session.job_id.is_some() || session.any_owned() {
            return Err(invalid());
        }
        let root_path = session.service.root.clone();
        check_interrupted_root(&root_path)?;
        let root = DirectoryPin::open(&root_path)?;
        let identity = session.service.personal_identity()?;
        Self::open(root, root_path, &identity, expected)
    }

    fn open(
        root: DirectoryPin,
        root_path: PathBuf,
        identity: &(String, String, String),
        expected: Restart<'_>,
    ) -> Result<Self> {
        let transfer = expected.transfer;
        let job = transfer
            .job_id
            .as_deref()
            .filter(|job| valid_id(job))
            .ok_or_else(invalid)?;
        let intent = transfer.intent.as_ref().ok_or_else(invalid)?;
        if transfer.format != 1
            || intent.profile.as_deref() != Some("bounded-file-v1")
            || intent.oex_format.as_deref() != Some(FORMAT)
            || !(1..=512 * 1024 * 1024).contains(&intent.bytes)
            || identity
                != &(
                    transfer.workspace_id.clone(),
                    transfer.tenant_id.clone(),
                    transfer.principal_id.clone(),
                )
        {
            return Err(invalid());
        }
        let (bytes, stamp) = read_record(&root.try_clone_file()?, &witness_name(job), 1)?;
        let witness: Witness = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        witness.check()?;
        if serde_json::to_vec(&witness).map_err(|_| invalid())? != bytes
            || witness.job_id != job
            || witness.workspace_id != transfer.workspace_id
            || witness.tenant_id != transfer.tenant_id
            || witness.principal_id != transfer.principal_id
            || witness.import_request_id != intent.request_id
            || witness.source_archive_sha256 != intent.sha256
            || witness.attempt_id != expected.attempt_id
            || witness.runtime_generation != expected.runtime_generation
            || witness.claim_sha256 != expected.claim_sha256
            || &witness.save != expected.save
        {
            return Err(invalid());
        }
        let dirs = PriorDirectories::open(&root_path, expected.runtime_generation)?;
        let pinned = Self {
            root,
            root_path,
            dirs,
            witness,
            stamp,
            bytes,
        };
        pinned.recheck()?;
        Ok(pinned)
    }

    fn recheck(&self) -> Result<()> {
        self.root.check_path(&self.root_path)?;
        self.dirs.check_records(&self.witness, false)?;
        let (bytes, stamp) = read_record(&self.root.try_clone_file()?, &self.witness.name(), 1)?;
        if self.bytes != bytes || self.stamp != stamp {
            return Err(invalid());
        }
        self.root.check()
    }

    /// Recheck the same live root fence and current DB owner when supplying
    /// bytes on the authenticated recovery channel. No replay authority is
    /// granted by these bytes or by obtaining the root lock.
    #[allow(dead_code)]
    pub(super) fn checked_bytes(
        &self,
        session: &mut Session,
        owner: &oex_owner::Owner,
    ) -> Result<&[u8]> {
        owner.recheck_database_identity(session)?;
        if session.service.root != self.root_path
            || owner.system_id != self.witness.database_system_id
            || owner.database_oid != self.witness.database_oid
            || session.service.personal_identity()?
                != (
                    self.witness.workspace_id.clone(),
                    self.witness.tenant_id.clone(),
                    self.witness.principal_id.clone(),
                )
        {
            return Err(invalid());
        }
        self.recheck()?;
        owner.recheck_database_identity(session)?;
        self.recheck()?;
        Ok(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installations::native_oex_import::{Intent, Transfer};
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    fn id(value: u8) -> String {
        format!("01234567-89ab-4def-8123-0000000000{value:02x}")
    }

    struct Fixture {
        root: PathBuf,
        witness: Witness,
        transfer: Transfer,
        anchor: PathBuf,
        alias: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
            let root = parent.join(format!("exhibitos-oex-recovery-{}", Uuid::new_v4()));
            installations::new_directory(&root).unwrap();
            let generation = id(1);
            for name in [
                format!("native-oex-journal-{generation}"),
                format!("native-oex-budget-{generation}"),
                "blobs".into(),
            ] {
                installations::new_directory(&root.join(name)).unwrap();
            }
            let dirs = PriorDirectories::open(&root, &generation).unwrap();
            let (journal, coordinator, store) = dirs.identities().unwrap();
            let archive = "a".repeat(64);
            let claim = serde_json::json!({
                "version": 1, "kind": "resource-claim", "operation": "oex-import", "purpose": "import",
                "tenantId": id(2), "resourceId": id(3), "attemptId": id(4), "runtimeGeneration": generation,
                "userId": id(5), "requestId": id(6), "sourceDigest": archive, "stageId": id(7),
                "owner": {"databaseSystemId": "123", "databaseOid": "456", "storeRoot": store.root,
                    "storeDevice": store.device, "storeInode": store.inode},
                "journal": journal, "coordinator": coordinator
            });
            let claim = serde_json::to_vec(&claim).unwrap();
            let provider = serde_json::to_vec(&serde_json::json!({
                "version": 1, "kind": "resource-claim-provider", "journal": journal, "coordinator": coordinator
            })).unwrap();
            let scope =
                serde_json::to_vec(&serde_json::json!({"tenantId": id(2), "resourceId": id(3)}))
                    .unwrap();
            let anchor = journal
                .root
                .join(format!("resource-{}.claim.json", digest(&scope)));
            let alias = journal.root.join(format!("{}.claim.json", id(4)));
            let write_private = |path: &Path, bytes: &[u8]| {
                let mut file = private_options().open(path).unwrap();
                file.write_all(bytes).unwrap();
                file.sync_all().unwrap();
            };
            write_private(&alias, &claim);
            fs::hard_link(&alias, &anchor).unwrap();
            write_private(&journal.root.join("claim-provider.json"), &provider);
            File::open(&journal.root).unwrap().sync_all().unwrap();
            let witness = Witness {
                version: 1,
                kind: "workspace-oex-manager-retired".into(),
                workspace_id: id(3),
                tenant_id: id(2),
                principal_id: id(5),
                owner_uid: unsafe { libc::geteuid() },
                job_id: id(8),
                attempt_id: id(4),
                import_request_id: id(6),
                source_archive_sha256: archive.clone(),
                runtime_generation: generation,
                claim_sha256: digest(&claim),
                provider_sha256: digest(&provider),
                journal,
                coordinator,
                database_system_id: "123".into(),
                database_oid: "456".into(),
                stage_id: id(7),
                settlement_receipt_sha256: digest(b"synthetic exact receipt"),
                save: SaveIdentity {
                    exhibition_id: id(9),
                    etag: format!("\"studio-r2-{}\"", "b".repeat(64)),
                    request_id: id(10),
                },
            };
            let transfer = Transfer {
                format: 1,
                revision: 1,
                workspace_id: id(3),
                tenant_id: id(2),
                principal_id: id(5),
                job_id: Some(id(8)),
                file_name: Some("synthetic.oex".into()),
                intent: Some(Intent {
                    request_id: id(6),
                    sha256: archive,
                    bytes: 42,
                    profile: Some("bounded-file-v1".into()),
                    oex_format: Some(FORMAT.into()),
                }),
            };
            Self {
                root,
                witness,
                transfer,
                anchor,
                alias,
            }
        }

        fn dirs(&self) -> PriorDirectories {
            PriorDirectories::open(&self.root, &self.witness.runtime_generation).unwrap()
        }

        fn write(&self) -> Result<()> {
            persist_witness(
                &DirectoryPin::open(&self.root).unwrap(),
                &self.dirs(),
                &self.witness,
            )
        }

        fn expected(&self) -> Restart<'_> {
            Restart {
                transfer: &self.transfer,
                attempt_id: &self.witness.attempt_id,
                runtime_generation: &self.witness.runtime_generation,
                claim_sha256: &self.witness.claim_sha256,
                save: &self.witness.save,
            }
        }

        fn restart(&self, expected: Restart<'_>) -> Result<PinnedRecovery> {
            let service = LifecycleService {
                root: self.root.clone(),
            };
            let _guard = service.lock()?;
            PinnedRecovery::open(
                DirectoryPin::open(&self.root)?,
                self.root.clone(),
                &(id(3), id(2), id(5)),
                expected,
            )
        }

        fn witness_path(&self) -> PathBuf {
            self.root.join(self.witness.name())
        }

        fn finalized(&self) {
            fs::remove_file(&self.anchor).unwrap();
            File::open(&self.witness.journal.root)
                .unwrap()
                .sync_all()
                .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn workspace_oex_recovery_witness_is_private_create_only_and_requires_retired_anchor() {
        let fixture = Fixture::new();
        assert!(fixture.restart(fixture.expected()).is_err());
        fixture.write().unwrap();
        let path = fixture.witness_path();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        assert!(fixture.write().is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(fixture.restart(fixture.expected()).is_err());
        fixture.finalized();
        let pinned = fixture.restart(fixture.expected()).unwrap();
        assert_eq!(pinned.bytes, bytes);
        pinned.recheck().unwrap();

        let wrong_save = SaveIdentity {
            etag: format!("\"studio-r2-{}\"", "c".repeat(64)),
            exhibition_id: fixture.witness.save.exhibition_id.clone(),
            request_id: fixture.witness.save.request_id.clone(),
        };
        assert!(
            fixture
                .restart(Restart {
                    save: &wrong_save,
                    ..fixture.expected()
                })
                .is_err()
        );
        assert!(
            fixture
                .restart(Restart {
                    attempt_id: &id(21),
                    ..fixture.expected()
                })
                .is_err()
        );
        assert!(
            fixture
                .restart(Restart {
                    runtime_generation: &id(22),
                    ..fixture.expected()
                })
                .is_err()
        );
        assert!(
            fixture
                .restart(Restart {
                    claim_sha256: &"d".repeat(64),
                    ..fixture.expected()
                })
                .is_err()
        );

        fs::rename(&path, fixture.root.join("witness-renamed")).unwrap();
        fs::copy(fixture.root.join("witness-renamed"), &path).unwrap();
        assert!(
            pinned.recheck().is_err(),
            "identical bytes at a substituted inode"
        );
    }

    #[test]
    fn workspace_oex_recovery_rejects_partial_evidence_and_replaced_owner_paths() {
        let fixture = Fixture::new();
        // A second hardlink to a different inode cannot be used as the anchor.
        fs::remove_file(&fixture.anchor).unwrap();
        let substitute = fixture.witness.journal.root.join("substitute");
        fs::copy(&fixture.alias, &substitute).unwrap();
        fs::hard_link(&substitute, &fixture.anchor).unwrap();
        fs::hard_link(
            &fixture.alias,
            fixture.witness.journal.root.join("other-link"),
        )
        .unwrap();
        assert!(fixture.write().is_err());
        assert!(!fixture.witness_path().exists());

        let fixture = Fixture::new();
        private_options()
            .open(fixture.witness_path())
            .unwrap()
            .write_all(b"{\"version\":1")
            .unwrap();
        assert!(fixture.write().is_err());
        fixture.finalized();
        assert!(fixture.restart(fixture.expected()).is_err());

        let fixture = Fixture::new();
        fixture.write().unwrap();
        fixture.finalized();
        let pinned = fixture.restart(fixture.expected()).unwrap();
        let provider = fixture.witness.journal.root.join("claim-provider.json");
        fs::write(&provider, b"{}").unwrap();
        assert!(pinned.recheck().is_err());
        assert!(fixture.restart(fixture.expected()).is_err());

        let fixture = Fixture::new();
        fixture.write().unwrap();
        fixture.finalized();
        let path = &fixture.witness.coordinator.root;
        fs::rename(path, path.with_extension("retained")).unwrap();
        installations::new_directory(path).unwrap();
        assert!(fixture.restart(fixture.expected()).is_err());
    }

    // The child holds the real root flock and uses the actual durable writer.
    // These markers are harness milestones, not owner-channel protocol frames.
    #[test]
    #[ignore = "invoked as an isolated child by workspace_oex_recovery_survives_process_exit"]
    fn workspace_oex_recovery_crash_child() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_RECOVERY_ROOT").unwrap());
        let mode = std::env::var("EXHIBITOS_TEST_RECOVERY_MODE").unwrap();
        let service = LifecycleService { root: root.clone() };
        let _guard = service.lock().unwrap();
        let witness: Witness =
            serde_json::from_slice(&fs::read(root.join("synthetic-input.json")).unwrap()).unwrap();
        let dirs = PriorDirectories::open(&root, &witness.runtime_generation).unwrap();
        if mode != "before-write" {
            persist_witness(&DirectoryPin::open(&root).unwrap(), &dirs, &witness).unwrap();
        }
        println!("RECOVERY-CHILD:{mode}");
        std::io::stdout().flush().unwrap();
        if mode != "normal" {
            loop {
                std::thread::park();
            }
        }
    }

    #[test]
    fn workspace_oex_recovery_survives_process_exit() {
        for mode in ["normal", "before-write", "before-ack", "after-ack"] {
            let fixture = Fixture::new();
            fs::write(
                fixture.root.join("synthetic-input.json"),
                serde_json::to_vec(&fixture.witness).unwrap(),
            )
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "native_runtime::oex_recovery::tests::workspace_oex_recovery_crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EXHIBITOS_TEST_RECOVERY_ROOT", &fixture.root)
                .env("EXHIBITOS_TEST_RECOVERY_MODE", mode)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let marker = format!("RECOVERY-CHILD:{mode}");
            let seen = BufReader::new(child.stdout.take().unwrap())
                .lines()
                .map_while(std::result::Result::ok)
                .any(|line| line.contains(&marker));
            if seen && mode != "normal" {
                let service = LifecycleService {
                    root: fixture.root.clone(),
                };
                assert!(
                    service.lock().is_err(),
                    "child still holds the root operation guard"
                );
            }
            if seen && mode == "after-ack" {
                assert!(
                    fixture.witness_path().exists(),
                    "durable witness precedes ACK"
                );
                // The fixture peer accepts ACK and retires its anchor while
                // the child still holds the root lock, before the crash.
                fixture.finalized();
            }
            if mode != "normal" || !seen {
                let _ = child.kill();
            }
            let status = child.wait().unwrap();
            assert!(seen, "{mode} did not reach its isolation checkpoint");
            assert_eq!(status.success(), mode == "normal");
            assert_eq!(fixture.witness_path().exists(), mode != "before-write");
            if mode != "after-ack" {
                assert!(
                    fixture.restart(fixture.expected()).is_err(),
                    "{mode}: live anchor"
                );
            }
            if mode == "normal" {
                fixture.finalized();
            }
            if matches!(mode, "normal" | "after-ack") {
                let pinned = fixture.restart(fixture.expected()).unwrap();
                pinned.recheck().unwrap();
            }
        }
    }
}
