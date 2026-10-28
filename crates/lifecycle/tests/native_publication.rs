// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::{
    installations::{InstallationController, native_publication::Pending},
    native_installation::Ports,
    native_payload::Payload,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};
#[test]
#[ignore = "Explicit owned new root and qualified local payload only"]
fn publication_copy_intent_is_private_immutable_revision_checked_and_scope_bound() {
    let payload = Payload::open(&PathBuf::from(
        std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD").unwrap(),
    ))
    .unwrap();
    let root = PathBuf::from(std::env::var_os("EXHIBITOS_PUBLICATION_TEST_ROOT").unwrap());
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let c = InstallationController::new(root.join("profile"), None).unwrap();
    let context = c.context().unwrap();
    c.with_current_mutation(&context.selection_token, |s| {
        s.prepare_native_personal(&payload, Ports::default())
    })
    .unwrap();
    let empty = c.native_publication(&context.selection_token).unwrap();
    let original_document = c.native_document(&context.selection_token).unwrap();
    let mut state = empty.clone();
    let source = uuid::Uuid::new_v4().to_string();
    let copy = uuid::Uuid::new_v4().to_string();
    state.pending = Some(Pending {
        source_exhibition_id: source,
        source_etag: format!("\"studio-r1-{}\"", "a".repeat(64)),
        snapshot: serde_json::json!({"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.1","id":uuid::Uuid::new_v4().to_string(),"exhibitionId":copy,"editVersion":1,"candidate":{"id":copy}}),
        save_request_id: uuid::Uuid::new_v4().to_string(),
        snapshot_etag: None,
        publish_request_id: uuid::Uuid::new_v4().to_string(),
        plan_binding_sha256: None,
    });
    let saved = c
        .save_native_publication(&context.selection_token, state)
        .unwrap();
    assert_eq!(saved.revision, 1);
    let restored = c.native_publication(&context.selection_token).unwrap();
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(&restored).unwrap()
    );
    assert_eq!(
        c.save_native_publication(&context.selection_token, empty)
            .err()
            .unwrap()
            .code,
        "NATIVE_PUBLICATION_STATE_CHANGED"
    );
    let mut foreign = saved.clone();
    foreign.principal_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        c.save_native_publication(&context.selection_token, foreign)
            .err()
            .unwrap()
            .code,
        "NATIVE_PUBLICATION_SCOPE_CHANGED"
    );
    let mut changed = saved.clone();
    changed.pending.as_mut().unwrap().publish_request_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        c.save_native_publication(&context.selection_token, changed)
            .err()
            .unwrap()
            .code,
        "NATIVE_PUBLICATION_REQUEST_CHANGED"
    );
    let mut accepted = saved.clone();
    accepted.pending.as_mut().unwrap().snapshot_etag =
        Some(format!("\"studio-r1-{}\"", "b".repeat(64)));
    let accepted = c
        .save_native_publication(&context.selection_token, accepted)
        .unwrap();
    let mut changed = accepted.clone();
    changed.pending.as_mut().unwrap().snapshot_etag = None;
    assert_eq!(
        c.save_native_publication(&context.selection_token, changed)
            .err()
            .unwrap()
            .code,
        "NATIVE_PUBLICATION_REQUEST_CHANGED"
    );
    assert_eq!(
        serde_json::to_value(c.native_document(&context.selection_token).unwrap()).unwrap(),
        serde_json::to_value(original_document).unwrap()
    );
    let path = fs::read_dir(root.join("profile"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("native-publication-")
        })
        .unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let retained = root.join("valid-publication-preserved.json");
    fs::rename(&path, &retained).unwrap();
    fs::write(&path, b"{}").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(c.native_publication(&context.selection_token).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{}");
    fs::rename(&path, root.join("corrupt-publication-preserved.json")).unwrap();
    symlink(&retained, &path).unwrap();
    assert!(c.native_publication(&context.selection_token).is_err());
}
