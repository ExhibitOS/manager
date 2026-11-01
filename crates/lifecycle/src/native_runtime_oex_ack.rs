// SPDX-License-Identifier: Apache-2.0
//! Inactive bounded OEX worker response validation. This validates structure and
//! equality only: it does not authenticate a caller, establish the independent
//! Spec manifest, prove worker descendants/writes drained, or issue a seal.
use super::*;
use std::collections::HashSet;

const MAX_ACK: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 254;
const MAX_ENTRY: u64 = 32 * 1024 * 1024;
const MAX_OUTPUT: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Entry { path: String, offset: u64, bytes: u64, sha256: String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Ack {
    version: u32, kind: String, status: String, attempt_id: String,
    runtime_generation: String, reservation_id: String, reservation_generation: String,
    receipt_sha256: String, manifest_sha256: String, output_device: String,
    output_inode: String, output_bytes: u64, entries: Vec<Entry>,
}
/// Values must come from the held Session, locked provider, pinned output and
/// independently revalidated Spec manifest, not the worker's own reply.
#[allow(dead_code)]
pub(super) struct Expected<'a> {
    pub attempt_id: &'a str, pub runtime_generation: &'a str,
    pub reservation_id: &'a str, pub reservation_generation: &'a str,
    pub receipt_sha256: &'a str, pub manifest_sha256: &'a str,
    pub output_device: u64, pub output_inode: u64,
    pub actual_output_bytes: u64, pub capacity: u64,
}
#[allow(dead_code)]
pub(super) fn valid_expected(expected: &Expected<'_>) -> bool {
    !(expected.capacity == 0 || expected.capacity > MAX_OUTPUT
        || expected.actual_output_bytes > expected.capacity || expected.output_inode == 0
        || !hash_valid(expected.receipt_sha256) || !hash_valid(expected.manifest_sha256)
        || [expected.attempt_id, expected.runtime_generation, expected.reservation_id, expected.reservation_generation]
            .iter().any(|v| Uuid::parse_str(v).ok().is_none_or(|id| id.to_string() != *v || id.get_variant() != uuid::Variant::RFC4122 || !(1..=8).contains(&id.get_version_num()))))
}
#[allow(dead_code)]
pub(super) fn check(bytes: &[u8], expected: &Expected<'_>) -> Result<()> {
    let bad = || err("NATIVE_OEX_WORKER_UNCERTAIN");
    if bytes.is_empty() || bytes.len() > MAX_ACK || !valid_expected(expected) {
        return Err(bad());
    }
    let ack: Ack = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if ack.version != 1 || ack.kind != "prepared" || ack.status != "complete"
        || ack.attempt_id != expected.attempt_id || ack.runtime_generation != expected.runtime_generation
        || ack.reservation_id != expected.reservation_id || ack.reservation_generation != expected.reservation_generation
        || ack.receipt_sha256 != expected.receipt_sha256 || ack.manifest_sha256 != expected.manifest_sha256
        || ack.output_device != expected.output_device.to_string() || ack.output_inode != expected.output_inode.to_string()
        || ack.output_bytes != expected.actual_output_bytes || ack.entries.len() > MAX_ENTRIES {
        return Err(bad());
    }
    let mut paths = HashSet::new(); let mut entries = ack.entries;
    for entry in &entries {
        // Identifiers only. Full Spec path/inventory validation remains with
        // Platform's independently checked manifest, not this structural gate.
        if entry.path.is_empty() || entry.path.len() > 240 || !entry.path.is_ascii()
            || entry.path.bytes().any(|v| v.is_ascii_control())
            || !paths.insert(entry.path.to_ascii_lowercase()) || entry.bytes == 0 || entry.bytes > MAX_ENTRY
            || !hash_valid(&entry.sha256) { return Err(bad()); }
    }
    entries.sort_unstable_by_key(|v| v.offset); let mut end = 0u64;
    for entry in entries {
        if entry.offset != end { return Err(bad()); }
        end = end.checked_add(entry.bytes).ok_or_else(bad)?;
        if end > expected.capacity { return Err(bad()); }
    }
    if end != expected.actual_output_bytes { return Err(bad()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_changed_identity_duplicate_names_overlap_and_unknown_fields() {
        let attempt=Uuid::new_v4().to_string(); let runtime=Uuid::new_v4().to_string();
        let reservation=Uuid::new_v4().to_string(); let generation=Uuid::new_v4().to_string();
        let receipt="a".repeat(64); let manifest="b".repeat(64); let hash="c".repeat(64);
        let expected=Expected {attempt_id:&attempt,runtime_generation:&runtime,reservation_id:&reservation,reservation_generation:&generation,receipt_sha256:&receipt,manifest_sha256:&manifest,output_device:1,output_inode:2,actual_output_bytes:4,capacity:4};
        let valid=serde_json::json!({"version":1,"kind":"prepared","status":"complete","attemptId":attempt,"runtimeGeneration":runtime,"reservationId":reservation,"reservationGeneration":generation,"receiptSha256":receipt,"manifestSha256":manifest,"outputDevice":"1","outputInode":"2","outputBytes":4,"entries":[{"path":"assets/a","offset":0,"bytes":2,"sha256":hash},{"path":"assets/b","offset":2,"bytes":2,"sha256":hash}]});
        let encoded=|value:&Value|serde_json::to_vec(value).unwrap();
        assert!(check(&encoded(&valid),&expected).is_ok());
        for changed in ["identity","manifest","name","offset","unknown","device"] {
            let mut item=valid.clone(); match changed {
                "identity" => item["attemptId"]=Value::String(Uuid::new_v4().to_string()),
                "manifest" => item["manifestSha256"]=Value::String("d".repeat(64)),
                "name" => item["entries"][1]["path"]=Value::String("ASSETS/A".into()),
                "offset" => item["entries"][1]["offset"]=Value::from(1),
                "unknown" => item["unexpected"]=Value::Bool(true),
                _ => item["outputDevice"]=Value::String("01".into()),
            }
            assert!(check(&encoded(&item),&expected).is_err(),"{changed}");
        }
    }
}
