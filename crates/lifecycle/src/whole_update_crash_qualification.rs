// SPDX-License-Identifier: Apache-2.0
//! Explicit native synthetic qualification; never enables public migration admission.
use super::*;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};

fn input() -> Value {
    serde_json::from_slice(
        &fs::read(std::env::var("EXHIBITOS_WHOLE_CRASH_INPUT").unwrap()).unwrap(),
    )
    .unwrap()
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
    assert_eq!(store.current.generation, 21);
    assert_eq!(
        intent.update.plan().operation_id,
        "c5b5becd-4f70-435f-909e-73031e5def37"
    );
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
#[ignore = "child only: explicit signed current21 synthetic fixture, actual owned Applying/application; waits for parent SIGKILL"]
fn actual_changed_executor_crash_child() {
    let v = input();
    let mut store = checked_store(&v);
    let profile = store.profile.clone();
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
    let proof = serde_json::json!({"pid":std::process::id(),"operationId":"c5b5becd-4f70-435f-909e-73031e5def37","candidateApplied":true,"receipt":ready.receipt(),"health":ready.observed_health()});
    crate::restoration::private_bytes(&marker, &serde_json::to_vec(&proof).unwrap()).unwrap();
    // Keep all actual fences/opaque permits alive until the parent kills this PID.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::hint::black_box(&ready);
    }
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
    use std::os::unix::fs::PermissionsExt;
    let v = input();
    let store = checked_store(&v);
    let profile = store.profile.clone();
    let authority = store.root.clone();
    let plan = store.intent().unwrap().update.plan().clone();
    let generation = store.current.generation;
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
    assert!(!point.join("candidate-applied.json").exists());
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
    let report = serde_json::json!({"state":"PASS","wholeExecutorSigkillVerified":true,"executorSignal":9,"marker":marker,"interruptedGeneration":interrupted_generation,"freshOriginalRestoration":restored,"originalActivation":completion,"coldSelectionVerified":true,"originalSourceKeyHistoryPreserved":true,"recoveryCandidateStopped":true,"newWholeHostCopy":false,"publicChangedSchemaGate":"CLOSED","lostHostRecoveryVerified":false,"nativeLinuxWindowsGuiVerified":false});
    crate::restoration::private_bytes(
        &point.join("report.json"),
        &serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
