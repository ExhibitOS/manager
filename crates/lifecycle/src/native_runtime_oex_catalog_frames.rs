// SPDX-License-Identifier: Apache-2.0
//! The peer's frame is a claim, never evidence of a live owner or catalog.
use super::*;

pub(super) const SCHEMA_026: &str = "9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112";
pub(super) const SCHEMA_027: &str = "09bd82b3195e2f54545f6af45173bbab569effac95553a50f573745dab96f2c7";
fn bad() -> LifecycleError { err("NATIVE_OWNER_CHANNEL_UNAVAILABLE") }
fn uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|v| v.to_string() == value && v.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&v.get_version_num()))
}
fn decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 24 && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogFrame {
    version: u32, kind: String, protocol: String, correlation_id: String,
    workspace_id: String, tenant_id: String, principal_id: String, runtime_generation: String,
    payload_manifest_sha256: String, database_system_id: String, database_oid: String,
    store_device: String, store_inode: String, schema026_sha256: String, schema027_sha256: String,
}
/// Captured independently from the owned session, PostgreSQL and pinned store.
pub(super) struct Observation<'a> {
    pub workspace: &'a str, pub tenant: &'a str, pub principal: &'a str, pub generation: &'a str,
    pub manifest: &'a str, pub system: &'a str, pub oid: &'a str, pub device: u64, pub inode: u64,
}
pub(super) fn acknowledge(bytes: &[u8], observed: &Observation<'_>) -> Result<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > 8192 { return Err(bad()); }
    let mut f: CatalogFrame = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if serde_json::to_vec(&f).map_err(|_| bad())? != bytes || f.version != 2 || f.kind != "catalog-ready-v2"
        || f.protocol != "normal-oex-027-v2" || !uuid(&f.correlation_id)
        || [&f.workspace_id, &f.tenant_id, &f.principal_id, &f.runtime_generation].iter().any(|v| !uuid(v))
        || f.workspace_id != observed.workspace || f.tenant_id != observed.tenant || f.principal_id != observed.principal
        || f.runtime_generation != observed.generation || !hash_valid(&f.payload_manifest_sha256) || f.payload_manifest_sha256 != observed.manifest
        || [&f.database_system_id, &f.database_oid, &f.store_device, &f.store_inode].iter().any(|v| !decimal(v))
        || f.database_system_id != observed.system || f.database_oid != observed.oid
        || f.store_device != observed.device.to_string() || f.store_inode != observed.inode.to_string()
        || f.schema026_sha256 != SCHEMA_026 || f.schema027_sha256 != SCHEMA_027 { return Err(bad()); }
    f.kind = "catalog-ack-v2".into();
    serde_json::to_vec(&f).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_ack_requires_canonical_scoped_frame_and_observed_identity() {
        let w=Uuid::new_v4().to_string(); let t=Uuid::new_v4().to_string(); let p=Uuid::new_v4().to_string();
        let generation=Uuid::new_v4().to_string(); let manifest="a".repeat(64);
        let frame=CatalogFrame {version:2,kind:"catalog-ready-v2".into(),protocol:"normal-oex-027-v2".into(),correlation_id:Uuid::new_v4().to_string(),
            workspace_id:w.clone(),tenant_id:t.clone(),principal_id:p.clone(),runtime_generation:generation.clone(),payload_manifest_sha256:manifest.clone(),
            database_system_id:"123".into(),database_oid:"456".into(),store_device:"7".into(),store_inode:"8".into(),schema026_sha256:SCHEMA_026.into(),schema027_sha256:SCHEMA_027.into()};
        let observed=Observation {workspace:&w,tenant:&t,principal:&p,generation:&generation,manifest:&manifest,system:"123",oid:"456",device:7,inode:8};
        let bytes=serde_json::to_vec(&frame).unwrap();let ack=acknowledge(&bytes,&observed).unwrap();let mut expected=frame.clone();expected.kind="catalog-ack-v2".into();
        assert_eq!(ack,serde_json::to_vec(&expected).unwrap());
        let mut mismatch=frame.clone();mismatch.database_oid="457".into();assert!(acknowledge(&serde_json::to_vec(&mismatch).unwrap(),&observed).is_err());
        mismatch=frame.clone();mismatch.store_inode="08".into();assert!(acknowledge(&serde_json::to_vec(&mismatch).unwrap(),&observed).is_err());
        mismatch=frame.clone();mismatch.runtime_generation=Uuid::new_v4().to_string();assert!(acknowledge(&serde_json::to_vec(&mismatch).unwrap(),&observed).is_err());
        mismatch=frame.clone();mismatch.schema027_sha256="b".repeat(64);assert!(acknowledge(&serde_json::to_vec(&mismatch).unwrap(),&observed).is_err());
        let mut spaced=bytes.clone();spaced.push(b' ');assert!(acknowledge(&spaced,&observed).is_err());
        assert!(acknowledge(&[],&observed).is_err());assert!(acknowledge(&vec![0;8193],&observed).is_err());
    }
}
