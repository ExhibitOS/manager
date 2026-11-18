// SPDX-License-Identifier: Apache-2.0
//! Offline recovery of the one reviewed r178 custody move.

use super::*;
use crate::maintenance_source::DirectoryPin;
use std::ffi::CString;
use std::os::fd::AsRawFd;

// Pins the two path digests, old sealed checkpoint, current owner/environment,
// cold pointer and run, four active journals, and the historical detail record.
const R178_CUSTODY: &str = "62d8d499d60f1c7bc835a01f653d206ea9282290290bfbfb86d513515c34f367";
const R178_PATHS: &str = "f2deb96b22b7bf2132559f33d49f94ecfae82db2b563fe5c6d23ede7d46411f6";
const JOURNALS: [(&str, u64); 4] = [
    ("native-document", 1_048_576),
    ("native-upload", 128 * 1024),
    ("native-publication", 2_200_000),
    ("native-static-export", 65_536),
];
const FENCE: &str = "native-personal-r178-rebind-pending.json";

fn rejected() -> LifecycleError {
    err("NATIVE_R178_REBIND_UNAVAILABLE")
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(rejected()),
    }
}

fn at_destination(root: &Path, old: &str, new: &str) -> Result<bool> {
    match (exists(&root.join(old))?, exists(&root.join(new))?) {
        (true, false) => Ok(false),
        (false, true) => Ok(true),
        _ => Err(rejected()),
    }
}

