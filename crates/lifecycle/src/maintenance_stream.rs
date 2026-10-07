// SPDX-License-Identifier: Apache-2.0
//! Internal authenticated stream codec. Open into private quarantine only: a
//! valid prefix is never proof of a complete archive or permission to publish.
use super::{Result, err};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

#[path = "maintenance_compact.rs"]
mod compact;
pub(crate) use compact::seal as seal_compact;
pub(crate) const MAGIC: &[u8] = b"ExhibitOS-stream-v1\0";
const CHUNK: usize = 1024 * 1024;
const DATA: u8 = 1;
const FINAL: u8 = 2;
fn input_error() -> super::LifecycleError {
    err("STREAM_AUTHENTICATION_FAILED")
}
fn output_error() -> super::LifecycleError {
    err("STREAM_WRITE_UNCERTAIN")
}
fn aad(header: &[u8], context: &[u8], index: u64, kind: u8, len: u32) -> Vec<u8> {
    let mut a = header.to_vec();
    a.extend_from_slice(&(context.len() as u64).to_be_bytes());
    a.extend_from_slice(context);
    a.extend_from_slice(&index.to_be_bytes());
    a.push(kind);
    a.extend_from_slice(&len.to_be_bytes());
    a
}
fn frame<W: Write>(
    w: &mut W,
    cipher: &Aes256Gcm,
    header: &[u8],
    context: &[u8],
    index: u64,
    kind: u8,
    plain: &[u8],
) -> Result<()> {
    let len = u32::try_from(plain.len()).map_err(|_| err("STREAM_QUOTA"))?;
    // Independent OS-random96-bit nonce for each record; no repeat on counter reset.
    let mut nonce = [0u8; 12];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| err("STREAM_RANDOM_UNAVAILABLE"))?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: &aad(header, context, index, kind, len),
            },
        )
        .map_err(|_| err("STREAM_ENCRYPTION_FAILED"))?;
    w.write_all(&[kind])
        .and_then(|()| w.write_all(&len.to_be_bytes()))
        .and_then(|()| w.write_all(&nonce))
        .and_then(|()| w.write_all(&encrypted))
        .map_err(|_| output_error())
}
/// Memory stays bounded by one1MiB plaintext record. Caller controls the private
/// staging file and must fsync/verify/no-replace publish only after success.
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
        frame(w, &cipher, &header, context, index, DATA, &buf[..n])?;
        index = index.checked_add(1).ok_or_else(|| err("STREAM_QUOTA"))?;
    }
    let mut final_record = total.to_be_bytes().to_vec();
    final_record.extend(hash.finalize());
    frame(w, &cipher, &header, context, index, FINAL, &final_record)?;
    Ok(total)
}
/// Authenticated records may reach the sink before final authentication. The
/// sink MUST be unpublished private quarantine; retain failure artifacts and
/// never register/activate them. Authentication failure cannot roll back a sink.
pub(crate) fn open<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    key: &[u8],
    context: &[u8],
    limit: u64,
) -> Result<u64> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| err("STREAM_KEY_INVALID"))?;
    let mut header = vec![0u8; MAGIC.len() + 16];
    r.read_exact(&mut header).map_err(|_| input_error())?;
    if header.starts_with(compact::MAGIC) {
        return compact::open_records(r, w, &cipher, &header, context, limit);
    }
    if !header.starts_with(MAGIC) {
        return Err(input_error());
    }
    let mut total = 0u64;
    let mut index = 0u64;
    let mut hash = Sha256::new();
    loop {
        let mut framing = [0u8; 17];
        r.read_exact(&mut framing).map_err(|_| input_error())?;
        let kind = framing[0];
        let len = u32::from_be_bytes(framing[1..5].try_into().unwrap());
        if !(kind == DATA && len > 0 && len as usize <= CHUNK || kind == FINAL && len == 40) {
            return Err(input_error());
        }
        if kind == DATA
            && total
                .checked_add(u64::from(len))
                .filter(|n| *n <= limit)
                .is_none()
        {
            return Err(err("STREAM_QUOTA"));
        }
        let mut encrypted = vec![0u8; len as usize + 16];
        r.read_exact(&mut encrypted).map_err(|_| input_error())?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&framing[5..]),
                Payload {
                    msg: &encrypted,
                    aad: &aad(&header, context, index, kind, len),
                },
            )
            .map_err(|_| input_error())?;
        if kind == FINAL {
            if plain[..8] != total.to_be_bytes() || plain[8..] != hash.finalize()[..] {
                return Err(input_error());
            }
            let mut trailing = [0u8; 1];
            // read_exact distinguishes clean EOF from actual read errors and retries EINTR.
            match r.read_exact(&mut trailing) {
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(total),
                _ => return Err(input_error()),
            }
        }
        hash.update(&plain);
        total += u64::from(len);
        w.write_all(&plain).map_err(|_| output_error())?;
        index = index.checked_add(1).ok_or_else(|| err("STREAM_QUOTA"))?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn archive(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        seal(
            &mut &data[..],
            &mut out,
            &[7; 32],
            b"synthetic-candidates",
            data.len() as u64,
        )
        .unwrap();
        out
    }
    fn decode(bytes: &[u8], key: &[u8], context: &[u8], limit: u64) -> Result<Vec<u8>> {
        let mut sink = Vec::new();
        open(&mut &bytes[..], &mut sink, key, context, limit)?;
        Ok(sink)
    }
    fn frames(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut pos = MAGIC.len() + 16;
        let mut all = Vec::new();
        while pos < bytes.len() {
            let n = u32::from_be_bytes(bytes[pos + 1..pos + 5].try_into().unwrap()) as usize + 33;
            all.push(bytes[pos..pos + n].to_vec());
            pos += n;
        }
        all
    }
    #[test]
    fn multi_record_empty_and_randomized_roundtrip() {
        for data in [vec![], vec![31; CHUNK * 2 + 127]] {
            let a = archive(&data);
            assert_ne!(a, archive(&data));
            assert_eq!(
                decode(&a, &[7; 32], b"synthetic-candidates", data.len() as u64).unwrap(),
                data
            );
        }
    }
    #[test]
    fn sequence_identity_context_key_and_every_record_are_authenticated() {
        let data = vec![11; CHUNK + 37];
        let a = archive(&data);
        let b = archive(&data);
        let records = frames(&a);
        let header = &a[..MAGIC.len() + 16];
        let changes = [
            vec![records[1].clone(), records[0].clone(), records[2].clone()],
            vec![records[0].clone(), records[0].clone(), records[2].clone()],
            vec![records[1].clone(), records[2].clone()],
            vec![
                records[0].clone(),
                frames(&b)[1].clone(),
                records[2].clone(),
            ],
        ];
        for r in changes {
            let mut bad = header.to_vec();
            for p in r {
                bad.extend(p);
            }
            assert!(decode(&bad, &[7; 32], b"synthetic-candidates", data.len() as u64).is_err());
        }
        assert!(decode(&a, &[8; 32], b"synthetic-candidates", data.len() as u64).is_err());
        assert!(decode(&a, &[7; 32], b"profile-v1", data.len() as u64).is_err());
        for pos in [
            MAGIC.len(),
            MAGIC.len() + 16,
            MAGIC.len() + 18,
            MAGIC.len() + 22,
            a.len() - 1,
        ] {
            let mut bad = a.clone();
            bad[pos] ^= 1;
            assert!(decode(&bad, &[7; 32], b"synthetic-candidates", data.len() as u64).is_err());
        }
    }
    #[test]
    fn truncation_complete_prefix_trailer_and_quota_cannot_publish() {
        let data = vec![44; CHUNK + 17];
        let a = archive(&data);
        let r = frames(&a);
        for len in [
            0,
            MAGIC.len() + 16,
            MAGIC.len() + 16 + r[0].len(),
            a.len() - 1,
        ] {
            assert!(
                decode(
                    &a[..len],
                    &[7; 32],
                    b"synthetic-candidates",
                    data.len() as u64
                )
                .is_err()
            );
        }
        let mut extra = a.clone();
        extra.push(0);
        assert!(decode(&extra, &[7; 32], b"synthetic-candidates", data.len() as u64).is_err());
        assert_eq!(
            decode(&a, &[7; 32], b"synthetic-candidates", data.len() as u64 - 1)
                .unwrap_err()
                .code,
            "STREAM_QUOTA"
        );
        let mut staged = Vec::new();
        assert_eq!(
            seal(
                &mut data.as_slice(),
                &mut staged,
                &[7; 32],
                b"synthetic-candidates",
                0
            )
            .unwrap_err()
            .code,
            "STREAM_QUOTA"
        );
        assert!(
            decode(
                &staged,
                &[7; 32],
                b"synthetic-candidates",
                data.len() as u64
            )
            .is_err()
        );
        let mut oversize = MAGIC.to_vec();
        oversize.extend([0; 16]);
        oversize.push(DATA);
        oversize.extend(u32::MAX.to_be_bytes());
        oversize.extend([0; 12]);
        assert!(decode(&oversize, &[7; 32], b"synthetic-candidates", u64::MAX).is_err());
    }
    #[test]
    fn authenticated_final_count_or_hash_mismatch_is_refused() {
        let a = archive(b"candidate");
        let records = frames(&a);
        let header = &a[..MAGIC.len() + 16];
        let cipher = Aes256Gcm::new_from_slice(&[7; 32]).unwrap();
        for field in [0, 8] {
            let mut bad = header.to_vec();
            bad.extend(&records[0]);
            let mut final_record = 9u64.to_be_bytes().to_vec();
            final_record.extend(Sha256::digest(b"candidate"));
            final_record[field] ^= 1;
            frame(
                &mut bad,
                &cipher,
                header,
                b"synthetic-candidates",
                1,
                FINAL,
                &final_record,
            )
            .unwrap();
            assert!(decode(&bad, &[7; 32], b"synthetic-candidates", 100).is_err());
        }
    }
    struct BrokenSource;
    impl Read for BrokenSource {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic read failure"))
        }
    }
    #[test]
    fn read_errors_and_short_reads_do_not_forge_empty_completion() {
        let mut out = Vec::new();
        assert_eq!(
            seal(
                &mut BrokenSource,
                &mut out,
                &[7; 32],
                b"synthetic-candidates",
                100
            )
            .unwrap_err()
            .code,
            "STREAM_SOURCE_UNAVAILABLE"
        );
        assert!(decode(&out, &[7; 32], b"synthetic-candidates", 100).is_err());
        struct Short(std::io::Cursor<Vec<u8>>);
        impl Read for Short {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                let n = b.len().min(17);
                self.0.read(&mut b[..n])
            }
        }
        let data = vec![12; CHUNK + 31];
        let mut encrypted = Vec::new();
        seal(
            &mut Short(std::io::Cursor::new(data.clone())),
            &mut encrypted,
            &[7; 32],
            b"synthetic-candidates",
            data.len() as u64,
        )
        .unwrap();
        let mut opened = Vec::new();
        open(
            &mut Short(std::io::Cursor::new(encrypted)),
            &mut opened,
            &[7; 32],
            b"synthetic-candidates",
            data.len() as u64,
        )
        .unwrap();
        assert_eq!(opened, data);
    }
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn source_and_destination_failures_are_not_completion() {
        let a = archive(b"candidate");
        assert_eq!(
            open(
                &mut a.as_slice(),
                &mut Broken,
                &[7; 32],
                b"synthetic-candidates",
                100
            )
            .unwrap_err()
            .code,
            "STREAM_WRITE_UNCERTAIN"
        );
        assert_eq!(
            seal(
                &mut b"candidate".as_slice(),
                &mut Broken,
                &[7; 32],
                b"synthetic-candidates",
                100
            )
            .unwrap_err()
            .code,
            "STREAM_WRITE_UNCERTAIN"
        );
    }
}
