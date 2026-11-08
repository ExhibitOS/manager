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

#[test]
fn selected_session_probe_arm_matches_the_platform_key_order() {
    use super::hayes::operation::session_probe_arm;
    let generation = Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap();
    let hash = "a".repeat(64);
    let pin = HayesSessionPin::new(generation, &hash).unwrap();
    assert_eq!(session_probe_arm(&pin).unwrap(), format!(
        "{{\"version\":1,\"kind\":\"hayes-session-probe-start\",\"runtimeGeneration\":\"{generation}\",\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{hash}\"}}",
        pin.native_owner_id(),
    ).as_bytes());
}

fn trace_probe_event(event: &'static str) {
    let mut stamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let monotonic_ns = if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut stamp) } == 0
    {
        Some((stamp.tv_sec as u64 * 1_000_000_000 + stamp.tv_nsec as u64).to_string())
    } else {
        None
    };
    println!(
        "{}",
        serde_json::json!({
            "kind": "hayes-probe-manager-phase",
            "event": event,
            "monotonicNs": monotonic_ns,
        })
    );
}

fn probe_path(key: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(key).unwrap_or_else(|| panic!("{key} required")))
}

#[test]
#[ignore = "runs the exact private Platform caller and sealed worker in a new isolated Session"]
fn actual_042_session_holds_checked_worker_output() {
    actual_session_probe(WorkspaceFurnishingCatalog::HayesOriginal042);
}

#[test]
#[ignore = "requires the exact reviewed 043 pair and a new isolated profile and custody"]
fn actual_043_session_stages_output_and_retains_native_custody() {
    actual_session_probe(WorkspaceFurnishingCatalog::HayesDisposition043);
}

#[test]
#[ignore = "requires the exact reviewed 044 pair and a new isolated profile and custody"]
fn actual_044_session_commits_and_returns_native_custody() {
    actual_session_probe(WorkspaceFurnishingCatalog::HayesRelease044);
}

