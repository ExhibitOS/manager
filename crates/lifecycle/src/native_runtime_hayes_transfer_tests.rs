// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::{collections::VecDeque, env, fs, os::unix::net::UnixListener, path::PathBuf, thread};

fn ids() -> (Uuid, Uuid) {
    (
        Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap(),
        Uuid::parse_str("10000000-0000-4000-8000-000000000002").unwrap(),
    )
}

#[derive(Default)]
struct MemoryFrames {
    input: VecDeque<Vec<u8>>,
    output: Vec<Vec<u8>>,
}

impl Frames for MemoryFrames {
    fn check(&mut self) -> Result<()> {
        Ok(())
    }
    fn receive(&mut self) -> Result<Vec<u8>> {
        self.input.pop_front().ok_or_else(invalid)
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.output.push(frame.into());
        Ok(())
    }
}

fn sample() -> Vec<u8> {
    (0..4103).map(|i| (i % 251) as u8).collect()
}

#[test]
fn wire_header_and_chunk_order_match_api_contract() {
    let (attempt, correlation) = ids();
    let source = Transfer::source(attempt, correlation).unwrap();
    assert_eq!(
        source.header(HeaderKind::HayesTransferStart).unwrap(),
        br#"{"version":1,"kind":"hayes-transfer-start","attemptId":"10000000-0000-4000-8000-000000000001","correlationId":"10000000-0000-4000-8000-000000000002","direction":"source","bytes":8383096,"sha256":"e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6"}"#
    );
    assert_eq!(
        encode(&source.data(4096, "YQ==".into())).unwrap(),
        br#"{"version":1,"kind":"hayes-transfer-data","attemptId":"10000000-0000-4000-8000-000000000001","correlationId":"10000000-0000-4000-8000-000000000002","direction":"source","offset":4096,"data":"YQ=="}"#
    );
    assert!(Transfer::output(attempt, correlation, OUTPUT_LIMIT + 1, SOURCE_SHA256).is_err());
    assert!(Transfer::output(Uuid::nil(), correlation, 1, SOURCE_SHA256).is_err());
}

fn requested(flow: Transfer<'_>, include_accepted: bool) -> MemoryFrames {
    let mut wire = MemoryFrames::default();
    for offset in (0..flow.bytes).step_by(CHUNK).chain([flow.bytes]) {
        wire.input.push_back(flow.next(offset).unwrap());
    }
    if include_accepted {
        wire.input
            .push_back(flow.header(HeaderKind::HayesTransferAccepted).unwrap());
    }
    wire
}

#[test]
fn serves_exact_output_one_requested_frame_at_a_time_and_requires_the_final_ack() {
    let (attempt, correlation) = ids();
    let bytes = sample();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let flow = Transfer::output(attempt, correlation, bytes.len() as u64, &sha).unwrap();
    assert_eq!(
        flow.next(4096).unwrap(),
        br#"{"version":1,"kind":"hayes-transfer-next","attemptId":"10000000-0000-4000-8000-000000000001","correlationId":"10000000-0000-4000-8000-000000000002","direction":"output","offset":4096}"#
    );

    let mut unsolicited = MemoryFrames::default();
    assert!(flow
        .serve_output(&mut unsolicited, &mut bytes.as_slice())
        .is_err());
    assert_eq!(unsolicited.output.len(), 1);

    let mut wrong = requested(flow, true);
    wrong.input[1] = flow.next(0).unwrap();
    assert!(flow
        .serve_output(&mut wrong, &mut bytes.as_slice())
        .is_err());
    assert_eq!(wrong.output.len(), 2);

    let mut wire = requested(flow, false);
    assert!(flow.serve_output(&mut wire, &mut bytes.as_slice()).is_err());
    assert_eq!(wire.output.len(), 4);
    wire.input.extend(wire.output.drain(..));
    let mut received = Vec::new();
    flow.receive(&mut wire, &mut received).unwrap();
    assert!(wire.output.is_empty());
    assert_eq!(received, bytes);
    flow.accept(&mut wire).unwrap();
    assert_eq!(
        wire.output,
        [flow.header(HeaderKind::HayesTransferAccepted).unwrap()]
    );
    flow.serve_output(&mut requested(flow, true), &mut bytes.as_slice())
        .unwrap();
}

