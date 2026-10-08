// SPDX-License-Identifier: Apache-2.0
//! Version2 authenticates bounded independently compressed records. Quotas and
//! final digest describe expanded bytes; no prefix grants publication authority.
use super::*;
use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
pub(super) const MAGIC: &[u8] = b"ExhibitOS-stream-v2\0";
const COMPRESSED: u8 = 3;

pub(crate) fn seal<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    key: &[u8],
    context: &[u8],
    limit: u64,
) -> Result<u64> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| err("STREAM_KEY_INVALID"))?;
    let mut identity = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut identity)
        .map_err(|_| err("STREAM_RANDOM_UNAVAILABLE"))?;
    let mut header = MAGIC.to_vec();
    header.extend(identity);
    w.write_all(&header).map_err(|_| output_error())?;
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    let mut index = 0u64;
    let mut hash = Sha256::new();
    loop {
        let mut n = 0;
        while n < CHUNK {
            match r.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(m) => n += m,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(err("STREAM_SOURCE_UNAVAILABLE")),
            }
        }
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .filter(|v| *v <= limit)
            .ok_or_else(|| err("STREAM_QUOTA"))?;
        hash.update(&buf[..n]);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&buf[..n])
            .map_err(|_| err("STREAM_COMPRESSION_FAILED"))?;
        let compressed = encoder
            .finish()
            .map_err(|_| err("STREAM_COMPRESSION_FAILED"))?;
        if compressed.len() + 4 < n {
            let mut payload = (n as u32).to_be_bytes().to_vec();
            payload.extend(compressed);
            frame(w, &cipher, &header, context, index, COMPRESSED, &payload)?;
        } else {
            frame(w, &cipher, &header, context, index, DATA, &buf[..n])?;
        }
        index = index.checked_add(1).ok_or_else(|| err("STREAM_QUOTA"))?;
    }
    let mut final_record = total.to_be_bytes().to_vec();
    final_record.extend(hash.finalize());
    frame(w, &cipher, &header, context, index, FINAL, &final_record)?;
    Ok(total)
}