fn actual_session_probe(catalog: WorkspaceFurnishingCatalog) {
    use super::{
        hayes::{tests::Fixture, Lease},
        hayes_probe_source::ProbeSourceFd,
    };
    use std::mem::ManuallyDrop;

    let releases_output = catalog == WorkspaceFurnishingCatalog::HayesRelease044;
    let stages_output = catalog.version() >= 43;
    let (pin, prefix) = match catalog {
        WorkspaceFurnishingCatalog::HayesRelease044 => (
            &crate::native_payload::HAYES_RELEASE_PROBE_PIN,
            "EXHIBITOS_TEST_HAYES_044_PROBE_",
        ),
        WorkspaceFurnishingCatalog::HayesDisposition043 => (
            &crate::native_payload::HAYES_DISPOSITION_PROBE_PIN,
            "EXHIBITOS_TEST_HAYES_043_PROBE_",
        ),
        WorkspaceFurnishingCatalog::HayesOriginal042 => (
            &crate::native_payload::HAYES_SESSION_PROBE_PIN,
            "EXHIBITOS_TEST_HAYES_PROBE_",
        ),
        _ => unreachable!(),
    };
    let pin = pin
        .as_ref()
        .expect("paired probe host manifest must be admitted");
    let path = |name: &str| probe_path(&format!("{prefix}{name}"));
    let runtime = path("RUNTIME");
    let payload = Arc::new(Payload::open(&runtime).unwrap());
    let manifest = payload.context().manifest_sha256;
    assert_eq!(manifest, pin.host.2);
    assert!(payload.hayes_session_probe_script().unwrap().is_some());
    assert_eq!(
        payload.workspace_furnishing_catalog().unwrap(),
        Some(catalog)
    );
    let worker = Payload::open(&path("WORKER")).unwrap();
    let original_path = probe_path("EXHIBITOS_TEST_HAYES_ORIGINAL");
    let original = ProbeSourceFd::open_original(&original_path).unwrap();

    let root = path("ROOT");
    let custody_path = path("CUSTODY");
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
    let pre_open_deadline = Instant::now() + Duration::from_secs(180);
    let mut exchange = session
        .begin_owned_hayes_probe_open(custody.acquire(), pre_open_deadline, &mut trace_probe_event)
        .unwrap();
    let worker_deadline = Instant::now() + Duration::from_secs(180);
    exchange.receive_source(worker_deadline).unwrap();
    exchange.revalidate().unwrap();
    exchange
        .observe_revalidated_workers(&worker, worker_deadline)
        .unwrap();
    let staged = if stages_output {
        exchange.emit_output_ready(worker_deadline).unwrap();
        exchange
            .receive_output_reservation(worker_deadline)
            .unwrap();
        exchange.transfer_reserved_output(worker_deadline).unwrap();
        Some(await_probe_event(
            &root,
            generation,
            "hayes-probe-output-staged",
            worker_deadline,
        ))
    } else {
        None
    };
    if releases_output {
        let proof = release_probe::before_release(&custody_path, staged.as_ref().unwrap());
        exchange
            .observe_committed_disposition(worker_deadline)
            .unwrap();
        exchange.return_committed_custody(worker_deadline).unwrap();
        let recorded = await_probe_event(
            &root,
            generation,
            "hayes-probe-release-recorded",
            worker_deadline,
        );
        let committed = await_probe_event(
            &root,
            generation,
            "hayes-probe-disposition-committed",
            worker_deadline,
        );
        drop(exchange);
        assert!(session.retained_hayes.is_none());
        assert_eq!(proof.binding.generation, generation);
        assert_eq!(proof.binding.owner, native_owner);
        assert!(!custody_path.join("active.json").exists());
        drop(Lease::acquire(custody.path(), custody.authority()).unwrap());
        release_probe::verify_release(
            &mut session,
            &root,
            &original_path,
            &proof,
            &committed,
            &recorded,
        );
        stop_probe_session(&mut session, &root, catalog);
        println!("HAYES_SESSION_RELEASE_RECORDED manifest={manifest} generation={generation} nativeOwner={native_owner} attempt={} job={} dbStage={} receipt={} completionFrame={} web={web} api={api} postgres={database}",
            proof.binding.attempt, proof.binding.job, proof.db_stage_id, committed["receiptId"].as_str().unwrap(), recorded["completionFrameSha256"].as_str().unwrap());
        return;
    }
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
    assert_eq!(
        held["workerManifestSha256"],
        "94a6551d16af3fb134f94549d9f087ee8ccb9fd2372d914de5f738a78d3505c6"
    );
    assert_eq!(held["outputBytes"], 16_036_864);
    let tenant = Uuid::parse_str(held["binding"]["tenantId"].as_str().unwrap()).unwrap();
    let lease = Uuid::parse_str(held["binding"]["leaseId"].as_str().unwrap()).unwrap();
    let snapshot = session
        .query_database_before(
            "exhibitos",
            &format!(
                "SELECT state,hayes_attempt_id::text,asset_id IS NULL,original_asset_id IS NULL,
          GREATEST(0,floor(EXTRACT(EPOCH FROM (lease_until-clock_timestamp()))*1000))::bigint,
          (SELECT count(*) FROM assets a WHERE (a.tenant_id,a.artwork_id)=(j.tenant_id,j.artwork_id)
            AND a.state='approved' AND a.deleted_at IS NULL)
          FROM import_jobs j WHERE tenant_id='{tenant}' AND id='{}' AND lease_id='{lease}'",
                binding.job
            ),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let fields: Vec<_> = snapshot.split('|').collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(fields[5], "0");
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
    if let Some(staged) = staged {
        assert_eq!(staged["jobId"], binding.job.to_string());
        assert_eq!(staged["sha256"], expected_hash);
        assert_eq!(staged["bytes"], body.len());
        let stage_id = Uuid::parse_str(staged["stageId"].as_str().unwrap()).unwrap();
        let reservation: Value = serde_json::from_slice(
            &fs::read(attempt.join("operation-output-reservation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reservation["dbStageId"], stage_id.to_string());
        assert_eq!(reservation["output"]["stageId"], body_id.to_string());
        let key = format!("{tenant}/hayes-output/{}/{expected_hash}", binding.attempt);
        let stored = fs::read(root.join("blobs").join(&key)).unwrap();
        assert_eq!(stored, body);
        let check = format!(
            "SELECT count(*) FROM hayes_output_stages WHERE tenant_id='{tenant}' AND id='{stage_id}'
                AND attempt_id='{}' AND job_id='{}' AND output_sha256='{expected_hash}'
                AND output_bytes=16036864 AND object_key='{key}'
                AND NOT EXISTS (SELECT 1 FROM hayes_original_dispositions d WHERE d.tenant_id='{tenant}' AND d.attempt_id='{}')",
            binding.attempt, binding.job, binding.attempt,
        );
        assert_eq!(
            session
                .query_database_before(
                    "exhibitos",
                    &check,
                    Instant::now() + Duration::from_secs(10)
                )
                .unwrap(),
            "1"
        );
        println!(
            "HAYES_SESSION_OUTPUT_STAGED dbStage={stage_id} output={expected_hash} bytes=16036864"
        );
    }
    assert!(!attempt.join("terminal.json").exists());

    stop_probe_session(&mut session, &root, catalog);
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

fn stop_probe_session(session: &mut Session, root: &Path, catalog: WorkspaceFurnishingCatalog) {
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
    let current = current_run(root).unwrap().unwrap().furnishing.unwrap();
    assert_eq!(current.catalog, catalog.version());
    assert_eq!(current.phase, Phase::Clean);
}

#[path = "native_runtime_hayes_044_probe.rs"]
mod release_probe;

fn await_probe_event(root: &Path, generation: Uuid, kind: &str, deadline: Instant) -> Value {
    let log = root
        .join("native-runs")
        .join(generation.to_string())
        .join("process.log");
    let prefix = format!("{{\"kind\":\"{kind}\",");
    loop {
        assert!(
            Instant::now() < deadline,
            "Hayes probe did not emit {kind} before the deadline"
        );
        let lines = fs::read_to_string(&log).unwrap();
        if let Some(line) = lines.lines().find(|line| line.starts_with(&prefix)) {
            return serde_json::from_str(line).unwrap();
        }
        assert!(
            !lines
                .lines()
                .any(|line| line == "HAYES_SESSION_PROBE_FAILED"),
            "Hayes probe failed before {kind}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
