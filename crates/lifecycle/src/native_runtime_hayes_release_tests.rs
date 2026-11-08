// SPDX-License-Identifier: Apache-2.0
use super::super::super::super::tests::{binding, id};
use super::*;
use crate::native_runtime::{
    hayes::{stage_return::RetainedStages, tests::Fixture, Binding, Lease, ACTIVE},
    oex_read_fd::{observe_stage, Stage},
};
use std::{fs, io::Write, sync::atomic::AtomicBool};

fn scope(binding: &OperationBinding) -> ReleaseScope<'_> {
    ReleaseScope {
        binding,
        native_owner_id: id(10),
        runtime_manifest_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        worker_manifest_sha256: "94a6551d16af3fb134f94549d9f087ee8ccb9fd2372d914de5f738a78d3505c6",
        native_evidence_sha256: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        ready_frame_sha256: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        db_stage_id: id(16),
        database: Database {
            system_identifier: "123",
            database_oid: "456",
        },
        challenge: id(14),
        receipt_id: id(15),
        receipt_sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    }
}

fn owned(root: &Fixture, scope: &ReleaseScope<'_>) -> Attempt {
    root.acquire()
        .begin(Binding {
            attempt: scope.binding.attempt_id,
            generation: scope.binding.runtime_generation,
            owner: scope.native_owner_id,
            job: scope.binding.job_id,
        })
        .unwrap()
}

fn stage_ids(scope: &ReleaseScope<'_>) -> [Uuid; 4] {
    [scope.binding.attempt_id, id(21), id(22), id(23)]
}

fn stages(attempt: &Attempt, ids: [Uuid; 4]) -> RetainedStages {
    RetainedStages::new(ids.map(|id| {
        let stage = Stage::create(&attempt.stage.fd, &id.to_string()).unwrap();
        let bytes = id.as_bytes();
        stage.materializer_write().write_all(bytes).unwrap();
        let observed = observe_stage(
            stage,
            bytes.len() as u64,
            &crate::digest(bytes),
            &AtomicBool::new(false),
            &mut || Ok(()),
        )
        .unwrap();
        (id, observed)
    }))
}

#[test]
fn completion_is_durable_only_after_exact_stage_and_lock_return() {
    let root = Fixture::new();
    let binding = binding();
    let scope = scope(&binding);
    let attempt = owned(&root, &scope);
    let stages = stages(&attempt, stage_ids(&scope));
    let inventory = stages.inventory(&attempt).unwrap();
    let intent = Intent {
        version: 1,
        scope: &scope,
        stages: &inventory,
    };
    attempt.journal.create(INTENT_RECORD, &intent).unwrap();
    let intent_sha = attempt
        .journal
        .confirmed_digest(INTENT_RECORD, &intent)
        .unwrap();
    let original = root.path().join("acquired-original");
    let durable = root.path().join("durable-output");
    fs::write(&original, b"immutable acquired original").unwrap();
    fs::write(&durable, b"durable copy").unwrap();
    let mut check = |held: &Attempt| {
        assert_eq!(
            held.journal
                .confirmed_digest(INTENT_RECORD, &intent)
                .unwrap(),
            intent_sha
        );
        assert!(!held.journal.exists(COMPLETION_RECORD).unwrap());
        assert!(root.path().join(ACTIVE).exists());
        Ok(())
    };
    let returned = stages.return_owned(&attempt, &mut check).unwrap();
    assert_eq!(
        Lease::acquire(root.path(), root.authority())
            .err()
            .unwrap()
            .code,
        "HAYES_RESERVATION_BUSY"
    );
    assert!(fs::read_dir(&attempt.stage.path).unwrap().next().is_none());
    let closed = returned.close_custody(attempt, &mut check).unwrap();
    assert!(!root.path().join(ACTIVE).exists());
    let successor = Lease::acquire(root.path(), root.authority()).unwrap();
    assert!(!closed.journal().exists(COMPLETION_RECORD).unwrap());
    let completion = persist_completion(&closed, &intent, &intent_sha).unwrap();
    assert_eq!(
        completion,
        crate::digest(&closed.journal().read_bytes(COMPLETION_RECORD).unwrap())
    );
    assert!(persist_completion(&closed, &intent, &intent_sha).is_err());
    assert_eq!(fs::read(original).unwrap(), b"immutable acquired original");
    assert_eq!(fs::read(durable).unwrap(), b"durable copy");
    drop(successor);
}

#[test]
fn extra_replaced_or_partially_returned_stages_hold_active_custody() {
    for failure in ["extra", "replaced", "cancelled"] {
        let root = Fixture::new();
        let binding = binding();
        let scope = scope(&binding);
        let attempt = owned(&root, &scope);
        let ids = stage_ids(&scope);
        let stages = stages(&attempt, ids);
        let file = |index: usize| {
            attempt
                .stage
                .path
                .join(format!(".oex-stage-{}", ids[index]))
        };
        let inventory = stages.inventory(&attempt).unwrap();
        let intent = Intent {
            version: 1,
            scope: &scope,
            stages: &inventory,
        };
        attempt.journal.create(INTENT_RECORD, &intent).unwrap();
        if failure == "extra" {
            fs::write(attempt.stage.path.join("extra"), b"unknown").unwrap();
        }
        let mut calls = 0;
        let moved = root.path().join("retained-stage");
        let result = stages.return_owned(&attempt, &mut |_| {
            calls += 1;
            if calls == 3 && failure == "replaced" {
                fs::rename(file(1), &moved).unwrap();
                fs::write(file(1), b"replacement").unwrap();
            }
            if calls == 3 && failure == "cancelled" {
                return Err(crate::err("NATIVE_CANCELLED"));
            }
            Ok(())
        });
        assert!(result.is_err(), "{failure}");
        assert_eq!(file(0).exists(), failure == "extra");
        assert!(file(1).exists() && file(2).exists() && file(3).exists());
        if failure == "replaced" {
            assert_eq!(fs::read(file(1)).unwrap(), b"replacement");
            assert_eq!(fs::read(moved).unwrap(), ids[1].as_bytes());
        }
        assert!(root.path().join(ACTIVE).exists());
        assert!(attempt.journal.exists(INTENT_RECORD).unwrap());
        assert!(!attempt.journal.exists(COMPLETION_RECORD).unwrap());
    }
}

#[test]
#[ignore = "emits a data-only vector for the Platform decoder"]
fn api_release_completion_vector() {
    let binding = binding();
    let scope = scope(&binding);
    let frame = encode(&CompletedFrame {
        version: 1,
        kind: "hayes-release-completed",
        scope: &scope,
        intent_sha256: &"b".repeat(64),
        completion_sha256: &"d".repeat(64),
    })
    .unwrap();
    println!(
        "HAYES_RELEASE_VECTOR {}",
        std::str::from_utf8(&frame).unwrap()
    );
}
