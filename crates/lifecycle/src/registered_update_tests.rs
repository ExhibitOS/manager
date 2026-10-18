// SPDX-License-Identifier: Apache-2.0
//! Synthetic storage/dispatch tests. No Docker, runtime, operational key or full-update success claim.
use super::*;
use crate::{execution_reference as refs, update_execution_handoff as handoff};
use serde_json::Value;
fn registered_fixture() -> (PathBuf, String, u64, String) {
    let (profile, key, policy, mut release) = tests::fixture();
    release.artifact.schema_sha256 = "a".repeat(64);
    let envelope = tests::seal(&key, &release);
    let mut store = Store::provision(&profile, "default", policy, 10).unwrap();
    let mut verified = store.verify_for_preparation(&envelope, 20).unwrap();
    verified
        .verify_artifact(&mut b"fixture".as_slice())
        .unwrap();
    let mut plan = tests::plan();
    plan.source_instance = uuid::Uuid::new_v4().to_string();
    plan.target_instance = uuid::Uuid::new_v4().to_string();
    plan.target_schema = plan.source_schema.clone();
    let installation = plan.source_instance.clone();
    store
        .prepare_update(&envelope, &verified, plan, 20)
        .unwrap();
    let generation = store.receipt().generation;
    drop(store);
    let registry = crate::installations::Registry {
        format: 1,
        active_id: installation.clone(),
        installations: vec![crate::installations::Entry {
            id: installation.clone(),
            kind: "default".into(),
            created_at: 1,
        }],
    };
    crate::installations::save(&profile, &registry, None).unwrap();
    let (_, registry_bytes) = crate::installations::load(&profile).unwrap().unwrap();
    drop(
        crate::private_options()
            .open(profile.join("operation.lock"))
            .unwrap(),
    );
    let root = profile.join("update-execution-references");
    crate::installations::new_directory(&root).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let external = profile.parent().unwrap();
    let record = refs::Reference {
        format: 1,
        id: id.clone(),
        installation_id: installation.clone(),
        scope: "default".into(),
        operation_id: "update-1".into(),
        generation,
        registry_sha256: crate::digest(&registry_bytes),
        artifact: external.join("runtime.tar"),
        artifact_sha256: crate::digest(b"fixture"),
        artifact_bytes: 7,
        python: external.join("unused-python"),
        source_commit: "a".repeat(40),
        maintenance_image: format!("sha256:{}", "a".repeat(64)),
        export_parent: external.into(),
        host_archive: external.join("unused-host"),
        trust_archive: external.join("unused-trust"),
        pair_binding: external.join("unused-pair"),
        key_file: external.join("unused-key"),
        qualification_destination: external.join("old-qualification"),
        qualification_sha256: "b".repeat(64),
        created_at: 20,
    };
    crate::write_private_new(
        &root,
        &format!("{id}.json"),
        &serde_json::to_vec(&record).unwrap(),
        16384,
    )
    .unwrap();
    (profile, installation, generation, id)
}
fn input(id: &str, generation: u64) -> handoff::ApplyInput {
    handoff::ApplyInput {
        reference_id: id.into(),
        operation_id: "update-1".into(),
        generation,
        preserve_data: true,
        close_app_and_apply: true,
        external_writers_quiesced: true,
    }
}
#[test]
fn registered_update_reference_list_is_readonly_safe_and_selection_bound() {
    let (profile, installation, generation, id) = registered_fixture();
    let (_, bytes) = crate::installations::load(&profile).unwrap().unwrap();
    let root = profile.join("update-execution-references");
    let before = fs::read(root.join(format!("{id}.json"))).unwrap();
    let summaries = refs::list(&profile, &installation, "default", &bytes).unwrap();
    assert_eq!(summaries.len(), 1);
    let summary = serde_json::to_value(&summaries[0]).unwrap();
    assert_eq!(summary.as_object().unwrap().len(), 9);
    assert_eq!(summary["generation"], generation);
    assert_eq!(summary["referenceId"], id);
    assert_eq!(summary["version"], "0.2.0-dev.1");
    assert_eq!(summary["changedSchema"], false);
    assert_eq!(summary["executionRequalificationRequired"], true);
    for secret in [
        "keyFile",
        "artifact",
        "python",
        "exportParent",
        "qualificationDestination",
        "qualificationSha256",
    ] {
        assert!(summary.get(secret).is_none());
    }
    assert_eq!(fs::read(root.join(format!("{id}.json"))).unwrap(), before);
    assert!(
        refs::list(&profile, &installation, "default", b"changed registry")
            .unwrap()
            .is_empty()
    );
    assert!(refs::load(&profile, "../escape").is_err());
    let mut malformed: Value = serde_json::from_slice(&before).unwrap();
    malformed["untrustedSuccess"] = serde_json::json!(true);
    fs::write(
        root.join(format!("{id}.json")),
        serde_json::to_vec(&malformed).unwrap(),
    )
    .unwrap();
    assert!(refs::load(&profile, &id).is_err());
}
#[test]
fn registered_update_apply_requires_consent_and_current_generation_without_side_effects() {
    let (profile, installation, generation, id) = registered_fixture();
    let (_, bytes) = crate::installations::load(&profile).unwrap().unwrap();
    let mut denied = input(&id, generation);
    denied.external_writers_quiesced = false;
    assert!(handoff::prepare(&profile, &installation, "default", &bytes, denied).is_err());
    assert!(!profile.join("update-apply-handoffs").exists());
    let mut stale = input(&id, generation);
    stale.generation += 1;
    assert!(handoff::prepare(&profile, &installation, "default", &bytes, stale).is_err());
    assert!(!profile.join("update-apply-handoffs").exists());
    let mut operation = input(&id, generation);
    operation.operation_id = "other".into();
    assert!(handoff::prepare(&profile, &installation, "default", &bytes, operation).is_err());
    assert!(!profile.join("update-apply-handoffs").exists());
}
#[test]
fn registered_update_helper_waits_for_app_close_then_requires_real_requalification() {
    let (profile, installation, generation, id) = registered_fixture();
    let (_, bytes) = crate::installations::load(&profile).unwrap().unwrap();
    let request = handoff::prepare(
        &profile,
        &installation,
        "default",
        &bytes,
        input(&id, generation),
    )
    .unwrap();
    let session = crate::profile_backup::session_lock(&profile, false).unwrap();
    let child_profile = profile.clone();
    let child_id = request.id.clone();
    let helper = std::thread::spawn(move || handoff::execute(&child_profile, &child_id));
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "uncertain"
    );
    assert!(
        profile
            .join("update-apply-handoffs")
            .join(format!("{}.claim.json", request.id))
            .exists()
    );
    drop(session);
    let result = helper.join().unwrap();
    assert!(result.is_err());
    let receipt = handoff::latest(&profile, &installation).unwrap().unwrap();
    assert_eq!(receipt.state, "failed");
    assert!(receipt.code.is_some());
    assert_eq!(
        inspect_update_status(&profile, "default").unwrap().stage,
        Some(crate::update::Stage::Prepared)
    );
    assert_eq!(
        crate::installations::load(&profile).unwrap().unwrap().1,
        bytes
    );
    assert!(!profile.parent().unwrap().join("unused-key").exists());
    assert!(handoff::execute(&profile, &request.id).is_err());
}
#[test]
fn registered_update_reference_change_after_request_is_refused_without_authority_write() {
    let (profile, installation, generation, id) = registered_fixture();
    let (_, bytes) = crate::installations::load(&profile).unwrap().unwrap();
    let request = handoff::prepare(
        &profile,
        &installation,
        "default",
        &bytes,
        input(&id, generation),
    )
    .unwrap();
    let path = profile
        .join("update-execution-references")
        .join(format!("{id}.json"));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["createdAt"] = serde_json::json!(21);
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(
        handoff::execute(&profile, &request.id).unwrap_err().code,
        "UPDATE_REFERENCE_STALE"
    );
    assert_eq!(
        inspect_update_status(&profile, "default")
            .unwrap()
            .generation,
        Some(generation)
    );
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "failed"
    );
}

