// SPDX-License-Identifier: Apache-2.0
//! Closed, same-payload ordinary furnishing lifecycle. One binding lives in
//! the existing native current-run record, separate from its diagnostic ID.
use super::furnishing_lifetime::WriterIdentity;
use super::*;
use crate::installations::native_restore::RestoreProof;

pub(super) const LIMIT: usize = 2048;
pub(super) const ERROR: &str = "NATIVE_FURNISHING_ORDINARY_UNQUALIFIED";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Admission {
    FreshEmpty,
    CleanReopen,
    VerifiedRestore,
    #[serde(rename = "catalog-upgrade-039-to-040")]
    CatalogUpgrade039To040,
}

impl Admission {
    fn tag(self) -> u8 {
        match self {
            Self::FreshEmpty => 1,
            Self::CleanReopen => 2,
            Self::VerifiedRestore => 3,
            Self::CatalogUpgrade039To040 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Phase {
    Active,
    Clean,
    ActivatedRestore,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Owner {
    pub(super) workspace_id: String,
    pub(super) tenant_id: String,
    pub(super) principal_id: String,
    pub(super) owner_uid: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Restore {
    pub(super) id: String,
    pub(super) backup_id: String,
    pub(super) archive_manifest_sha256: String,
    pub(super) target_system_identifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) provenance: Option<RestoreProof>,
}

/// Canonical Active bytes are serde's compact JSON for this closed structure
/// in declaration order, with ASCII values, null optionals and no newline.
/// These exact bytes (not the surrounding Current record) are hashed on wire.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Binding {
    pub(super) version: u32,
    pub(super) phase: Phase,
    pub(super) generation: String,
    pub(super) previous_generation: Option<String>,
    pub(super) admission_kind: Admission,
    pub(super) owner: Owner,
    pub(super) source_commit: String,
    pub(super) producer_sha256: String,
    pub(super) payload_manifest_sha256: String,
    pub(super) catalog: u32,
    pub(super) physical: WriterIdentity,
    pub(super) restore: Option<Restore>,
}

impl Binding {
    pub(super) fn check(&self) -> Result<()> {
        if self.version != 1
            || !matches!(self.catalog, 37..=41)
            || (self.catalog == 41
                && (self.restore.is_some()
                    || !matches!(self.admission_kind, Admission::FreshEmpty | Admission::CleanReopen)))
            || !installations::uuid(&self.generation)
            || self
                .previous_generation
                .as_ref()
                .is_some_and(|prior| prior == &self.generation || !installations::uuid(prior))
            || ![
                &self.owner.workspace_id,
                &self.owner.tenant_id,
                &self.owner.principal_id,
            ]
            .iter()
            .all(|id| installations::uuid(id))
            || self.owner.owner_uid == 0
            || self.source_commit.len() != 40
            || !self
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !hash_valid(&self.producer_sha256)
            || !hash_valid(&self.payload_manifest_sha256)
            || self.physical.root_inode == 0
            || self.physical.lock_inode == 0
        {
            return Err(err(ERROR));
        }
        let shape = match (self.phase, self.admission_kind) {
            (Phase::Active | Phase::Clean, Admission::FreshEmpty) => {
                self.previous_generation.is_none() && self.restore.is_none()
            }
            (Phase::Active | Phase::Clean, Admission::CleanReopen) => {
                self.previous_generation.is_some()
            }
            (Phase::Active | Phase::Clean, Admission::VerifiedRestore) => {
                self.previous_generation.is_some() && self.restore.is_some()
            }
            (Phase::Active | Phase::Clean, Admission::CatalogUpgrade039To040) => {
                self.catalog == 40 && self.previous_generation.is_some() && self.restore.is_none()
            }
            (Phase::ActivatedRestore, Admission::VerifiedRestore) => {
                self.previous_generation.is_none() && self.restore.is_some()
            }
            _ => false,
        };
        if !shape
            || self.restore.as_ref().is_some_and(|proof| {
                !installations::uuid(&proof.id)
                    || !installations::uuid(&proof.backup_id)
                    || !hash_valid(&proof.archive_manifest_sha256)
                    || !proof
                        .target_system_identifier
                        .parse::<u64>()
                        .is_ok_and(|value| {
                            value != 0 && value.to_string() == proof.target_system_identifier
                        })
                    || proof.provenance.as_ref().is_some_and(|provenance| self.catalog != 40
                        || !provenance.matches(&proof.id, &proof.backup_id, &proof.archive_manifest_sha256, &proof.target_system_identifier))
            })
        {
            return Err(err(ERROR));
        }
        Ok(())
    }

    pub(super) fn same_installation(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.source_commit == other.source_commit
            && self.producer_sha256 == other.producer_sha256
            && self.payload_manifest_sha256 == other.payload_manifest_sha256
            && self.catalog == other.catalog
            && self.physical == other.physical
    }

    pub(super) fn consume(&mut self, prior: &Self) -> Result<()> {
        prior.check()?;
        if self.generation == prior.generation || !self.same_installation(prior) {
            return Err(err(ERROR));
        }
        match prior.phase {
            Phase::Active => return Err(err(ERROR)),
            Phase::Clean => self.admission_kind = Admission::CleanReopen,
            Phase::ActivatedRestore => self.admission_kind = Admission::VerifiedRestore,
        }
        self.restore = prior.restore.clone();
        self.previous_generation = Some(prior.generation.clone());
        self.check()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn consume_catalog40_transition(&mut self, prior: &Self, transition: Catalog40Transition) -> Result<()> {
        prior.check()?;
        self.check()?;
        if prior.phase != Phase::Clean || self.phase != Phase::Active || prior.restore.is_some()
            || self.generation == prior.generation || self.owner != prior.owner
            || self.physical != prior.physical
            || !prior.matches_catalog_tuple(transition.source_catalog().version(), transition.source())
            || !self.matches_catalog_tuple(40, transition.target())
        {
            return Err(err(ERROR));
        }
        self.admission_kind = Self::transition_admission(transition);
        self.previous_generation = Some(prior.generation.clone());
        self.check()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn transition_admission(transition: Catalog40Transition) -> Admission {
        if transition.upgrades_catalog() { Admission::CatalogUpgrade039To040 }
        else { Admission::CleanReopen }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn matches_catalog_tuple(&self, catalog: u32, tuple: (&str, &str, &str)) -> bool {
        self.catalog == catalog && tuple == (
            self.source_commit.as_str(),
            self.producer_sha256.as_str(),
            self.payload_manifest_sha256.as_str(),
        )
    }

    pub(super) fn active_sha256(&self) -> Result<String> {
        if self.phase != Phase::Active {
            return Err(err(ERROR));
        }
        self.sha256()
    }

    pub(super) fn sha256(&self) -> Result<String> {
        self.check()?;
        Ok(digest(&serde_json::to_vec(self).map_err(|_| err(ERROR))?))
    }
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Admission,
    AdmissionAck,
    Stop,
    Drained,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Admission => "furnishing-ordinary-admission",
            Self::AdmissionAck => "furnishing-ordinary-admission-ack",
            Self::Stop => "furnishing-ordinary-stop",
            Self::Drained => "furnishing-ordinary-drained",
        }
    }
}

pub(super) struct Exchange {
    generation: String,
    lease_transcript_sha256: String,
    admission_kind: Admission,
    active_binding_sha256: String,
    restore_proof: Option<RestoreProof>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame<'a> {
    version: u32,
    kind: &'static str,
    runtime_generation: &'a str,
    #[serde(rename = "leaseTranscriptSHA256")]
    lease_transcript_sha256: &'a str,
    admission_kind: Admission,
    #[serde(rename = "activeBindingSHA256")]
    active_binding_sha256: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    restore_proof: Option<&'a RestoreProof>,
    mac: String,
}

impl Exchange {
    pub(super) fn new(binding: &Binding, lease_body: &[u8]) -> Result<Self> {
        Ok(Self {
            generation: binding.generation.clone(),
            lease_transcript_sha256: digest(lease_body),
            admission_kind: binding.admission_kind,
            active_binding_sha256: binding.active_sha256()?,
            restore_proof: binding.restore.as_ref().and_then(|restore| restore.provenance.clone()),
        })
    }

    fn version(&self) -> u32 {
        if self.restore_proof.is_some() { 2 } else { 1 }
    }

    fn message(&self, kind: Kind) -> Result<Vec<u8>> {
        let id = Uuid::parse_str(&self.generation).map_err(|_| err(ERROR))?;
        if id.to_string() != self.generation
            || !hash_valid(&self.lease_transcript_sha256)
            || !hash_valid(&self.active_binding_sha256)
        {
            return Err(err(ERROR));
        }
        let mut bytes = format!("ExhibitOS/workspace-furnishing/{}/v{}\0", kind.name(), self.version()).into_bytes();
        bytes.extend_from_slice(id.as_bytes());
        let decode = |digest: &str| {
            (0..digest.len())
                .step_by(2)
                .map(|index| {
                    u8::from_str_radix(&digest[index..index + 2], 16).map_err(|_| err(ERROR))
                })
                .collect::<Result<Vec<_>>>()
        };
        bytes.extend(decode(&self.lease_transcript_sha256)?);
        bytes.push(self.admission_kind.tag());
        bytes.extend(decode(&self.active_binding_sha256)?);
        if let Some(proof) = &self.restore_proof {
            let canonical = serde_json::to_vec(proof).map_err(|_| err(ERROR))?;
            bytes.extend(decode(&digest(&canonical))?);
        }
        Ok(bytes)
    }

    pub(super) fn signed(&self, key: &[u8; 32], kind: Kind) -> Result<Vec<u8>> {
        let mac = super::owner_binding_encoding::hmac_sha256(key, &self.message(kind)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        serde_json::to_vec(&Frame {
            version: self.version(),
            kind: kind.name(),
            runtime_generation: &self.generation,
            lease_transcript_sha256: &self.lease_transcript_sha256,
            admission_kind: self.admission_kind,
            active_binding_sha256: &self.active_binding_sha256,
            restore_proof: self.restore_proof.as_ref(),
            mac,
        })
        .map_err(|_| err(ERROR))
    }

    pub(super) fn check_frame(&self, key: &[u8; 32], kind: Kind, bytes: &[u8]) -> Result<()> {
        let expected = self.signed(key, kind)?;
        if bytes.len() != expected.len() {
            return Err(err(ERROR));
        }
        let diff = bytes
            .iter()
            .zip(expected)
            .fold(0, |acc, (a, b)| acc | (a ^ b));
        if std::hint::black_box(diff) != 0 {
            return Err(err(ERROR));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GENERATION: &str = "10000000-0000-4000-8000-000000000001";
    const NEXT: &str = "10000000-0000-4000-8000-000000000005";
    const LATER: &str = "10000000-0000-4000-8000-000000000006";

    fn binding(generation: &str) -> Binding {
        Binding {
            version: 1,
            phase: Phase::Active,
            generation: generation.into(),
            previous_generation: None,
            admission_kind: Admission::FreshEmpty,
            owner: Owner {
                workspace_id: "10000000-0000-4000-8000-000000000002".into(),
                tenant_id: "10000000-0000-4000-8000-000000000003".into(),
                principal_id: "10000000-0000-4000-8000-000000000004".into(),
                owner_uid: 501,
            },
            source_commit: "1".repeat(40),
            producer_sha256: "2".repeat(64),
            payload_manifest_sha256: "3".repeat(64),
            catalog: 37,
            physical: WriterIdentity {
                root_device: 7,
                root_inode: 11,
                lock_device: 7,
                lock_inode: 13,
            },
            restore: None,
        }
    }

    #[test]
    fn ordinary_restore_v2_authenticates_proof_in_all_four_frames() {
        let proof: RestoreProof = serde_json::from_value(serde_json::json!({
            "restoreId":"10000000-0000-4000-8000-000000000007",
            "backupId":"10000000-0000-4000-8000-000000000008",
            "authenticatedManifestSha256":"c".repeat(64),
            "sourceDatabaseSystemId":"110", "sourceDatabaseOid":"16000",
            "targetDatabaseSystemId":"220", "targetDatabaseOid":"16002",
            "provenanceRowCount":1, "provenanceTableSha256":"d".repeat(64)
        })).unwrap();
        let exchange = Exchange {
            generation: GENERATION.into(), lease_transcript_sha256: "a".repeat(64),
            admission_kind: Admission::VerifiedRestore, active_binding_sha256: "b".repeat(64),
            restore_proof: Some(proof.clone()),
        };
        for (kind, mac) in [
            (Kind::Admission, "2a5aec78c967db201a7574b194e89f8f66db707d31709dd8b8ebbadbf4a9178c"),
            (Kind::AdmissionAck, "f243163bf2bfb722608c71f467bbf6a65414f3866af5fbc2706a456a7621237d"),
            (Kind::Stop, "8b8b053f0a6726e4b6b09e1c82eb732d2af1a273f4c3a37487e176a7637d0584"),
            (Kind::Drained, "b5fa66a62a03edfd394dd9433d0b8b73d71055d12acfcac4988dd8963cd26bdf"),
        ] {
            let key = [7; 32];
            let bytes = exchange.signed(&key, kind).unwrap();
            assert!(bytes.len() < LIMIT);
            let frame: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(frame["version"], 2);
            assert_eq!(frame["mac"], mac);
            exchange.check_frame(&key, kind, &bytes).unwrap();
            let mut altered = frame;
            altered["restoreProof"]["targetDatabaseOid"] = "16003".into();
            assert!(exchange.check_frame(&key, kind, &serde_json::to_vec(&altered).unwrap()).is_err());
        }

        let mut restored = binding(GENERATION);
        restored.catalog = 40;
        restored.phase = Phase::ActivatedRestore;
        restored.admission_kind = Admission::VerifiedRestore;
        restored.restore = Some(Restore {
            id: "10000000-0000-4000-8000-000000000007".into(),
            backup_id: "10000000-0000-4000-8000-000000000008".into(),
            archive_manifest_sha256: "c".repeat(64), target_system_identifier: "220".into(),
            provenance: Some(proof),
        });
        let mut active = binding(NEXT);
        active.catalog = 40;
        active.consume(&restored).unwrap();
        assert_eq!(active.admission_kind, Admission::VerifiedRestore);
        assert_eq!(Exchange::new(&active, b"lease").unwrap().restore_proof, exchange.restore_proof);
        active.phase = Phase::Clean;
        let mut reopened = binding(LATER);
        reopened.catalog = 40;
        reopened.consume(&active).unwrap();
        assert_eq!(reopened.admission_kind, Admission::CleanReopen);
        assert_eq!(Exchange::new(&reopened, b"new lease").unwrap().restore_proof, exchange.restore_proof);
    }

    #[test]
    fn ordinary_furnishing_cross_language_vector() {
        let active = binding(GENERATION);
        assert_eq!(
            active.active_sha256().unwrap(),
            "4df29ffd09687619e95c9dfcbc7652845a3fc389e9189bc8ecd1b1ebc69e4ff2"
        );
        let owner = super::super::LocalAuthority {
            format: 1,
            workspace_id: active.owner.workspace_id.clone(),
            tenant_id: active.owner.tenant_id.clone(),
            principal_id: active.owner.principal_id.clone(),
            owner_uid: active.owner.owner_uid,
            session_token: "synthetic-session".into(),
            csrf_token: "synthetic-csrf".into(),
        };
        let lease = super::super::furnishing_lease::Lease::new(
            &owner,
            GENERATION,
            super::super::FurnishingStart::Initialize,
            active.payload_manifest_sha256.clone(),
            active.physical,
        );
        let key = [7; 32];
        let body = lease
            .signed(&key, super::super::furnishing_lease::Kind::Lease)
            .unwrap();
        assert_eq!(
            digest(&body),
            "1dd681e12b23b2c06eabb39fc9b1057bbe342cc3003db15e9fd36307ec344e41"
        );
        let exchange = Exchange::new(&active, &body).unwrap();
        for (kind, expected) in [
            (
                Kind::Admission,
                "95b291b3c59ca04e3f86264786b133a6ecec33452a6ffc9f40288d0542123a95",
            ),
            (
                Kind::AdmissionAck,
                "d88379ef03b14301c05d4d9f0db7a4f4b06c48c41fde40931bc6f7a4afd3dc48",
            ),
            (
                Kind::Stop,
                "f5ca59051097e4521c51aaad381a3f63a5b4462ba2cfc790b6046bb4a4a8db1f",
            ),
            (
                Kind::Drained,
                "0b7149e5ac6b083d115741625097a12a18c19cf8d3d1cdd25e27de1f3114baa3",
            ),
        ] {
            let mut frame = exchange.signed(&key, kind).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&frame).unwrap()["mac"],
                expected
            );
            exchange.check_frame(&key, kind, &frame).unwrap();
            frame.push(b'\n');
            assert!(exchange.check_frame(&key, kind, &frame).is_err());
        }
        let stop = exchange.signed(&key, Kind::Stop).unwrap();
        assert!(exchange.check_frame(&key, Kind::Drained, &stop).is_err());
    }

    #[test]
    fn ordinary_furnishing_refuses_active_same_generation_and_copied_physical_binding() {
        let mut prior = binding(GENERATION);
        assert!(binding(NEXT).consume(&prior).is_err());
        prior.phase = Phase::Clean;
        assert!(binding(GENERATION).consume(&prior).is_err());
        let mut copied = binding(NEXT);
        copied.physical.lock_inode += 1;
        assert!(copied.consume(&prior).is_err());
        let mut upgrade = binding(NEXT);
        upgrade.payload_manifest_sha256 = "4".repeat(64);
        assert!(upgrade.consume(&prior).is_err());
        let mut next = binding(NEXT);
        next.consume(&prior).unwrap();
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        assert!(binding(LATER).consume(&next).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn catalog40_binding(generation: &str, tuple: (&str, &str, &str)) -> Binding {
        let mut result = binding(generation);
        result.catalog = 40;
        result.source_commit = tuple.0.into();
        result.producer_sha256 = tuple.1.into();
        result.payload_manifest_sha256 = tuple.2.into();
        result
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_transition_refuses_unqualified_or_unclean_bindings() {
        use crate::native_payload::{CATALOG40_FRAME_SOURCE, CATALOG40_FRAME_TARGET};
        let mut prior = catalog40_binding(GENERATION, CATALOG40_FRAME_SOURCE);
        let target = catalog40_binding(NEXT, CATALOG40_FRAME_TARGET);
        assert!(target.clone().consume_catalog40_transition(&prior, Catalog40Transition::White).is_err());
        prior.phase = Phase::Clean;
        assert!(target.clone().consume(&prior).is_err());
        let mut next = target.clone();
        next.consume_catalog40_transition(&prior, Catalog40Transition::White).unwrap();
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));

        let mut bad = prior.clone();
        bad.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_catalog40_transition(&bad, Catalog40Transition::White).is_err());
        bad = prior.clone();
        bad.physical.lock_inode += 1;
        assert!(target.clone().consume_catalog40_transition(&bad, Catalog40Transition::White).is_err());
        bad = prior.clone();
        bad.producer_sha256 = "a".repeat(64);
        assert!(target.clone().consume_catalog40_transition(&bad, Catalog40Transition::White).is_err());
        bad = target.clone();
        bad.payload_manifest_sha256 = "a".repeat(64);
        assert!(bad.consume_catalog40_transition(&prior, Catalog40Transition::White).is_err());
        bad = target;
        bad.catalog = 39;
        assert!(bad.consume_catalog40_transition(&prior, Catalog40Transition::White).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_transition_requires_the_exact_directed_tangent_step() {
        use crate::native_payload::{CATALOG40_FRAME_SOURCE, CATALOG40_FRAME_TARGET, CATALOG40_TANGENT_TARGET};
        let mut white = catalog40_binding(GENERATION, CATALOG40_FRAME_TARGET);
        white.phase = Phase::Clean;
        let tangent = catalog40_binding(NEXT, CATALOG40_TANGENT_TARGET);
        let step = Catalog40Transition::Tangent;
        assert_eq!(Catalog40Transition::from_manifests((
            &white.payload_manifest_sha256, &tangent.payload_manifest_sha256,
        )), Some(step));
        let mut next = tangent.clone();
        next.consume_catalog40_transition(&white, step).unwrap();
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert!(tangent.clone().consume(&white).is_err());
        assert!(tangent.clone().consume_catalog40_transition(&white, Catalog40Transition::White).is_err());

        let mut original = catalog40_binding(GENERATION, CATALOG40_FRAME_SOURCE);
        original.phase = Phase::Clean;
        assert!(tangent.clone().consume_catalog40_transition(&original, step).is_err());
        assert_eq!(Catalog40Transition::from_manifests((
            &original.payload_manifest_sha256, &tangent.payload_manifest_sha256,
        )), None);
        assert_eq!(Catalog40Transition::from_manifests((
            &tangent.payload_manifest_sha256, &white.payload_manifest_sha256,
        )), None);
        let mut bad = tangent;
        bad.producer_sha256 = white.producer_sha256.clone();
        assert!(bad.consume_catalog40_transition(&white, step).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog39_upgrade_uses_a_distinct_admission_and_reopens_normally_after_stop() {
        let transition = Catalog40Transition::test_upgrade();
        let mut prior = catalog40_binding(GENERATION, transition.source());
        prior.catalog = 39;
        prior.phase = Phase::Clean;
        let target = catalog40_binding(NEXT, transition.target());
        assert!(target.clone().consume(&prior).is_err());
        assert!(target.clone().consume_catalog40_transition(&prior, Catalog40Transition::Tangent).is_err());
        let mut active = target.clone();
        active.consume_catalog40_transition(&prior, transition).unwrap();
        assert_eq!(active.admission_kind, Admission::CatalogUpgrade039To040);
        assert_eq!(active.admission_kind.tag(), 4);
        assert_eq!(serde_json::to_value(active.admission_kind).unwrap(), "catalog-upgrade-039-to-040");
        assert_eq!(active.previous_generation.as_deref(), Some(GENERATION));
        assert!(catalog40_binding(LATER, transition.target()).consume(&active).is_err());
        active.phase = Phase::Clean;
        let mut reopened = catalog40_binding(LATER, transition.target());
        reopened.consume(&active).unwrap();
        assert_eq!(reopened.admission_kind, Admission::CleanReopen);
        assert!(catalog40_binding(LATER, transition.target()).consume_catalog40_transition(&active, transition).is_err());

        let mut invalid = prior;
        invalid.catalog = 40;
        assert!(target.clone().consume_catalog40_transition(&invalid, transition).is_err());
        invalid.catalog = 39;
        invalid.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_catalog40_transition(&invalid, transition).is_err());
        invalid.owner.principal_id = target.owner.principal_id.clone();
        invalid.physical.lock_inode += 1;
        assert!(target.clone().consume_catalog40_transition(&invalid, transition).is_err());
        invalid.physical = target.physical;
        invalid.phase = Phase::Active;
        assert!(target.clone().consume_catalog40_transition(&invalid, transition).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_transition_recovers_only_its_own_consumption_and_keeps_trial_record() {
        for transition in [Catalog40Transition::White, Catalog40Transition::Tangent, Catalog40Transition::test_upgrade()] {
            check_catalog40_recovery(transition);
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn check_catalog40_recovery(transition: Catalog40Transition) {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("catalog40-transition-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let writer = furnishing_lifetime::WriterLifetime::acquire(&root).unwrap();
        let mut prior = catalog40_binding(GENERATION, transition.source());
        prior.catalog = transition.source_catalog().version();
        prior.physical = writer.identity().unwrap();
        prior.phase = Phase::Clean;
        let source = Current { format: 1, id: GENERATION.into(), furnishing: Some(prior.clone()) };
        let trial = Catalog40Trial { source: source.clone(), generation: NEXT.into() };
        let runs = directory(&root, "native-runs").unwrap();
        let prior_run = directory(&runs, GENERATION).unwrap();
        let trial_run = directory(&runs, NEXT).unwrap();
        let mut context = Context {
            format: 1,
            job_id: Some(GENERATION.into()),
            state: "stopped".into(),
            ports: Ports::default(),
            readiness: Readiness::default(),
            error_code: None,
            failure_code: None,
            postgres_pid: None,
            node_pid: None,
            auxiliary_pid: None,
            exits: [("node".into(), 0), ("postgres".into(), 0)].into(),
            log_truncated: false,
        };
        let proof = |consumed: &str| Some(FurnishingCompletion {
            binding_sha256: prior.sha256().unwrap(), consumed_by: Some(consumed.into()),
        });
        write_run(&prior_run, &context, proof(LATER)).unwrap();
        write_json(&root, "native-current-run.json", &source).unwrap();
        assert!(trial.recover_source(&service, &writer, transition).is_err());
        assert!(unconsumed_furnishing_completion(&root, &prior).is_err());

        write_run(&prior_run, &context, proof(NEXT)).unwrap();
        let mut target = catalog40_binding(NEXT, transition.target());
        target.physical = prior.physical;
        target.consume_catalog40_transition(&prior, transition).unwrap();
        write_json(&root, "native-current-run.json", &Current {
            format: 1, id: NEXT.into(), furnishing: Some(target.clone()),
        }).unwrap();

        // The helper crashed after publishing Active and before it could spawn.
        context.job_id = Some(NEXT.into());
        context.state = "starting".into();
        context.exits.clear();
        write_run(&trial_run, &context, None).unwrap();
        trial.check_recovery_root(&root, &writer, transition).unwrap();
        context.state = "starting_database".into();
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition).is_err());
        if transition.upgrades_catalog() {
            // This state covers a committed SQL transaction before Runtime Ready.
            // The PostgreSQL ledger alone cannot authorize an old-runtime reopen.
            context.state = "starting_services".into();
            write_run(&trial_run, &context, None).unwrap();
            assert!(trial.check_recovery_root(&root, &writer, transition).is_err());
            assert!(catalog40_binding(LATER, transition.target()).consume_catalog40_transition(&target, transition).is_err());
        }

        // The helper drained the failed trial and exited before sealing the
        // supplemental checkpoint journal. Only the matching drain proof works.
        context.state = "failed".into();
        context.failure_code = Some("NATIVE_TEST_FAILURE".into());
        context.exits.insert("postgres".into(), 0);
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition).is_err());
        write_json(&trial_run, "catalog40-drained.json", &"a".repeat(64)).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition).is_err());
        write_json(&trial_run, "catalog40-drained.json", &target.sha256().unwrap()).unwrap();
        context.node_pid = Some(std::process::id());
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition).is_err());
        context.node_pid = None;
        write_run(&trial_run, &context, None).unwrap();
        trial.check_recovery_root(&root, &writer, transition).unwrap();
        trial.recover_source(&service, &writer, transition).unwrap();
        trial.recover_source(&service, &writer, transition).unwrap();
        assert_eq!(current_run(&root).unwrap(), Some(source));
        unconsumed_furnishing_completion(&root, &prior).unwrap();
        assert!(read_run(&root, NEXT).is_ok());
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_furnishing_publication_reopen_cannot_consume_a_037_completion() {
        let mut prior = binding(GENERATION);
        prior.phase = Phase::Clean;
        let mut next = binding(NEXT);
        next.catalog = 38;
        next.check().unwrap();
        assert!(next.consume(&prior).is_err());

        prior.catalog = 38;
        next.consume(&prior).unwrap();
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(next.catalog, 38);
        assert!(binding(LATER).consume(&next).is_err());
    }

    #[test]
    fn ordinary_furnishing_material_publication_requires_its_own_completion() {
        let mut prior = binding(GENERATION);
        prior.phase = Phase::Clean;
        prior.catalog = 38;
        let mut next = binding(NEXT);
        next.catalog = 39;
        assert!(next.consume(&prior).is_err());
        prior.catalog = 39;
        next.consume(&prior).unwrap();
        assert_eq!(next.catalog, 39);
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));

        let mut multimap = binding(LATER);
        multimap.catalog = 40;
        assert!(multimap.consume(&prior).is_err());
        prior.catalog = 40;
        multimap.consume(&prior).unwrap();
        assert_eq!(multimap.catalog, 40);
        assert_eq!(multimap.admission_kind, Admission::CleanReopen);
    }

    #[test]
    fn ordinary_frame_preparation_admits_only_fresh_and_same_catalog_reopen() {
        let mut prior = binding(GENERATION);
        prior.phase = Phase::Clean;
        prior.catalog = 40;
        let mut next = binding(NEXT);
        next.catalog = 41;
        next.check().unwrap();
        assert!(next.consume(&prior).is_err());
        prior.catalog = 41;
        next.consume(&prior).unwrap();
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        next.admission_kind = Admission::VerifiedRestore;
        assert!(next.check().is_err());
    }

    #[test]
    fn ordinary_furnishing_publication_consumes_only_same_payload_and_physical_restore() {
        let mut restored = binding(GENERATION);
        restored.catalog = 38;
        restored.phase = Phase::ActivatedRestore;
        restored.admission_kind = Admission::VerifiedRestore;
        restored.restore = Some(Restore {
            id: "10000000-0000-4000-8000-000000000007".into(),
            backup_id: "10000000-0000-4000-8000-000000000008".into(),
            archive_manifest_sha256: "5".repeat(64),
            target_system_identifier: "123".into(),
            provenance: None,
        });
        let mut next = binding(NEXT);
        next.catalog = 38;
        let mut foreign = restored.clone();
        foreign.catalog = 37;
        assert!(next.consume(&foreign).is_err());
        foreign = restored.clone();
        foreign.payload_manifest_sha256 = "4".repeat(64);
        assert!(next.consume(&foreign).is_err());
        foreign = restored.clone();
        foreign.physical.root_inode += 1;
        assert!(next.consume(&foreign).is_err());
        next.consume(&restored).unwrap();
        assert_eq!(next.admission_kind, Admission::VerifiedRestore);
        assert_eq!(next.owner, restored.owner);
        assert_eq!(next.restore, restored.restore);
    }

    #[test]
    fn ordinary_furnishing_consumption_survives_db_only_pointer_rotation_and_refuses_replay() {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("furnishing-binding-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let runs = directory(&root, "native-runs").unwrap();
        let prior_run = directory(&runs, GENERATION).unwrap();
        let mut prior = binding(GENERATION);
        prior.phase = Phase::Clean;
        let context = Context {
            format: 1,
            job_id: Some(GENERATION.into()),
            state: "stopped".into(),
            ports: Ports::default(),
            readiness: Readiness::default(),
            error_code: None,
            failure_code: None,
            postgres_pid: None,
            node_pid: None,
            auxiliary_pid: None,
            exits: [("node".into(), 0), ("postgres".into(), 0)].into(),
            log_truncated: false,
        };
        write_run(
            &prior_run,
            &context,
            Some(FurnishingCompletion {
                binding_sha256: prior.sha256().unwrap(),
                consumed_by: None,
            }),
        )
        .unwrap();
        write_json(
            &root,
            "native-current-run.json",
            &Current {
                format: 1,
                id: NEXT.into(),
                furnishing: Some(prior.clone()),
            },
        )
        .unwrap();
        let observed = current_run(&root).unwrap().unwrap();
        assert_eq!(observed.id, NEXT);
        assert_eq!(observed.furnishing.as_ref().unwrap().generation, GENERATION);
        assert!(validate_cold_furnishing_source(&root, None, &binding(NEXT)).is_err());
        let mut replacement = binding(NEXT);
        replacement.physical.lock_inode += 1;
        assert!(validate_cold_furnishing_source(&root, Some(&prior), &replacement).is_err());
        validate_cold_furnishing_source(&root, Some(&prior), &binding(NEXT)).unwrap();
        consume_furnishing_completion(&root, &prior, LATER).unwrap();
        // Crash after consuming but before replacing the current pointer.
        assert!(consume_furnishing_completion(&root, &prior, NEXT).is_err());
        assert!(validate_cold_furnishing_source(&root, Some(&prior), &binding(NEXT)).is_err());
        let (_, proof) = read_run(&root, GENERATION).unwrap();
        assert_eq!(proof.unwrap().consumed_by.as_deref(), Some(LATER));
        write_json(
            &root,
            "native-current-run.json",
            &serde_json::json!({
                "format":1, "id":NEXT, "furnishing":null
            }),
        )
        .unwrap();
        assert!(current_run(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_furnishing_verified_restore_is_a_distinct_one_time_transition() {
        let mut prior = binding(GENERATION);
        prior.phase = Phase::ActivatedRestore;
        prior.admission_kind = Admission::VerifiedRestore;
        assert!(binding(NEXT).consume(&prior).is_err());
        prior.restore = Some(Restore {
            id: "10000000-0000-4000-8000-000000000008".into(),
            backup_id: "10000000-0000-4000-8000-000000000009".into(),
            archive_manifest_sha256: "6".repeat(64),
            target_system_identifier: "1234".into(),
            provenance: None,
        });
        let mut copied = binding(NEXT);
        copied.physical.root_inode += 1;
        assert!(copied.consume(&prior).is_err());
        let mut next = binding(NEXT);
        next.consume(&prior).unwrap();
        assert_eq!(next.admission_kind, Admission::VerifiedRestore);
        assert_eq!(next.restore, prior.restore);
        assert!(binding(LATER).consume(&next).is_err());
        next.phase = Phase::Clean;
        let mut later = binding(LATER);
        later.consume(&next).unwrap();
        assert_eq!(later.admission_kind, Admission::CleanReopen);
        assert_eq!(later.restore, prior.restore);
    }

    #[test]
    fn ordinary_furnishing_restore_before_intent_failure_uses_new_recovery_target() {
        use crate::installations::{InstallationController, native_maintenance::Input, native_ui::{Command, Request}};
        use std::os::unix::fs::DirBuilderExt;

        let parent = std::env::temp_dir().join(format!("furnishing-restore-retry-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let controller = Arc::new(InstallationController::new(parent.join("profile"), None).unwrap());
        let original = controller.context().unwrap();
        let failed = controller.create(&original.selection_token, true).unwrap();
        let failed_root = failed.installations.iter().find(|entry| entry.id == failed.active_id).unwrap();
        let failed_root = Path::new(&failed_root.path);
        let database = directory(failed_root, "native-database").unwrap();
        write_private_new(&database, "PG_VERSION", b"18\n", 32).unwrap();
        write_json(failed_root, "native-current-run.json", &Current {
            format: 1, id: GENERATION.into(), furnishing: None,
        }).unwrap();
        let journal = fs::read(failed_root.join("native-current-run.json")).unwrap();
        assert!(controller.native_pending_restoration(&failed.selection_token).unwrap().is_none());
        assert!(furnishing_restore_stage_creates_lock(current_run(failed_root).unwrap().as_ref(), true, false).is_err());

        let mut host = crate::native_host::Host::new(controller.clone(), parent.join("unused-resources"));
        host.handle(Request {
            version: 1, id: 1,
            command: Command::NativeMaintenance {
                selection_token: failed.selection_token,
                maintenance: Input::CreateRecovery { acknowledged: true },
            },
        }).unwrap();
        let replacement = controller.context().unwrap();
        assert_ne!(replacement.active_id, failed.active_id);
        assert!(replacement.installations.iter().any(|entry| entry.id == original.active_id));
        assert!(replacement.installations.iter().any(|entry| entry.id == failed.active_id));
        assert_eq!(fs::read(failed_root.join("native-current-run.json")).unwrap(), journal);
        assert_eq!(fs::read(database.join("PG_VERSION")).unwrap(), b"18\n");
        let target = replacement.installations.iter().find(|entry| entry.id == replacement.active_id).unwrap();
        let target = Path::new(&target.path);
        assert!(furnishing_restore_stage_creates_lock(
            current_run(target).unwrap().as_ref(),
            exists(&target.join("native-database")).unwrap(),
            controller.native_pending_restoration(&replacement.selection_token).unwrap().is_some(),
        ).unwrap());
        drop(host);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn ordinary_furnishing_reopen_does_not_create_missing_lock() {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("furnishing-missing-lock-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        assert!(furnishing_lifetime::WriterLifetime::acquire_existing(&root).is_err());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        fs::remove_dir(root).unwrap();
    }
}
