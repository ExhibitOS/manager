// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{net::TcpListener, os::unix::fs::DirBuilderExt};

#[test]
#[ignore = "starts the exact independently reviewed 042 host in a new isolated profile"]
fn actual_042_host_bootstrap_and_clean_stop() {
    let runtime = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_042_RUNTIME").unwrap());
    let payload = Arc::new(Payload::open(&runtime).unwrap());
    let (source, producer, manifest) =
        crate::native_payload::hayes_bootstrap_validation_tuple().unwrap();
    let context = payload.context();
    assert_eq!(
        (
            context.source_commit.as_str(),
            payload.producer_sha256(),
            context.manifest_sha256.as_str()
        ),
        (source, producer, manifest)
    );
    let catalog = WorkspaceFurnishingCatalog::HayesOriginal042;
    assert_eq!(
        payload.workspace_furnishing_catalog().unwrap(),
        Some(catalog)
    );
    let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
    assert_eq!(ledger.lines().count(), 42);
    assert_eq!(ledger.lines().last(), Some("042_hayes_original_attempt.sql|074de4171572039f2f400d49d07aff8a3ec83ec222c1684cbc5452c6cc8b3ece"));

    let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_042_ROOT").unwrap());
    assert_eq!(
        root.parent().unwrap(),
        fs::canonicalize(root.parent().unwrap()).unwrap()
    );
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let root_pin = crate::maintenance_source::DirectoryPin::open(&root).unwrap();
    let service = LifecycleService::bound_existing(root.clone()).unwrap();
    let guard = service.lock().unwrap();
    check_selected_empty_root(&service, &guard, &root_pin).unwrap();

    let listeners = [(); 3].map(|()| TcpListener::bind("127.0.0.1:0").unwrap());
    let [web, api, database] = listeners
        .each_ref()
        .map(|listener| listener.local_addr().unwrap().port());
    let ports = Ports { web, api, database };
    service
        .prepare_native_personal_locked(&payload, ports, &guard)
        .unwrap();
    let mut session = Session::new_with_guard(&service, payload, ports, guard).unwrap();
    session
        .authorize_fresh_workspace_furnishing(&root_pin)
        .unwrap();
    let unarmed = session
        .start()
        .err()
        .expect("042 must require the private arm");
    assert_eq!(unarmed.code, "HAYES_OPERATION_UNAVAILABLE");
    assert!(!root.join("native-database").exists());
    assert!(!root.join("native-current-run.json").exists());

    session.hayes_bootstrap = HayesBootstrap::Armed(manifest.into());
    drop(listeners);
    let ready = session.start().unwrap();
    assert_eq!(ready.state, "ready");
    assert!(ready.readiness.ready);
    assert!(ready.node_pid.is_some() && ready.postgres_pid.is_some());
    assert!(session.owner_channel.is_some());
    let HayesBootstrap::Issued(issued) = &session.hayes_bootstrap else {
        panic!("actual Session did not authenticate the dedicated pin");
    };
    let generation = issued.pin.runtime_generation();
    assert_eq!(
        ready.job_id.as_deref(),
        Some(generation.to_string().as_str())
    );
    assert_eq!(issued.pin.host_manifest_sha256(), manifest);
    let native_owner = issued.pin.native_owner_id();
    assert_ne!(native_owner, generation);
    session.verify_workspace_furnishing_ledger().unwrap();

    let stopped = session.stop().unwrap();
    assert_eq!(stopped.state, "stopped");
    assert!(stopped.failure_code.is_none());
    assert!(
        stopped.node_pid.is_none()
            && stopped.postgres_pid.is_none()
            && stopped.auxiliary_pid.is_none()
    );
    assert_eq!(stopped.exits.get("node"), Some(&0));
    assert_eq!(stopped.exits.get("postgres"), Some(&0));
    assert!(matches!(session.hayes_bootstrap, HayesBootstrap::Spent));
    let current = current_run(&root).unwrap().unwrap();
    let binding = current.furnishing.unwrap();
    assert_eq!(binding.phase, Phase::Clean);
    assert_eq!(binding.catalog, 42);
    assert_eq!(binding.payload_manifest_sha256, manifest);
    println!("042_BOOTSTRAP_READY_CLEAN manifest={manifest} generation={generation} nativeOwner={native_owner} ledgerRows=42 web={web} api={api} postgres={database}");
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionProbeArm<'a> {
    version: u8,
    kind: &'static str,
    runtime_generation: Uuid,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
}

