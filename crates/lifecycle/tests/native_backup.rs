// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::installations::{InstallationController, native_ui::Request};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn backup_command_is_closed_and_requires_the_async_host() {
    let token = uuid::Uuid::new_v4().to_string();
    let valid = json!({"version":1,"id":1,"command":"native_backup","selection_token":token,"backup":{"keyFile":"/not-read/key.bin","destination":"/not-written/backup"}});
    let request: Request = serde_json::from_value(valid.clone()).unwrap();
    let root = std::env::temp_dir().join(format!(
        "exhibitos-native-backup-wire-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let controller = InstallationController::new(root.join("profile"), None).unwrap();
    assert_eq!(
        request.execute(&controller).err().unwrap().code,
        "NATIVE_HOST_REQUIRED"
    );
    for field in [
        "state",
        "account",
        "transfer",
        "document",
        "publication",
        "export",
    ] {
        let mut wrong = valid.clone();
        wrong[field] = json!({});
        assert!(serde_json::from_value::<Request>(wrong).is_err());
    }
    let mut wrong = valid.clone();
    wrong["backup"]["shell"] = json!("never execute");
    assert!(serde_json::from_value::<Request>(wrong).is_err());
    let mut wrong = valid;
    wrong["command"] = json!("presentation");
    assert!(serde_json::from_value::<Request>(wrong).is_err());
    drop(controller);
    fs::remove_dir_all(root).unwrap();
}
