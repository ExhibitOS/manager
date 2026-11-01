// SPDX-License-Identifier: Apache-2.0
//! Inactive, closed wire representation for the owned materializer. This
//! validates correlation only; a reply is not authority or a drain proof.
use super::*;

const CHUNK_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const WIRE_LIMIT: usize = 8192;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Chunk { pub device: String, pub inode: String, pub bytes: u64 }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    pub version: u32, pub kind: String, pub tenant_id: String, pub resource_id: String,
    pub job_id: String, pub user_id: String, pub request_id: String, pub lease_id: String,
    pub attempt_id: String, pub runtime_generation: String, pub stage_id: String,
    pub reservation_id: String, pub reservation_generation: String, pub archive_bytes: u64,
    pub archive_sha256: String, pub stage_device: String, pub stage_inode: String,
    pub chunks: Vec<Chunk>, pub reservation_digest: String, pub source_digest: String,
    pub worker_digest: String, pub profile: String, pub format: String,
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn request() -> Request {
        let id=||Uuid::new_v4().to_string();
        Request { version:1,kind:"oex-chunks-v1".into(),tenant_id:id(),resource_id:id(),job_id:id(),user_id:id(),
            request_id:id(),lease_id:id(),attempt_id:id(),runtime_generation:id(),stage_id:id(),reservation_id:id(),
            reservation_generation:id(),archive_bytes:3,archive_sha256:digest(b"abc"),stage_device:"1".into(),stage_inode:"2".into(),
            chunks:vec![Chunk{device:"1".into(),inode:"3".into(),bytes:3}],reservation_digest:"a".repeat(64),
            source_digest:digest(b"abc"),worker_digest:"c".repeat(64),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into() }
    }
    #[test]
    fn correlates_exact_bytes_and_refuses_foreign_ack() {
        let r=request();let bytes=r.encode().unwrap();
        let mut ack=serde_json::json!({"version":1,"kind":"materialized","status":"complete","attemptId":r.attempt_id,
            "runtimeGeneration":r.runtime_generation,"stageId":r.stage_id,"reservationId":r.reservation_id,
            "reservationGeneration":r.reservation_generation,"receiptSha256":digest(&bytes),"stageDevice":r.stage_device,
            "stageInode":r.stage_inode,"bytes":r.archive_bytes,"inputSha256":r.archive_sha256});
        assert!(r.check_ack(&bytes,&serde_json::to_vec(&ack).unwrap()).is_ok());
        ack["stageId"]=Value::String(Uuid::new_v4().to_string());
        assert!(r.check_ack(&bytes,&serde_json::to_vec(&ack).unwrap()).is_err());
        assert!(r.check_ack(b"{}",&serde_json::to_vec(&ack).unwrap()).is_err());
        let mut r=request();r.chunks[0].inode=r.stage_inode.clone();assert!(r.encode().is_err());
        let mut r=request();r.chunks[0].bytes=2;assert!(r.encode().is_err());
        let mut r=request();r.source_digest="d".repeat(64);assert!(r.encode().is_err());
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack {
    version: u32, kind: String, status: String, attempt_id: String, runtime_generation: String,
    stage_id: String, reservation_id: String, reservation_generation: String, receipt_sha256: String,
    stage_device: String, stage_inode: String, bytes: u64, input_sha256: String,
}

fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value
        && id.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num()))
}
fn decimal(value: &str, nonzero: bool) -> bool {
    value.parse::<u64>().is_ok_and(|n| n.to_string() == value && (!nonzero || n != 0))
}
#[allow(dead_code)]
impl Request {
    fn valid(&self) -> bool {
        if self.version != 1 || self.kind != "oex-chunks-v1" || self.profile != "bounded-file-v1"
            || self.format != "1.0.0-draft.9" || !(1..=MAX_BYTES).contains(&self.archive_bytes)
            || self.source_digest != self.archive_sha256
            || [&self.tenant_id,&self.resource_id,&self.job_id,&self.user_id,&self.request_id,&self.lease_id,
                &self.attempt_id,&self.runtime_generation,&self.stage_id,&self.reservation_id,&self.reservation_generation]
                .iter().any(|value| !uuid(value))
            || [&self.archive_sha256,&self.reservation_digest,&self.source_digest,&self.worker_digest]
                .iter().any(|value| !hash_valid(value))
            || !decimal(&self.stage_device,false) || !decimal(&self.stage_inode,true)
            || self.chunks.len() != self.archive_bytes.div_ceil(CHUNK_BYTES) as usize { return false; }
        let mut seen = std::collections::HashSet::new();
        self.chunks.iter().enumerate().all(|(index,chunk)| {
            let expected = CHUNK_BYTES.min(self.archive_bytes - index as u64 * CHUNK_BYTES);
            decimal(&chunk.device,false) && decimal(&chunk.inode,true) && chunk.bytes == expected
                && (&chunk.device,&chunk.inode) != (&self.stage_device,&self.stage_inode)
                && seen.insert((&chunk.device,&chunk.inode))
        })
    }
    /// Callers must construct the fields from the authenticated claim and
    /// pinned descriptors, not from an untrusted worker response.
    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        if !self.valid() { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
        let bytes=serde_json::to_vec(self).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.len() < 2 || bytes.len() > WIRE_LIMIT { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
        Ok(bytes)
    }
    /// The actual bytes sent determine receiptSha256. The ACK is raw JSON to
    /// EOF (not length-framed), bounded by the caller while reading.
    pub(super) fn check_ack(&self, sent: &[u8], reply: &[u8]) -> Result<()> {
        if self.encode()?.as_slice() != sent || reply.is_empty() || reply.len() > WIRE_LIMIT {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let ack: Ack=serde_json::from_slice(reply).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        if ack.version != 1 || ack.kind != "materialized" || ack.status != "complete"
            || ack.attempt_id != self.attempt_id || ack.runtime_generation != self.runtime_generation
            || ack.stage_id != self.stage_id || ack.reservation_id != self.reservation_id
            || ack.reservation_generation != self.reservation_generation || ack.receipt_sha256 != digest(sent)
            || ack.stage_device != self.stage_device || ack.stage_inode != self.stage_inode
            || ack.bytes != self.archive_bytes || ack.input_sha256 != self.archive_sha256 {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        Ok(())
    }
}
