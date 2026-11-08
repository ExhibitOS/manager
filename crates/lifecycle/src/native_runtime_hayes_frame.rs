// SPDX-License-Identifier: Apache-2.0
//! Preflight for the exact selected Hayes generate frame. This is a byte
//! observation only. It grants neither Validate nor output-transfer authority.

use crate::{err, Result};
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) const BODY_BYTES: u32 = 16_036_864;
pub(super) const BODY_SHA: &str =
    "694a9cf81dfbf12fd0841ae45240c5ae0f1d7cb129d0ee122be7cff86c4625bc";

#[derive(Debug, PartialEq)]
pub(super) struct GenerateTelemetry {
    pub(super) relative_error: f32,
    pub(super) peak_rss_kib: u32,
}

pub(super) fn inspect_selected_frame(frame: &mut impl Read) -> Result<GenerateTelemetry> {
    inspect_frame(frame, BODY_BYTES, BODY_SHA)
}

fn inspect_frame(
    frame: &mut impl Read,
    expected_bytes: u32,
    expected_sha: &str,
) -> Result<GenerateTelemetry> {
    let mut telemetry = [0u8; 8];
    frame.read_exact(&mut telemetry).map_err(|_| invalid())?;
    let relative_error = f32::from_le_bytes(telemetry[..4].try_into().unwrap());
    let peak_rss_kib = u32::from_le_bytes(telemetry[4..].try_into().unwrap());
    if !(0.0..=0.005).contains(&relative_error) || peak_rss_kib == 0 || expected_bytes < 12 {
        return Err(invalid());
    }
    let mut header = [0u8; 12];
    frame.read_exact(&mut header).map_err(|_| invalid())?;
    if &header[..4] != b"glTF"
        || u32::from_le_bytes(header[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(header[8..].try_into().unwrap()) != expected_bytes
    {
        return Err(invalid());
    }
    let mut digest = Sha256::new();
    digest.update(header);
    let mut remaining = expected_bytes as usize - header.len();
    let mut buffer = [0u8; 16 * 1024];
    let result = (|| {
        while remaining != 0 {
            let n = remaining.min(buffer.len());
            frame.read_exact(&mut buffer[..n]).map_err(|_| invalid())?;
            digest.update(&buffer[..n]);
            remaining -= n;
        }
        if frame.read(&mut buffer[..1]).map_err(|_| invalid())? != 0
            || format!("{:x}", digest.finalize()) != expected_sha
        {
            return Err(invalid());
        }
        Ok(GenerateTelemetry {
            relative_error,
            peak_rss_kib,
        })
    })();
    buffer.fill(0);
    result
}

fn invalid() -> crate::LifecycleError {
    err("HAYES_GENERATE_FRAME_INVALID")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_hash_covers_glb_only_and_validates_telemetry_and_eof() {
        let mut body = b"glTF".to_vec();
        body.extend(2u32.to_le_bytes());
        body.extend(16u32.to_le_bytes());
        body.extend([1, 2, 3, 4]);
        let sha = format!("{:x}", Sha256::digest(&body));
        let mut frame = 0.001f32.to_le_bytes().to_vec();
        frame.extend(18_000u32.to_le_bytes());
        frame.extend(&body);
        assert_eq!(
            inspect_frame(&mut frame.as_slice(), 16, &sha).unwrap(),
            GenerateTelemetry {
                relative_error: 0.001,
                peak_rss_kib: 18_000
            }
        );
        assert!(inspect_selected_frame(&mut frame.as_slice()).is_err());
        let whole_frame_sha = format!("{:x}", Sha256::digest(&frame));
        assert!(inspect_frame(&mut frame.as_slice(), 16, &whole_frame_sha).is_err());
        assert!(inspect_frame(&mut &frame[..frame.len() - 1], 16, &sha).is_err());
        let mut extra = frame.clone();
        extra.push(0);
        assert!(inspect_frame(&mut extra.as_slice(), 16, &sha).is_err());
        frame[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(inspect_frame(&mut frame.as_slice(), 16, &sha).is_err());
    }
}
