// SPDX-License-Identifier: Apache-2.0
//! Explicit native synthetic qualification; never enables public migration admission.
use super::*;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};

fn input() -> Value {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(std::env::var("EXHIBITOS_WHOLE_CRASH_INPUT").unwrap())
        .unwrap()
        .take(65537)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 65536, "qualification input exceeds bound");
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    if value.get("freshAuthority").is_some() {
        fresh_publication_authority::parse_input(&bytes).unwrap();
    }
    value
}

fn path(v: &Value, name: &str) -> PathBuf {
    PathBuf::from(v[name].as_str().unwrap())
}
fn checked_store(v: &Value) -> crate::signed_release::trust::Store {
    let root = fs::canonicalize(path(v, "root")).unwrap();
    assert_eq!(
        root.file_name().unwrap(),
        "exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a"
    );
    let store =
        crate::signed_release::trust::Store::open(&root.join("profile"), "default").unwrap();
    let public = ed25519_dalek::SigningKey::from_bytes(&[31; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(store.current.policy.public_keys, vec![public]);
    let intent = store.intent().unwrap();
    assert_eq!(intent.update.stage(), crate::update::Stage::Prepared);
    assert!(matches!(
        (
            store.current.generation,
            intent.update.plan().operation_id.as_str()
        ),
        (21, "c5b5becd-4f70-435f-909e-73031e5def37")
            | (29, "2a517fd1-e818-4f90-8e4f-73c9fe7b976b")
            | (37, "a9ccf7b0-7e3f-4bae-bcf1-3f2c53e15c49")
    ));
    let envelope: crate::signed_release::Envelope = serde_json::from_str(&intent.envelope).unwrap();
    let release: crate::signed_release::Release = serde_json::from_str(&envelope.payload).unwrap();
    assert_eq!(release.channel, "development");
    assert_eq!(release.target, "linux-arm64");
    assert_ne!(
        intent.update.plan().source_schema,
        intent.update.plan().target_schema
    );
    assert_eq!(
        require_executable_schema(intent.update.plan())
            .unwrap_err()
            .code,
        "UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"
    );
    store
}

#[path = "fresh_publication_authority.rs"]
mod fresh_publication_authority;
use fresh_publication_authority::{FreshAuthority, Observation};

fn private_digest(path: &Path, limit: u64) -> String {
    private_digest_observed(path, limit, |_| {})
}
fn private_digest_observed(path: &Path, limit: u64, mut after_chunk: impl FnMut(u64)) -> String {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let before = fs::symlink_metadata(path).unwrap();
    assert!(
        before.is_file()
            && before.nlink() == 1
            && before.mode() & 0o077 == 0
            && before.uid() == unsafe { libc::geteuid() }
            && before.len() <= limit
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .unwrap();
    let unchanged = |m: &fs::Metadata| {
        before.dev() == m.dev()
            && before.ino() == m.ino()
            && before.len() == m.len()
            && before.mtime() == m.mtime()
            && before.mtime_nsec() == m.mtime_nsec()
            && before.ctime() == m.ctime()
            && before.ctime_nsec() == m.ctime_nsec()
            && before.mode() == m.mode()
            && before.nlink() == m.nlink()
    };
    assert!(unchanged(&file.metadata().unwrap()));
    let mut digest = sha2::Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        bytes += n as u64;
        assert!(bytes <= before.len());
        digest.update(&buffer[..n]);
        after_chunk(bytes);
    }
    assert_eq!(bytes, before.len());
    assert!(
        unchanged(&file.metadata().unwrap()) && unchanged(&fs::symlink_metadata(path).unwrap())
    );
    format!("{:x}", digest.finalize())
}

// Native qualification only, constrained to the already protected synthetic root.
// Every parent and child checks fresh exact authority before creating anything.
fn checked_fresh_publication_store(v: &Value, child: bool) -> crate::signed_release::trust::Store {
    let expected: FreshAuthority = serde_json::from_value(v["freshAuthority"].clone()).unwrap();
    let root = fs::canonicalize(path(v, "root")).unwrap();
    assert_eq!(
        root.file_name().unwrap(),
        "exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a"
    );
    let store =
        crate::signed_release::trust::Store::open_mode(&root.join("profile"), "default", false)
            .unwrap();
    let intent = store.intent().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&source)
        .output()
        .unwrap();
    assert!(head.status.success());
    let source_commit = String::from_utf8(head.stdout).unwrap();
    let clean = Command::new("git")
        .args(["diff", "--quiet", "HEAD", "--"])
        .current_dir(&source)
        .status()
        .unwrap();
    expected
        .check(&Observation {
            generation: store.current.generation,
            head: &store.current_sha256,
            scope: &store.scope,
            plan: intent.update.plan(),
            stage: intent.update.stage(),
            public_keys: &store.current.policy.public_keys,
            manager_source_commit: source_commit.trim(),
            dirty_source: !clean.success(),
        })
        .unwrap();
    assert_eq!(
        v["publicationPhase"].as_str().unwrap(),
        expected.publication_phase
    );
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(intent.envelope.as_bytes())),
        expected.envelope_sha256
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut verified = store
        .verify_for_preparation(intent.envelope.as_bytes(), now)
        .unwrap();
    assert!(verified.binds(intent.update.plan()));
    assert_eq!(verified.release().channel, "development");
    assert_eq!(verified.release().target, "linux-arm64");
    assert_eq!(verified.release().artifact.sha256, expected.artifact_sha256);
    assert_eq!(
        private_digest(&path(v, "artifact"), verified.release().artifact.bytes),
        expected.artifact_sha256
    );
    // Independently retain the normal exact signed size/sha verification too.
    verified
        .verify_artifact(&mut fs::File::open(path(v, "artifact")).unwrap())
        .unwrap();
    assert_eq!(
        private_digest(&path(v, "catalog"), 16 * 1024 * 1024),
        expected.catalog_sha256
    );
    assert_eq!(
        v["catalogSha256"].as_str().unwrap(),
        expected.catalog_sha256
    );
    let (registry, original_selection) = installations::load(&store.profile).unwrap().unwrap();
    assert_eq!(registry.active_id, expected.plan.source_instance);
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&original_selection)),
        expected.original_selection_sha256
    );
    let point = fs::canonicalize(path(v, "point")).unwrap();
    assert_eq!(point.parent(), store.profile.parent());
    assert_eq!(
        point.file_name().unwrap().to_str().unwrap(),
        format!(
            "native-publication-{}-{}",
            expected.publication_phase, expected.plan.operation_id
        )
    );
    // One operation cannot be reused for another phase, even if authority files
    // were manually put back. Old successful/failed points remain evidence.
    let operation_suffix = format!("-{}", expected.plan.operation_id);
    for entry in fs::read_dir(root.as_path()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        if name.ends_with(&operation_suffix)
            && (name.starts_with("native-publication-") || name.starts_with("checkpoint-"))
        {
            let current_point = format!(
                "native-publication-{}{}",
                expected.publication_phase, operation_suffix
            );
            let current_checkpoint = format!(
                "checkpoint-{}{}",
                expected.publication_phase, operation_suffix
            );
            assert!(
                name == current_point || name == current_checkpoint,
                "operation already reserved for another publication phase"
            );
        }
    }
    let allowed = [
        "storage-budget-before-native-publication.json",
        "phase-budget-before-native-publication.json",
        "child.out",
        "child.err",
    ];
    for entry in fs::read_dir(&point).unwrap() {
        let entry = entry.unwrap();
        assert!(
            child && allowed.contains(&entry.file_name().to_str().unwrap()),
            "new native point must not reuse any previous outputs"
        );
    }
    let checkpoint = fs::canonicalize(path(v, "binding").parent().unwrap()).unwrap();
    assert_eq!(checkpoint.parent(), store.profile.parent());
    assert_eq!(
        checkpoint.file_name().unwrap().to_str().unwrap(),
        format!(
            "checkpoint-{}-{}",
            expected.publication_phase, expected.plan.operation_id
        )
    );
    for (name, digest) in [
        ("binding", &expected.binding_sha256),
        ("hostArchive", &expected.host_archive_sha256),
        ("trustArchive", &expected.trust_archive_sha256),
    ] {
        let file = path(v, name);
        assert_eq!(
            fs::canonicalize(file.parent().unwrap()).unwrap(),
            checkpoint
        );
        assert_eq!(&private_digest(&file, 64 * 1024 * 1024 * 1024), digest);
    }
    // Store retains the exclusive trust.lock (normal writers cannot advance it).
    // Also reobserve actual disk bytes/head rather than trusting cached fields.
    let latest = fs::read_dir(&store.root)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_str().unwrap();
            if name.len() == 25
                && name.ends_with(".json")
                && name[..20].bytes().all(|b| b.is_ascii_digit())
            {
                Some(name[..20].parse::<u64>().unwrap())
            } else {
                None
            }
        })
        .max()
        .unwrap();
    assert_eq!(latest, expected.generation);
    let disk_bytes =
        crate::signed_release::trust::read_record(&store.root.join(format!("{:020}.json", latest)))
            .unwrap();
    let disk: crate::signed_release::trust::Record = serde_json::from_slice(&disk_bytes).unwrap();
    crate::signed_release::trust::valid_record(&disk, &store.scope).unwrap();
    let disk_head = format!("{:x}", sha2::Sha256::digest(&disk_bytes));
    let disk_intent = disk.intent.as_ref().unwrap();
    expected
        .check(&Observation {
            generation: disk.generation,
            head: &disk_head,
            scope: &disk.scope,
            plan: disk_intent.update.plan(),
            stage: disk_intent.update.stage(),
            public_keys: &disk.policy.public_keys,
            manager_source_commit: source_commit.trim(),
            dirty_source: false,
        })
        .unwrap();
    assert_eq!(disk_head, store.current_sha256);
    // Time and selection may change during large read-only archive checks.
    // Reobserve immediately before returning to either caller's first writes.
    let late_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    store
        .verify_for_preparation(intent.envelope.as_bytes(), late_now)
        .unwrap();
    assert_eq!(
        installations::load(&store.profile).unwrap().unwrap().1,
        original_selection
    );
    let late_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&source)
        .output()
        .unwrap();
    assert!(late_head.status.success());
    assert_eq!(
        String::from_utf8(late_head.stdout).unwrap().trim(),
        expected.manager_source_commit
    );
    assert!(
        Command::new("git")
            .args(["diff", "--quiet", "HEAD", "--"])
            .current_dir(&source)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        require_executable_schema(intent.update.plan())
            .unwrap_err()
            .code,
        "UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"
    );
    store
}

