// SPDX-License-Identifier: Apache-2.0
use super::super::tests::{binding, id};
use super::*;

fn frame() -> Reservation {
    Reservation {
        version: 1,
        kind: Kind::HayesOutputReservationReady,
        binding: binding(),
        native_evidence_sha256: "e".repeat(64),
        db_stage_id: id(14),
        output: ReservedOutput {
            stage_id: id(11),
            bytes: u64::from(BODY_BYTES),
            sha256: BODY_SHA.into(),
        },
    }
}

#[test]
fn reservation_requires_the_canonical_current_binding_evidence_and_stages() {
    let expected = frame();
    let check = |raw: &[u8]| {
        check_reservation(
            raw,
            &expected.binding,
            &expected.native_evidence_sha256,
            &expected.output,
        )
    };
    let bytes = encode(&expected).unwrap();
    assert_eq!(check(&bytes).unwrap().db_stage_id, id(14));
    let mut noncanonical = vec![b' '];
    noncanonical.extend_from_slice(&bytes);
    assert!(check(&noncanonical).is_err());
    for scenario in [
        "version",
        "kind",
        "lease",
        "evidence",
        "invalid id",
        "source alias",
        "output alias",
        "stage",
        "size",
        "digest",
    ] {
        let mut bad = frame();
        match scenario {
            "version" => bad.version = 2,
            "kind" => bad.kind = Kind::HayesOperationRevalidate,
            "lease" => bad.binding.lease_id = id(20),
            "evidence" => bad.native_evidence_sha256 = "d".repeat(64),
            "invalid id" => bad.db_stage_id = Uuid::nil(),
            "source alias" => bad.db_stage_id = bad.binding.attempt_id,
            "output alias" => bad.db_stage_id = bad.output.stage_id,
            "stage" => bad.output.stage_id = id(20),
            "size" => bad.output.bytes -= 1,
            "digest" => bad.output.sha256 = "a".repeat(64),
            _ => unreachable!(),
        }
        assert!(check(&encode(&bad).unwrap()).is_err(), "{scenario}");
    }
}

#[test]
#[ignore = "emits the exact query for the isolated PostgreSQL fixture"]
fn api_reservation_sql_fixture() {
    let frame = frame();
    println!(
        "HAYES_RESERVATION_SQL_BEGIN\n{}\nHAYES_RESERVATION_SQL_END",
        super::super::live_claim_sql(&frame.binding, Some(frame.db_stage_id))
    );
}