fn rename_exclusive(root: &DirectoryPin, old: &str, new: &str) -> Result<()> {
    root.check()?;
    let dir = root.try_clone_file()?;
    let old = CString::new(old).map_err(|_| rejected())?;
    let new = CString::new(new).map_err(|_| rejected())?;
    if unsafe {
        libc::renameatx_np(
            dir.as_raw_fd(),
            old.as_ptr(),
            dir.as_raw_fd(),
            new.as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err(rejected());
    }
    dir.sync_all().map_err(|_| rejected())?;
    root.check()
}

struct Journal {
    names: [String; 2],
    digest: String,
    limit: u64,
}

impl Journal {
    fn location(&self, profile: &Path) -> Result<bool> {
        at_destination(profile, &self.names[0], &self.names[1])
    }

    fn bytes(&self, profile: &Path, destination: bool) -> Result<Vec<u8>> {
        installation_backup::source_bytes(
            profile,
            &self.names[usize::from(destination)],
            self.limit,
            true,
        )
    }
}

fn move_journals(
    profile: &Path,
    service: &Path,
    journals: &[Journal],
    owner_hash: &str,
    to_destination: bool,
) -> Result<()> {
    let profile_pin = DirectoryPin::open(profile)?;
    let service_pin = DirectoryPin::open(service)?;
    let fenced = at_destination(service, RECORD, FENCE)?;
    let owner = if fenced { FENCE } else { RECORD };
    if digest(&installation_backup::source_bytes(
        service, owner, 4096, true,
    )?) != owner_hash
    {
        return Err(rejected());
    }
    let mut positions = Vec::with_capacity(journals.len());
    for journal in journals {
        let at_destination = journal.location(profile)?;
        if digest(&journal.bytes(profile, at_destination)?) != journal.digest {
            return Err(rejected());
        }
        positions.push(at_destination);
    }
    if !fenced {
        if !positions.windows(2).all(|pair| pair[0] == pair[1]) {
            return Err(rejected());
        }
        if positions.iter().all(|&position| position == to_destination) {
            return Ok(());
        }
        // All existing helpers require the owner record to open a managed
        // document or start its runtime. Existing env/DB/config also prevent
        // treating a fenced profile as a new installation after interruption.
        rename_exclusive(&service_pin, RECORD, FENCE)?;
    }
    for (journal, at_destination) in journals.iter().zip(positions) {
        service_pin.check()?;
        if at_destination != to_destination {
            rename_exclusive(
                &profile_pin,
                &journal.names[usize::from(at_destination)],
                &journal.names[usize::from(to_destination)],
            )?;
        }
        if journal.location(profile)? != to_destination
            || digest(&journal.bytes(profile, to_destination)?) != journal.digest
        {
            return Err(rejected());
        }
    }
    rename_exclusive(&service_pin, FENCE, RECORD)
}

/// The offline CLI is the only entry point. An interrupted transaction keeps
/// the original owner inode under the fence until every record is coherent.
pub fn rebind_r178_workspace(
    profile: &Path,
    original: &Path,
    staging: &Path,
    key_path: &Path,
    rollback: bool,
) -> Result<&'static str> {
    let parent = profile.parent().ok_or_else(rejected)?;
    let original_parent = original.parent().ok_or_else(rejected)?;
    let original_root = original.join("local-runtime");
    let service = profile.join("local-runtime");
    let old_key = digest(original_root.as_os_str().as_encoded_bytes());
    let new_key = digest(service.as_os_str().as_encoded_bytes());
    let stage_name = staging
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(rejected)?;
    if digest(format!("{old_key}\n{new_key}").as_bytes()) != R178_PATHS
        || !original.is_absolute()
        || exists(original)?
        || staging.parent() != Some(parent)
        || key_path != parent.join(format!("{stage_name}.key"))
        || fs::canonicalize(&service).map_err(|_| rejected())? != service
        || fs::canonicalize(staging).map_err(|_| rejected())? != staging
    {
        return Err(rejected());
    }
    let session = crate::profile_backup::session_lock(profile, true)?;
    let _host = crate::profile_backup::current_host_locks(profile, &session)?;
    let _writer =
        crate::native_runtime::FurnishingTransitionWriterGuard::acquire_existing(&service)?;
    crate::native_runtime::check_cold_checkpoint_root(&service)?;
    installations::gpu_model_job_root_empty(&service)?;
    for root in [profile, service.as_path()] {
        LifecycleService::bound_existing(root.to_owned())?.ensure_external_archive_reconciled()?;
    }
    if exists(&service.join("native-restore-current.json"))? {
        return Err(rejected());
    }
    for suffix in [&old_key, &new_key] {
        for name in [
            "native-oex-import",
            "native-oex-workspace-complete",
            "native-furnishing-thumbnail",
            "detail-native-static-export",
        ] {
            if exists(&profile.join(format!("{name}-{suffix}.json")))? {
                return Err(rejected());
            }
        }
    }
    let (registry, _) = installations::load(profile)?.ok_or_else(rejected)?;
    let active = registry
        .installations
        .iter()
        .find(|entry| entry.id == registry.active_id)
        .ok_or_else(rejected)?;
    if registry.installations.len() != 1 || installations::root(profile, active) != service {
        return Err(rejected());
    }
    let key = Bytes(crate::profile_backup::external_key(profile, key_path)?);
    let envelope =
        installation_backup::source_bytes(staging, ENVELOPE, ENVELOPE_LIMIT as u64, true)?;
    let proof = open(
        &envelope,
        key.0.as_slice().try_into().map_err(|_| rejected())?,
    )?;
    let source = crate::native_payload::R285_NATIVE_DETAIL_CONTEXT_TARGET.2;
    let current = crate::native_payload::R285_POINT_VOICE_TARGET.2;
    if proof.source != original_root
        || proof.staged != original_parent.join(stage_name)
        || proof.source_identity != directory_identity(&service)?
        || proof.staged_identity != directory_identity(staging)?
        || proof.owner_uid != unsafe { libc::geteuid() }
        || proof.old_manifest != source
        || proof.new_manifest != current
        || proof.saved_document_sha256.is_none()
        || proof.restore.is_some()
    {
        return Err(rejected());
    }
    let read = |name, limit| installation_backup::source_bytes(&service, name, limit, true);
    let owner_bytes = read(
        if at_destination(&service, RECORD, FENCE)? {
            FENCE
        } else {
            RECORD
        },
        4096,
    )?;
    let owner: super::super::Personal =
        serde_json::from_slice(&owner_bytes).map_err(|_| rejected())?;
    let env = Bytes(read(ENVIRONMENT, 8192)?);
    let env_hash = digest(&env.0);
    if owner.format != 1
        || owner.owner_uid != proof.owner_uid
        || owner.manifest_sha256 != current
        || owner.environment_sha256 != env_hash
        || (&owner.workspace_id, &owner.tenant_id, &owner.principal_id)
            != (&proof.workspace_id, &proof.tenant_id, &proof.principal_id)
    {
        return Err(rejected());
    }
    let pointer_bytes = read("native-current-run.json", 65_536)?;
    let pointer: serde_json::Value =
        serde_json::from_slice(&pointer_bytes).map_err(|_| rejected())?;
    let run = pointer["id"]
        .as_str()
        .filter(|id| installations::uuid(id))
        .ok_or_else(rejected)?;
    let run_record = read(&format!("native-runs/{run}/state.json"), 65_536)?;
    let mut pins = vec![
        old_key.clone(),
        new_key.clone(),
        digest(&envelope),
        digest(&owner_bytes),
        env_hash,
        digest(&pointer_bytes),
        digest(&run_record),
    ];
    let mut journals = Vec::new();
    for (kind, limit) in JOURNALS {
        let mut journal = Journal {
            names: [
                format!("{kind}-{old_key}.json"),
                format!("{kind}-{new_key}.json"),
            ],
            digest: String::new(),
            limit,
        };
        let bytes = journal.bytes(profile, journal.location(profile)?)?;
        if kind == "native-document" {
            let doc: installations::native_document::DocumentState =
                serde_json::from_slice(&bytes).map_err(|_| rejected())?;
            installations::native_document::validate(&doc)?;
            check_saved_transition_document(
                (
                    current,
                    crate::native_payload::R285_PLAIN_BASEBOARD_TARGET.2,
                ),
                &doc,
                (&owner.workspace_id, &owner.tenant_id, &owner.principal_id),
            )?;
            if (&doc.workspace_id, &doc.tenant_id, &doc.principal_id)
                != (&owner.workspace_id, &owner.tenant_id, &owner.principal_id)
            {
                return Err(rejected());
            }
        }
        journal.digest = digest(&bytes);
        pins.push(journal.digest.clone());
        journals.push(journal);
    }
    let terminal = format!("terminal-detail-native-static-export-{old_key}.json");
    pins.push(digest(&installation_backup::source_bytes(
        profile, &terminal, 65_536, true,
    )?));
    if digest(pins.join("\n").as_bytes()) != R178_CUSTODY {
        return Err(rejected());
    }
    check_staged_names(staging, true)?;
    if inventory_with_limit(staging, true, proof.inventory_limit())? != proof.files
        || installation_backup::source_bytes(staging, ENVELOPE, ENVELOPE_LIMIT as u64, true)?
            != envelope
        || directory_identity(staging)? != proof.staged_identity
        || directory_identity(&service)? != proof.source_identity
    {
        return Err(rejected());
    }
    session.check_exclusive(profile)?;
    move_journals(
        profile,
        &service,
        &journals,
        &digest(&owner_bytes),
        !rollback,
    )?;
    Ok(if rollback {
        "original-journal-names-restored"
    } else {
        "workspace-journals-rebound"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::DirBuilderExt;

    struct Synthetic {
        root: PathBuf,
        profile: PathBuf,
        service: PathBuf,
        owner_hash: String,
        journals: Vec<Journal>,
    }

    fn private_file(root: &Path, name: &str, bytes: &[u8]) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join(name))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    impl Synthetic {
        fn new() -> Self {
            let root = fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("exhibitos-rebind-test-{}", uuid::Uuid::new_v4()));
            let profile = root.join("profile");
            let service = profile.join("local-runtime");
            DirBuilder::new().mode(0o700).create(&root).unwrap();
            DirBuilder::new().mode(0o700).create(&profile).unwrap();
            DirBuilder::new().mode(0o700).create(&service).unwrap();
            let id = "bb4b11b4-1cb7-4310-af04-1bdb8911905e";
            let owner = serde_json::to_vec(&serde_json::json!({
                "format": 1, "workspaceId": id, "tenantId": id, "principalId": id,
                "ownerUid": unsafe { libc::geteuid() },
                "ports": {"web": 58411, "api": 58412, "database": 58413},
                "manifestSha256": "a".repeat(64), "environmentSha256": "b".repeat(64),
            }))
            .unwrap();
            private_file(&service, RECORD, &owner);
            private_file(&service, "runtime.env", b"unchanged");
            let registry = serde_json::to_vec(&serde_json::json!({
                "format": 1, "activeId": id,
                "installations": [{"id": id, "kind": "default", "createdAt": 1}],
            }))
            .unwrap();
            private_file(&profile, "installation-selection.json", &registry);
            let document = serde_json::to_vec(&serde_json::json!({
                "format": 1, "revision": 514, "workspaceId": id, "tenantId": id, "principalId": id,
                "draft": {"kind": "exhibition-draft", "schemaVersion": "1.0.0-draft.2",
                    "id": id, "exhibitionId": id, "editVersion": 234, "candidate": {
                        "kind": "exhibition-revision", "schemaVersion": "1.0.0-draft.2", "id": id}},
                "etag": format!("\"studio-r178-{}\"", "a".repeat(64)),
                "savedEditVersion": 234, "pending": null,
            }))
            .unwrap();
            let new_key = digest(service.as_os_str().as_encoded_bytes());
            let journals = JOURNALS
                .iter()
                .enumerate()
                .map(|(i, (kind, limit))| {
                    let names = [format!("{kind}-old.json"), format!("{kind}-{new_key}.json")];
                    let filler = [i as u8];
                    let body = if i == 0 {
                        document.as_slice()
                    } else {
                        filler.as_slice()
                    };
                    private_file(&profile, &names[0], &body);
                    Journal {
                        names,
                        digest: digest(&body),
                        limit: *limit,
                    }
                })
                .collect();
            Self {
                root,
                profile,
                service,
                owner_hash: digest(&owner),
                journals,
            }
        }
    }

    impl Drop for Synthetic {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn r178_rebind_interrupted_after_each_rename_resumes_or_rolls_back() {
        for start_at_destination in [false, true] {
            for moved in 0..=JOURNALS.len() {
                for destination in [false, true] {
                    let t = Synthetic::new();
                    let manager = LifecycleService::bound_existing(t.service.clone()).unwrap();
                    let identity = manager.personal_identity().unwrap();
                    let owner_inode = fs::metadata(t.service.join(RECORD)).unwrap().ino();
                    let journal_inodes: Vec<_> = t
                        .journals
                        .iter()
                        .map(|entry| fs::metadata(t.profile.join(&entry.names[0])).unwrap().ino())
                        .collect();
                    let profile = DirectoryPin::open(&t.profile).unwrap();
                    let service = DirectoryPin::open(&t.service).unwrap();
                    if start_at_destination {
                        move_journals(&t.profile, &t.service, &t.journals, &t.owner_hash, true)
                            .unwrap();
                    }
                    rename_exclusive(&service, RECORD, FENCE).unwrap();
                    for entry in t.journals.iter().take(moved) {
                        rename_exclusive(
                            &profile,
                            &entry.names[usize::from(start_at_destination)],
                            &entry.names[usize::from(!start_at_destination)],
                        )
                        .unwrap();
                    }
                    assert!(manager.personal_identity().is_err());
                    {
                        let controller =
                            InstallationController::open_existing_managed(t.profile.clone())
                                .unwrap();
                        let token = controller.context().unwrap().selection_token;
                        assert!(controller.native_document(&token).is_err());
                        assert!(crate::profile_backup::session_lock(&t.profile, true).is_err());
                    }
                    move_journals(
                        &t.profile,
                        &t.service,
                        &t.journals,
                        &t.owner_hash,
                        destination,
                    )
                    .unwrap();
                    assert_eq!(manager.personal_identity().unwrap(), identity);
                    assert_eq!(
                        fs::metadata(t.service.join(RECORD)).unwrap().ino(),
                        owner_inode
                    );
                    assert!(!t.service.join(FENCE).exists());
                    for (entry, inode) in t.journals.iter().zip(journal_inodes) {
                        assert_eq!(entry.location(&t.profile).unwrap(), destination);
                        assert_eq!(
                            digest(&entry.bytes(&t.profile, destination).unwrap()),
                            entry.digest
                        );
                        assert_eq!(
                            fs::metadata(t.profile.join(&entry.names[usize::from(destination)]))
                                .unwrap()
                                .ino(),
                            inode
                        );
                    }
                    assert_eq!(
                        fs::read(t.service.join("runtime.env")).unwrap(),
                        b"unchanged"
                    );
                    {
                        let controller =
                            InstallationController::open_existing_managed(t.profile.clone())
                                .unwrap();
                        let token = controller.context().unwrap().selection_token;
                        let document = controller.native_document(&token).unwrap();
                        if destination {
                            assert_eq!(
                                (document.revision, document.saved_edit_version),
                                (514, Some(234))
                            );
                        } else {
                            assert!(document.draft.is_none());
                        }
                    }
                    move_journals(
                        &t.profile,
                        &t.service,
                        &t.journals,
                        &t.owner_hash,
                        destination,
                    )
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn r178_rebind_unrelated_path_is_rejected_before_lock_creation() {
        let t = Synthetic::new();
        let error = rebind_r178_workspace(
            &t.profile,
            &t.root.join("old"),
            &t.root.join("checkpoint"),
            &t.root.join("checkpoint.key"),
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, "NATIVE_R178_REBIND_UNAVAILABLE");
        assert_eq!(fs::read_dir(&t.root).unwrap().count(), 1);
        assert!(!t.profile.join("profile-session.lock").exists());
        assert!(
            t.journals
                .iter()
                .all(|entry| !entry.location(&t.profile).unwrap())
        );
    }

    #[test]
    fn r178_rebind_rejects_collision_wrong_bytes_and_unfenced_partial_state() {
        for fault in 0..3 {
            let t = Synthetic::new();
            let entry = &t.journals[1];
            match fault {
                0 => private_file(&t.profile, &entry.names[1], b"collision"),
                1 => fs::write(t.profile.join(&entry.names[0]), b"wrong bytes").unwrap(),
                _ => rename_exclusive(
                    &DirectoryPin::open(&t.profile).unwrap(),
                    &entry.names[0],
                    &entry.names[1],
                )
                .unwrap(),
            }
            assert!(
                move_journals(&t.profile, &t.service, &t.journals, &t.owner_hash, true).is_err()
            );
            assert!(t.service.join(RECORD).exists());
            assert!(!t.service.join(FENCE).exists());
            assert!(!t.journals[0].location(&t.profile).unwrap());
        }
    }
}