fn checked_resuming_store(v: &Value) -> crate::signed_release::trust::Store {
    let root = fs::canonicalize(path(v, "root")).unwrap();
    assert_eq!(
        root.file_name().unwrap(),
        "exhibitos-release-trust-817e82d0-bfe4-413e-bb1c-d8c252401d8a"
    );
    let store =
        crate::signed_release::trust::Store::open(&root.join("profile"), "default").unwrap();
    let public = ed25519_dalek::SigningKey::from_bytes(&[31; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(store.current.policy.public_keys, vec![public]);
    let intent = store.intent().unwrap();
    assert_eq!(
        intent.update.stage(),
        crate::update::Stage::RecoveryRequired
    );
    assert!(matches!(
        (
            store.current.generation,
            intent.update.plan().operation_id.as_str()
        ),
        (32, "2a517fd1-e818-4f90-8e4f-73c9fe7b976b")
    ));
    let envelope: crate::signed_release::Envelope = serde_json::from_str(&intent.envelope).unwrap();
    let release: crate::signed_release::Release = serde_json::from_str(&envelope.payload).unwrap();
    assert_eq!(release.channel, "development");
    assert_eq!(release.target, "linux-arm64");
    assert_ne!(
        intent.update.plan().source_schema,
        intent.update.plan().target_schema
    );
    assert_eq!(
        require_executable_schema(intent.update.plan())
            .unwrap_err()
            .code,
        "UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"
    );
    store
}

#[test]
#[ignore = "child only: explicit signed allowlisted synthetic fixture, actual owned Applying/application; waits for parent SIGKILL"]
fn actual_changed_executor_crash_child() {
    let v = input();
    let publication = v.get("freshAuthority").is_some();
    assert!(
        publication
            || (v.get("publicationPhase").is_none()
                && std::env::var_os("EXHIBITOS_PUBLICATION_CRASH_PHASE").is_none()),
        "publication child requires fresh exact authority"
    );
    let mut store = if publication {
        checked_fresh_publication_store(&v, true)
    } else {
        checked_store(&v)
    };
    let profile = store.profile.clone();
    let operation_id = store.intent().unwrap().update.plan().operation_id.clone();
    let point = fs::canonicalize(path(&v, "point")).unwrap();
    assert_eq!(point.parent(), profile.parent());
    let marker = point.join("candidate-applied.json");
    assert!(!marker.exists());
    let key_file = path(&v, "key");
    let key: [u8; 32] = super::super::super::super::read_record(&key_file)
        .unwrap()
        .try_into()
        .unwrap();
    let binding = path(&v, "binding");
    let host = path(&v, "hostArchive");
    let trust = path(&v, "trustArchive");
    let python = path(&v, "python");
    let catalog = path(&v, "catalog");
    let retained_host = path(&v, "retainedHost");
    let exports = point.join("exports");
    let staging = point.join("staging");
    installations::new_directory(&exports).unwrap();
    installations::new_directory(&staging).unwrap();
    let destination = point.join("inactive-authority");
    let inputs = RecoveryRuntimeInputs {
        checkpoint: CheckpointInputs {
            binding: &binding,
            host: &host,
            trust: &trust,
            key: &key,
        },
        export_parent: &exports,
        python: &python,
        source_commit: v["sourceCommit"].as_str().unwrap(),
        maintenance_image: v["maintenance"].as_str().unwrap(),
        external_writers_quiesced: true,
    };
    let migration = MigrationRuntimeInputs {
        catalog: &catalog,
        catalog_sha256: v["catalogSha256"].as_str().unwrap(),
    };
    let mut session = store.execution().unwrap();
    let mut artifact = session
        .stage_prepared_artifact(&path(&v, "artifact"), &staging)
        .unwrap();
    let permit = session
        .prepare_owned_migrated_update_reusing_host(
            &mut artifact,
            &inputs,
            &migration,
            &retained_host,
            &destination,
            &key_file,
        )
        .unwrap();
    // The exact private production body is qualified; public begin remains closed.
    let started = permit.begin_journaled().unwrap();
    let ready = started.apply_candidate().unwrap();
    let proof = serde_json::json!({"pid":std::process::id(),"operationId":operation_id,"candidateApplied":true,"receipt":ready.receipt(),"health":ready.observed_health()});
    crate::restoration::private_bytes(&marker, &serde_json::to_vec(&proof).unwrap()).unwrap();
    if publication {
        // Only this fresh allowlisted operation reaches actual native publication.
        let phase = std::env::var("EXHIBITOS_PUBLICATION_CRASH_PHASE").unwrap();
        assert!(matches!(
            phase.as_str(),
            "pending-synced" | "selection-renamed" | "selection-dir-synced"
        ));
        ready.activate().unwrap();
        panic!("native publication hook did not stop this fresh worker");
    }
    // Keep all actual fences/opaque permits alive until the parent kills this PID.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::hint::black_box(&ready);
    }
}

fn owned_engine_state(profile: &Path, id: &str) -> Vec<u8> {
    let (registry, _) = installations::load(profile).unwrap().unwrap();
    let entry = registry.installations.iter().find(|e| e.id == id).unwrap();
    let service =
        crate::LifecycleService::open_retry_diagnostics(installations::root(profile, entry))
            .unwrap();
    let manifest = service.manifest().unwrap();
    service.validate_ownership(&manifest, "docker").unwrap();
    service.validate_volumes(&manifest, "docker").unwrap();
    let ids = crate::run(
        "docker",
        &[
            "ps".into(),
            "-aq".into(),
            "--filter".into(),
            format!("label=com.docker.compose.project={}", manifest.project_name),
        ],
        None,
        30,
    )
    .unwrap();
    let mut ids = std::str::from_utf8(&ids)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids.len(), 2);
    let mut state = Vec::new();
    for id in ids {
        let raw = crate::run(
            "docker",
            &[
                "inspect".into(),
                "--format".into(),
                "{{.Id}} {{json .State}} {{json .Mounts}}".into(),
                id,
            ],
            None,
            30,
        )
        .unwrap();
        state.extend_from_slice(&raw);
    }
    state
}

