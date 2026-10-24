// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use aes_gcm::aead::rand_core::{OsRng, RngCore};
use exhibitos_lifecycle::{
    installations::{InstallationController, native_backup, native_restore},
    native_installation::Ports,
    native_payload::Payload,
    native_runtime::Session,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn ports() -> Ports {
    let ls: Vec<_> = (0..3)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    Ports {
        web: ls[0].local_addr().unwrap().port(),
        api: ls[1].local_addr().unwrap().port(),
        database: ls[2].local_addr().unwrap().port(),
    }
}
fn make_key(path: &Path) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    bytes.fill(0);
}
fn allocated(path: &Path) -> u64 {
    let meta = fs::symlink_metadata(path).unwrap();
    assert!(!meta.is_symlink());
    if meta.is_file() {
        meta.blocks() * 512
    } else {
        fs::read_dir(path)
            .unwrap()
            .map(|e| allocated(&e.unwrap().path()))
            .sum()
    }
}
fn file_witness(path: &Path) -> (u64, String, u32, u64, u64) {
    let before = fs::symlink_metadata(path).unwrap();
    assert!(before.is_file() && !before.is_symlink());
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .unwrap();
    let opened = file.metadata().unwrap();
    assert_eq!((opened.dev(), opened.ino()), (before.dev(), before.ino()));
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut count = 0;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        count += n as u64;
    }
    let after = file.metadata().unwrap();
    assert_eq!(
        (count, after.len(), after.mtime(), after.mtime_nsec()),
        (
            before.len(),
            before.len(),
            before.mtime(),
            before.mtime_nsec()
        )
    );
    (
        count,
        format!("{:x}", digest.finalize()),
        after.mode(),
        after.uid() as u64,
        after.ino(),
    )
}
fn tree_witness(root: &Path) -> BTreeMap<String, (u64, String, u32, u64, u64)> {
    fn visit(root: &Path, path: &Path, rows: &mut BTreeMap<String, (u64, String, u32, u64, u64)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.is_symlink());
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        if metadata.is_dir() {
            rows.insert(
                name,
                (
                    0,
                    "directory".into(),
                    metadata.mode(),
                    metadata.uid() as u64,
                    metadata.ino(),
                ),
            );
            for child in fs::read_dir(path).unwrap() {
                visit(root, &child.unwrap().path(), rows);
            }
        } else {
            rows.insert(name, file_witness(path));
        }
    }
    let mut rows = BTreeMap::new();
    visit(root, root, &mut rows);
    rows
}
struct Hidden {
    original: PathBuf,
    retained: PathBuf,
}
impl Drop for Hidden {
    fn drop(&mut self) {
        if !self.original.exists() && self.retained.exists() {
            fs::rename(&self.retained, &self.original)
                .expect("restore owned synthetic source path");
        }
    }
}

