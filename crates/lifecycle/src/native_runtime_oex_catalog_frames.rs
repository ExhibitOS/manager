// SPDX-License-Identifier: Apache-2.0
//! The peer's frame is a claim, never evidence of a live owner or catalog.
use super::*;
use crate::native_payload::{
    NORMAL_OEX_028_SCHEMA_SHA256, NORMAL_OEX_029_FLOOR_SCHEMA_SHA256,
    NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256,
};

const SCHEMA_026: &str = "9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112";
const SCHEMA_027: &str = "09bd82b3195e2f54545f6af45173bbab569effac95553a50f573745dab96f2c7";
const SCHEMA_031: &str = "2425f4d24859bf1627d1be9bb3d4b0ecc7829d074437a4aebd9e52d57bf45eb5";
const SCHEMA_032: &str = "bc0017937498f37cfc98acf2851cd70752c76be353208e466aa6a2a5b1808efc";
pub(super) const WORKSPACE_V5_LEDGER: [(&str, &str); 7] = [
    ("026_oex_file_profile.sql", SCHEMA_026),
    ("027_oex_exhibition_provenance.sql", SCHEMA_027),
    ("028_oex_material_catalogue.sql", NORMAL_OEX_028_SCHEMA_SHA256),
    ("029_oex_quiet_oak_floor.sql", NORMAL_OEX_029_FLOOR_SCHEMA_SHA256),
    ("030_oex_concrete_surface.sql", NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256),
    ("031_workspace_material.sql", SCHEMA_031),
    ("032_oex_workspace_material.sql", SCHEMA_032),
];