#[test]
#[ignore = "fresh exact signed authority only: actual native candidate migration/health/publication, SIGKILL boundary and cold selection reconciliation; never enables public admission"]
fn actual_native_publication_sigkill_and_cold_selection() {
    use std::os::unix::fs::PermissionsExt;
    let v = input();
    let store = checked_fresh_publication_store(&v, false);
    let profile = store.profile.clone();
    let plan = store.intent().unwrap().update.plan().clone();
    let point = fs::canonicalize(path(&v, "point")).unwrap();
    assert_eq!(point.parent(), profile.parent());
    let phase = v["publicationPhase"].as_str().unwrap();
    assert!(matches!(
        phase,
        "pending-synced" | "selection-renamed" | "selection-dir-synced"
    ));
    let marker = point.join("publication.marker");
    assert!(absent(&marker) && absent(&point.join("candidate-applied.json")));
    let original_selection = installations::load(&profile).unwrap().unwrap().1;
    let source = installations::root(
        &profile,
        installations::load(&profile)
            .unwrap()
            .unwrap()
            .0
            .installations
            .iter()
            .find(|e| e.id == plan.source_instance)
            .unwrap(),
    );
    let originals = [
        "installed.json",
        "engine.json",
        "runtime.env",
        "bundle/manifest.json",
        "bundle/compose.yaml",
    ]
    .into_iter()
    .map(|n| {
        let p = source.join(n);
        let bytes = fs::read(&p).unwrap();
        let mode = fs::metadata(&p).unwrap().permissions().mode();
        (p, bytes, mode)
    })
    .collect::<Vec<_>>();
    let records = fs::read_dir(&store.root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect::<Vec<_>>();
    let key = fs::read(path(&v, "key")).unwrap();
    // Three image exports, one512MiB native DB copy, artifact stage+OCI,
    // metadata allowance, signed headroom and6GiB floor remain conservative.
    require_phase_capacity(
        &profile,
        &point,
        "before-native-publication",
        781_992_960 + 536_870_912 + 187_522_048 + 67_108_864,
        plan.required_free_bytes,
    );
    drop(store);
    let out =
        crate::restoration::private_bytes(&point.join("child.out"), b"native publication worker\n");
    out.unwrap();
    let errfile = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(point.join("child.err"))
        .unwrap();
    let worker = format!(
        "{}::actual_changed_executor_crash_child",
        module_path!().split_once("::").unwrap().1
    );
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &worker, "--ignored", "--nocapture"])
            .env("EXHIBITOS_PUBLICATION_CRASH_PHASE", phase)
            .env("EXHIBITOS_PUBLICATION_CRASH_MARKER", &marker)
            .stdout(Stdio::from(
                fs::OpenOptions::new()
                    .append(true)
                    .open(point.join("child.out"))
                    .unwrap(),
            ))
            .stderr(Stdio::from(errfile))
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1800);
    while !fs::read(&marker).is_ok_and(|bytes| bytes == phase.as_bytes()) {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "native worker exited; preserve full logs and candidate"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "native publication timeout; preserve recovery inputs"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let applied: Value =
        serde_json::from_slice(&fs::read(point.join("candidate-applied.json")).unwrap()).unwrap();
    assert_eq!(applied["pid"].as_u64(), Some(u64::from(child.0.id())));
    assert_eq!(applied["operationId"], plan.operation_id);
    assert_eq!(applied["candidateApplied"], true);
    let selected = installations::load(&profile).unwrap().unwrap().0.active_id;
    assert_eq!(
        selected,
        if phase == "pending-synced" {
            &plan.source_instance
        } else {
            &plan.target_instance
        }
        .as_str()
    );
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    let mut cold = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
    assert_eq!(
        cold.intent().unwrap().update.stage(),
        crate::update::Stage::RecoveryRequired
    );
    assert_eq!(cold.intent().unwrap().update.plan(), &plan);
    // Only stop the exact migrated candidate; no original writer is started.
    stop_owned(&profile, &plan.target_instance).unwrap();
    let original_engine = owned_engine_state(&profile, &plan.source_instance);
    let candidate_engine = owned_engine_state(&profile, &plan.target_instance);
    let generation = cold.current.generation;
    let receipt = cold
        .reconcile_selection_activation(&plan.operation_id)
        .unwrap();
    assert!(receipt.original_selection_restored && !receipt.selection_completed);
    assert_eq!(
        installations::load(&profile).unwrap().unwrap().1,
        original_selection
    );
    assert_eq!(
        owned_engine_state(&profile, &plan.source_instance),
        original_engine
    );
    assert_eq!(
        owned_engine_state(&profile, &plan.target_instance),
        candidate_engine
    );
    assert_eq!(cold.current.generation, generation);
    for (p, bytes, mode) in originals {
        assert_eq!(fs::read(&p).unwrap(), bytes);
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode(), mode);
    }
    for (p, bytes) in records {
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
    assert_eq!(fs::read(path(&v, "key")).unwrap(), key);
    drop(cold);
    let reopened = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
    assert_eq!(reopened.current.generation, generation);
    assert_eq!(
        reopened.intent().unwrap().update.stage(),
        crate::update::Stage::RecoveryRequired
    );
    assert_eq!(
        installations::load(&profile).unwrap().unwrap().1,
        original_selection
    );
    let report = serde_json::json!({"state":"PASS","publicationPhase":phase,"executorSignal":9,"actualCandidateApplicationAndHealth":applied,"selectionBeforeKill":selected,"coldRecoveryGeneration":generation,"selectionReconciliation":receipt,"exactOriginalSelectionRestored":true,"engineAndHealthReplayed":false,"originalSourceKeyHistoryPreserved":true,"candidateStopped":true,"wholeOriginalDataRestorationVerified":false,"publicChangedSchemaGate":"CLOSED","otherPublicationPhasesVerified":false});
    crate::restoration::private_bytes(
        &point.join("report.json"),
        &serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn stop_owned(profile: &Path, id: &str) -> crate::Result<()> {
    let (registry, _) =
        installations::load(profile)?.ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
    let entry = registry
        .installations
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
    let service =
        crate::LifecycleService::open_retry_diagnostics(installations::root(profile, entry))?;
    let manifest = service.manifest()?;
    service.validate_ownership(&manifest, "docker")?;
    service.validate_volumes(&manifest, "docker")?;
    crate::run(
        "docker",
        &crate::compose_args(&manifest, &["stop", "--timeout", "30"]),
        Some(&service.root.join("bundle")),
        180,
    )?;
    Ok(())
}

#[test]
#[ignore = "explicit current21 synthetic fixture: SIGKILL whole executor after actual candidate apply, cold interruption, fresh original restore/native health/selection"]
fn actual_whole_executor_sigkill_and_original_recovery() {
    run_whole_recovery(false, false);
}

#[test]
#[ignore = "explicit current29 synthetic fixture: actual executor SIGKILL, both primary namespaces absent, independent authority then historical host then fresh original service recovery"]
fn actual_whole_executor_sigkill_missing_host_authority_and_original_recovery() {
    run_whole_recovery(true, false);
}

#[test]
#[ignore = "explicit same32/operation fixture only: resume actual verified SIGKILL after unused extraction retirement failure; never begin or reapply"]
fn actual_combined_loss_resume_after_verified_sigkill() {
    run_whole_recovery(true, true);
}

fn run_whole_recovery(lose_namespaces: bool, resume: bool) {
    use std::os::unix::fs::PermissionsExt;
    let v = input();
    let store = if resume {
        checked_resuming_store(&v)
    } else {
        checked_store(&v)
    };
    let profile = store.profile.clone();
    let authority = store.root.clone();
    let plan = store.intent().unwrap().update.plan().clone();
    let generation = if resume { 29 } else { store.current.generation };
    if lose_namespaces {
        assert_eq!(generation, 29);
        assert_eq!(plan.operation_id, "2a517fd1-e818-4f90-8e4f-73c9fe7b976b");
    }
    let initial_key: [u8; 32] = fs::read(path(&v, "key")).unwrap().try_into().unwrap();
    let initial_checkpoint = if lose_namespaces {
        Some(if resume {
            store
                .verify_rollback_checkpoint_pair(
                    &path(&v, "binding"),
                    &path(&v, "hostArchive"),
                    &path(&v, "trustArchive"),
                    &initial_key,
                )
                .unwrap()
                .receipt()
                .checkpoint
        } else {
            store
                .verify_checkpoint_pair(
                    &path(&v, "binding"),
                    &path(&v, "hostArchive"),
                    &path(&v, "trustArchive"),
                    &initial_key,
                )
                .unwrap()
                .receipt()
        })
    } else {
        None
    };
    let records = fs::read_dir(&authority)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect::<Vec<_>>();
    let (registry, _) = installations::load(&profile).unwrap().unwrap();
    assert_eq!(registry.active_id, plan.source_instance);
    let entry = registry
        .installations
        .iter()
        .find(|e| e.id == plan.source_instance)
        .unwrap();
    let source = installations::root(&profile, entry);
    let originals = [
        "installed.json",
        "engine.json",
        "runtime.env",
        "bundle/manifest.json",
        "bundle/compose.yaml",
    ]
    .into_iter()
    .map(|n| {
        let p = source.join(n);
        let b = fs::read(&p).unwrap();
        let m = fs::metadata(&p).unwrap().permissions().mode();
        (p, b, m)
    })
    .collect::<Vec<_>>();
    let key_file = path(&v, "key");
    let original_key = fs::read(&key_file).unwrap();
    let point = fs::canonicalize(path(&v, "point")).unwrap();
    assert_eq!(point.parent(), profile.parent());
    let marker = if resume {
        let previous: Value =
            serde_json::from_slice(&fs::read(point.join("runner-report.json")).unwrap()).unwrap();
        assert_eq!(previous["state"], "FAIL");
        assert_eq!(
            previous["source"],
            "af3fa840f0c8a374f698c995e14b2e024c867d98"
        );
        assert_eq!(previous["exitCode"], 101);
        assert!(!point.join("both-namespaces-absent.json").exists());
        // That exact prior binary reached this unique guard only after asserting
        // real candidate application, child SIGKILL9 and cold RecoveryRequired.
        let failure = fs::read_to_string(point.join("parent.err")).unwrap();
        assert!(failure.contains("unused host retirement refused; original namespaces preserved"));
        let marker: Value =
            serde_json::from_slice(&fs::read(point.join("candidate-applied.json")).unwrap())
                .unwrap();
        assert_eq!(marker["operationId"], plan.operation_id);
        assert_eq!(marker["candidateApplied"], true);
        let retirement: Value = serde_json::from_slice(
            &fs::read(path(&v, "retirementReceipt").join("receipt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(retirement["state"], "PASS");
        assert_eq!(retirement["retired"], true);
        assert_eq!(
            retirement["manifest_sha256"],
            initial_checkpoint.as_ref().unwrap().host_manifest_sha256
        );
        assert!(!path(&v, "retainedHost").join("profile").exists());
        drop(store);
        marker
    } else {
        assert!(!point.join("candidate-applied.json").exists());
        if lose_namespaces {
            require_phase_capacity(
                &profile,
                &point,
                "before-child",
                781_992_960 + 187_522_048 + 16_777_216,
                plan.required_free_bytes,
            );
        }
        drop(store);
        let out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(point.join("child.out"))
            .unwrap();
        let errout = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(point.join("child.err"))
            .unwrap();
        let child_test = format!(
            "{}::actual_changed_executor_crash_child",
            module_path!().split_once("::").unwrap().1
        );
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &child_test, "--ignored", "--nocapture"])
                .stdout(Stdio::from(out))
                .stderr(Stdio::from(errout))
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1200);
        let marker = loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "qualification child exited before durable application marker; retain child logs"
            );
            if let Ok(bytes) = fs::read(point.join("candidate-applied.json"))
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            {
                break value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "application marker timed out; own child is stopped by guard and all recovery data retained"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(child.0.id()));
        assert_eq!(marker["operationId"], plan.operation_id);
        assert_eq!(marker["candidateApplied"], true);
        child.0.kill().unwrap();
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(9));
        marker
    };
    let mut cold = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
    assert!(cold.current.generation > generation);
    assert_eq!(
        cold.intent().unwrap().update.stage(),
        crate::update::Stage::RecoveryRequired
    );
    assert_eq!(cold.intent().unwrap().update.plan(), &plan);
    assert_eq!(
        installations::load(&profile).unwrap().unwrap().0.active_id,
        plan.source_instance
    );
    let interrupted_generation = cold.current.generation;
    // Stop exact migrated candidate; preserve its failed data/history for diagnosis.
    stop_owned(&profile, &plan.target_instance).unwrap();
    let mut namespace_loss = Value::Null;
    // Drop every Store/operation fence before making both primary namespaces absent.
    // No held original is deleted, and neither vault nor key is moved.
    let _held = if lose_namespaces {
        drop(cold);
        let checkpoint = initial_checkpoint.as_ref().unwrap();
        let unused = path(&v, "retainedHost");
        assert_eq!(
            unused,
            profile.parent().unwrap().join(
                "current29-migrated-full-runtime-c0f2be0563314742b9a36ae661555245/recovery/host"
            )
        );
        if !resume {
            let unused_proof = crate::profile_backup::verify_unused_extraction_archive(
                &profile,
                &path(&v, "hostArchive"),
                &initial_key,
                &unused,
                &checkpoint.host_manifest_sha256,
            )
            .unwrap();
            assert_eq!(fs::read(&key_file).unwrap(), initial_key);
            let script = path(&v, "retirementScript");
            let script_bytes = fs::read(&script).unwrap();
            assert_eq!(
                format!("{:x}", sha2::Sha256::digest(&script_bytes)),
                v["retirementScriptSha256"]
            );
            let retired = Command::new(path(&v, "python"))
                .arg(script)
                .arg(&unused)
                .arg(path(&v, "hostManifest"))
                .arg(&checkpoint.host_manifest_sha256)
                .arg(path(&v, "retirementReceipt"))
                .arg(path(&v, "hostArchive"))
                .arg(&key_file)
                .status()
                .unwrap();
            assert!(
                retired.success(),
                "unused host retirement refused; original namespaces preserved"
            );
            assert!(!unused.join("profile").exists());
            crate::restoration::private_bytes(
                &point.join("unused-host-retired.json"),
                &serde_json::to_vec(&unused_proof).unwrap(),
            )
            .unwrap();
        }
        let growth = crate::profile_backup::authenticated_host_restore_bytes(
            &profile,
            &path(&v, "hostArchive"),
            &initial_key,
            &checkpoint.host_manifest_sha256,
        )
        .unwrap();
        require_phase_capacity(
            &profile,
            &point,
            "before-missing-host",
            growth.checked_add(16_777_216).unwrap(),
            plan.required_free_bytes,
        );
        require_namespace_capacity(&profile, plan.required_free_bytes);
        let held = HeldNamespaces::preserve(&profile, &authority, &point);
        let authority_receipt = crate::signed_release::trust::Store::restore_missing_authority(
            &profile, "default", true,
        )
        .unwrap();
        assert!(!profile.exists());
        assert_eq!(authority_receipt.generation, interrupted_generation);
        cold = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
        assert_eq!(cold.current.generation, interrupted_generation);
        assert_eq!(
            cold.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        assert_eq!(cold.intent().unwrap().update.plan(), &plan);
        let host_receipt = cold
            .restore_rollback_missing_host(
                &path(&v, "hostArchive"),
                &path(&v, "trustArchive"),
                &key_file,
                &path(&v, "binding"),
                true,
            )
            .unwrap();
        assert!(host_receipt.host_profile_restored && host_receipt.current_trust_preserved);
        assert!(!host_receipt.runtime_data_restored && !host_receipt.runtime_started);
        assert_eq!(
            host_receipt
                .historical_checkpoint
                .as_ref()
                .unwrap()
                .checkpoint
                .generation,
            generation
        );
        drop(cold);
        cold = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
        assert_eq!(cold.current.generation, interrupted_generation);
        assert_eq!(
            cold.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        assert_eq!(
            installations::load(&profile).unwrap().unwrap().0.active_id,
            plan.source_instance
        );
        namespace_loss = serde_json::json!({"bothPrimaryNamespacesAbsent":true,"authorityRecoveredBeforeHostCreation":true,"authority":authority_receipt,"historicalHost":host_receipt,"heldProfile":held.held_profile,"heldAuthority":held.held_authority,"newerAuthorityPreserved":true});
        Some(held)
    } else {
        None
    };
    if lose_namespaces {
        require_phase_capacity(
            &profile,
            &point,
            "before-original-service-restoration",
            1_500_000_000 + 16_777_216,
            plan.required_free_bytes,
        );
    }
    let reservation = cold.register_rollback_candidate(true).unwrap();
    let archive = path(&v, "serviceArchive");
    let port = v["recoveryPort"]
        .as_u64()
        .filter(|n| *n >= 1024 && *n <= 65535)
        .unwrap() as u16;
    let restored = cold
        .restore_registered_rollback_candidate(
            v["maintenance"].as_str().unwrap(),
            &key_file,
            &archive,
            port,
            true,
        )
        .unwrap();
    assert_eq!(restored.candidate_id, reservation.candidate_id);
    let completion = cold
        .activate_restored_rollback(v["maintenance"].as_str().unwrap(), true)
        .unwrap();
    assert!(completion.selection_completed);
    assert_eq!(
        cold.intent().unwrap().update.stage(),
        crate::update::Stage::RolledBack
    );
    stop_owned(&profile, &reservation.candidate_id).unwrap();
    for (p, b, m) in originals {
        assert_eq!(fs::read(&p).unwrap(), b);
        assert_eq!(fs::metadata(p).unwrap().permissions().mode(), m);
    }
    for (p, b) in records {
        assert_eq!(fs::read(p).unwrap(), b);
    }
    assert_eq!(fs::read(&key_file).unwrap(), original_key);
    drop(cold);
    let reopened = crate::signed_release::trust::Store::open(&profile, "default").unwrap();
    assert_eq!(reopened.root, authority);
    assert_eq!(
        reopened.intent().unwrap().update.stage(),
        crate::update::Stage::RolledBack
    );
    assert_eq!(
        installations::load(&profile).unwrap().unwrap().0.active_id,
        reservation.candidate_id
    );
    let report = serde_json::json!({"state":"PASS","wholeExecutorSigkillVerified":true,"executorSignal":9,"marker":marker,"interruptedGeneration":interrupted_generation,"freshOriginalRestoration":restored,"originalActivation":completion,"coldSelectionVerified":true,"originalSourceKeyHistoryPreserved":true,"recoveryCandidateStopped":true,"newWholeHostCopy":lose_namespaces,"newArchivedCheckpointCopy":false,"resumedAfterRetirementRefusal":resume,"publicChangedSchemaGate":"CLOSED","lostHostRecoveryVerified":lose_namespaces,"bothNamespaceLoss":namespace_loss,"nativeLinuxWindowsGuiVerified":false});
    crate::restoration::private_bytes(
        &point.join("report.json"),
        &serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

/// Preserves exact originals. On failure, only absent names are returned; a
/// published recovery namespace is never overwritten or rewound by this guard.
struct HeldNamespaces {
    profile: PathBuf,
    authority: PathBuf,
    held_profile: PathBuf,
    held_authority: PathBuf,
}
impl HeldNamespaces {
    fn preserve(profile: &Path, authority: &Path, point: &Path) -> Self {
        let held_profile = point.join("held-original-profile");
        let held_authority = point.join("held-original-authority");
        assert!(absent(&held_profile) && absent(&held_authority));
        let guard = Self {
            profile: profile.to_owned(),
            authority: authority.to_owned(),
            held_profile,
            held_authority,
        };
        move_absent(profile, &guard.held_profile).unwrap();
        move_absent(authority, &guard.held_authority).unwrap();
        assert!(absent(profile) && absent(authority));
        crate::restoration::private_bytes(&point.join("both-namespaces-absent.json"),
            &serde_json::to_vec(&serde_json::json!({"profileAbsent":true,"authorityAbsent":true,"heldProfile":guard.held_profile,"heldAuthority":guard.held_authority})).unwrap()).unwrap();
        guard
    }
}
impl Drop for HeldNamespaces {
    fn drop(&mut self) {
        for (held, original) in [
            (&self.held_profile, &self.profile),
            (&self.held_authority, &self.authority),
        ] {
            if absent(original) && held.exists() {
                let _ = move_absent(held, original);
            }
        }
    }
}

/// Independent future stages are gated at their real boundary. Already allocated
/// files are included in observed free space rather than added a second time.
/// Dense extraction remains budgeted in full; COW savings are not assumed.
fn require_phase_capacity(
    profile: &Path,
    point: &Path,
    phase: &str,
    additional: u64,
    headroom: u64,
) {
    let v = input();
    let tool = path(&v, "storageBudgetTool");
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(fs::read(&tool).unwrap())),
        v["storageBudgetToolSha256"]
    );
    let observed = Command::new(path(&v, "python"))
        .arg(tool)
        .args(["--root"])
        .arg(path(&v, "projectRoot"))
        .args([
            "--component",
            &format!("phase-growth={additional}"),
            "--component",
            &format!("signed-headroom={headroom}"),
        ])
        .output()
        .unwrap();
    crate::restoration::private_bytes(
        &point.join(format!("storage-budget-{phase}.json")),
        &observed.stdout,
    )
    .unwrap();
    assert!(
        observed.status.success(),
        "storage tool refuses phase; no further allocation"
    );
    let external: Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(external["budget_passed"], true);
    let required = additional
        .checked_add(headroom)
        .unwrap()
        .checked_add(6 * 1024 * 1024 * 1024)
        .unwrap();
    let available = fs2::available_space(profile).unwrap();
    let proof = serde_json::json!({"phase":phase,"additionalPeakBytes":additional,"signedHeadroomBytes":headroom,"retainedFloorBytes":6u64*1024*1024*1024,"requiredAvailableBytes":required,"actualAvailableBytes":available,"passed":available>=required});
    crate::restoration::private_bytes(
        &point.join(format!("phase-budget-{phase}.json")),
        &serde_json::to_vec(&proof).unwrap(),
    )
    .unwrap();
    assert!(
        available >= required,
        "phase capacity refused; preserve existing recovery data and floor"
    );
}

fn require_namespace_capacity(profile: &Path, headroom: u64) {
    fn logical_bytes(dir: &Path) -> u64 {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                assert!(!m.file_type().is_symlink());
                if m.is_dir() {
                    logical_bytes(&p)
                } else {
                    assert!(m.is_file());
                    m.len()
                }
            })
            .try_fold(0u64, |a, n| a.checked_add(n))
            .unwrap()
    }
    let required = logical_bytes(profile)
        .checked_add(headroom)
        .unwrap()
        .checked_add(6 * 1024 * 1024 * 1024)
        .unwrap();
    assert!(
        fs2::available_space(profile).unwrap() >= required,
        "namespace restoration exceeds expanded host/headroom/6GiB floor; preserve primary namespaces"
    );
}

