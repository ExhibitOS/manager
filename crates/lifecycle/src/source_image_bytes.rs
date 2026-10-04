// SPDX-License-Identifier: Apache-2.0
//! Bounded current Engine image export, not a caller-supplied archive proof.
use crate::*;
use std::io::Write;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageArchiveProof {
    pub reference: String,
    pub content_id: String,
    pub archive: String,
    pub bytes: u64,
    pub sha256: String,
}
fn bounded(
    mut input: impl Read,
    mut file: File,
    limit: u64,
) -> Result<crate::backup_creation::FileHash> {
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    let mut sha = Sha256::new();
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > limit {
            return Err(err("STORAGE_QUOTA"));
        }
        file.write_all(&buffer[..count])
            .map_err(|_| err("STORAGE_UNAVAILABLE"))?;
        sha.update(&buffer[..count]);
    }
    if bytes == 0 {
        return Err(err("UPDATE_SOURCE_IMAGES_INVALID"));
    }
    file.sync_all().map_err(|_| err("STORAGE_UNAVAILABLE"))?;
    Ok(crate::backup_creation::FileHash {
        bytes,
        sha256: format!("{:x}", sha.finalize()),
    })
}
fn save(image: &str, path: &Path, limit: u64) -> Result<crate::backup_creation::FileHash> {
    let file = private_options()
        .open(path)
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    let binary = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
    let mut child = process_window::background_command(binary)
        .args(["image", "save", image])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| err("ENGINE_PERMISSION"))?;
    let pipe = child
        .stdout
        .take()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let _ = tx.send(bounded(pipe, file, limit));
    });
    let result = rx.recv_timeout(Duration::from_secs(600));
    let result = match result {
        Ok(Ok(proof)) => Ok(proof),
        Ok(Err(e)) => {
            let _ = child.kill();
            Err(e)
        }
        Err(_) => {
            let _ = child.kill();
            Err(err("ENGINE_TIMEOUT"))
        }
    };
    let exited = child.wait().map_err(|_| err("ENGINE_OPERATION_FAILED"))?;
    let _ = reader.join();
    let proof = result?;
    if !exited.success() {
        return Err(err("ENGINE_OPERATION_FAILED"));
    }
    let repeated = crate::backup_creation::hash_file(path)?;
    if proof.bytes != repeated.bytes || proof.sha256 != repeated.sha256 {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    Ok(proof)
}
pub(super) fn export(
    work: &Path,
    expected: &[crate::restoration::PreservedImage],
) -> Result<Vec<ImageArchiveProof>> {
    let mut bytes = 0u64;
    for image in expected {
        bytes = bytes
            .checked_add(image.bytes)
            .ok_or_else(|| err("STORAGE_QUOTA"))?;
    }
    if bytes > 2 * 1024 * 1024 * 1024
        || fs2::available_space(work).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < bytes + 2 * 1024 * 1024 * 1024
    {
        return Err(err("STORAGE_QUOTA"));
    }
    let mut proofs = Vec::new();
    for (i, image) in expected.iter().enumerate() {
        let id = crate::backup_creation::local_image("docker", &image.reference)?;
        if id != image.content_id {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
        let path = work.join(format!("image-{i}.tar"));
        let saved = save(&id, &path, image.bytes)?;
        if saved.bytes != image.bytes
            || saved.sha256 != image.sha256
            || crate::backup_creation::local_image("docker", &image.reference)? != id
        {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
        proofs.push(ImageArchiveProof {
            reference: image.reference.clone(),
            content_id: id,
            archive: image.archive.clone(),
            bytes: saved.bytes,
            sha256: saved.sha256,
        });
    }
    Ok(proofs)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_image_stream_never_writes_beyond_authenticated_budget() {
        let path = std::env::temp_dir().join(format!("exhibitos-image-stream-{}", Uuid::new_v4()));
        let file = private_options().open(&path).unwrap();
        let error = bounded(&b"four"[..], file, 3).unwrap_err();
        assert_eq!(error.code, "STORAGE_QUOTA");
        assert!(fs::metadata(path).unwrap().len() <= 3);
    }
}
