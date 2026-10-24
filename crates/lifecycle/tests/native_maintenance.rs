// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    installations::InstallationController, native_installation::Ports, native_payload::Payload,
    native_runtime::Session,
};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::Arc,
};

#[test]
#[ignore = "Explicit isolated root and admitted payload; starts only owned PG/Node"]
fn actual_empty_database_before_migration_and_writer_quiescence_keep_fence() {
    let payload_root =
        PathBuf::from(std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").expect("explicit payload"));
    let payload = Arc::new(Payload::open(&payload_root).unwrap());
    let root = PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_MAINTENANCE_TEST_ROOT").expect("new isolated root"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let controller = InstallationController::new(root.join("profile"), None).unwrap();
    let context = controller.context().unwrap();
    let token = &context.selection_token;
    let listeners: Vec<_> = (0..3)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let ports = Ports {
        web: listeners[0].local_addr().unwrap().port(),
        api: listeners[1].local_addr().unwrap().port(),
        database: listeners[2].local_addr().unwrap().port(),
    };
    drop(listeners);
    controller
        .with_current_mutation(token, |service| {
            service.prepare_native_personal(&payload, ports)
        })
        .unwrap();
    let installation = PathBuf::from(
        &context
            .installations
            .iter()
            .find(|s| s.id == context.active_id)
            .unwrap()
            .path,
    );
    let original_env = fs::read(installation.join("runtime.env")).unwrap();
    let personal = fs::read(installation.join("native-personal.json")).unwrap();
    let mut session = controller
        .with_current_mutation(token, |service| {
            Session::new(service, payload.clone(), ports)
        })
        .unwrap();
    let database = session.start_database_only().unwrap();
    assert_eq!(database.state, "database_ready");
    assert!(!database.readiness.ready);
    assert!(database.postgres_pid.is_some() && database.node_pid.is_none());
    assert!(TcpStream::connect(("127.0.0.1", ports.web)).is_err());
    assert!(TcpStream::connect(("127.0.0.1", ports.api)).is_err());
    assert!(session.authoring_session().is_err());
    let password = std::str::from_utf8(&original_env)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("POSTGRES_PASSWORD="))
        .unwrap();
    let output = Command::new(payload_root.join("postgres/bin/psql"))
        .env_clear().env("PATH", "/usr/bin:/bin").env("PGPASSWORD", password)
        .args(["-h", "127.0.0.1", "-p", &ports.database.to_string(), "-U", "exhibitos", "-d", "exhibitos", "--no-password", "--no-psqlrc", "-At", "-c", "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S','f')"])
        .output().unwrap();
    assert!(output.status.success());
    assert_eq!(std::str::from_utf8(&output.stdout).unwrap().trim(), "0");
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
    assert_eq!(session.stop().unwrap().exits.get("postgres"), Some(&0));
    // Only now allow normal startup to migrate/bootstrap the same synthetic DB.
    let ready = session.start().unwrap();
    assert!(ready.readiness.ready);
    let quiesced = session.quiesce_for_backup().unwrap();
    assert_eq!(quiesced.state, "database_ready");
    assert_eq!(quiesced.postgres_pid, ready.postgres_pid);
    assert!(quiesced.node_pid.is_none() && !quiesced.readiness.ready);
    assert_eq!(quiesced.exits.get("node"), Some(&0));
    assert!(session.authoring_session().is_err());
    assert!(TcpStream::connect(("127.0.0.1", ports.web)).is_err());
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
    let stopped = session.stop().unwrap();
    assert_eq!(stopped.state, "stopped");
    assert_eq!(stopped.exits.get("postgres"), Some(&0));
    assert_eq!(
        fs::read(installation.join("runtime.env")).unwrap(),
        original_env
    );
    assert_eq!(
        fs::read(installation.join("native-personal.json")).unwrap(),
        personal
    );
    fs::write(root.join("result.json"), serde_json::to_vec_pretty(&serde_json::json!({"scope":"OWNED_PG_ONLY_AND_QUIESCENCE_NOT_FULL_BACKUP_RESTORE", "emptyPublicSchemaBeforeNode":true,"noHTTPListenersDuringMaintenance":true,"samePostgresThroughQuiescence":true,"rootFenceHeld":true,"authoringAuthorityRevoked":true,"configurationAndOwnerPreserved":true,"stopped":stopped})).unwrap()).unwrap();
}
