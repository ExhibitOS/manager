// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    installations::{InstallationController, native_ui::Request},
    native_host::Host,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

fn fixture() -> (PathBuf, Arc<InstallationController>) {
    let root =
        std::env::temp_dir().join(format!("native-maintenance-wire-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let controller = Arc::new(InstallationController::new(root.join("profile"), None).unwrap());
    (root, controller)
}
fn request(token: &str, input: Value) -> Request {
    serde_json::from_value(json!({"version":1,"id":1,"command":"native_maintenance","selection_token":token,"maintenance":input})).unwrap()
}
fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn closed_maintenance_wire_requires_host_and_selection_never_replaces_the_original() {
    let (root, controller) = fixture();
    let original = controller.context().unwrap();
    let input = json!({"action":"create_recovery","acknowledged":true});
    assert_eq!(
        request(&original.selection_token, input.clone())
            .execute(&controller)
            .unwrap_err()
            .code,
        "NATIVE_HOST_REQUIRED"
    );
    let mut host = Host::new(controller.clone(), root.join("absent-resources-never-read"));
    assert!(
        host.handle(request(
            &original.selection_token,
            json!({"action":"create_recovery","acknowledged":false})
        ))
        .is_err()
    );
    assert_eq!(controller.context().unwrap().active_id, original.active_id);
    let original_path = PathBuf::from(
        &original
            .installations
            .iter()
            .find(|v| v.id == original.active_id)
            .unwrap()
            .path,
    );
    private(&original_path.join("retained-user-data"), b"preserved");
    let fresh = host
        .handle(request(&original.selection_token, input.clone()))
        .unwrap();
    let token = fresh["selectionToken"].as_str().unwrap();
    assert_ne!(fresh["activeId"], original.active_id);
    assert_eq!(fresh["installations"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read(original_path.join("retained-user-data")).unwrap(),
        b"preserved"
    );
    assert_eq!(
        host.handle(request(
            &original.selection_token,
            json!({"action":"create_key","path":root.join("not-created.key")})
        ))
        .unwrap_err()
        .code,
        "INSTALLATION_SELECTION_CHANGED"
    );
    assert!(!root.join("not-created.key").exists());
    let back = host
        .handle(request(
            token,
            json!({"action":"select","id":original.active_id,"acknowledged":true}),
        ))
        .unwrap();
    assert_eq!(back["activeId"], original.active_id);
    for field in [
        "account",
        "state",
        "backup",
        "document",
        "transfer",
        "publication",
        "export",
    ] {
        let mut wrong = json!({"version":1,"id":1,"command":"native_maintenance","selection_token":token,"maintenance":input});
        wrong[field] = json!({});
        assert!(serde_json::from_value::<Request>(wrong).is_err());
    }
    for wrong in [
        json!({"action":"run","shell":"never"}),
        json!({"action":"create_key","path":"/never","shell":"never"}),
        json!({"action":"select","id":original.active_id}),
        json!({"action":"activate_restore","id":"invalid","keyFile":"/never"}),
    ] {
        assert!(serde_json::from_value::<Request>(json!({"version":1,"id":1,"command":"native_maintenance","selection_token":token,"maintenance":wrong})).is_err());
    }
    drop(host);
    drop(controller);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cold_pending_hint_preserves_candidate_and_rejects_invalid_marker_or_cross_job_plan() {
    let (root, controller) = fixture();
    let context = controller.context().unwrap();
    let token = &context.selection_token;
    assert!(
        controller
            .native_pending_restoration(token)
            .unwrap()
            .is_none()
    );
    let selected = PathBuf::from(
        &context
            .installations
            .iter()
            .find(|v| v.id == context.active_id)
            .unwrap()
            .path,
    );
    let id = uuid::Uuid::new_v4().to_string();
    let directory = selected.join(format!("native-restore-{id}"));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = selected.join("native-restore-current.json");
    private(
        &marker,
        &serde_json::to_vec(&json!({"format":1,"id":id})).unwrap(),
    );
    assert_eq!(
        controller
            .native_pending_restoration(token)
            .unwrap()
            .unwrap()
            .state,
        "interrupted"
    );
    let plan = directory.join("stage.json");
    private(
        &plan,
        &serde_json::to_vec(&json!({"id":uuid::Uuid::new_v4(),"backupId":uuid::Uuid::new_v4()}))
            .unwrap(),
    );
    assert!(controller.native_pending_restoration(token).is_err());
    private(
        &plan,
        &serde_json::to_vec(&json!({"id":id,"backupId":uuid::Uuid::new_v4()})).unwrap(),
    );
    // A presentation hint is deliberately not proof that this incomplete plan
    // can activate. The full core independently validates every staged field.
    assert_eq!(
        controller
            .native_pending_restoration(token)
            .unwrap()
            .unwrap()
            .state,
        "staged"
    );
    private(&marker, b"{\"format\":1,\"id\":\"../other\"}");
    assert!(controller.native_pending_restoration(token).is_err());
    assert!(plan.exists());
    drop(controller);
    fs::remove_dir_all(root).unwrap();
}
