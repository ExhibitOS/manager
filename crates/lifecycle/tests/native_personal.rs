// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    installations::InstallationController, native_installation::Ports, native_payload::Payload,
};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
#[test]
#[ignore = "Requires exact native payload and a new explicitly owned private test directory"]
fn personal_owner_mismatch_and_legacy_configuration_are_preserved() {
    let payload = Payload::open(&PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").expect("payload required"),
    ))
    .unwrap();
    let root = PathBuf::from(
        std::env::var_os("EXHIBITOS_PERSONAL_REFUSAL_ROOT").expect("new root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let controller = InstallationController::new(root.join("personal"), None).unwrap();
    let context = controller.context().unwrap();
    controller
        .with_current_mutation(&context.selection_token, |s| {
            s.prepare_native_personal(&payload, Ports::default())
        })
        .unwrap();
    controller
        .with_current(&context.selection_token, |s| {
            let path = PathBuf::from(
                &context
                    .installations
                    .iter()
                    .find(|i| i.id == context.active_id)
                    .unwrap()
                    .path,
            )
            .join("native-personal.json");
            let original = fs::read(&path).unwrap();
            let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
            record["ownerUid"] = serde_json::json!(unsafe { libc::geteuid() } + 1);
            let changed = serde_json::to_vec(&record).unwrap();
            fs::write(&path, &changed).unwrap();
            assert_eq!(
                s.native_prepared().err().unwrap().code,
                "NATIVE_PERSONAL_OWNER_MISMATCH"
            );
            assert_eq!(fs::read(&path).unwrap(), changed);
            fs::write(&path, original).unwrap();
            assert_eq!(s.native_prepared()?.unwrap().kind, "personal");
            Ok(())
        })
        .unwrap();
    let original_transfer = controller.native_upload(&context.selection_token).unwrap();
    let saved_transfer = controller
        .save_native_upload(&context.selection_token, original_transfer.clone())
        .unwrap();
    assert_eq!(saved_transfer.revision, 1);
    assert_eq!(
        controller
            .save_native_upload(&context.selection_token, original_transfer)
            .err()
            .unwrap()
            .code,
        "NATIVE_UPLOAD_STATE_CHANGED"
    );
    let mut foreign = saved_transfer.clone();
    foreign.principal_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        controller
            .save_native_upload(&context.selection_token, foreign)
            .err()
            .unwrap()
            .code,
        "NATIVE_UPLOAD_SCOPE_CHANGED"
    );
    let transfer_path = fs::read_dir(root.join("personal"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("native-upload-")
        })
        .unwrap();
    let original = fs::read(&transfer_path).unwrap();
    fs::write(&transfer_path, b"broken").unwrap();
    assert!(controller.native_upload(&context.selection_token).is_err());
    assert!(
        controller
            .save_native_upload(&context.selection_token, saved_transfer)
            .is_err()
    );
    assert_eq!(fs::read(&transfer_path).unwrap(), b"broken");
    let preserved = root.join("preserved-native-upload.json");
    fs::write(&preserved, &original).unwrap();
    fs::remove_file(&transfer_path).unwrap();
    std::os::unix::fs::symlink(&preserved, &transfer_path).unwrap();
    assert!(controller.native_upload(&context.selection_token).is_err());
    assert_eq!(fs::read(&preserved).unwrap(), original);
    let legacy = InstallationController::new(root.join("legacy"), None).unwrap();
    let context = legacy.context().unwrap();
    let path = PathBuf::from(
        &context
            .installations
            .iter()
            .find(|i| i.id == context.active_id)
            .unwrap()
            .path,
    );
    let env = path.join("runtime.env");
    fs::write(&env, b"legacy synthetic bytes must remain").unwrap();
    fs::set_permissions(&env, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        legacy
            .with_current_mutation(&context.selection_token, |s| s
                .prepare_native_personal(&payload, Ports::default()))
            .err()
            .unwrap()
            .code,
        "NATIVE_MIGRATION_REQUIRED"
    );
    assert_eq!(
        fs::read(&env).unwrap(),
        b"legacy synthetic bytes must remain"
    );
    assert!(!path.join("native-personal.json").exists());
}
