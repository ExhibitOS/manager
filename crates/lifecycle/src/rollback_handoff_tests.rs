// SPDX-License-Identifier: Apache-2.0
//! Synthetic dispatch/storage only: no Engine, operational archive or key is accessed.
use super::*;
use crate::update_rollback_handoff as handoff;
fn fixture() -> (PathBuf, String, u64) {
    let (profile, store) = rollback_registration::tests::fixture();
    let generation = store.receipt().generation;
    let installation = installations::load(&profile).unwrap().unwrap().0.active_id;
    drop(store);
    drop(
        crate::private_options()
            .open(profile.join("operation.lock"))
            .unwrap(),
    );
    (profile, installation, generation)
}
fn input(profile: &Path, generation: u64) -> handoff::RollbackInput {
    handoff::RollbackInput {
        action: handoff::RollbackAction::RestoreOriginal,
        operation_id: "update-1".into(),
        generation,
        maintenance_image: format!("sha256:{}", "a".repeat(64)),
        key_file: Some(profile.parent().unwrap().join("unused-key")),
        archive: Some(profile.parent().unwrap().join("missing-original-backup")),
        port: Some(50449),
        preserve_data: true,
        close_app_and_rollback: true,
        external_writers_quiesced: true,
    }
}
fn prepare(profile: &Path, installation: &str, input: handoff::RollbackInput) -> handoff::Receipt {
    let bytes = installations::load(profile).unwrap().unwrap().1;
    handoff::prepare(profile, installation, "default", &bytes, input).unwrap()
}
fn retire(profile: &Path) {
    fs::remove_dir_all(profile.parent().unwrap()).unwrap();
}
#[test]
fn rollback_handoff_context_is_readonly_safe_and_selection_bound() {
    let (profile, installation, generation) = fixture();
    let store = Store::open_for_inspection(&profile, "default").unwrap();
    let head = store.current_sha256.clone();
    let root = store.root.clone();
    drop(store);
    let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
    let selected = controller.context().unwrap();
    let context = controller
        .update_rollback_context(&selected.selection_token)
        .unwrap()
        .unwrap();
    assert_eq!(context.installation_id, installation);
    assert_eq!(context.generation, generation);
    assert_eq!(context.operation_id, "update-1");
    assert!(context.can_restore_original && !context.can_complete_restored);
    let json = serde_json::to_value(&context).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 12);
    for forbidden in ["keyFile", "archive", "scope", "candidatePath", "health"] {
        assert!(json.get(forbidden).is_none());
    }
    assert!(
        controller
            .update_rollback_context(&uuid::Uuid::new_v4().to_string())
            .is_err()
    );
    assert_eq!(
        hash(&fs::read(root.join(format!("{generation:020}.json"))).unwrap()),
        head
    );
    assert!(!profile.join("update-rollback-handoffs").exists());
    drop(controller);
    retire(&profile);
}
#[test]
fn rollback_handoff_requires_consent_exact_action_inputs_and_current_authority() {
    let (profile, installation, generation) = fixture();
    let bytes = installations::load(&profile).unwrap().unwrap().1;
    let mut cases = vec![];
    let mut v = input(&profile, generation);
    v.external_writers_quiesced = false;
    cases.push(v);
    let mut v = input(&profile, generation);
    v.preserve_data = false;
    cases.push(v);
    let mut v = input(&profile, generation);
    v.close_app_and_rollback = false;
    cases.push(v);
    let mut v = input(&profile, generation);
    v.key_file = None;
    cases.push(v);
    let mut v = input(&profile, generation);
    v.port = Some(1023);
    cases.push(v);
    let mut v = input(&profile, generation);
    v.action = handoff::RollbackAction::CompleteRestored;
    cases.push(v);
    let mut v = input(&profile, generation);
    v.generation += 1;
    cases.push(v);
    for v in cases {
        assert!(handoff::prepare(&profile, &installation, "default", &bytes, v).is_err());
    }
    let mut closed = serde_json::to_value(input(&profile, generation)).unwrap();
    closed["scope"] = serde_json::json!("default");
    assert!(serde_json::from_value::<handoff::RollbackInput>(closed).is_err());
    assert!(!profile.join("update-rollback-handoffs").exists());
    retire(&profile);
}
#[test]
fn rollback_handoff_stale_stage_is_refused_before_interruption_recovery() {
    let (profile, installation, generation) = fixture();
    let receipt = prepare(&profile, &installation, input(&profile, generation));
    let mut store = Store::open_for_prepared_handoff(&profile, "default").unwrap();
    store
        .begin_restore("update-1", generation, uuid::Uuid::new_v4().to_string(), 23)
        .unwrap();
    let changed = store.receipt().generation;
    let head = store.current_sha256.clone();
    drop(store);
    assert!(handoff::execute(&profile, &receipt.id).is_err());
    let store = Store::open_for_inspection(&profile, "default").unwrap();
    assert_eq!(store.current.generation, changed);
    assert_eq!(store.current_sha256, head);
    assert_eq!(
        store.intent().unwrap().update.stage(),
        crate::update::Stage::Restoring
    );
    drop(store);
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "failed"
    );
    retire(&profile);
}
#[test]
fn rollback_handoff_waits_for_app_close_single_owner_and_never_claims_restore_success() {
    let (profile, installation, generation) = fixture();
    let receipt = prepare(&profile, &installation, input(&profile, generation));
    let source = profile.join("local-runtime/source-witness");
    let target = profile
        .join("installations")
        .join("cc414c3c-dd99-45e2-8307-131a49f72d68")
        .join("target-witness");
    fs::write(&source, b"preserve-source").unwrap();
    fs::write(&target, b"preserve-target").unwrap();
    let (_, app) = profile_backup::anchor_lock(&profile, false).unwrap();
    let thread_profile = profile.clone();
    let thread_id = receipt.id.clone();
    let worker = std::thread::spawn(move || handoff::execute(&thread_profile, &thread_id));
    let claim = profile
        .join("update-rollback-handoffs")
        .join(format!("{}.claim.json", receipt.id));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !claim.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(claim.exists());
    assert_eq!(
        installations::load(&profile)
            .unwrap()
            .unwrap()
            .0
            .installations
            .len(),
        2
    );
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "uncertain"
    );
    assert_eq!(
        handoff::execute(&profile, &receipt.id).unwrap_err().code,
        "UPDATE_HANDOFF_ALREADY_CLAIMED"
    );
    assert!(
        !profile
            .join("update-rollback-handoffs")
            .join(format!("{}.outcome.json", receipt.id))
            .exists()
    );
    drop(app);
    // The missing archive refuses before any Engine or key access. Registration alone is never success.
    assert!(worker.join().unwrap().is_err());
    let outcome = handoff::latest(&profile, &installation).unwrap().unwrap();
    assert_eq!(outcome.state, "failed");
    assert_eq!(outcome.code.as_deref(), Some("BACKUP_PATH_INVALID"));
    let registry = installations::load(&profile).unwrap().unwrap().0;
    assert_eq!(registry.active_id, installation);
    assert_eq!(registry.installations.len(), 3);
    assert_eq!(fs::read(source).unwrap(), b"preserve-source");
    assert_eq!(fs::read(target).unwrap(), b"preserve-target");
    retire(&profile);
}

