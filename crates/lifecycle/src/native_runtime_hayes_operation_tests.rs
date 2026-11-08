// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::native_runtime::hayes::{
    tests::{wait_for, Fixture},
    Role,
};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    thread,
};

pub(super) fn id(index: u32) -> Uuid {
    Uuid::parse_str(&format!("10000000-0000-4000-8000-{index:012}")).unwrap()
}

pub(super) fn binding() -> OperationBinding {
    let flow = Transfer::source(id(6), id(9)).unwrap();
    OperationBinding {
        version: 1,
        tenant_id: id(1),
        workspace_id: id(2),
        principal_id: id(3),
        job_id: id(4),
        lease_id: id(5),
        attempt_id: id(6),
        runtime_generation: id(7),
        artwork_id: id(8),
        correlation_id: id(9),
        artwork_revision: 12,
        source_sha256: flow.sha256().into(),
        source_bytes: flow.bytes(),
    }
}

pub(super) fn record(binding: OperationBinding) -> Record {
    Record {
        version: 1,
        binding,
        native_owner_id: id(10),
        runtime_manifest_sha256: "a".repeat(64),
        runtime_source_commit: "b".repeat(40),
        runtime_producer_sha256: "c".repeat(64),
        database_system_id: "123".into(),
        database_oid: "456".into(),
    }
}

#[test]
fn open_uses_closed_canonical_v1_and_exact_session_and_source() {
    let mut request = Open {
        version: 1,
        kind: Kind::HayesOperationOpen,
        binding: binding(),
    };
    let bytes = encode(&request).unwrap();
    assert_eq!(
        bytes,
        br#"{"version":1,"kind":"hayes-operation-open","binding":{"version":1,"tenantId":"10000000-0000-4000-8000-000000000001","workspaceId":"10000000-0000-4000-8000-000000000002","principalId":"10000000-0000-4000-8000-000000000003","jobId":"10000000-0000-4000-8000-000000000004","leaseId":"10000000-0000-4000-8000-000000000005","attemptId":"10000000-0000-4000-8000-000000000006","runtimeGeneration":"10000000-0000-4000-8000-000000000007","artworkId":"10000000-0000-4000-8000-000000000008","correlationId":"10000000-0000-4000-8000-000000000009","artworkRevision":12,"sourceSha256":"e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6","sourceBytes":8383096}}"#,
    );
    let parsed = parse_open(&bytes).unwrap();
    let expected = ExecuteBinding {
        job_id: parsed.job_id,
        artwork_id: parsed.artwork_id,
        artwork_revision: 12,
    };
    assert!(expected.matches(&parsed));
    assert!(!ExecuteBinding { job_id: parsed.attempt_id, ..expected }.matches(&parsed));
    assert!(!ExecuteBinding { artwork_id: parsed.workspace_id, ..expected }.matches(&parsed));
    assert!(!ExecuteBinding { artwork_revision: 13, ..expected }.matches(&parsed));
    assert_eq!(parsed, request.binding);
    parsed
        .check_session(
            &id(2).to_string(),
            &id(1).to_string(),
            &id(3).to_string(),
            &id(7).to_string(),
        )
        .unwrap();
    assert!(parsed
        .check_session(
            &id(2).to_string(),
            &id(1).to_string(),
            &id(3).to_string(),
            &id(8).to_string()
        )
        .is_err());

    let mut with_space = bytes;
    with_space.insert(0, b' ');
    assert!(parse_open(&with_space).is_err());
    request.binding.artwork_revision = MAX_SAFE_INTEGER + 1;
    assert!(parse_open(&encode(&request).unwrap()).is_err());
    request.binding = binding();
    request.binding.source_bytes -= 1;
    assert!(parse_open(&encode(&request).unwrap()).is_err());
    request.binding = binding();
    request.binding.lease_id = Uuid::nil();
    assert!(parse_open(&encode(&request).unwrap()).is_err());
}

#[test]
fn requires_exact_challenge_binding_and_manifest_and_accepts_explicit_abort_only_for_same_attempt()
{
    let record = record(binding());
    let mut challenge = record.challenge();
    let good = encode(&Revalidation {
        kind: Kind::HayesOperationRevalidated,
        ..challenge
    })
    .unwrap();
    check_revalidated(&good, &mut challenge).unwrap();

    for scenario in ["nonce", "lease", "manifest", "replay"] {
        let mut other_binding = binding();
        other_binding.lease_id = id(12);
        let mut frame = Revalidation {
            kind: Kind::HayesOperationRevalidated,
            ..challenge
        };
        match scenario {
            "nonce" => frame.challenge = id(12),
            "lease" => frame.binding = &other_binding,
            "manifest" => frame.runtime_manifest_sha256 = "different",
            "replay" => frame.kind = Kind::HayesOperationRevalidate,
            _ => unreachable!(),
        }
        assert!(check_revalidated(&encode(&frame).unwrap(), &mut challenge).is_err());
    }
    let abort = encode(&Open {
        version: 1,
        kind: Kind::HayesOperationAbort,
        binding: binding(),
    })
    .unwrap();
    assert_eq!(
        check_revalidated(&abort, &mut challenge).unwrap_err().code,
        "HAYES_OPERATION_ABORTED"
    );
}