#[test]
fn registered_update_duplicate_helper_cannot_publish_outcome_while_owner_waits() {
    let (profile, installation, generation, id) = registered_fixture();
    let (_, bytes) = crate::installations::load(&profile).unwrap().unwrap();
    let request = handoff::prepare(
        &profile,
        &installation,
        "default",
        &bytes,
        input(&id, generation),
    )
    .unwrap();
    let session = crate::profile_backup::session_lock(&profile, false).unwrap();
    let first_profile = profile.clone();
    let first_id = request.id.clone();
    let owner = std::thread::spawn(move || handoff::execute(&first_profile, &first_id));
    let root = profile.join("update-apply-handoffs");
    let claim = root.join(format!("{}.claim.json", request.id));
    let outcome = root.join(format!("{}.outcome.json", request.id));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !claim.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        claim.exists(),
        "helper must own the request before waiting for app close"
    );
    let original_claim = fs::read(&claim).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let second_profile = profile.clone();
    let second_id = request.id.clone();
    let duplicate = std::thread::spawn(move || {
        send.send(handoff::execute(&second_profile, &second_id).map_err(|e| e.code))
            .unwrap();
    });
    let rejected = receive
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("competing helper must not enter the anchor wait/timeout path");
    assert_eq!(rejected.unwrap_err(), "UPDATE_HANDOFF_ALREADY_CLAIMED");
    assert!(!outcome.exists());
    assert_eq!(fs::read(&claim).unwrap(), original_claim);
    assert_eq!(
        inspect_update_status(&profile, "default")
            .unwrap()
            .generation,
        Some(generation)
    );
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "uncertain"
    );
    duplicate.join().unwrap();
    drop(session);
    assert!(owner.join().unwrap().is_err());
    assert_eq!(
        handoff::latest(&profile, &installation)
            .unwrap()
            .unwrap()
            .state,
        "failed"
    );
    assert_eq!(fs::read(claim).unwrap(), original_claim);
    assert_eq!(
        crate::installations::load(&profile).unwrap().unwrap().1,
        bytes
    );
}
