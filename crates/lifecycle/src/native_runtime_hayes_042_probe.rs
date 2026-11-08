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