#[test]
fn one_shot_connection_and_durable_active_record_block_replay() {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let mut channel = Connection::hayes_fixture(socket).unwrap();
    channel.claim_hayes().unwrap();
    assert!(channel.claim_hayes().is_err());
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);

    let root = Fixture::new();
    let record = record(binding());
    let attempt = record.persist(root.acquire()).unwrap();
    check_record(&attempt, &record).unwrap();
    drop(attempt);
    assert_eq!(
        record.persist(root.acquire()).err().unwrap().code,
        "HAYES_RESERVATION_RETAINED"
    );
}

/// Uses the actual Manager framing, record, source stage and challenge
/// helpers against the independent API codec. Live Session/DB authentication
/// is intentionally outside this synthetic fixture.
#[test]
#[ignore]
fn api_control_interop_fixture() {
    let socket =
        PathBuf::from(env::var_os("HAYES_CONTROL_SOCKET").expect("private fixture socket"));
    let custody =
        PathBuf::from(env::var_os("HAYES_CONTROL_CUSTODY").expect("new private custody root"));
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
    };
    let mut connection = Connection::hayes_fixture(stream).unwrap();
    connection.claim_hayes().unwrap();
    let cancel = AtomicBool::new(false);
    let mut check = || Ok(());
    let binding = parse_open(
        &connection
            .read_frame(deadline, &cancel, &mut check)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(binding, self::binding());
    let record = record(binding);
    let root = std::mem::ManuallyDrop::new(Fixture::provision(custody));
    let mut attempt = record.persist(root.acquire()).unwrap();
    connection
        .send_frame(&record.accepted().unwrap(), deadline, &cancel, &mut check)
        .unwrap();
    let source = attempt
        .receive_source(
            &mut connection,
            record.binding.correlation_id,
            deadline,
            &cancel,
            &mut check,
        )
        .unwrap();
    attempt
        .check_stage_file(source.binding.attempt, &source.observed)
        .unwrap();
    let mut challenge = record.challenge();
    connection
        .send_frame(&encode(&challenge).unwrap(), deadline, &cancel, &mut check)
        .unwrap();
    let reply = connection
        .read_frame(deadline, &cancel, &mut check)
        .unwrap();
    check_revalidated(&reply, &mut challenge).unwrap();
    check_record(&attempt, &record).unwrap();
    attempt
        .journal
        .create(REVALIDATED_RECORD, &challenge)
        .unwrap();
    connection.invalidate();
    drop(listener);
    fs::remove_file(socket).unwrap();
}

#[test]
fn worker_owner_socket_vetoes_eof_and_premature_frames_without_consuming() {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let mut channel = Connection::hayes_fixture(socket).unwrap();
    assert!(channel.check_hayes_idle().is_err());
    channel.claim_hayes().unwrap();
    assert!(channel.check_hayes_idle().is_ok());
    peer.write_all(&[0, 0, 0, 1, b'x']).unwrap();
    assert!(channel.check_hayes_idle().is_err());
    let frame = channel
        .read_frame(
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            &mut || Ok(()),
        )
        .unwrap();
    assert_eq!(frame, b"x");
    drop(peer);
    assert!(channel.check_hayes_idle().is_err());
}

#[test]
fn session_retained_attempt_outlives_direct_child_reap() {
    let root = Fixture::new();
    let record = record(binding());
    let mut attempt = record.persist(root.acquire()).unwrap();
    let expected = attempt.intent.binding;
    let permit = attempt.before_spawn(Role::Generate).unwrap();
    let child = permit
        .spawn_fixture(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("read -r line <&3 || exit 0")],
        )
        .unwrap();
    let mut retained = RetainedHayes {
        _attempt: attempt,
        _source: None,
        _validated: None,
        child: Some(child),
    };
    assert!(!retained.poll().unwrap());
    retained.child.as_mut().unwrap().owner.take();
    wait_for(|| retained.poll().unwrap());
    assert!(!retained.has_child());
    assert_eq!(
        Lease::acquire(root.path(), root.authority())
            .err()
            .unwrap()
            .code,
        "HAYES_RESERVATION_BUSY"
    );
    drop(retained);
    assert_eq!(
        root.acquire()
            .check_interrupted(expected)
            .err()
            .unwrap()
            .code,
        "HAYES_DRAIN_UNCONFIRMED"
    );
}
