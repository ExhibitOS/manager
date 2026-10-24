// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    installations::{InstallationController, native_document::Pending},
    native_installation::Ports,
    native_payload::Payload,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};
#[test]
#[ignore = "Requires a qualified local validation payload and an explicitly owned new test root"]
fn retained_document_request_cas_scope_and_corruption_refusals() {
    let payload = Payload::open(&PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").unwrap(),
    ))
    .unwrap();
    let root = PathBuf::from(std::env::var_os("EXHIBITOS_DOCUMENT_TEST_ROOT").unwrap());
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let controller = InstallationController::new(root.join("profile"), None).unwrap();
    let context = controller.context().unwrap();
    controller
        .with_current_mutation(&context.selection_token, |s| {
            s.prepare_native_personal(&payload, Ports::default())
        })
        .unwrap();
    let empty = controller
        .native_document(&context.selection_token)
        .unwrap();
    let mut value = empty.clone();
    let id = uuid::Uuid::new_v4().to_string();
    value.draft = Some(
        serde_json::json!({"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.1","exhibitionId":id,"id":uuid::Uuid::new_v4().to_string(),"editVersion":1,"candidate":{"id":id},"extensions":{"retained":"합성 원문"}}),
    );
    value.pending = Some(Pending {
        request_id: uuid::Uuid::new_v4().to_string(),
        etag: None,
        draft: value.draft.clone().unwrap(),
    });
    let saved = controller
        .save_native_document(&context.selection_token, value.clone())
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        serde_json::to_value(
            controller
                .native_document(&context.selection_token)
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    assert_eq!(
        controller
            .save_native_document(&context.selection_token, empty)
            .err()
            .unwrap()
            .code,
        "NATIVE_DOCUMENT_STATE_CHANGED"
    );
    let mut foreign = saved.clone();
    foreign.principal_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        controller
            .save_native_document(&context.selection_token, foreign)
            .err()
            .unwrap()
            .code,
        "NATIVE_DOCUMENT_SCOPE_CHANGED"
    );
    let mut changed = saved.clone();
    changed.pending.as_mut().unwrap().draft["exhibitionId"] =
        serde_json::json!(uuid::Uuid::new_v4().to_string());
    assert_eq!(
        controller
            .save_native_document(&context.selection_token, changed)
            .err()
            .unwrap()
            .code,
        "NATIVE_DOCUMENT_INVALID"
    );
    let path = fs::read_dir(root.join("profile"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("native-document-")
        })
        .unwrap();
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"corrupt").unwrap();
    assert_eq!(
        controller
            .native_document(&context.selection_token)
            .err()
            .unwrap()
            .code,
        "NATIVE_DOCUMENT_INVALID"
    );
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    fs::rename(&path, root.join("corrupt-preserved.json")).unwrap();
    let target = root.join("symlink-target");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, &path).unwrap();
    assert!(
        controller
            .native_document(&context.selection_token)
            .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    fs::rename(&path, root.join("symlink-preserved")).unwrap();
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        controller
            .native_document(&context.selection_token)
            .unwrap()
            .pending
            .unwrap()
            .request_id,
        saved.pending.unwrap().request_id
    );
}
