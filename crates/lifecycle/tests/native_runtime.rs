// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    initial_account::InitialAccountInput,
    installations::{InstallationController, native_ui::Mode},
    native_installation::Ports,
    native_payload::Payload,
    native_runtime::Session,
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn ports() -> Ports {
    let listeners: Vec<_> = (0..3)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    Ports {
        web: listeners[0].local_addr().unwrap().port(),
        api: listeners[1].local_addr().unwrap().port(),
        database: listeners[2].local_addr().unwrap().port(),
    }
}
fn login(port: u16, tenant: &str) -> bool {
    let body=serde_json::to_vec(&serde_json::json!({"subject":"native-synthetic","password":"synthetic native $quote\" test password","tenantId":tenant})).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream,"POST /api/v1/auth/login HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
    stream.write_all(&body).unwrap();
    let mut response = Vec::new();
    stream.take(65537).read_to_end(&mut response).unwrap();
    let accepted = response.starts_with(b"HTTP/1.1 200 ") || response.starts_with(b"HTTP/1.0 200 ");
    response.fill(0);
    accepted
}
#[test]
#[ignore = "Requires explicit isolated root and exact native payload; starts only owned PG/Node children"]
fn actual_owned_lifecycle_reopen_failure_retry_and_mode_state() {
    let payload = Arc::new(
        Payload::open(&PathBuf::from(
            std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").expect("exact payload required"),
        ))
        .unwrap(),
    );
    let root = PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_RUNTIME_TEST_ROOT").expect("new isolated root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let controller = InstallationController::new(root.join("profile"), None).unwrap();
    let selected = controller.context().unwrap();
    let token = &selected.selection_token;
    let installation = PathBuf::from(
        &selected
            .installations
            .iter()
            .find(|s| s.id == selected.active_id)
            .unwrap()
            .path,
    );
    let ports = ports();
    let configured = controller
        .with_current_mutation(token, |service| {
            service.prepare_native_installation(
                &payload,
                ports,
                Some(InitialAccountInput {
                    subject: "native-synthetic".into(),
                    password: "synthetic native $quote\" test password".into(),
                }),
            )
        })
        .unwrap();
    let configuration = fs::read(installation.join("runtime.env")).unwrap();
    let mut session = controller
        .with_current_mutation(token, |service| {
            Session::new(service, payload.clone(), ports)
        })
        .unwrap();
    let first = session.start().unwrap();
    assert_eq!(first.state, "ready");
    assert!(first.readiness.ready);
    assert!(login(ports.web, &configured.tenant_id));
    eprintln!("PHASE native owned start/readiness/account PASS");
    assert_eq!(session.start().err().unwrap().code, "BUSY");
    assert_eq!(
        controller
            .with_current_mutation(token, |service| Session::new(
                service,
                payload.clone(),
                ports
            ))
            .err()
            .unwrap()
            .code,
        "BUSY"
    );
    let mut presentation = controller.presentation().unwrap();
    presentation.mode = Mode::Expert;
    controller.save_presentation(presentation).unwrap();
    let same = session.status().unwrap();
    assert!(same.readiness.ready);
    assert_eq!(same.job_id, first.job_id);
    assert_eq!(same.node_pid, first.node_pid);
    assert_eq!(controller.context().unwrap().active_id, selected.active_id);
    let stopped = session.stop().unwrap();
    assert_eq!(stopped.state, "stopped");
    assert!(stopped.postgres_pid.is_none() && stopped.node_pid.is_none());
    assert_eq!(stopped.exits.get("node"), Some(&0));
    assert_eq!(stopped.exits.get("postgres"), Some(&0));
    drop(session);
    eprintln!("PHASE same mode-state and normal stop PASS");
    let mut reopened = controller
        .with_current_mutation(token, |service| {
            Session::new(service, payload.clone(), ports)
        })
        .unwrap();
    let second = reopened.start().unwrap();
    assert!(second.readiness.ready);
    assert_ne!(first.job_id, second.job_id);
    assert!(login(ports.web, &configured.tenant_id));
    assert!(
        configuration == fs::read(installation.join("runtime.env")).unwrap(),
        "existing account/configuration changed"
    );
    // Exact owned child, still retained by Session: no PID discovery/adoption.
    assert_eq!(
        unsafe { libc::kill(second.node_pid.unwrap() as i32, libc::SIGTERM) },
        0
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let failed = loop {
        let state = reopened.status().unwrap();
        if state.state == "failed" {
            break state;
        }
        assert!(
            Instant::now() < deadline,
            "owned child failure did not settle"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        failed.failure_code.as_deref(),
        Some("NATIVE_PROCESS_EXITED")
    );
    assert!(failed.postgres_pid.is_none() && failed.node_pid.is_none());
    assert!(installation.join("native-database/PG_VERSION").exists());
    eprintln!("PHASE reopened data and partial failure containment PASS");
    let retry = reopened.start().unwrap();
    assert!(retry.readiness.ready);
    assert_ne!(retry.job_id, failed.job_id);
    assert!(login(ports.web, &configured.tenant_id));
    let final_stop = reopened.stop().unwrap();
    assert_eq!(final_stop.state, "stopped");
    drop(reopened);
    let control_before = fs::read(installation.join("native-database/global/pg_control")).unwrap();
    let holder = TcpListener::bind(("127.0.0.1", ports.web)).unwrap();
    let mut conflict = controller
        .with_current_mutation(token, |service| {
            Session::new(service, payload.clone(), ports)
        })
        .unwrap();
    assert_eq!(conflict.start().err().unwrap().code, "NATIVE_PORT_IN_USE");
    let refused = conflict.status().unwrap();
    assert!(refused.postgres_pid.is_none() && refused.node_pid.is_none());
    assert_eq!(holder.local_addr().unwrap().port(), ports.web);
    assert!(
        control_before == fs::read(installation.join("native-database/global/pg_control")).unwrap()
    );
    assert!(configuration == fs::read(installation.join("runtime.env")).unwrap());
    drop(conflict);
    drop(holder);
    let result = serde_json::json!({"result":"PASS_OWNED_NATIVE_CORE_LIFECYCLE","root":root,"manifest":payload.context().manifest_sha256,"first":first,"stopped":stopped,"reopened":second,"containedFailure":failed,"retry":retry,"finalStop":final_stop,"portConflict":refused,"configurationPreserved":true,"actualModeState":"same native job/PIDs and installation across expert presentation change; GUI not qualified","applicationHostIntegration":false});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    eprintln!("PHASE retry/port conflict/data preservation PASS");
}

#[test]
#[ignore = "Requires a separate new isolated refusal root and exact payload; no service launch"]
fn live_unknown_pid_is_refused_without_adoption_or_signal() {
    use exhibitos_lifecycle::LifecycleService;
    let payload = Arc::new(
        Payload::open(&PathBuf::from(
            std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").expect("exact payload required"),
        ))
        .unwrap(),
    );
    let root = PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_RUNTIME_TEST_ROOT").expect("new isolated root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let service = LifecycleService::new(root.clone()).unwrap();
    // This refusal happens before binding any service port; no socket is needed.
    let ports = Ports::default();
    service
        .prepare_native_installation(
            &payload,
            ports,
            Some(InitialAccountInput {
                subject: "native-synthetic".into(),
                password: "synthetic-refusal-only-password".into(),
            }),
        )
        .unwrap();
    fs::create_dir(root.join("native-database")).unwrap();
    fs::set_permissions(
        root.join("native-database"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let fault = format!("{}\n", std::process::id());
    let path = root.join("native-database/postmaster.pid");
    fs::write(&path, &fault).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut session = Session::new(&service, payload, ports).unwrap();
    assert_eq!(
        session.start().err().unwrap().code,
        "NATIVE_RECOVERY_REQUIRED"
    );
    let status = session.status().unwrap();
    assert!(status.node_pid.is_none() && status.postgres_pid.is_none());
    assert!(!root.join("native-runs").exists());
    assert!(fs::read_to_string(path).unwrap() == fault);
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"result":"PASS_UNKNOWN_LIVE_PID_REFUSED","status":status,"existingPIDFilePreserved":true,"serviceProcessesStarted":0,"nonOwnedSignalsSent":0})).unwrap()).unwrap();
}