#[test]
fn held_namespaces_failure_returns_only_absent_original_names() {
    let temp = SyntheticScope::new();
    let profile = temp.path().join("profile");
    let authority = temp.path().join("authority");
    fs::create_dir(&profile).unwrap();
    fs::create_dir(&authority).unwrap();
    fs::write(profile.join("original"), b"source").unwrap();
    fs::write(authority.join("original"), b"newest-authority").unwrap();
    let held = HeldNamespaces::preserve(&profile, &authority, temp.path());
    assert!(!profile.exists() && !authority.exists());
    drop(held);
    assert_eq!(fs::read(profile.join("original")).unwrap(), b"source");
    assert_eq!(
        fs::read(authority.join("original")).unwrap(),
        b"newest-authority"
    );
}

#[test]
fn held_namespaces_failure_never_overwrites_published_recovery() {
    let temp = SyntheticScope::new();
    let profile = temp.path().join("profile");
    let authority = temp.path().join("authority");
    fs::create_dir(&profile).unwrap();
    fs::create_dir(&authority).unwrap();
    fs::write(profile.join("original"), b"source").unwrap();
    fs::write(authority.join("original"), b"old-authority").unwrap();
    let held = HeldNamespaces::preserve(&profile, &authority, temp.path());
    fs::create_dir(&profile).unwrap();
    fs::create_dir(&authority).unwrap();
    fs::write(profile.join("new"), b"recovered").unwrap();
    fs::write(authority.join("new"), b"newer-authority").unwrap();
    drop(held);
    assert_eq!(fs::read(profile.join("new")).unwrap(), b"recovered");
    assert_eq!(fs::read(authority.join("new")).unwrap(), b"newer-authority");
    assert_eq!(
        fs::read(temp.path().join("held-original-profile/original")).unwrap(),
        b"source"
    );
    assert_eq!(
        fs::read(temp.path().join("held-original-authority/original")).unwrap(),
        b"old-authority"
    );
}

