// SPDX-License-Identifier: Apache-2.0
//! Closed v1 post-hello frames for a private native furnishing writer.
use super::furnishing_lifetime::WriterIdentity;
use super::*;

pub(super) const LIMIT: usize = 2048;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Lease,
    Ack,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Lease => "furnishing-writer-lease",
            Self::Ack => "furnishing-writer-lease-ack",
        }
    }
}

pub(super) struct Lease {
    workspace: String,
    tenant: String,
    principal: String,
    generation: String,
    pub(super) start: FurnishingStart,
    manifest_sha256: String,
    pub(super) identity: WriterIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame<'a> {
    version: u32,
    kind: &'static str,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    runtime_generation: &'a str,
    mode: &'static str,
    #[serde(rename = "payloadManifestSHA256")]
    payload_manifest_sha256: &'a str,
    root_device: String,
    root_inode: String,
    lock_device: String,
    lock_inode: String,
    mac: String,
}

fn invalid() -> LifecycleError {
    err("NATIVE_FURNISHING_WRITER_LEASE_INVALID")
}

impl Lease {
    pub(super) fn new(
        auth: &LocalAuthority,
        generation: &str,
        start: FurnishingStart,
        manifest_sha256: String,
        identity: WriterIdentity,
    ) -> Self {
        Self {
            workspace: auth.workspace_id.clone(),
            tenant: auth.tenant_id.clone(),
            principal: auth.principal_id.clone(),
            generation: generation.into(),
            start,
            manifest_sha256,
            identity,
        }
    }

