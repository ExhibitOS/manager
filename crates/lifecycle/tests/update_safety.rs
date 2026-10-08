// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::update::*;
use serde_json::{Value, json};
fn hash(c: char) -> String {
    c.to_string().repeat(64)
}
fn plan(migration: bool) -> Plan {
    Plan {
        operation_id: "synthetic-operation-1".into(),
        source_instance: "original-runtime".into(),
        target_instance: "updated-runtime".into(),
        source_image: hash('a'),
        target_image: hash('b'),
        source_schema: hash('c'),
        target_schema: if migration { hash('d') } else { hash('c') },
        backup_id: "synthetic-backup-1".into(),
        backup_manifest: hash('e'),
        source_inventory: hash('f'),
        required_free_bytes: 1024,
    }
}
fn preflight(p: &Plan) -> Preflight {
    Preflight {
        plan: p.clone(),
        signature_verified: true,
        artifact_verified: true,
        compatibility_verified: true,
        backup_restore_verified: true,
        current_source_matches_backup: true,
        available_free_bytes: 1024,
        image_only_rollback_verified: p.source_schema == p.target_schema,
    }
}
fn health(p: &Plan, rollback: bool) -> HealthReceipt {
    HealthReceipt {
        operation_id: p.operation_id.clone(),
        instance_id: if rollback {
            "restore-candidate".into()
        } else {
            p.target_instance.clone()
        },
        image: if rollback {
            p.source_image.clone()
        } else {
            p.target_image.clone()
        },
        schema: if rollback {
            p.source_schema.clone()
        } else {
            p.target_schema.clone()
        },
        ready: true,
    }
}
fn restore(p: &Plan) -> RestoreReceipt {
    RestoreReceipt {
        operation_id: p.operation_id.clone(),
        backup_id: p.backup_id.clone(),
        backup_manifest: p.backup_manifest.clone(),
        inventory_digest: p.source_inventory.clone(),
        candidate_id: "restore-candidate".into(),
        schema: p.source_schema.clone(),
        inventory_verified: true,
        separate_candidate: true,
    }
}
fn applying(migration: bool) -> Update {
    let p = plan(migration);
    let mut u = Update::new(p.clone()).unwrap();
    u.begin_update(preflight(&p)).unwrap();
    u
}
fn encode(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

#[test]
fn every_preflight_gate_is_closed_without_exact_verified_evidence() {
    let p = plan(true);
    let expected = [
        Error::SignatureUnverified,
        Error::ArtifactUnverified,
        Error::CompatibilityUnverified,
        Error::BackupUnverified,
        Error::SourceChanged,
        Error::InsufficientSpace,
    ];
    for (index, error) in expected.into_iter().enumerate() {
        let mut e = preflight(&p);
        match index {
            0 => e.signature_verified = false,
            1 => e.artifact_verified = false,
            2 => e.compatibility_verified = false,
            3 => e.backup_restore_verified = false,
            4 => e.current_source_matches_backup = false,
            _ => e.available_free_bytes = 1023,
        }
        let mut u = Update::new(p.clone()).unwrap();
        let original = u.clone();
        assert_eq!(u.begin_update(e), Err(error));
        assert_eq!(u, original);
    }
    for field in [
        "operationId",
        "sourceImage",
        "targetImage",
        "sourceSchema",
        "targetSchema",
        "backupId",
        "backupManifest",
        "sourceInventory",
        "requiredFreeBytes",
    ] {
        let mut e = serde_json::to_value(preflight(&p)).unwrap();
        e["plan"][field] = if field == "requiredFreeBytes" {
            json!(1)
        } else {
            json!("mismatched")
        };
        let mut u = Update::new(p.clone()).unwrap();
        assert_eq!(
            u.begin_update(serde_json::from_value(e).unwrap()),
            Err(Error::EvidenceMismatch)
        );
        assert_eq!(u.stage(), Stage::Prepared);
    }
}
#[test]
fn invalid_plan_ids_digests_and_zero_disk_requirement_rejected() {
    let p = plan(true);
    for field in [
        "operationId",
        "backupId",
        "sourceImage",
        "targetImage",
        "sourceSchema",
        "targetSchema",
        "backupManifest",
        "sourceInventory",
    ] {
        let mut v = serde_json::to_value(&p).unwrap();
        v[field] = json!("../private/token");
        assert_eq!(
            Update::new(serde_json::from_value(v).unwrap()),
            Err(Error::InvalidPlan)
        );
    }
    let mut p = p.clone();
    p.required_free_bytes = 0;
    assert_eq!(Update::new(p), Err(Error::InvalidPlan));
    let mut p = plan(true);
    p.target_image = p.source_image.clone();
    assert_eq!(Update::new(p), Err(Error::InvalidPlan));
    let mut p = plan(true);
    p.operation_id = "x".repeat(129);
    assert_eq!(Update::new(p), Err(Error::InvalidPlan));
}
#[test]
fn application_completion_and_unready_wrong_health_never_mark_success() {
    let mut u = applying(true);
    let p = u.plan().clone();
    assert_eq!(
        u.observe_health(health(&p, false)),
        Err(Error::InvalidTransition)
    );
    u.application_finished().unwrap();
    assert_eq!(u.stage(), Stage::AwaitingHealth);
    let mut h = health(&p, false);
    h.ready = false;
    assert_eq!(u.observe_health(h), Err(Error::ObservationUnverified));
    for field in ["operationId", "image", "schema"] {
        let mut v = serde_json::to_value(health(&p, false)).unwrap();
        v[field] = json!("wrong");
        assert_eq!(
            u.observe_health(serde_json::from_value(v).unwrap()),
            Err(Error::EvidenceMismatch)
        );
    }
    u.observe_health(health(&p, false)).unwrap();
    assert_eq!(u.stage(), Stage::Updated);
    let loaded = Update::from_json(&u.to_json().unwrap()).unwrap();
    assert_eq!(loaded, u);
}
#[test]
fn failed_migration_always_requires_full_candidate_restore_and_original_health() {
    let mut u = applying(true);
    let p = u.plan().clone();
    u.update_failed().unwrap();
    assert_eq!(u.failure(), Some(Failure::UpdateFailed));
    assert_eq!(u.image_only_rollback(), Err(Error::DataRestoreRequired));
    u.begin_restore("restore-candidate".into()).unwrap();
    for field in [
        "operationId",
        "backupId",
        "backupManifest",
        "inventoryDigest",
        "schema",
    ] {
        let mut v = serde_json::to_value(restore(&p)).unwrap();
        v[field] = json!("wrong");
        assert_eq!(
            u.restore_finished(serde_json::from_value(v).unwrap()),
            Err(Error::EvidenceMismatch)
        );
        assert_eq!(u.stage(), Stage::Restoring);
    }
    for field in ["inventoryVerified", "separateCandidate"] {
        let mut v = serde_json::to_value(restore(&p)).unwrap();
        v[field] = json!(false);
        assert_eq!(
            u.restore_finished(serde_json::from_value(v).unwrap()),
            Err(Error::ObservationUnverified)
        );
    }
    u.restore_finished(restore(&p)).unwrap();
    assert_eq!(u.stage(), Stage::AwaitingRollbackHealth);
    assert_eq!(
        u.observe_health(health(&p, false)),
        Err(Error::EvidenceMismatch)
    );
    u.observe_health(health(&p, true)).unwrap();
    assert_eq!(u.stage(), Stage::RolledBack);
    assert_eq!(Update::from_json(&u.to_json().unwrap()).unwrap(), u);
}
#[test]
fn unchanged_schema_permits_image_rollback_but_not_unobserved_success() {
    let mut u = applying(false);
    let p = u.plan().clone();
    u.application_finished().unwrap();
    u.update_failed().unwrap();
    assert_eq!(u.failure(), Some(Failure::HealthFailed));
    u.image_only_rollback().unwrap();
    assert_eq!(u.stage(), Stage::AwaitingRollbackHealth);
    let mut h = health(&p, true);
    h.instance_id = p.source_instance.clone();
    u.observe_health(h).unwrap();
    assert_eq!(u.stage(), Stage::RolledBack);
}
#[test]
fn explicit_rollback_after_success_retains_migration_safety() {
    let mut u = applying(true);
    let p = u.plan().clone();
    u.application_finished().unwrap();
    u.observe_health(health(&p, false)).unwrap();
    u.request_rollback().unwrap();
    assert_eq!(u.image_only_rollback(), Err(Error::DataRestoreRequired));
    u.begin_restore("restore-candidate".into()).unwrap();
    u.restore_finished(restore(&p)).unwrap();
    u.observe_health(health(&p, true)).unwrap();
    assert_eq!(u.stage(), Stage::RolledBack);
}
#[test]
fn restart_matrix_never_replays_or_presumes_inflight_work_complete() {
    for migration in [false, true] {
        for stage in [
            Stage::Applying,
            Stage::AwaitingHealth,
            Stage::Restoring,
            Stage::AwaitingRollbackHealth,
        ] {
            let mut u = applying(migration);
            let p = u.plan().clone();
            match stage {
                Stage::Applying => (),
                Stage::AwaitingHealth => u.application_finished().unwrap(),
                _ => {
                    u.update_failed().unwrap();
                    u.begin_restore("restore-candidate".into()).unwrap();
                    if stage == Stage::AwaitingRollbackHealth {
                        u.restore_finished(restore(&p)).unwrap();
                    }
                }
            }
            let loaded = Update::from_json(&u.to_json().unwrap()).unwrap();
            assert_eq!(loaded.stage(), Stage::RecoveryRequired);
            assert_eq!(loaded.failure(), Some(Failure::Interrupted));
            loaded.validate().unwrap();
            let mut loaded = loaded;
            if migration {
                assert_eq!(
                    loaded.image_only_rollback(),
                    Err(Error::DataRestoreRequired)
                );
            } else {
                loaded.image_only_rollback().unwrap();
            }
        }
    }
}
#[test]
fn restore_failure_and_rollback_health_failure_preserve_recovery_requirement() {
    let mut u = applying(true);
    let p = u.plan().clone();
    u.update_failed().unwrap();
    u.begin_restore("restore-candidate".into()).unwrap();
    u.recovery_failed().unwrap();
    assert_eq!(u.failure(), Some(Failure::RestoreFailed));
    assert_eq!(u.image_only_rollback(), Err(Error::DataRestoreRequired));
    u.begin_restore("restore-candidate".into()).unwrap();
    u.restore_finished(restore(&p)).unwrap();
    u.recovery_failed().unwrap();
    assert_eq!(u.failure(), Some(Failure::HealthFailed));
    assert_eq!(u.image_only_rollback(), Err(Error::DataRestoreRequired));
    u.begin_restore("restore-candidate".into()).unwrap();
    u.restore_finished(restore(&p)).unwrap();
    u.observe_health(health(&p, true)).unwrap();
}
#[test]
fn malformed_unknown_oversized_or_inconsistent_records_fail_closed() {
    assert_eq!(Update::from_json(b"not json"), Err(Error::InvalidRecord));
    assert_eq!(
        Update::from_json(&vec![b' '; MAX_RECORD_BYTES + 1]),
        Err(Error::InvalidRecord)
    );
    let u = Update::new(plan(true)).unwrap();
    let base = serde_json::to_value(&u).unwrap();
    for (field, value) in [
        ("version", json!(2)),
        ("unknown", json!(true)),
        ("stage", json!("updated")),
        ("stage", json!("rolled_back")),
        ("stage", json!("applying")),
        ("failure", json!("interrupted")),
    ] {
        let mut v = base.clone();
        v[field] = value;
        assert!(Update::from_json(&encode(&v)).is_err());
    }
    let mut u = applying(true);
    u.update_failed().unwrap();
    let mut v = serde_json::to_value(&u).unwrap();
    v["stage"] = json!("awaiting_rollback_health");
    assert_eq!(
        Update::from_json(&encode(&v)),
        Err(Error::DataRestoreRequired)
    );
    let mut v = serde_json::to_value(applying(true)).unwrap();
    v["preflight"]["signatureVerified"] = json!(false);
    let mut invalid: Update = serde_json::from_value(v).unwrap();
    assert_eq!(
        invalid.application_finished(),
        Err(Error::SignatureUnverified)
    );
    assert_eq!(invalid.to_json(), Err(Error::SignatureUnverified));
}
#[test]
fn premature_duplicate_and_terminal_transitions_rejected_without_mutation() {
    let p = plan(true);
    let mut u = Update::new(p.clone()).unwrap();
    let before = u.clone();
    assert_eq!(u.application_finished(), Err(Error::InvalidTransition));
    assert_eq!(
        u.begin_restore("restore-candidate".into()),
        Err(Error::InvalidTransition)
    );
    assert_eq!(u.image_only_rollback(), Err(Error::InvalidTransition));
    assert_eq!(u, before);
    u.begin_update(preflight(&p)).unwrap();
    assert_eq!(u.begin_update(preflight(&p)), Err(Error::InvalidTransition));
    u.application_finished().unwrap();
    u.observe_health(health(&p, false)).unwrap();
    let terminal = u.clone();
    assert_eq!(u.update_failed(), Err(Error::InvalidTransition));
    assert_eq!(u.application_finished(), Err(Error::InvalidTransition));
    assert_eq!(u, terminal);
}

#[test]
fn equal_schema_without_explicit_data_compatibility_still_requires_restore() {
    let p = plan(false);
    let mut evidence = preflight(&p);
    evidence.image_only_rollback_verified = false;
    let mut u = Update::new(p.clone()).unwrap();
    u.begin_update(evidence).unwrap();
    u.update_failed().unwrap();
    assert_eq!(u.image_only_rollback(), Err(Error::DataRestoreRequired));
    u.begin_restore("restore-candidate".into()).unwrap();
    u.restore_finished(restore(&p)).unwrap();
    u.observe_health(health(&p, true)).unwrap();
}
#[test]
fn restore_candidate_is_bound_to_receipt_and_activated_health() {
    let mut u = applying(true);
    let p = u.plan().clone();
    u.update_failed().unwrap();
    for candidate in [&p.source_instance, &p.target_instance, &"../invalid".into()] {
        assert_eq!(
            u.begin_restore(candidate.clone()),
            Err(Error::EvidenceMismatch)
        );
    }
    u.begin_restore("restore-candidate".into()).unwrap();
    let mut r = restore(&p);
    r.candidate_id = "different-candidate".into();
    assert_eq!(u.restore_finished(r), Err(Error::EvidenceMismatch));
    u.restore_finished(restore(&p)).unwrap();
    for instance in [
        &p.source_instance,
        &p.target_instance,
        &"different-candidate".into(),
    ] {
        let mut h = health(&p, true);
        h.instance_id = instance.clone();
        assert_eq!(u.observe_health(h), Err(Error::EvidenceMismatch));
        assert_eq!(u.stage(), Stage::AwaitingRollbackHealth);
    }
    u.observe_health(health(&p, true)).unwrap();
}
#[test]
fn inconsistent_persisted_restore_candidate_is_rejected() {
    let mut u = applying(true);
    u.update_failed().unwrap();
    u.begin_restore("restore-candidate".into()).unwrap();
    for candidate in [json!(null), json!("original-runtime"), json!("../invalid")] {
        let mut value = serde_json::to_value(&u).unwrap();
        value["restoreCandidate"] = candidate;
        assert_eq!(
            Update::from_json(&encode(&value)),
            Err(Error::InvalidRecord)
        );
    }
    let p = u.plan().clone();
    u.restore_finished(restore(&p)).unwrap();
    let mut value = serde_json::to_value(&u).unwrap();
    value["restore"]["candidateId"] = json!("different");
    assert_eq!(
        Update::from_json(&encode(&value)),
        Err(Error::EvidenceMismatch)
    );
}
