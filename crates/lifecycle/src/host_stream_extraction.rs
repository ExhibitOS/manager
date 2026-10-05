// SPDX-License-Identifier: Apache-2.0
//! Authenticated chunk sink; its private staging tree is never a published proof.
use super::*;
use std::io::Write;

pub(super) struct Extraction {
    profile: PathBuf,
    recovered: PathBuf,
    prefix: Vec<u8>,
    manifest_len: Option<usize>,
    manifest: Vec<u8>,
    inventory: Option<Inventory>,
    next: usize,
    output: Option<File>,
    remaining: u64,
    hash: Sha256,
}
impl Extraction {
    pub(super) fn new(profile: &Path, recovered: &Path) -> Self {
        Self {
            profile: profile.into(),
            recovered: recovered.into(),
            prefix: Vec::new(),
            manifest_len: None,
            manifest: Vec::new(),
            inventory: None,
            next: 0,
            output: None,
            remaining: 0,
            hash: Sha256::new(),
        }
    }
    fn advance(&mut self) -> Result<()> {
        let m = self.inventory.as_ref().ok_or_else(fail)?;
        loop {
            let Some(e) = m.items.get(self.next) else {
                return Ok(());
            };
            if e.kind == "directory" {
                installations::new_directory(&self.recovered.join(&e.path))?;
                self.next += 1;
                continue;
            }
            if self.output.is_none() {
                self.output = Some(private_new(&self.recovered.join(&e.path))?);
                self.remaining = e.bytes;
                self.hash = Sha256::new();
            }
            if self.remaining != 0 {
                return Ok(());
            }
            if Some(format!("{:x}", self.hash.clone().finalize())) != e.sha256 {
                return Err(fail());
            }
            self.output
                .take()
                .ok_or_else(fail)?
                .sync_all()
                .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
            self.next += 1;
        }
    }
    fn accept(&mut self, mut bytes: &[u8]) -> Result<()> {
        if self.prefix.len() < 8 {
            let n = bytes.len().min(8 - self.prefix.len());
            self.prefix.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.prefix.len() < 8 {
                return Ok(());
            }
            let len = u64::from_be_bytes(self.prefix.as_slice().try_into().map_err(|_| fail())?);
            if len == 0 || len > MANIFEST_LIMIT {
                return Err(fail());
            }
            self.manifest_len = Some(len as usize);
        }
        if self.inventory.is_none() {
            let len = self.manifest_len.ok_or_else(fail)?;
            let n = bytes.len().min(len - self.manifest.len());
            self.manifest.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.manifest.len() < len {
                return Ok(());
            }
            let m: Inventory = serde_json::from_slice(&self.manifest).map_err(|_| fail())?;
            validate_inventory(&m, &self.profile)?;
            installations::new_directory(&self.recovered)?;
            self.inventory = Some(m);
            self.advance()?;
        }
        while !bytes.is_empty() {
            let n = bytes
                .len()
                .min(usize::try_from(self.remaining).map_err(|_| fail())?);
            if n == 0 {
                return Err(fail());
            }
            self.output
                .as_mut()
                .ok_or_else(fail)?
                .write_all(&bytes[..n])
                .map_err(|_| err("HOST_WRITE_UNCERTAIN"))?;
            self.hash.update(&bytes[..n]);
            self.remaining -= n as u64;
            bytes = &bytes[n..];
            self.advance()?;
        }
        Ok(())
    }
    pub(super) fn finish(mut self) -> Result<(Inventory, Vec<u8>)> {
        self.advance()?;
        let m = self.inventory.take().ok_or_else(fail)?;
        if self.next != m.items.len() || self.remaining != 0 || self.output.is_some() {
            return Err(fail());
        }
        Ok((m, self.manifest))
    }
}
impl Write for Extraction {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.accept(bytes)
            .map_err(|_| std::io::Error::other("HOST_CHECKPOINT_INVALID"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
