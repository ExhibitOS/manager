// SPDX-License-Identifier: Apache-2.0
//! Inactive Hayes owner-channel data exchange. Acceptance verifies transfer
//! integrity only. It is never a worker drain, DB commit, or stage release.

use crate::{err, hash_valid, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use uuid::Uuid;

const SOURCE_BYTES: u64 = 8_383_096;
const SOURCE_SHA256: &str = "e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6";
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
const FRAME_LIMIT: usize = 8192;
const CHUNK: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Direction {
    Source,
    Output,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataIdentity {
    attempt_id: Uuid,
    correlation_id: Uuid,
    direction: Direction,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum HeaderKind {
    HayesTransferStart,
    HayesTransferEnd,
    HayesTransferAccepted,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Header<'a> {
    version: u8,
    kind: HeaderKind,
    #[serde(flatten)]
    identity: DataIdentity,
    bytes: u64,
    sha256: &'a str,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Data {
    version: u8,
    kind: String,
    attempt_id: Uuid,
    correlation_id: Uuid,
    direction: Direction,
    offset: u64,
    data: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Next {
    version: u8,
    kind: &'static str,
    #[serde(flatten)]
    identity: DataIdentity,
    offset: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Transfer<'a> {
    identity: DataIdentity,
    bytes: u64,
    sha256: &'a str,
}

/// Carries one already authenticated exchange with the same absolute deadline
/// for every operation. Implementations must invalidate the channel on error.
pub(super) trait Frames {
    fn check(&mut self) -> Result<()>;
    fn receive(&mut self) -> Result<Vec<u8>>;
    fn send(&mut self, frame: &[u8]) -> Result<()>;
}

pub(super) struct OwnerFrames<'a> {
    connection: &'a mut super::super::owner_channel::Connection,
    deadline: Instant,
    cancellation: &'a AtomicBool,
    check_owner: &'a mut dyn FnMut() -> Result<()>,
}

impl<'a> OwnerFrames<'a> {
    pub(super) fn new(
        connection: &'a mut super::super::owner_channel::Connection,
        deadline: Instant,
        cancellation: &'a AtomicBool,
        check_owner: &'a mut dyn FnMut() -> Result<()>,
    ) -> Self {
        Self {
            connection,
            deadline: deadline.min(Instant::now() + Duration::from_secs(300)),
            cancellation,
            check_owner,
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.connection.invalidate();
    }
}

impl Frames for OwnerFrames<'_> {
    fn check(&mut self) -> Result<()> {
        if self.cancellation.load(Ordering::SeqCst) || super::super::cancelled() {
            return Err(err("NATIVE_CANCELLED"));
        }
        if Instant::now() >= self.deadline {
            return Err(invalid());
        }
        (self.check_owner)()
    }

    fn receive(&mut self) -> Result<Vec<u8>> {
        self.connection
            .read_frame(self.deadline, self.cancellation, self.check_owner)
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.connection
            .send_frame(frame, self.deadline, self.cancellation, self.check_owner)
    }
}

impl<'a> Transfer<'a> {
    pub(super) fn source(attempt: Uuid, correlation: Uuid) -> Result<Self> {
        Self::new(
            attempt,
            correlation,
            Direction::Source,
            SOURCE_BYTES,
            SOURCE_SHA256,
        )
    }

    pub(super) fn output(
        attempt: Uuid,
        correlation: Uuid,
        bytes: u64,
        sha256: &'a str,
    ) -> Result<Self> {
        Self::new(attempt, correlation, Direction::Output, bytes, sha256)
    }

    fn new(
        attempt_id: Uuid,
        correlation_id: Uuid,
        direction: Direction,
        bytes: u64,
        sha256: &'a str,
    ) -> Result<Self> {
        let canonical = |id: Uuid| {
            id.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num())
        };
        if !canonical(attempt_id)
            || !canonical(correlation_id)
            || !(1..=OUTPUT_LIMIT).contains(&bytes)
            || !hash_valid(sha256)
        {
            return Err(invalid());
        }
        Ok(Self {
            identity: DataIdentity {
                attempt_id,
                correlation_id,
                direction,
            },
            bytes,
            sha256,
        })
    }

    pub(super) fn bytes(self) -> u64 {
        self.bytes
    }

    pub(super) fn sha256(self) -> &'a str {
        self.sha256
    }

    fn header(self, kind: HeaderKind) -> Result<Vec<u8>> {
        encode(&Header {
            version: 1,
            kind,
            identity: self.identity,
            bytes: self.bytes,
            sha256: self.sha256,
        })
    }

    fn data(self, offset: u64, data: String) -> Data {
        Data {
            version: 1,
            kind: "hayes-transfer-data".into(),
            attempt_id: self.identity.attempt_id,
            correlation_id: self.identity.correlation_id,
            direction: self.identity.direction,
            offset,
            data,
        }
    }

    fn check_header(self, frames: &mut impl Frames, kind: HeaderKind) -> Result<()> {
        if frames.receive()? != self.header(kind)? {
            return Err(invalid());
        }
        Ok(())
    }

    fn next(self, offset: u64) -> Result<Vec<u8>> {
        encode(&Next {
            version: 1,
            kind: "hayes-transfer-next",
            identity: self.identity,
            offset,
        })
    }

    fn check_next(self, frames: &mut impl Frames, offset: u64) -> Result<()> {
        if frames.receive()? != self.next(offset)? {
            return Err(invalid());
        }
        Ok(())
    }

    /// Write only into an unpublished stage. The caller sends ACCEPTED
    /// separately, after it has synced and re-read its exact stage.
    pub(super) fn receive(self, frames: &mut impl Frames, sink: &mut impl Write) -> Result<()> {
        self.check_header(frames, HeaderKind::HayesTransferStart)?;
        let mut hasher = Sha256::new();
        let mut offset = 0;
        let mut decoded = [0u8; CHUNK];
        let result = (|| {
            while offset < self.bytes {
                let raw = frames.receive()?;
                if raw.is_empty() || raw.len() > FRAME_LIMIT {
                    return Err(invalid());
                }
                let frame: Data = serde_json::from_slice(&raw).map_err(|_| invalid())?;
                let n = (self.bytes - offset).min(CHUNK as u64) as usize;
                let count = STANDARD
                    .decode_slice(&frame.data, &mut decoded)
                    .map_err(|_| invalid())?;
                if count != n || raw != encode(&self.data(offset, STANDARD.encode(&decoded[..n])))?
                {
                    return Err(invalid());
                }
                sink.write_all(&decoded[..n]).map_err(|_| invalid())?;
                hasher.update(&decoded[..n]);
                offset += n as u64;
            }
            self.check_header(frames, HeaderKind::HayesTransferEnd)?;
            if format!("{:x}", hasher.finalize()) != self.sha256 {
                return Err(invalid());
            }
            Ok(())
        })();
        decoded.fill(0);
        result
    }

    pub(super) fn accept(self, frames: &mut impl Frames) -> Result<()> {
        frames.send(&self.header(HeaderKind::HayesTransferAccepted)?)
    }

    /// Serve one frame for each exact NEXT after START. The caller must
    /// already hold verified output custody and an immutable readback.
    pub(super) fn serve_output(
        self,
        frames: &mut impl Frames,
        source: &mut impl Read,
    ) -> Result<()> {
        if self.identity.direction != Direction::Output {
            return Err(invalid());
        }
        frames.send(&self.header(HeaderKind::HayesTransferStart)?)?;
        let mut hasher = Sha256::new();
        let mut offset = 0;
        let mut buffer = [0u8; CHUNK];
        let result = (|| {
            while offset < self.bytes {
                self.check_next(frames, offset)?;
                let n = (self.bytes - offset).min(CHUNK as u64) as usize;
                source.read_exact(&mut buffer[..n]).map_err(|_| invalid())?;
                hasher.update(&buffer[..n]);
                frames.send(&encode(&self.data(offset, STANDARD.encode(&buffer[..n])))?)?;
                offset += n as u64;
            }
            if source.read(&mut buffer[..1]).map_err(|_| invalid())? != 0
                || format!("{:x}", hasher.finalize()) != self.sha256
            {
                return Err(invalid());
            }
            self.check_next(frames, offset)?;
            frames.send(&self.header(HeaderKind::HayesTransferEnd)?)?;
            self.check_header(frames, HeaderKind::HayesTransferAccepted)
        })();
        buffer.fill(0);
        result
    }
}

fn encode(frame: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(frame).map_err(|_| invalid())?;
    if raw.is_empty() || raw.len() > FRAME_LIMIT {
        return Err(invalid());
    }
    Ok(raw)
}

fn invalid() -> crate::LifecycleError {
    err("HAYES_TRANSFER_INVALID")
}

#[cfg(test)]
#[path = "native_runtime_hayes_transfer_tests.rs"]
mod tests;
