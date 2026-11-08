// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::{
    collections::VecDeque,
    env, fs,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
};

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

#[test]
fn transfers_exact_total_and_hash_and_requires_the_final_ack() {
    let (attempt, correlation) = ids();
    let bytes = sample();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let flow = Transfer::output(attempt, correlation, bytes.len() as u64, &sha).unwrap();
    let mut wire = MemoryFrames::default();
    assert!(flow.send(&mut wire, &mut bytes.as_slice()).is_err());
    assert_eq!(wire.output.len(), 4);
    wire.input.extend(wire.output.drain(..));
    let mut received = Vec::new();
    flow.receive(&mut wire, &mut received).unwrap();
    assert!(wire.output.is_empty());
    assert_eq!(received, bytes);
    flow.accept(&mut wire).unwrap();
    wire.input.extend(wire.output.drain(..));
    flow.send(&mut wire, &mut bytes.as_slice()).unwrap();
}

#[test]
fn rejects_short_extra_reordered_and_hash_mismatched_data_without_accepting() {
    let (attempt, correlation) = ids();
    let bytes = sample();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let flow = Transfer::output(attempt, correlation, bytes.len() as u64, &sha).unwrap();
    let mut sent = MemoryFrames::default();
    assert!(flow.send(&mut sent, &mut bytes.as_slice()).is_err());
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
    let mut frames = MemoryFrames::default();
    assert!(flow
        .send(&mut frames, &mut with_trailing_byte.as_slice())
        .is_err());
    assert_eq!(frames.output.len(), 3); // No END on an overlong source.
}

struct FixtureSocket {
    socket: UnixStream,
    deadline: Instant,
}

impl Frames for FixtureSocket {
    fn check(&mut self) -> Result<()> {
        let timeout = self.deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(invalid());
        }
        self.socket
            .set_read_timeout(Some(timeout))
            .and_then(|()| self.socket.set_write_timeout(Some(timeout)))
            .map_err(|_| invalid())
    }
    fn receive(&mut self) -> Result<Vec<u8>> {
        self.check()?;
        let mut prefix = [0; 4];
        self.socket.read_exact(&mut prefix).map_err(|_| invalid())?;
        let size = u32::from_be_bytes(prefix) as usize;
        if !(1..=FRAME_LIMIT).contains(&size) {
            return Err(invalid());
        }
        self.check()?;
        let mut body = vec![0; size];
        self.socket.read_exact(&mut body).map_err(|_| invalid())?;
        Ok(body)
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.check()?;
        self.socket
            .write_all(&(frame.len() as u32).to_be_bytes())
            .and_then(|()| self.socket.write_all(frame))
            .map_err(|_| invalid())
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
    let mut socket = FixtureSocket {
        socket: stream,
        deadline,
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
        .capture_source(&mut socket, correlation, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(source.binding, binding);
    assert_eq!(source.observed.bytes, SOURCE_BYTES);
    source.observed.check_unchanged().unwrap();
    let output = sample();
    let sha = format!("{:x}", Sha256::digest(&output));
    Transfer::output(attempt_id, correlation, output.len() as u64, &sha)
        .unwrap()
        .send(&mut socket, &mut output.as_slice())
        .unwrap();
    drop(listener);
    fs::remove_file(path).unwrap();
}