#[test]
fn rollback_native_tool_preparation_requires_current_authorized_intent_and_leaves_reads_untouched() {
    let (profile, _, generation) = fixture();
    let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
    let selected = controller.context().unwrap();
    // Invalid packaged metadata proves whether preparation was reached without invoking an engine.
    let resources = profile.parent().unwrap().join("invalid-tool-resources");
    fs::create_dir(&resources).unwrap();
    fs::write(resources.join("prepared.json"), b"not packaged metadata").unwrap();
    let token = &selected.selection_token;
    assert!(controller.update_rollback_handoff_prepared(token, None, &resources).unwrap().1.is_none());
    let mut unconsented = input(&profile, generation);
    unconsented.external_writers_quiesced = false;
    assert_eq!(controller.update_rollback_handoff_prepared(token, Some(unconsented), &resources).err().unwrap().code, "UPDATE_HANDOFF_CONSENT_REQUIRED");
    let stale = input(&profile, generation + 1);
    assert_eq!(controller.update_rollback_handoff_prepared(token, Some(stale), &resources).err().unwrap().code, "UPDATE_HANDOFF_AUTHORITY_CHANGED");
    assert_eq!(controller.update_rollback_handoff_prepared(&uuid::Uuid::new_v4().to_string(), Some(input(&profile, generation)), &resources).err().unwrap().code, "INSTALLATION_SELECTION_CHANGED");
    assert_eq!(controller.update_rollback_handoff_prepared(token, Some(input(&profile, generation)), &resources).err().unwrap().code, "PLATFORM_RUNTIME_INVALID");
    assert!(!profile.join("update-rollback-handoffs").exists());
    // Missing declaration preserves the existing explicitly selected custom-tool path.
    fs::remove_file(resources.join("prepared.json")).unwrap();
    let receipt = controller.update_rollback_handoff_prepared(token, Some(input(&profile, generation)), &resources).unwrap().1.unwrap();
    assert_eq!(receipt.state, "awaiting-app-close");
    drop(controller);
    retire(&profile);
}

#[cfg(not(windows))]
#[test]
fn rollback_prepared_tool_waits_for_external_orphan_reconciliation_in_both_owners() {
    for profile_owner in [true, false] {
        let (profile, _, generation) = fixture();
        let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
        let selected = controller.context().unwrap();
        let current_root = controller.with_current(&selected.selection_token, |service| Ok(service.root.clone())).unwrap();
        let owner = if profile_owner { &profile } else { &current_root };
        let mut job = crate::external_restoration::fixture_job();
        job.state = "interrupted".into();job.stage = "interrupted".into();job.error_code = Some("INTERRUPTED".into());
        let folder = owner.join(format!("external-restoration-{}", job.id));
        crate::restoration::directory(&folder).unwrap();
        crate::write_json(&folder, "job.json", &job).unwrap();
        let resources = profile.parent().unwrap().join("invalid-tool-resources");
        fs::create_dir(&resources).unwrap();fs::write(resources.join("prepared.json"), b"not packaged metadata").unwrap();
        assert!(controller.update_rollback_handoff_prepared(&selected.selection_token, None, &resources).unwrap().1.is_none());
        assert_eq!(controller.update_rollback_handoff_prepared(&selected.selection_token, Some(input(&profile, generation)), &resources).err().unwrap().code, "CANCEL_RECOVERY_REQUIRED");
        assert!(!profile.join("update-rollback-handoffs").exists());assert!(folder.join("job.json").is_file());
        drop(controller);retire(&profile);
    }
}
