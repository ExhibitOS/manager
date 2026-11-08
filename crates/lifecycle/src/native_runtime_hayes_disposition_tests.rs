// SPDX-License-Identifier: Apache-2.0
use super::super::super::tests::{binding, id};
use super::super::{Containment, LeafEvidence, LockIdentity, ReadyStage, CONTAINMENT_KIND};
use super::*;
use crate::native_runtime::hayes::Role;

fn pins(b: &OperationBinding) -> ReadyPins {
    ReadyPins {
        evidence_sha256: "e".repeat(64),
        frame_sha256: "f".repeat(64),
        worker_manifest_sha256: "94a6551d16af3fb134f94549d9f087ee8ccb9fd2372d914de5f738a78d3505c6",
        attestation: Attestation {
            source: ReadyStage::fixture(b.attempt_id, 8, b.source_bytes, &b.source_sha256),
            output: ReadyStage::fixture(id(11), 9, u64::from(BODY_BYTES), BODY_SHA),
            workers: [
                LeafEvidence::fixture(Role::Generate, id(12)),
                LeafEvidence::fixture(Role::Validate, id(13)),
            ],
            lock: LockIdentity {
                device: "7".into(),
                inode: "10".into(),
            },
            containment: Containment {
                kind: CONTAINMENT_KIND,
                receipt_sha256: "d".repeat(64),
            },
        },
    }
}

fn notice() -> Notice {
    Notice {
        version: 1,
        kind: Kind::HayesDispositionCommitted,
        binding: binding(),
        native_owner_id: id(10),
        runtime_manifest_sha256: "a".repeat(64),
        ready_frame_sha256: "f".repeat(64),
        db_stage_id: id(16),
        challenge: id(14),
        receipt_id: id(15),
        receipt_sha256: "c".repeat(64),
    }
}

fn challenge(notice: &Notice) -> Challenge<'_> {
    Challenge {
        version: 1,
        kind: Kind::HayesDispositionChallenge,
        binding: &notice.binding,
        native_owner_id: notice.native_owner_id,
        runtime_manifest_sha256: &notice.runtime_manifest_sha256,
        ready_frame_sha256: &notice.ready_frame_sha256,
        db_stage_id: notice.db_stage_id,
        challenge: notice.challenge,
    }
}

#[test]
fn committed_notice_requires_current_exact_challenge_and_canonical_binding() {
    let valid = notice();
    let expected = challenge(&valid);
    assert_eq!(
        check_notice(&encode(&valid).unwrap(), &expected)
            .unwrap()
            .receipt_id,
        valid.receipt_id
    );
    for scenario in [
        "kind",
        "challenge",
        "binding",
        "owner",
        "host",
        "ready digest",
        "stage",
        "receipt",
        "digest",
    ] {
        let mut wrong = notice();
        match scenario {
            "kind" => wrong.kind = Kind::HayesDispositionChallenge,
            "challenge" => wrong.challenge = id(99),
            "binding" => wrong.binding.lease_id = id(99),
            "owner" => wrong.native_owner_id = id(99),
            "host" => wrong.runtime_manifest_sha256 = "b".repeat(64),
            "ready digest" => wrong.ready_frame_sha256 = "b".repeat(64),
            "stage" => wrong.db_stage_id = id(99),
            "receipt" => wrong.receipt_id = Uuid::nil(),
            "digest" => wrong.receipt_sha256 = "x".repeat(64),
            _ => unreachable!(),
        }
        assert!(
            check_notice(&encode(&wrong).unwrap(), &expected).is_err(),
            "{scenario}"
        );
    }
    let raw = String::from_utf8(encode(&valid).unwrap()).unwrap();
    let wrong = raw.replace(
        "\"binding\":{\"version\":1,",
        "\"binding\":{ \"version\":1,",
    );
    assert_ne!(wrong, raw);
    assert!(check_notice(wrong.as_bytes(), &expected).is_err());
}