    fn message(&self, kind: Kind) -> Result<Vec<u8>> {
        let mut bytes = format!("ExhibitOS/workspace-furnishing/{}/v1", kind.name()).into_bytes();
        bytes.push(0);
        for value in [
            &self.workspace,
            &self.tenant,
            &self.principal,
            &self.generation,
        ] {
            let id = Uuid::parse_str(value).map_err(|_| invalid())?;
            if id.to_string() != *value
                || id.get_variant() != uuid::Variant::RFC4122
                || !(1..=8).contains(&id.get_version_num())
            {
                return Err(invalid());
            }
            bytes.extend_from_slice(id.as_bytes());
        }
        bytes.push(match self.start {
            FurnishingStart::Initialize => 1,
            FurnishingStart::Reopen => 2,
            FurnishingStart::SelectedImport => 3,
        });
        if !hash_valid(&self.manifest_sha256)
            || self.identity.root_inode == 0
            || self.identity.lock_inode == 0
        {
            return Err(invalid());
        }
        for pair in self.manifest_sha256.as_bytes().chunks_exact(2) {
            let nibble = |byte| {
                if byte <= b'9' {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            bytes.push(nibble(pair[0]) << 4 | nibble(pair[1]));
        }
        for number in [
            self.identity.root_device,
            self.identity.root_inode,
            self.identity.lock_device,
            self.identity.lock_inode,
        ] {
            bytes.extend_from_slice(&number.to_be_bytes());
        }
        Ok(bytes)
    }

    pub(super) fn signed(&self, key: &[u8; 32], kind: Kind) -> Result<Vec<u8>> {
        let mac = super::owner_binding_encoding::hmac_sha256(key, &self.message(kind)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let frame = Frame {
            version: 1,
            kind: kind.name(),
            workspace_id: &self.workspace,
            tenant_id: &self.tenant,
            principal_id: &self.principal,
            runtime_generation: &self.generation,
            mode: self.start.owner_mode(),
            payload_manifest_sha256: &self.manifest_sha256,
            root_device: self.identity.root_device.to_string(),
            root_inode: self.identity.root_inode.to_string(),
            lock_device: self.identity.lock_device.to_string(),
            lock_inode: self.identity.lock_inode.to_string(),
            mac,
        };
        let bytes = serde_json::to_vec(&frame).map_err(|_| invalid())?;
        if bytes.len() > LIMIT {
            return Err(invalid());
        }
        Ok(bytes)
    }

    pub(super) fn check_ack(&self, key: &[u8; 32], bytes: &[u8]) -> Result<()> {
        let expected = self.signed(key, Kind::Ack)?;
        // Compare against the canonical bytes, so alternate types, duplicate
        // keys and alternate orders cannot carry the same authenticated data.
        if bytes.len() != expected.len() {
            return Err(invalid());
        }
        let difference = bytes
            .iter()
            .zip(expected)
            .fold(0, |acc, (actual, expected)| acc | (actual ^ expected));
        if std::hint::black_box(difference) != 0 {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> Lease {
        Lease {
            workspace: "10000000-0000-4000-8000-000000000001".into(),
            tenant: "10000000-0000-4000-8000-000000000002".into(),
            principal: "10000000-0000-4000-8000-000000000003".into(),
            generation: "10000000-0000-4000-8000-000000000004".into(),
            start: FurnishingStart::Initialize,
            manifest_sha256: (32..64).map(|byte| format!("{byte:02x}")).collect(),
            identity: WriterIdentity {
                root_device: 7,
                root_inode: 9_007_199_254_740_993,
                lock_device: 7,
                lock_inode: 9_007_199_254_740_997,
            },
        }
    }

    #[test]
    fn furnishing_lease_matches_shared_canonical_golden_vectors() {
        let lease = golden();
        let key = std::array::from_fn(|index| index as u8);
        for (kind, message_bytes, frame_hash, mac) in [
            (
                Kind::Lease,
                187,
                "feb869d16152612226418a37d7975335a0d5336516e5632704a0988b898a36b6",
                "8e0f0647ab4c55a17f9603ee76cf37855929b8eff27a6220aba7c46e8cfdb90f",
            ),
            (
                Kind::Ack,
                191,
                "cb2a9b49ff65a0f1c840e597ee9b4ac4a77d67e95db09b1510ad0752de8392a0",
                "ada68e6943e3e1481722829ec70fbbf80c91d185db3077fb871a98d3f48fd25d",
            ),
        ] {
            assert_eq!(lease.message(kind).unwrap().len(), message_bytes);
            let body = lease.signed(&key, kind).unwrap();
            assert_eq!(digest(&body), frame_hash);
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["mac"], mac);
        }
        assert!(
            lease
                .check_ack(&key, &lease.signed(&key, Kind::Ack).unwrap())
                .is_ok()
        );
    }

    #[test]
    fn furnishing_selected_import_lease_matches_mode_three_vectors() {
        let mut lease = golden();
        let key = std::array::from_fn(|index| index as u8);
        let ordinary = lease.signed(&key, Kind::Ack).unwrap();
        lease.start = FurnishingStart::SelectedImport;
        for (kind, expected_mac) in [
            (Kind::Lease, "813f55583aa1cd1b00dc638b206228e6fac1b2bddd51600a94a0a8cdb0cc07e2"),
            (Kind::Ack, "4e65a50d15bb2e610bce5bf35f04c2172374a32b160b2fc083df439bc544f7af"),
        ] {
            let body = lease.signed(&key, kind).unwrap();
            let frame: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(frame["mode"], "workspace-furnishing-038-selected-import-v7");
            assert_eq!(frame["mac"], expected_mac);
        }
        assert!(lease.check_ack(&key, &ordinary).is_err());
    }

    #[test]
    fn furnishing_ack_refuses_signed_different_scope_and_noncanonical_bytes() {
        let lease = golden();
        let key = std::array::from_fn(|index| index as u8);
        const OTHER_ID: &str = "10000000-0000-4000-8000-000000000005";
        let changes: &[fn(&mut Lease)] = &[
            |lease| lease.workspace = OTHER_ID.into(),
            |lease| lease.tenant = OTHER_ID.into(),
            |lease| lease.principal = OTHER_ID.into(),
            |lease| lease.generation = OTHER_ID.into(),
            |lease| lease.start = FurnishingStart::Reopen,
            |lease| lease.manifest_sha256 = "a".repeat(64),
            |lease| lease.identity.root_device += 1,
            |lease| lease.identity.root_inode += 1,
            |lease| lease.identity.lock_device += 1,
            |lease| lease.identity.lock_inode += 1,
        ];
        for change in changes {
            let mut wrong = golden();
            change(&mut wrong);
            assert!(
                lease
                    .check_ack(&key, &wrong.signed(&key, Kind::Ack).unwrap())
                    .is_err()
            );
        }
        let correct = String::from_utf8(lease.signed(&key, Kind::Ack).unwrap()).unwrap();
        for invalid in [
            format!("{correct} "),
            correct.replace("\"rootDevice\":\"7\"", "\"rootDevice\":\"07\""),
            correct.replace("\"version\":1", "\"version\":1,\"version\":1"),
            correct.replace(
                "\"rootDevice\":\"7\",\"rootInode\":\"9007199254740993\"",
                "\"rootInode\":\"9007199254740993\",\"rootDevice\":\"7\"",
            ),
            correct.replace(
                "ada68e6943e3e1481722829ec70fbbf80c91d185db3077fb871a98d3f48fd25d",
                "9b45a63cbce298cd6064420681a264f95b97de90f5d5c54a467ef2d1c1489539",
            ),
        ] {
            assert_ne!(invalid, correct);
            assert!(lease.check_ack(&key, invalid.as_bytes()).is_err());
        }
        assert!(
            lease
                .check_ack(&key, &lease.signed(&key, Kind::Lease).unwrap())
                .is_err()
        );
        assert!(lease.check_ack(&[9; 32], correct.as_bytes()).is_err());
        let mut invalid = golden();
        invalid.identity.lock_inode = 0;
        assert!(invalid.signed(&key, Kind::Lease).is_err());
    }
}
