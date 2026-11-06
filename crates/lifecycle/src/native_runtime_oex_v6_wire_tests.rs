// SPDX-License-Identifier: Apache-2.0
use super::*;

fn fixture(variable: &str, sha256: &str) -> Option<Value> {
    let path = std::env::var_os(variable)?;
    let bytes = fs::read(path).unwrap();
    assert_eq!(digest(&bytes), sha256);
    Some(serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn workspace_v6_wire_matches_platform_selected_owner_bytes() {
    let Some(fixture) = fixture("EXHIBITOS_TEST_OEX_V6_OWNER_WIRE",
        "836e46fa52ad4ea4f14dfc9a9fc3004fe379057932b46629776570af8baa58a3") else { return };
    let frame = |name: &str| {
        let value = &fixture["frames"][name];
        let bytes = value["utf8"].as_str().unwrap().as_bytes();
        assert_eq!(digest(bytes), value["sha256"].as_str().unwrap());
        bytes
    };
    let selection: owner_claim_frames::UploadSelection = serde_json::from_slice(frame("uploadSelection")).unwrap();
    assert_eq!(selection.archive_format.as_deref(), Some("1.0.0-draft.12"));
    assert_eq!(selection.encode().unwrap(), frame("uploadSelection"));
    selection.check_selected(frame("uploadSelected")).unwrap();

    let control: oex_preflight::Control = serde_json::from_slice(frame("preflightControl")).unwrap();
    assert_eq!(control.encode().unwrap(), frame("preflightControl"));
    let preflight = oex_preflight::Prepared::test_only_raw(&control, frame("preflightAck").to_vec());
    preflight.inventory().check_format(Some("1.0.0-draft.12")).unwrap();
    assert!(preflight.inventory().check_format(Some("1.0.0-draft.10")).is_err());
    let challenge = owner_claim_frames::ClaimChallenge {
        version: 1, kind: "claim-challenge-v3".into(), correlation_id: selection.correlation_id.clone(),
        workspace_id: selection.workspace_id, tenant_id: selection.tenant_id, principal_id: selection.principal_id,
        runtime_generation: control.runtime_generation.clone(), job_id: selection.request_id.clone(),
        attempt_id: control.attempt_id.clone(), generation: selection.request_id, stage_id: control.stage_id.clone(),
        reservation_id: control.reservation_id.clone(), reservation_generation: control.reservation_generation.clone(),
        claim_digest: "0".repeat(64), receipt_digest: control.reservation_digest.clone(),
    };
    assert_eq!(preflight.report(&control, &challenge).unwrap(), frame("preflightReport"));

    let notice: Value = serde_json::from_slice(frame("packedControl")).unwrap();
    let packed: oex_control_v2::Control = serde_json::from_value(notice["control"].clone()).unwrap();
    assert_eq!(packed.format, "1.0.0-draft.12");
    assert!(packed.encode().is_ok());
    assert_eq!(owner_channel::worker_control_notice(&packed, &challenge.correlation_id, &challenge.attempt_id, true)
        .unwrap(), frame("packedControl"));
}

#[test]
fn workspace_v6_wire_matches_platform_materializer_bytes() {
    let Some(fixture) = fixture("EXHIBITOS_TEST_OEX_V6_MATERIALIZER_WIRE",
        "3f4605e8db77a5d8f85c6abdd8c199342ba1b80b7d826735acecf9e8db80fbea") else { return };
    let control = fixture["controlUtf8"].as_str().unwrap().as_bytes();
    let ack = fixture["ackUtf8"].as_str().unwrap().as_bytes();
    assert_eq!(digest(control), fixture["controlSha256"].as_str().unwrap());
    assert_eq!(digest(ack), fixture["ackSha256"].as_str().unwrap());
    let request: oex_materializer_protocol::Request = serde_json::from_slice(control).unwrap();
    assert_eq!(request.format, "1.0.0-draft.12");
    assert_eq!(request.encode().unwrap(), control);
    request.check_ack(control, ack).unwrap();
    let mut noncanonical = ack.to_vec();
    noncanonical.push(b' ');
    assert!(request.check_ack(control, &noncanonical).is_err());
}