#[test]
fn canonical_receipt_must_match_native_evidence_and_independent_db_row() {
    let mut notice = notice();
    let mut pins = pins(&notice.binding);
    // The order and field names are the independently agreed Platform vector.
    let receipt = format!(concat!(
        "{{\"version\":1,\"outcome\":\"committed\",\"receiptId\":\"{}\",\"binding\":{},",
        "\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{}\",\"workerManifestSha256\":\"{}\",",
        "\"nativeEvidenceSha256\":\"{}\",\"attestation\":{},\"dbStageId\":\"{}\",",
        "\"originalAssetId\":\"{}\",\"displayAssetId\":\"{}\",\"derivativeId\":\"{}\",",
        "\"database\":{{\"systemIdentifier\":\"123\",\"databaseOid\":\"456\"}}}}"
    ), notice.receipt_id, serde_json::to_string(&notice.binding).unwrap(), notice.native_owner_id,
        notice.runtime_manifest_sha256, pins.worker_manifest_sha256, pins.evidence_sha256,
        serde_json::to_string(&pins.attestation).unwrap(), notice.db_stage_id, id(17), id(18), id(19));
    notice.receipt_sha256 = crate::digest(receipt.as_bytes());
    let encoded = STANDARD.encode(receipt.as_bytes());
    let row = format!("123|456|{}|{}|{}|{encoded}", id(17), id(18), id(19));
    Readback::parse(&row)
        .unwrap()
        .check_receipt(&notice, &pins)
        .unwrap();

    let wrong = format!("123|456|{}|{}|{}|{encoded}", id(20), id(18), id(19));
    assert!(Readback::parse(&wrong)
        .unwrap()
        .check_receipt(&notice, &pins)
        .is_err());
    let wrong = format!("123|456|{}|{}|{}|{encoded}", id(18), id(18), id(19));
    assert!(Readback::parse(&wrong).is_err());
    let wrong = row.replacen("123|456", "123|457", 1);
    assert!(Readback::parse(&wrong)
        .unwrap()
        .check_receipt(&notice, &pins)
        .is_err());
    assert!(Readback::parse(&format!("{row}\n{row}")).is_err());
    assert!(Readback::parse(&format!("{row}=")).is_err());
    pins.evidence_sha256 = "d".repeat(64);
    assert!(Readback::parse(&row)
        .unwrap()
        .check_receipt(&notice, &pins)
        .is_err());
}

#[test]
fn release_uses_the_smaller_current_lease_and_operation_budget() {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(90);
    assert_eq!(
        live_release_deadline(started, deadline, "120000").unwrap(),
        deadline
    );
    assert_eq!(
        live_release_deadline(started, deadline, "60000").unwrap(),
        started + Duration::from_secs(60)
    );
    for invalid in ["0", "-1", "060000", "300001", ""] {
        assert!(live_release_deadline(started, deadline, invalid).is_err());
    }
    assert!(live_release_deadline(started - Duration::from_secs(2), deadline, "1000").is_err());
}

#[test]
#[ignore = "emits exact disposition protocol and SQL vectors for isolated fixtures"]
fn api_disposition_vectors() {
    let mut notice = notice();
    if let Ok(hash) = std::env::var("EXHIBITOS_TEST_HAYES_RECEIPT_SHA256") {
        assert!(hash_valid(&hash));
        notice.receipt_sha256 = hash;
    }
    let pins = pins(&notice.binding);
    println!(
        "HAYES_DISPOSITION_CHALLENGE {}",
        String::from_utf8(encode(&challenge(&notice)).unwrap()).unwrap()
    );
    println!(
        "HAYES_DISPOSITION_NOTICE {}",
        String::from_utf8(encode(&notice).unwrap()).unwrap()
    );
    println!(
        "HAYES_DISPOSITION_SQL_BEGIN\n{}\nHAYES_DISPOSITION_SQL_END",
        committed_sql(
            &notice,
            &pins,
            std::env::var_os("EXHIBITOS_TEST_HAYES_RELEASE_LEASE").is_some()
        )
    );
}