#[test]
fn held_namespaces_second_move_failure_returns_original_profile() {
    let temp = SyntheticScope::new();
    let profile = temp.path().join("profile");
    fs::create_dir(&profile).unwrap();
    fs::write(profile.join("original"), b"source").unwrap();
    let result = std::panic::catch_unwind(|| {
        HeldNamespaces::preserve(&profile, &temp.path().join("absent-authority"), temp.path())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(profile.join("original")).unwrap(), b"source");
}

struct SyntheticScope(PathBuf);
impl SyntheticScope {
    fn new() -> Self {
        let p = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("exhibitos-held-namespace-{}", uuid::Uuid::new_v4()));
        let scope = Self(p);
        installations::new_directory(&scope.0).unwrap();
        scope
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for SyntheticScope {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn absent(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}
fn move_absent(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(source.as_os_str().as_bytes())?;
        let b = std::ffi::CString::new(target.as_os_str().as_bytes())?;
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
            return Err(std::io::Error::last_os_error());
        }
        fs::File::open(source.parent().unwrap())?.sync_all()?;
        fs::File::open(target.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (source, target);
        Err(std::io::Error::other("native no-replace unavailable"))
    }
}
#[test]
fn held_namespaces_broken_symlink_collision_preserves_every_namespace() {
    let temp = SyntheticScope::new();
    let profile = temp.path().join("profile");
    let authority = temp.path().join("authority");
    fs::create_dir(&profile).unwrap();
    fs::create_dir(&authority).unwrap();
    fs::write(profile.join("original"), b"source").unwrap();
    let held = HeldNamespaces::preserve(&profile, &authority, temp.path());
    std::os::unix::fs::symlink(temp.path().join("absent"), &profile).unwrap();
    drop(held);
    assert!(
        fs::symlink_metadata(&profile)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read(temp.path().join("held-original-profile/original")).unwrap(),
        b"source"
    );
    assert!(authority.is_dir());
}

#[test]
fn fresh_publication_authority_private_hash_reads_exact_bytes_and_refuses_aliases_quota() {
    use std::os::unix::fs::symlink;
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "exhibitos-fresh-authority-{}",
            uuid::Uuid::new_v4()
        ));
    installations::new_directory(&root).unwrap();
    let file = root.join("input.bin");
    crate::restoration::private_bytes(&file, b"synthetic input").unwrap();
    let before = fs::read(&file).unwrap();
    assert_eq!(
        private_digest(&file, 1024),
        format!("{:x}", sha2::Sha256::digest(&before))
    );
    assert!(std::panic::catch_unwind(|| private_digest(&file, 1)).is_err());
    let alias = root.join("alias.bin");
    fs::hard_link(&file, &alias).unwrap();
    assert!(std::panic::catch_unwind(|| private_digest(&file, 1024)).is_err());
    fs::remove_file(&alias).unwrap();
    let linked = root.join("symlink.bin");
    symlink(&file, &linked).unwrap();
    assert!(std::panic::catch_unwind(|| private_digest(&linked, 1024)).is_err());
    assert_eq!(fs::read(&file).unwrap(), before);
    // Only this tiny owned successful synthetic fixture is retired.
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_publication_authority_private_hash_refuses_nonprivate_and_special_files() {
    use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("exhibitos-fresh-fd-{}", uuid::Uuid::new_v4()));
    installations::new_directory(&root).unwrap();
    let file = root.join("shared.bin");
    crate::restoration::private_bytes(&file, b"preserved").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(std::panic::catch_unwind(|| private_digest(&file, 1024)).is_err());
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(fs::read(&file).unwrap(), b"preserved");
    let fifo = root.join("fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(std::panic::catch_unwind(|| private_digest(&fifo, 1024)).is_err());
    assert!(std::panic::catch_unwind(|| private_digest(&root, 1024)).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_publication_authority_private_hash_detects_open_fd_path_replacement() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("exhibitos-fresh-replace-{}", uuid::Uuid::new_v4()));
    installations::new_directory(&root).unwrap();
    let file = root.join("input.bin");
    let retained = root.join("retained.bin");
    let original = vec![7u8; 131072];
    crate::restoration::private_bytes(&file, &original).unwrap();
    let result = std::panic::catch_unwind(|| {
        private_digest_observed(&file, 131072, |bytes| {
            if bytes == 65536 {
                fs::rename(&file, &retained).unwrap();
                crate::restoration::private_bytes(&file, &vec![9u8; 131072]).unwrap();
            }
        })
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&retained).unwrap(), original);
    assert_eq!(fs::read(&file).unwrap(), vec![9u8; 131072]);
    fs::remove_dir_all(root).unwrap();
}