#[test]
#[ignore = "Requires one explicit new GLB/exhibition fixture, exact new payload and absent owned destination"]
fn actual_encrypted_backup_same_id_recovery_activation_and_reopen() {
    let payload = Arc::new(
        Payload::open(&PathBuf::from(
            std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").expect("exact new payload"),
        ))
        .unwrap(),
    );
    let source_profile = PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_RECOVERY_SOURCE_PROFILE")
            .expect("owned prepared synthetic fixture only"),
    );
    let fixture: serde_json::Value = serde_json::from_slice(
        &fs::read(
            source_profile
                .parent()
                .expect("fixture parent")
                .join("fixture.json"),
        )
        .expect("owned synthetic fixture receipt"),
    )
    .unwrap();
    assert_eq!(
        fixture["scope"],
        "SYNTHETIC_VIEWPORT_GUI_FIXTURE_NOT_GUI_PASS"
    );
    assert_eq!(
        fixture["sha256"],
        "1e4e53565fdbc5a71b1aaa83d6b25df6e8025add2f08b35fdca20475fbbca460"
    );
    let requested = PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_RECOVERY_TEST_ROOT").expect("new owned test root"),
    );
    assert!(requested.is_absolute() && !requested.exists());
    fs::create_dir(&requested).unwrap();
    fs::set_permissions(&requested, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(requested).unwrap();
    assert!(
        fs2::available_space(&root).unwrap() > 14 * 1024 * 1024 * 1024,
        "bounded fixture peak8GiB+retained floor6GiB"
    );
    let source = InstallationController::new(source_profile.clone(), None).unwrap();
    let context = source.context().unwrap();
    let token = &context.selection_token;
    let source_root = PathBuf::from(
        &context
            .installations
            .iter()
            .find(|v| v.id == context.active_id)
            .unwrap()
            .path,
    );
    let prepared = source
        .with_current(token, |s| s.native_prepared())
        .unwrap()
        .unwrap();
    assert_eq!(prepared.kind, "personal");
    let db_allocation = allocated(&source_root.join("native-database"));
    let blob_allocation = allocated(&source_root.join("blobs"));
    assert!(
        db_allocation <= 512 * 1024 * 1024 && blob_allocation <= 16 * 1024 * 1024,
        "fixture scale cap, not general old-backup sizing proof"
    );
    let source_env = fs::read(source_root.join("runtime.env")).unwrap();
    let source_owner = fs::read(source_root.join("native-personal.json")).unwrap();
    let signing = fs::read(source_root.join("config/freeze-signing-key.json")).unwrap();
    let document = source.native_document(token).unwrap();
    assert!(document.draft.is_some());
    let upload = source.native_upload(token).unwrap();
    let upload = source.save_native_upload(token, upload).unwrap();
    let publication = source.native_publication(token).unwrap();
    let publication = source.save_native_publication(token, publication).unwrap();
    let keys = root.join("keys");
    fs::create_dir(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    let key = keys.join("correct.bin");
    let wrong = keys.join("wrong.bin");
    make_key(&key);
    make_key(&wrong);
    let correct_key_witness = file_witness(&key);
    let wrong_key_witness = file_witness(&wrong);
    let archive = root.join("service-backup");
    let mut running = source
        .with_current_mutation(token, |s| Session::new(s, payload.clone(), prepared.ports))
        .unwrap();
    running.start_database_only().unwrap();
    let backup = source
        .create_native_backup(
            token,
            &mut running,
            native_backup::Input {
                key_file: key.clone(),
                destination: archive.clone(),
            },
        )
        .unwrap();
    assert_eq!(backup.state, "completed");
    let source_stopped = running.stop().unwrap();
    assert_eq!(source_stopped.exits.get("postgres"), Some(&0));
    drop(running);
    // The source PG is fully stopped before the physical witness. Restoration
    // must not touch even its hidden retained tree, keys or profile journals.
    let source_database_witness = tree_witness(&source_root.join("native-database"));
    let source_journals: BTreeMap<_, _> = fs::read_dir(&source_profile)
        .unwrap()
        .map(|v| v.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("native-")
                && p.is_file()
        })
        .map(|p| {
            let witness = file_witness(&p);
            (p, witness)
        })
        .collect();
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(source_root.join(format!(
            "native-backup-{}/verified/manifest.json",
            backup.id
        )))
        .unwrap(),
    )
    .unwrap();
    let objects: Vec<(String, Vec<u8>)> = manifest["inventory"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let key = v["key"].as_str().unwrap().to_owned();
            let bytes = fs::read(source_root.join("blobs").join(&key)).unwrap();
            assert_eq!(hash(&bytes), v["sha256"].as_str().unwrap());
            (key, bytes)
        })
        .collect();
    assert!(!objects.is_empty());
    let encrypted_manifest = fs::read(archive.join("manifest.gcm")).unwrap();
    let archive_witness = tree_witness(&archive);
    let archive_allocated = allocated(&archive);
    let dump_bytes = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["role"] == "database")
        .unwrap()["bytes"]
        .as_u64()
        .unwrap();
    drop(source);
    let retained = source_root.with_file_name(format!(
        "{}-retained-unavailable",
        source_root.file_name().unwrap().to_string_lossy()
    ));
    assert!(!retained.exists());
    fs::rename(&source_root, &retained).unwrap();
    let hidden = Hidden {
        original: source_root.clone(),
        retained,
    };
    let target = InstallationController::new(root.join("target-profile"), None).unwrap();
    let target_context = target.context().unwrap();
    let target_token = &target_context.selection_token;
    let target_root = PathBuf::from(
        &target_context
            .installations
            .iter()
            .find(|v| v.id == target_context.active_id)
            .unwrap()
            .path,
    );
    let target_ports = ports();
    target
        .with_current_mutation(target_token, |s| {
            s.prepare_native_personal(&payload, target_ports)
        })
        .unwrap();
    let mut candidate = target
        .with_current_mutation(target_token, |s| {
            Session::new(s, payload.clone(), target_ports)
        })
        .unwrap();
    candidate.start_database_only().unwrap();
    assert!(
        target
            .stage_native_restore(
                target_token,
                &mut candidate,
                native_restore::Input {
                    source: archive.clone(),
                    key_file: wrong.clone()
                }
            )
            .is_err()
    );
    let pending: serde_json::Value =
        serde_json::from_slice(&fs::read(target_root.join("native-restore-current.json")).unwrap())
            .unwrap();
    let id = pending["id"].as_str().unwrap().to_owned();
    assert_eq!(candidate.status().unwrap().state, "stopped");
    candidate.start_database_only().unwrap();
    let staged = target
        .stage_native_restore(
            target_token,
            &mut candidate,
            native_restore::Input {
                source: archive.clone(),
                key_file: key.clone(),
            },
        )
        .unwrap();
    assert_eq!(staged.id, id);
    assert_eq!(
        candidate.start().err().unwrap().code,
        "NATIVE_RESTORE_ACTIVATION_REQUIRED"
    );
    let job = target_root.join(format!("native-restore-{id}"));
    let first_stage = fs::read(job.join("stage.json")).unwrap();
    // Preserve a completed codec result but model loss of the Manager stage
    // checkpoint. Reentry must authenticate/reconcile, never replay pg_restore.
    fs::rename(job.join("stage.json"), job.join("stage-before-fault.json")).unwrap();
    let resumed = target
        .stage_native_restore(
            target_token,
            &mut candidate,
            native_restore::Input {
                source: archive.clone(),
                key_file: key.clone(),
            },
        )
        .unwrap();
    assert_eq!(resumed.id, id);
    let second: serde_json::Value =
        serde_json::from_slice(&fs::read(job.join("stage.json")).unwrap()).unwrap();
    let first: serde_json::Value = serde_json::from_slice(&first_stage).unwrap();
    assert_ne!(second["attempt"], first["attempt"]);
    let attempt = second["attempt"].as_str().unwrap();
    let attempt_record: serde_json::Value =
        serde_json::from_slice(&fs::read(job.join(format!("attempt-{attempt}.json"))).unwrap())
            .unwrap();
    assert_eq!(attempt_record["operation"], "reconcile");
    let candidate_stopped = candidate.stop().unwrap();
    assert_eq!(candidate_stopped.exits.get("postgres"), Some(&0));
    drop(candidate);
    // Model interruption after only the first atomic configuration install.
    // Retain the original target bytes, then activate the same durable plan.
    fs::rename(
        target_root.join("runtime.env"),
        job.join("target-env-before-fault.bin"),
    )
    .unwrap();
    fs::copy(
        job.join(format!("activation-{attempt}/runtime.env")),
        target_root.join("runtime.env"),
    )
    .unwrap();
    fs::set_permissions(
        target_root.join("runtime.env"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let activated = target
        .activate_native_restore(target_token, &id, payload.clone())
        .unwrap();
    assert_eq!(activated.id, id);
    assert_eq!(activated.state, "activated");
    assert!(!target_root.join("native-restore-current.json").exists());
    assert_eq!(
        serde_json::to_value(target.native_document(target_token).unwrap()).unwrap(),
        serde_json::to_value(document).unwrap()
    );
    assert_eq!(
        serde_json::to_value(target.native_upload(target_token).unwrap()).unwrap(),
        serde_json::to_value(upload).unwrap()
    );
    assert_eq!(
        serde_json::to_value(target.native_publication(target_token).unwrap()).unwrap(),
        serde_json::to_value(publication).unwrap()
    );
    assert_eq!(
        fs::read(target_root.join("config/freeze-signing-key.json")).unwrap(),
        signing
    );
    for (key, bytes) in &objects {
        assert_eq!(
            &fs::read(target_root.join("blobs").join(key)).unwrap(),
            bytes
        );
    }
    let mut reopened = target
        .with_current_mutation(target_token, |s| {
            Session::new(s, payload.clone(), target_ports)
        })
        .unwrap();
    let ready = reopened.start().unwrap();
    assert!(ready.readiness.ready);
    let authority = reopened.authoring_session().unwrap();
    let owner: serde_json::Value = serde_json::from_slice(&source_owner).unwrap();
    assert_eq!(authority["tenantId"], owner["tenantId"]);
    let restored_owner: serde_json::Value =
        serde_json::from_slice(&fs::read(target_root.join("native-personal.json")).unwrap())
            .unwrap();
    assert_eq!(restored_owner["workspaceId"], owner["workspaceId"]);
    assert_eq!(authority["principalId"], owner["principalId"]);
    let stopped = reopened.stop().unwrap();
    assert_eq!(stopped.exits.get("postgres"), Some(&0));
    assert_eq!(stopped.exits.get("node"), Some(&0));
    drop(reopened);
    drop(target);
    assert_eq!(tree_witness(&archive), archive_witness);
    assert_eq!(file_witness(&key), correct_key_witness);
    assert_eq!(file_witness(&wrong), wrong_key_witness);
    assert_eq!(
        fs::read(archive.join("manifest.gcm")).unwrap(),
        encrypted_manifest
    );
    drop(hidden);
    assert_eq!(
        tree_witness(&source_root.join("native-database")),
        source_database_witness
    );
    assert_eq!(
        fs::read(source_root.join("config/freeze-signing-key.json")).unwrap(),
        signing
    );
    for (path, witness) in &source_journals {
        assert_eq!(&file_witness(path), witness);
    }
    assert_eq!(
        fs::read(source_root.join("runtime.env")).unwrap(),
        source_env
    );
    assert_eq!(
        fs::read(source_root.join("native-personal.json")).unwrap(),
        source_owner
    );
    for (key, bytes) in &objects {
        assert_eq!(
            &fs::read(source_root.join("blobs").join(key)).unwrap(),
            bytes
        );
    }
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"ACTUAL_BOUNDED_NATIVE_BACKUP_RESTORE_CORE_NOT_GUI_LEGACY_UPDATE_OR_RELEASE","backupId":backup.backup_id,"restoreId":id,"sourceUnavailableDuringRestore":true,"wrongKeySameIdRetry":true,"lostStageReconciledWithoutPGRestore":true,"partialActivationResumed":true,"ownerConfigSigningJournalsAndBlobsPreserved":true,"allCipherFilesAndBothExternalKeysUnchanged":true,"stoppedSourcePhysicalDatabaseUnchanged":true,"sourceDatabaseWitnessEntries":source_database_witness.len(),"sourceJournalFilesUnchanged":source_journals.len(),"archiveWitnessEntries":archive_witness.len(),"normalNativeReopenReady":true,"fixtureDatabaseAllocatedBytes":db_allocation,"fixtureBlobAllocatedBytes":blob_allocation,"authenticatedDumpBytes":dump_bytes,"encryptedArchiveAllocatedBytes":archive_allocated,"expectedPeakBytes":8u64*1024*1024*1024,"retainedFloorBytes":6u64*1024*1024*1024,"sourcePostgresExit":source_stopped.exits.get("postgres"),"targetNodeExit":stopped.exits.get("node"),"targetPostgresExit":stopped.exits.get("postgres")})).unwrap()).unwrap();
}