fn session_probe_arm(pin: &HayesSessionPin) -> Vec<u8> {
    serde_json::to_vec(&SessionProbeArm {
        version: 1,
        kind: "hayes-session-probe-start",
        runtime_generation: pin.runtime_generation(),
        native_owner_id: pin.native_owner_id(),
        runtime_manifest_sha256: pin.host_manifest_sha256(),
    })
    .unwrap()
}

#[test]
fn selected_session_probe_arm_matches_the_platform_key_order() {
    let generation = Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap();
    let hash = "a".repeat(64);
    let pin = HayesSessionPin::new(generation, &hash).unwrap();
    assert_eq!(session_probe_arm(&pin), format!(
        "{{\"version\":1,\"kind\":\"hayes-session-probe-start\",\"runtimeGeneration\":\"{generation}\",\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{hash}\"}}",
        pin.native_owner_id(),
    ).as_bytes());
}

fn probe_path(key: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(key).unwrap_or_else(|| panic!("{key} required")))
}

#[test]
#[ignore = "runs the exact private Platform caller and sealed worker in a new isolated Session"]
fn actual_042_session_holds_checked_worker_output() {
    use super::{
        hayes::{tests::Fixture, Lease},
        hayes_probe_source::ProbeSourceFd,
    };
    use std::mem::ManuallyDrop;

    let pin = crate::native_payload::HAYES_SESSION_PROBE_PIN
        .as_ref()
        .expect("paired probe host manifest must be admitted");
    let runtime = probe_path("EXHIBITOS_TEST_HAYES_PROBE_RUNTIME");
    let payload = Arc::new(Payload::open(&runtime).unwrap());
    let manifest = payload.context().manifest_sha256;
    assert_eq!(manifest, pin.host.2);
    assert!(payload.hayes_session_probe_script().unwrap().is_some());
    assert_eq!(
        payload.workspace_furnishing_catalog().unwrap(),
        Some(WorkspaceFurnishingCatalog::HayesOriginal042)
    );
    let worker = Payload::open(&probe_path("EXHIBITOS_TEST_HAYES_PROBE_WORKER")).unwrap();
    let original =
        ProbeSourceFd::open_original(&probe_path("EXHIBITOS_TEST_HAYES_ORIGINAL")).unwrap();

    let root = probe_path("EXHIBITOS_TEST_HAYES_PROBE_ROOT");
    let custody_path = probe_path("EXHIBITOS_TEST_HAYES_PROBE_CUSTODY");
    let parent = root.parent().unwrap();
    assert_eq!(parent, fs::canonicalize(parent).unwrap());
    assert_eq!(custody_path.parent(), Some(parent));
    assert_ne!(root, custody_path);
    assert!(!custody_path.exists());
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let root_pin = crate::maintenance_source::DirectoryPin::open(&root).unwrap();
    let service = LifecycleService::bound_existing(root.clone()).unwrap();
    let guard = service.lock().unwrap();
    check_selected_empty_root(&service, &guard, &root_pin).unwrap();
    let listeners = [(); 3].map(|()| TcpListener::bind("127.0.0.1:0").unwrap());
    let [web, api, database] = listeners
        .each_ref()
        .map(|listener| listener.local_addr().unwrap().port());
    let ports = Ports { web, api, database };
    service
        .prepare_native_personal_locked(&payload, ports, &guard)
        .unwrap();
    let mut session = Session::new_with_guard(&service, payload, ports, guard).unwrap();
    session
        .authorize_fresh_workspace_furnishing(&root_pin)
        .unwrap();
    session.hayes_bootstrap = HayesBootstrap::Armed(manifest.clone());
    session.hayes_probe_source = Some(original);
    drop(listeners);
    let ready = session.start().unwrap();
    assert!(ready.readiness.ready);
    assert!(ready.node_pid.is_some() && ready.postgres_pid.is_some());
    session.verify_workspace_furnishing_ledger().unwrap();

    let custody = ManuallyDrop::new(Fixture::provision(custody_path.clone()));
    let HayesBootstrap::Issued(issued) = &mut session.hayes_bootstrap else {
        panic!("actual probe Session did not issue an authenticated pin");
    };
    let generation = issued.pin.runtime_generation();
    let native_owner = issued.pin.native_owner_id();
    let arm = session_probe_arm(&issued.pin);
    let pre_open_deadline = Instant::now() + Duration::from_secs(180);
    issued
        .channel
        .send_frame(&arm, pre_open_deadline, &session.cancellation, &mut || {
            Ok(())
        })
        .unwrap();

    let mut exchange = session
        .begin_owned_hayes_open_until(custody.acquire(), pre_open_deadline)
        .unwrap();
    let worker_deadline = Instant::now() + Duration::from_secs(180);
    exchange.receive_source(worker_deadline).unwrap();
    exchange.revalidate().unwrap();
    exchange
        .observe_revalidated_workers(&worker, worker_deadline)
        .unwrap();
    drop(exchange);

    let retained = session
        .retained_hayes
        .as_ref()
        .expect("worker custody must stay in Session");
    assert!(!retained.has_child());
    assert_eq!(
        Lease::acquire(custody.path(), custody.authority())
            .err()
            .unwrap()
            .code,
        "HAYES_RESERVATION_BUSY"
    );
    let active: Value =
        serde_json::from_slice(&fs::read(custody_path.join("active.json")).unwrap()).unwrap();
    let binding: hayes::Binding = serde_json::from_value(active["binding"].clone()).unwrap();
    assert_eq!(binding.generation, generation);
    assert_eq!(binding.owner, native_owner);
    let attempt = custody_path.join(binding.attempt.to_string());
    let held: Value =
        serde_json::from_slice(&fs::read(attempt.join("operation-workers-held.json")).unwrap())
            .unwrap();
    assert_eq!(held["binding"]["attemptId"], binding.attempt.to_string());
    assert_eq!(held["binding"]["jobId"], binding.job.to_string());
    assert_eq!(held["runtimeManifestSha256"], manifest);
    assert_eq!(held["outputBytes"], 16_036_864);
    let tenant = Uuid::parse_str(held["binding"]["tenantId"].as_str().unwrap()).unwrap();
    let lease = Uuid::parse_str(held["binding"]["leaseId"].as_str().unwrap()).unwrap();
    let snapshot = session
        .query_database_before(
            "exhibitos",
            &format!(
                "SELECT state,hayes_attempt_id::text,asset_id IS NULL,original_asset_id IS NULL,
          GREATEST(0,floor(EXTRACT(EPOCH FROM (lease_until-clock_timestamp()))*1000))::bigint
          FROM import_jobs WHERE tenant_id='{tenant}' AND id='{}' AND lease_id='{lease}'",
                binding.job
            ),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let fields: Vec<_> = snapshot.split('|').collect();
    assert_eq!(fields.len(), 5);
    assert_eq!(
        &fields[..4],
        &["processing", binding.attempt.to_string().as_str(), "t", "t"]
    );
    let lease_remaining_ms: u64 = fields[4].parse().unwrap();
    let expected_hash = "694a9cf81dfbf12fd0841ae45240c5ae0f1d7cb129d0ee122be7cff86c4625bc";
    assert_eq!(held["outputSha256"], expected_hash);
    let body_id = Uuid::parse_str(held["outputStageId"].as_str().unwrap()).unwrap();
    let body = fs::read(attempt.join("stage").join(format!(".oex-stage-{body_id}"))).unwrap();
    assert_eq!(body.len(), 16_036_864);
    assert_eq!(digest(&body), expected_hash);
    assert!(!attempt.join("terminal.json").exists());

    let stopped = session.stop().unwrap();
    assert_eq!(stopped.state, "stopped");
    assert!(stopped.failure_code.is_none());
    assert!(
        stopped.node_pid.is_none()
            && stopped.postgres_pid.is_none()
            && stopped.auxiliary_pid.is_none()
    );
    assert_eq!(stopped.exits.get("node"), Some(&0));
    assert_eq!(stopped.exits.get("postgres"), Some(&0));
    let current = current_run(&root).unwrap().unwrap().furnishing.unwrap();
    assert_eq!(current.catalog, 42);
    assert_eq!(current.phase, Phase::Clean);
    assert_eq!(
        Lease::acquire(custody.path(), custody.authority())
            .err()
            .unwrap()
            .code,
        "HAYES_RESERVATION_BUSY"
    );
    drop(session);
    assert_eq!(
        custody
            .acquire()
            .check_interrupted(binding)
            .err()
            .unwrap()
            .code,
        "HAYES_DRAIN_UNCONFIRMED"
    );
    println!("HAYES_SESSION_WORKERS_HELD manifest={manifest} generation={generation} nativeOwner={native_owner} attempt={} job={} output={expected_hash} leaseRemainingMs={lease_remaining_ms} web={web} api={api} postgres={database}", binding.attempt, binding.job);
}