pub(super) fn open_records<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    cipher: &Aes256Gcm,
    header: &[u8],
    context: &[u8],
    limit: u64,
) -> Result<u64> {
    let mut total = 0u64;
    let mut index = 0u64;
    let mut hash = Sha256::new();
    loop {
        let mut framing = [0u8; 17];
        r.read_exact(&mut framing).map_err(|_| input_error())?;
        let kind = framing[0];
        let len = u32::from_be_bytes(framing[1..5].try_into().unwrap()) as usize;
        if !((kind == DATA || kind == COMPRESSED) && len > 0 && len <= CHUNK
            || kind == FINAL && len == 40)
        {
            return Err(input_error());
        }
        let mut encrypted = vec![0u8; len + 16];
        r.read_exact(&mut encrypted).map_err(|_| input_error())?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&framing[5..]),
                Payload {
                    msg: &encrypted,
                    aad: &aad(header, context, index, kind, len as u32),
                },
            )
            .map_err(|_| input_error())?;
        if kind == FINAL {
            if plain[..8] != total.to_be_bytes() || plain[8..] != hash.finalize()[..] {
                return Err(input_error());
            }
            let mut trailing = [0u8; 1];
            return match r.read_exact(&mut trailing) {
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(total),
                _ => Err(input_error()),
            };
        }
        let expanded = if kind == COMPRESSED {
            if plain.len() <= 4 {
                return Err(input_error());
            }
            let expected = u32::from_be_bytes(plain[..4].try_into().unwrap()) as usize;
            if expected == 0 || expected > CHUNK {
                return Err(input_error());
            }
            if total
                .checked_add(expected as u64)
                .filter(|v| *v <= limit)
                .is_none()
            {
                return Err(err("STREAM_QUOTA"));
            }
            // One extra byte exposes over-expansion without unbounded allocation.
            let mut out = vec![0u8; expected + 1];
            let mut decoder = Decompress::new(true);
            let status = decoder
                .decompress(&plain[4..], &mut out, FlushDecompress::Finish)
                .map_err(|_| input_error())?;
            if status != Status::StreamEnd
                || decoder.total_in() != (plain.len() - 4) as u64
                || decoder.total_out() != expected as u64
            {
                return Err(input_error());
            }
            out.truncate(expected);
            out
        } else {
            plain
        };
        total = total
            .checked_add(expanded.len() as u64)
            .filter(|v| *v <= limit)
            .ok_or_else(|| err("STREAM_QUOTA"))?;
        hash.update(&expanded);
        w.write_all(&expanded).map_err(|_| output_error())?;
        index = index.checked_add(1).ok_or_else(|| err("STREAM_QUOTA"))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(mut data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![];
        let limit = data.len() as u64;
        seal(&mut data, &mut bytes, &[7; 32], b"host-test", limit).unwrap();
        bytes
    }
    fn decode(a: &[u8], limit: u64) -> Result<Vec<u8>> {
        let mut out = vec![];
        super::super::open(&mut &a[..], &mut out, &[7; 32], b"host-test", limit)?;
        Ok(out)
    }
    #[test]
    fn compact_mixed_records_empty_and_legacy_roundtrip() {
        let mut random = vec![0u8; CHUNK];
        OsRng.fill_bytes(&mut random);
        let mut mixed = vec![42; CHUNK];
        mixed.extend(random);
        for data in [vec![], vec![11; CHUNK * 2 + 17], mixed] {
            let a = encode(&data);
            assert_eq!(decode(&a, data.len() as u64).unwrap(), data);
            assert_ne!(a, encode(&data));
            let mut old = vec![];
            super::super::seal(
                &mut &data[..],
                &mut old,
                &[7; 32],
                b"host-test",
                data.len() as u64,
            )
            .unwrap();
            assert_eq!(decode(&old, data.len() as u64).unwrap(), data);
        }
        assert!(encode(&vec![8; CHUNK * 2]).len() < CHUNK / 20);
    }
    #[test]
    fn compact_tamper_truncation_version_and_expanded_quota_refuse() {
        let a = encode(&vec![9; CHUNK + 7]);
        for n in [0, MAGIC.len() + 16, a.len() - 1] {
            assert!(decode(&a[..n], CHUNK as u64 + 7).is_err());
        }
        let mut bad = a.clone();
        bad[MAGIC.len()] ^= 1;
        assert!(decode(&bad, u64::MAX).is_err());
        let mut bad = a.clone();
        bad[..MAGIC.len()].copy_from_slice(super::super::MAGIC);
        assert!(decode(&bad, u64::MAX).is_err());
        let mut bad = a.clone();
        bad.push(0);
        assert!(decode(&bad, u64::MAX).is_err());
        let mut sink = vec![];
        assert_eq!(
            super::super::open(&mut &a[..], &mut sink, &[7; 32], b"host-test", 100)
                .unwrap_err()
                .code,
            "STREAM_QUOTA"
        );
        assert!(sink.is_empty());
    }
    #[test]
    fn authenticated_bad_compression_never_writes_a_record() {
        let cipher = Aes256Gcm::new_from_slice(&[7; 32]).unwrap();
        let mut header = MAGIC.to_vec();
        header.extend([0; 16]);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&[5; 100]).unwrap();
        let compressed = encoder.finish().unwrap();
        for size in [0, 99, 101, CHUNK as u32 + 1] {
            let mut payload = size.to_be_bytes().to_vec();
            payload.extend(&compressed);
            let mut a = header.clone();
            frame(
                &mut a,
                &cipher,
                &header,
                b"host-test",
                0,
                COMPRESSED,
                &payload,
            )
            .unwrap();
            let mut out = vec![];
            assert!(
                super::super::open(&mut &a[..], &mut out, &[7; 32], b"host-test", u64::MAX)
                    .is_err()
            );
            assert!(out.is_empty());
        }
        let mut payload = 100u32.to_be_bytes().to_vec();
        payload.extend(compressed);
        payload.push(0);
        let mut a = header.clone();
        frame(
            &mut a,
            &cipher,
            &header,
            b"host-test",
            0,
            COMPRESSED,
            &payload,
        )
        .unwrap();
        assert!(decode(&a, u64::MAX).is_err());
    }
    #[test]
    fn compact_frame_order_identity_wrong_key_and_final_digest_refuse() {
        let a = encode(&vec![37; CHUNK + 13]);
        let b = encode(&vec![37; CHUNK + 13]);
        let header_len = MAGIC.len() + 16;
        fn records(a: &[u8]) -> Vec<Vec<u8>> {
            let mut p = MAGIC.len() + 16;
            let mut rows = vec![];
            while p < a.len() {
                let len = u32::from_be_bytes(a[p + 1..p + 5].try_into().unwrap()) as usize + 33;
                rows.push(a[p..p + len].to_vec());
                p += len;
            }
            rows
        }
        let r = records(&a);
        for rows in [
            vec![r[1].clone(), r[0].clone(), r[2].clone()],
            vec![r[0].clone(), r[0].clone(), r[2].clone()],
            vec![r[0].clone(), records(&b)[1].clone(), r[2].clone()],
        ] {
            let mut bad = a[..header_len].to_vec();
            for row in rows {
                bad.extend(row);
            }
            assert!(decode(&bad, u64::MAX).is_err());
        }
        for (key, context) in [
            ([8; 32], b"host-test".as_slice()),
            ([7; 32], b"wrong-context".as_slice()),
        ] {
            assert!(
                super::super::open(&mut &a[..], &mut std::io::sink(), &key, context, u64::MAX)
                    .is_err()
            );
        }
        let cipher = Aes256Gcm::new_from_slice(&[7; 32]).unwrap();
        let mut bad = a[..a.len() - r[2].len()].to_vec();
        let mut final_bytes = (CHUNK as u64 + 13).to_be_bytes().to_vec();
        final_bytes.extend([0; 32]);
        frame(
            &mut bad,
            &cipher,
            &a[..header_len],
            b"host-test",
            2,
            FINAL,
            &final_bytes,
        )
        .unwrap();
        assert!(decode(&bad, u64::MAX).is_err());
    }
}