#[test]
fn rejects_short_extra_reordered_and_hash_mismatched_data_without_accepting() {
    let (attempt, correlation) = ids();
    let bytes = sample();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let flow = Transfer::output(attempt, correlation, bytes.len() as u64, &sha).unwrap();
    let mut sent = requested(flow, true);
    flow.serve_output(&mut sent, &mut bytes.as_slice()).unwrap();
    for scenario in ["short", "extra", "reordered", "noncanonical", "hash"] {
        let mut frames = MemoryFrames {
            input: sent.output.clone().into(),
            ..Default::default()
        };
        match scenario {
            "short" => {
                frames.input.pop_back();
            }
            "extra" => frames.input.insert(3, sent.output[2].clone()),
            "reordered" => frames.input.swap(1, 2),
            "noncanonical" => frames.input[1].insert(0, b' '),
            "hash" => {
                let mut wrong = bytes[..CHUNK].to_vec();
                wrong[0] ^= 1;
                frames.input[1] = encode(&flow.data(0, STANDARD.encode(wrong))).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            flow.receive(&mut frames, &mut Vec::new()).is_err(),
            "{scenario}"
        );
        assert!(frames.output.is_empty(), "{scenario}");
    }
    let mut with_trailing_byte = bytes.clone();
    with_trailing_byte.push(0);
    let mut frames = requested(flow, true);
    assert!(flow
        .serve_output(&mut frames, &mut with_trailing_byte.as_slice())
        .is_err());
    assert_eq!(frames.output.len(), 3); // No END on an overlong source.
}

struct TraceFrames<'a> {
    owner: OwnerFrames<'a>,
    received: usize,
    sent: usize,
}

impl Frames for TraceFrames<'_> {
    fn check(&mut self) -> Result<()> {
        self.owner.check()
    }
    fn receive(&mut self) -> Result<Vec<u8>> {
        let frame = self.owner.receive()?;
        self.received += 1;
        Ok(frame)
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.owner.send(frame)?;
        self.sent += 1;
        Ok(())
    }
}

/// Explicit peer for the separately owned API interop test. It receives the
/// exact real source into a synthetic isolated Manager stage, then serves a
/// fixed synthetic output vector. No payload, native worker or DB is started.
#[test]
#[ignore]
fn api_interop_fixture() {
    use super::super::{Binding, Lease};
    use crate::native_runtime::hayes::tests::Fixture;
    let path = PathBuf::from(env::var_os("HAYES_TEST_SOCKET").expect("private socket path"));
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
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
    let mut connection =
        crate::native_runtime::owner_channel::Connection::hayes_fixture(stream).unwrap();
    let cancelled = AtomicBool::new(false);
    let mut check = || Ok(());
    let mut frames = TraceFrames {
        owner: OwnerFrames::new(&mut connection, deadline, &cancelled, &mut check),
        received: 0,
        sent: 0,
    };
    let fixture = Fixture::new();
    let (attempt_id, correlation) = ids();
    let binding = Binding {
        attempt: attempt_id,
        generation: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        job: Uuid::new_v4(),
    };
    let mut attempt = Lease::acquire(fixture.path(), fixture.authority())
        .unwrap()
        .begin(binding)
        .unwrap();
    let source = attempt
        .capture_source(&mut frames, correlation, &cancelled)
        .unwrap_or_else(|error| {
            panic!(
                "source: {error}; received {}, sent {}",
                frames.received, frames.sent
            )
        });
    assert_eq!(source.binding, binding);
    assert_eq!(source.correlation, correlation);
    assert_eq!(source.observed.bytes, SOURCE_BYTES);
    source.observed.check_unchanged().unwrap();
    let output = sample();
    let sha = format!("{:x}", Sha256::digest(&output));
    Transfer::output(attempt_id, correlation, output.len() as u64, &sha)
        .unwrap()
        .serve_output(&mut frames, &mut output.as_slice())
        .unwrap_or_else(|error| {
            panic!(
                "output: {error}; received {}, sent {}",
                frames.received, frames.sent
            )
        });
    drop(listener);
    fs::remove_file(path).unwrap();
}