const SCHEMA_033: &str = "f69d3dc12d7b89f1a342daef1d7bca08787667efacdfca3d214d3101b051d1dd";
const SCHEMA_034: &str = "ebf2300ef875b635617d82369f4c7cad4cbe6d8fa43677bdbd2e5e21dd74da21";
const WORKSPACE_IMPORT_LEDGER: [(&str, &str); 2] = [
    ("033_workspace_material_import_provenance.sql", SCHEMA_033),
    ("034_workspace_material_terminal_proof.sql", SCHEMA_034),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Protocol {
    V2,
    WorkspaceV5,
    WorkspaceV6,
}
impl Protocol {
    pub(super) fn ledger(self) -> impl Iterator<Item = (&'static str, &'static str)> {
        let base = match self {
            Self::V2 => &WORKSPACE_V5_LEDGER[..2],
            _ => &WORKSPACE_V5_LEDGER[..],
        };
        let import = if self == Self::WorkspaceV6 { &WORKSPACE_IMPORT_LEDGER[..] } else { &[] };
        base.iter().chain(import).copied()
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    schema028_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema029_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema030_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema031_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema032_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema033_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema034_sha256: Option<String>,
}
/// Captured independently from the owned session, PostgreSQL and pinned store.
pub(super) struct Observation<'a> {
    pub workspace: &'a str, pub tenant: &'a str, pub principal: &'a str, pub generation: &'a str,
    pub manifest: &'a str, pub system: &'a str, pub oid: &'a str, pub device: u64, pub inode: u64,
}
pub(super) fn acknowledge(bytes: &[u8], observed: &Observation<'_>, protocol: Protocol) -> Result<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > 8192 { return Err(bad()); }
    let mut f: CatalogFrame = serde_json::from_slice(bytes).map_err(|_| bad())?;
    let (version, ready, ack, owner_mode) = match protocol {
        Protocol::V2 => (2, "catalog-ready-v2", "catalog-ack-v2", "normal-oex-027-v2"),
        Protocol::WorkspaceV5 => (5, "catalog-ready-v5", "catalog-ack-v5", "normal-oex-workspace-v5"),
        Protocol::WorkspaceV6 => (6, "catalog-ready-v6", "catalog-ack-v6", "normal-oex-workspace-import-v6"),
    };
    let mut ledger = protocol.ledger().map(|(_, digest)| digest).skip(2);
    let extras = [f.schema028_sha256.as_deref(), f.schema029_sha256.as_deref(), f.schema030_sha256.as_deref(),
        f.schema031_sha256.as_deref(), f.schema032_sha256.as_deref(), f.schema033_sha256.as_deref(), f.schema034_sha256.as_deref()];
    if serde_json::to_vec(&f).map_err(|_| bad())? != bytes || f.version != version || f.kind != ready
        || f.protocol != owner_mode || !uuid(&f.correlation_id)
        || [&f.workspace_id, &f.tenant_id, &f.principal_id, &f.runtime_generation].iter().any(|v| !uuid(v))
        || f.workspace_id != observed.workspace || f.tenant_id != observed.tenant || f.principal_id != observed.principal
        || f.runtime_generation != observed.generation || !hash_valid(&f.payload_manifest_sha256) || f.payload_manifest_sha256 != observed.manifest
        || [&f.database_system_id, &f.database_oid, &f.store_device, &f.store_inode].iter().any(|v| !decimal(v))
        || f.database_system_id != observed.system || f.database_oid != observed.oid
        || f.store_device != observed.device.to_string() || f.store_inode != observed.inode.to_string()
        || f.schema026_sha256 != SCHEMA_026 || f.schema027_sha256 != SCHEMA_027
        || extras.into_iter().any(|value| value != ledger.next()) { return Err(bad()); }
    f.kind = ack.into();
    serde_json::to_vec(&f).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> CatalogFrame {
        CatalogFrame {version:2,kind:"catalog-ready-v2".into(),protocol:"normal-oex-027-v2".into(),correlation_id:Uuid::new_v4().to_string(),
            workspace_id:Uuid::new_v4().to_string(),tenant_id:Uuid::new_v4().to_string(),principal_id:Uuid::new_v4().to_string(),
            runtime_generation:Uuid::new_v4().to_string(),payload_manifest_sha256:"a".repeat(64),
            database_system_id:"123".into(),database_oid:"456".into(),store_device:"7".into(),store_inode:"8".into(),
            schema026_sha256:SCHEMA_026.into(),schema027_sha256:SCHEMA_027.into(),
            schema028_sha256:None,schema029_sha256:None,schema030_sha256:None,schema031_sha256:None,schema032_sha256:None,schema033_sha256:None,schema034_sha256:None}
    }
    fn observation(frame: &CatalogFrame) -> Observation<'_> {
        Observation {workspace:&frame.workspace_id,tenant:&frame.tenant_id,principal:&frame.principal_id,
            generation:&frame.runtime_generation,manifest:&frame.payload_manifest_sha256,system:"123",oid:"456",device:7,inode:8}
    }
    #[test]
    fn catalog_ack_requires_canonical_scoped_frame_and_observed_identity() {
        let frame=fixture();
        let observed=observation(&frame);
        let check=|bytes: &[u8]| acknowledge(bytes,&observed,Protocol::V2);
        let bytes=serde_json::to_vec(&frame).unwrap();let ack=check(&bytes).unwrap();let mut expected=frame.clone();expected.kind="catalog-ack-v2".into();
        assert_eq!(ack,serde_json::to_vec(&expected).unwrap());
        let mut mismatch=frame.clone();mismatch.database_oid="457".into();assert!(check(&serde_json::to_vec(&mismatch).unwrap()).is_err());
        mismatch=frame.clone();mismatch.store_inode="08".into();assert!(check(&serde_json::to_vec(&mismatch).unwrap()).is_err());
        mismatch=frame.clone();mismatch.runtime_generation=Uuid::new_v4().to_string();assert!(check(&serde_json::to_vec(&mismatch).unwrap()).is_err());
        mismatch=frame.clone();mismatch.schema027_sha256="b".repeat(64);assert!(check(&serde_json::to_vec(&mismatch).unwrap()).is_err());
        let mut spaced=bytes.clone();spaced.push(b' ');assert!(check(&spaced).is_err());
        assert!(check(&[]).is_err());assert!(check(&vec![0;8193]).is_err());
    }
    #[test]
    fn workspace_v5_ack_requires_exact_schema_sequence_and_no_protocol_fallback() {
        let legacy = fixture();
        let observed = observation(&legacy);
        let check = |text: &str| acknowledge(text.as_bytes(), &observed, Protocol::WorkspaceV5);
        let v2 = serde_json::to_string(&legacy).unwrap();
        assert!(check(&v2).is_err());
        let v5 = v2.strip_suffix('}').unwrap().replacen(
            r#"{"version":2,"kind":"catalog-ready-v2","protocol":"normal-oex-027-v2","#,
            r#"{"version":5,"kind":"catalog-ready-v5","protocol":"normal-oex-workspace-v5","#, 1,
        );
        let wire = format!(
            r#"{v5},"schema028Sha256":"{}","schema029Sha256":"{}","schema030Sha256":"{}","schema031Sha256":"{SCHEMA_031}","schema032Sha256":"{SCHEMA_032}"}}"#,
            NORMAL_OEX_028_SCHEMA_SHA256, NORMAL_OEX_029_FLOOR_SCHEMA_SHA256, NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256,
        );
        assert_eq!(check(&wire).unwrap(), wire.replacen("catalog-ready-v5", "catalog-ack-v5", 1).into_bytes());
        assert!(acknowledge(wire.as_bytes(), &observed, Protocol::V2).is_err());
        for (_, hash) in WORKSPACE_V5_LEDGER {
            assert!(check(&wire.replacen(hash, &"0".repeat(64), 1)).is_err());
        }
        let without_032 = wire.replace(&format!(r#","schema032Sha256":"{SCHEMA_032}""#), "");
        assert!(check(&without_032).is_err());
        let extra = format!(r#"{},"extra":true}}"#, wire.strip_suffix('}').unwrap());
        assert!(check(&extra).is_err());
        assert!(check(&wire.replace("catalog-ready-v5", "catalog-ready-v2")).is_err());
    }
    #[test]
    fn workspace_v6_ack_requires_all_nine_ledger_rows_and_live_identity() {
        let mut frame = fixture();
        frame.version = 6;
        frame.kind = "catalog-ready-v6".into();
        frame.protocol = "normal-oex-workspace-import-v6".into();
        frame.schema028_sha256 = Some(NORMAL_OEX_028_SCHEMA_SHA256.into());
        frame.schema029_sha256 = Some(NORMAL_OEX_029_FLOOR_SCHEMA_SHA256.into());
        frame.schema030_sha256 = Some(NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256.into());
        frame.schema031_sha256 = Some(SCHEMA_031.into());
        frame.schema032_sha256 = Some(SCHEMA_032.into());
        frame.schema033_sha256 = Some(SCHEMA_033.into());
        frame.schema034_sha256 = Some(SCHEMA_034.into());
        let observed = observation(&frame);
        let wire = serde_json::to_string(&frame).unwrap();
        let check = |wire: &str| acknowledge(wire.as_bytes(), &observed, Protocol::WorkspaceV6);
        assert_eq!(check(&wire).unwrap(), wire.replacen("catalog-ready-v6", "catalog-ack-v6", 1).into_bytes());
        assert_eq!(Protocol::WorkspaceV6.ledger().count(), 9);
        for (_, digest) in Protocol::WorkspaceV6.ledger() {
            assert!(check(&wire.replacen(digest, &"0".repeat(64), 1)).is_err());
        }
        let incomplete = wire.replace(&format!(r#","schema034Sha256":"{SCHEMA_034}""#), "");
        assert!(check(&incomplete).is_err());
        assert!(check(&wire.replace(r#""storeInode":"8""#, r#""storeInode":"9""#)).is_err());
        assert!(acknowledge(wire.as_bytes(), &observed, Protocol::WorkspaceV5).is_err());
    }

}
