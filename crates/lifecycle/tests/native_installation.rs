// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    LifecycleService, initial_account::InitialAccountInput, native_installation::Ports,
    native_payload::Payload,
};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "exhibitos-native-installation-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&path).unwrap();
    fs::canonicalize(path).unwrap()
}
fn account() -> InitialAccountInput {
    InitialAccountInput {
        subject: "native-synthetic".into(),
        password: "synthetic-native-test-password".into(),
    }
}
#[test]
#[ignore = "Requires exact packaged payload; creates only new synthetic private roots"]
fn native_configuration_reuses_account_contract_and_preserves_existing_data() {
    let path = std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD")
        .expect("explicit isolated payload required");
    let payload = Payload::open(&PathBuf::from(path)).unwrap();
    let path = root();
    let service = LifecycleService::new(path.clone()).unwrap();
    assert_eq!(
        service
            .prepare_native_installation(&payload, Ports::default(), None)
            .err()
            .unwrap()
            .code,
        "INITIAL_ACCOUNT_REQUIRED"
    );
    assert!(!path.join("runtime.env").exists());
    let info = service
        .prepare_native_installation(&payload, Ports::default(), Some(account()))
        .unwrap();
    assert_eq!(info.state, "configured");
    let text = serde_json::to_string(&info).unwrap();
    assert!(!text.contains("synthetic-native-test-password"));
    assert!(!text.contains("DATABASE_URL"));
    let before = fs::read(path.join("runtime.env")).unwrap();
    assert_eq!(
        fs::metadata(path.join("runtime.env"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        service
            .prepare_native_installation(&payload, Ports::default(), Some(account()))
            .err()
            .unwrap()
            .code,
        "INITIAL_ACCOUNT_EXISTS"
    );
    service
        .prepare_native_installation(&payload, Ports::default(), None)
        .unwrap();
    assert_eq!(fs::read(path.join("runtime.env")).unwrap(), before);
    assert!(!path.join("native-database").exists());
    assert!(!path.join("installed.json").exists());
    fs::write(path.join("native-installation.json"), b"damaged").unwrap();
    assert!(
        service
            .prepare_native_installation(&payload, Ports::default(), None)
            .is_err()
    );
    assert_eq!(
        fs::read(path.join("native-installation.json")).unwrap(),
        b"damaged"
    );
    assert_eq!(fs::read(path.join("runtime.env")).unwrap(), before);
    drop(service);
    fs::remove_dir_all(path).unwrap();
    let path = root();
    let service = LifecycleService::new(path.clone()).unwrap();
    fs::write(
        path.join("installed.json"),
        b"preserved docker installation",
    )
    .unwrap();
    assert_eq!(
        service
            .prepare_native_installation(&payload, Ports::default(), Some(account()))
            .err()
            .unwrap()
            .code,
        "NATIVE_MIGRATION_REQUIRED"
    );
    assert_eq!(
        fs::read(path.join("installed.json")).unwrap(),
        b"preserved docker installation"
    );
    assert!(!path.join("runtime.env").exists());
    drop(service);
    fs::remove_dir_all(path).unwrap();
}
