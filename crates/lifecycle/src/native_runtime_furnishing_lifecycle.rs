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
    #[serde(rename = "catalog-upgrade-040-to-044")]
    CatalogUpgrade040To044,
    #[serde(rename = "catalog-upgrade-044-to-045")]
    CatalogUpgrade044To045,
    InterruptedReopen,
}

impl Admission {
    fn tag(self) -> u8 {
        match self {
            Self::FreshEmpty => 1,
            Self::CleanReopen => 2,
            Self::VerifiedRestore => 3,
            Self::CatalogUpgrade039To040 => 4,
            Self::CatalogUpgrade040To044 => 5,
            Self::CatalogUpgrade044To045 => 6,
            Self::InterruptedReopen => 7,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) carried_provenance: Option<crate::installations::native_restore::CarriedObservation>,
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
    // First is the exact claim generation authenticated on the owner channel.
    // Remaining entries preserve authority across another pre-ready interruption.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) interrupted_claim_generations: Vec<String>,
}

impl Binding {
    pub(super) fn check(&self) -> Result<()> {
        let interrupted = self.admission_kind == Admission::InterruptedReopen;
        let lineage = &self.interrupted_claim_generations;
        if interrupted {
            let tuple = (self.source_commit.as_str(), self.producer_sha256.as_str(), self.payload_manifest_sha256.as_str());
            let supported = match self.catalog {
                45 => crate::native_payload::catalog45_recovery_host_matches(tuple),
                46 => crate::native_payload::large_model_recovery_host_matches(tuple),
                47 => crate::native_payload::fresh_image_host_matches(tuple),
                _ => false,
            };
            if !supported || lineage.is_empty() || lineage.len() > 8
                || !self.previous_generation.as_ref().is_some_and(|id| lineage.contains(id))
                || lineage.iter().enumerate().any(|(index, id)| id == &self.generation
                    || !installations::uuid(id) || lineage[..index].contains(id))
            {
                return Err(err(ERROR));
            }
        } else if !lineage.is_empty() {
            return Err(err(ERROR));
        }
        let tuple = (self.source_commit.as_str(), self.producer_sha256.as_str(), self.payload_manifest_sha256.as_str());
        let retained_restore = self.catalog == 45 && self.admission_kind == Admission::CleanReopen
            && self.restore.as_ref().is_some_and(|restore| restore.provenance.is_some())
            && crate::native_payload::catalog45_retained_restore_payload(tuple).is_some();
        let carried_restore = self.catalog == 45
            && matches!(self.admission_kind, Admission::VerifiedRestore | Admission::CleanReopen)
            && self.restore.as_ref().is_some_and(|restore| restore.carried_provenance.is_some())
            && (crate::native_payload::catalog45_carried_restore_host_matches(tuple)
                || (self.admission_kind == Admission::CleanReopen
                    && crate::native_payload::catalog45_adoption_carried_receipt_payload(tuple).is_some()));
        let private_hayes_bootstrap = self.admission_kind == Admission::FreshEmpty && match self.catalog {
            42 => crate::native_payload::hayes_validation_host_matches(tuple),
            43 => crate::native_payload::hayes_disposition_host_matches(tuple),
            44 => crate::native_payload::hayes_release_host_matches(tuple),
            _ => false,
        };
        if self.version != 1
            || !(matches!(self.catalog, 37..=41) || private_hayes_bootstrap || retained_restore || carried_restore
                || (self.catalog == 44 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::CleanReopen | Admission::CatalogUpgrade040To044)
                    && crate::native_payload::ordinary_hayes_release_host_matches(tuple))
                || (self.catalog == 45 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::CleanReopen | Admission::CatalogUpgrade044To045 | Admission::InterruptedReopen)
                    && crate::native_payload::required_model_preparation_host_matches(tuple))
                || (self.catalog == 45 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::FreshEmpty | Admission::CleanReopen)
                    && crate::native_payload::ordinary_fresh_catalog45_host_matches(tuple))
                || (self.catalog == 45 && self.restore.is_some()
                    && matches!(self.admission_kind, Admission::VerifiedRestore | Admission::CleanReopen)
                    && crate::native_payload::catalog45_same_version_restore_host_matches(tuple))
                || (self.catalog == 46 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::FreshEmpty | Admission::CleanReopen | Admission::InterruptedReopen)
                    && crate::native_payload::large_model_host_matches(tuple))
                || (self.catalog == 47 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::FreshEmpty | Admission::CleanReopen | Admission::InterruptedReopen)
                    && crate::native_payload::fresh_image_host_matches(tuple))
                || (self.catalog == 48 && self.restore.is_none()
                    && matches!(self.admission_kind, Admission::FreshEmpty | Admission::CleanReopen)
                    && crate::native_payload::info_stand_preparation_host_matches(tuple)))
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
            (Phase::Active | Phase::Clean, Admission::InterruptedReopen) => {
                self.previous_generation.is_some() && self.restore.is_none()
            }
            (Phase::Active | Phase::Clean, Admission::VerifiedRestore) => {
                self.previous_generation.is_some() && self.restore.is_some()
            }
            (Phase::Active | Phase::Clean, Admission::CatalogUpgrade039To040) => {
                self.catalog == 40 && self.previous_generation.is_some() && self.restore.is_none()
            }
            (Phase::Active | Phase::Clean, Admission::CatalogUpgrade040To044) => {
                self.catalog == 44 && self.previous_generation.is_some() && self.restore.is_none()
            }
            (Phase::Active | Phase::Clean, Admission::CatalogUpgrade044To045) => {
                self.catalog == 45 && self.previous_generation.is_some() && self.restore.is_none()
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
                || proof.carried_provenance.as_ref().is_some_and(|provenance|
                    !carried_restore || proof.provenance.is_some()
                        || !provenance.matches(&proof.archive_manifest_sha256, &proof.target_system_identifier))
                || proof.provenance.as_ref().is_some_and(|provenance|
                    !(retained_restore || crate::native_payload::native_restore_provenance_host_matches(self.catalog, tuple))
                        || !provenance.matches(&proof.id, &proof.backup_id, &proof.archive_manifest_sha256, &proof.target_system_identifier))
            })
        {
            return Err(err(ERROR));
        }
        Ok(())
    }

    pub(super) fn verify_restore_activation(&self, root: &Path) -> Result<()> {
        self.check()?;
        let proof = self.restore.as_ref().ok_or_else(|| err(ERROR))?;
        let tuple = (self.source_commit.as_str(), self.producer_sha256.as_str(), self.payload_manifest_sha256.as_str());
        let receipt_payload = crate::native_payload::catalog45_retained_restore_payload(tuple)
            .or_else(|| crate::native_payload::catalog45_adoption_carried_receipt_payload(tuple))
            .unwrap_or(self.payload_manifest_sha256.as_str());
        crate::installations::native_restore::verify_ordinary_activation(root, &proof.id, &proof.backup_id,
            &proof.archive_manifest_sha256, &proof.target_system_identifier, receipt_payload, proof.provenance.as_ref(), proof.carried_provenance.as_ref())
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

    pub(super) fn consume_interrupted(&mut self, prior: &Self, prior_ready: bool) -> Result<()> {
        prior.check()?;
        if prior.phase != Phase::Active || prior.restore.is_some()
            || self.generation == prior.generation || !self.same_installation(prior)
        {
            return Err(err(ERROR));
        }
        self.admission_kind = Admission::InterruptedReopen;
        self.previous_generation = Some(prior.generation.clone());
        self.interrupted_claim_generations = if prior_ready { Vec::new() } else { prior.interrupted_claim_generations.clone() };
        self.interrupted_claim_generations.push(prior.generation.clone());
        self.check()
    }

    pub(super) fn select_interrupted_claim_generation(&mut self, observed: &str) -> Result<()> {
        let generation = if observed.is_empty() {
            self.previous_generation.as_deref().ok_or_else(|| err(ERROR))?
        } else {
            observed
        };
        let index = self.interrupted_claim_generations.iter().position(|id| id == generation).ok_or_else(|| err(ERROR))?;
        self.interrupted_claim_generations.swap(0, index);
        self.check()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn transition_restore(&self, transition: FurnishingTransition) -> Result<Option<&Restore>> {
        if transition.allows_catalog45_adoption_without_restore() && self.restore.is_none() {
            return if self.admission_kind == Admission::CleanReopen { Ok(None) } else { Err(err(ERROR)) };
        }
        if transition.retains_catalog45_adoption_carried_restore() {
            return self.restore.as_ref()
                .filter(|restore| self.admission_kind == Admission::CleanReopen
                    && restore.provenance.is_none() && restore.carried_provenance.is_some())
                .map(Some)
                .ok_or_else(|| err(ERROR));
        }
        match (&self.restore, transition.retains_catalog45_restore()) {
            (None, false) => Ok(None),
            (Some(restore), true) if self.admission_kind == Admission::CleanReopen && restore.provenance.is_some() => Ok(Some(restore)),
            _ => Err(err(ERROR)),
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn consume_furnishing_transition(&mut self, prior: &Self, transition: FurnishingTransition) -> Result<()> {
        prior.check()?;
        let admission = Self::transition_admission(transition);
        let source_phase = if admission == Admission::InterruptedReopen {
            if prior.admission_kind != Admission::CleanReopen {
                return Err(err(ERROR));
            }
            Phase::Active
        } else {
            Phase::Clean
        };
        if prior.phase != source_phase || self.phase != Phase::Active
            || self.previous_generation.is_some() || self.admission_kind != Admission::FreshEmpty
            || self.restore.is_some()
            || self.generation == prior.generation || self.owner != prior.owner
            || self.physical != prior.physical
            || !prior.matches_catalog_tuple(transition.source_catalog().version(), transition.source())
            || !self.matches_catalog_tuple(transition.target_catalog().version(), transition.target())
        {
            return Err(err(ERROR));
        }
        self.restore = prior.transition_restore(transition)?.cloned();
        self.admission_kind = admission;
        self.previous_generation = Some(prior.generation.clone());
        if admission == Admission::InterruptedReopen {
            self.interrupted_claim_generations.push(prior.generation.clone());
        }
        self.check()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn transition_admission(transition: FurnishingTransition) -> Admission {
        match transition {
            FurnishingTransition::Upgrade39(_) => Admission::CatalogUpgrade039To040,
            FurnishingTransition::Upgrade40(_) => Admission::CatalogUpgrade040To044,
            FurnishingTransition::Upgrade44 { .. } => Admission::CatalogUpgrade044To045,
            FurnishingTransition::Recover45 { .. } => Admission::InterruptedReopen,
            _ => Admission::CleanReopen,
        }
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CarriedChannelProof {
    restore_id: String,
    backup_id: String,
    authenticated_manifest_sha256: String,
    observation: crate::installations::native_restore::CarriedObservation,
}

pub(super) struct Exchange {
    generation: String,
    lease_transcript_sha256: String,
    admission_kind: Admission,
    active_binding_sha256: String,
    restore_proof: Option<RestoreProof>,
    carried_restore_proof: Option<CarriedChannelProof>,
    previous_runtime_generation: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    carried_restore_proof: Option<&'a CarriedChannelProof>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_runtime_generation: Option<&'a str>,
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
            carried_restore_proof: binding.restore.as_ref().and_then(|restore| {
                restore.carried_provenance.as_ref().map(|observation| CarriedChannelProof {
                    restore_id: restore.id.clone(), backup_id: restore.backup_id.clone(),
                    authenticated_manifest_sha256: restore.archive_manifest_sha256.clone(), observation: observation.clone(),
                })
            }),
            previous_runtime_generation: binding.interrupted_claim_generations.first().cloned(),
        })
    }

    fn version(&self) -> u32 {
        if self.carried_restore_proof.is_some() { 4 }
        else if self.previous_runtime_generation.is_some() { 3 }
        else if self.restore_proof.is_some() { 2 }
        else { 1 }
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
        if let Some(previous) = &self.previous_runtime_generation {
            let previous_id = Uuid::parse_str(previous).map_err(|_| err(ERROR))?;
            if previous_id == id || previous_id.to_string() != *previous || self.restore_proof.is_some()
                || self.admission_kind != Admission::InterruptedReopen
            {
                return Err(err(ERROR));
            }
            bytes.extend_from_slice(previous_id.as_bytes());
        }
        if let Some(proof) = &self.restore_proof {
            let canonical = serde_json::to_vec(proof).map_err(|_| err(ERROR))?;
            bytes.extend(decode(&digest(&canonical))?);
        }
        if let Some(proof) = &self.carried_restore_proof {
            if self.restore_proof.is_some() || self.previous_runtime_generation.is_some()
                || !matches!(self.admission_kind, Admission::VerifiedRestore | Admission::CleanReopen)
            { return Err(err(ERROR)); }
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
            carried_restore_proof: self.carried_restore_proof.as_ref(),
            previous_runtime_generation: self.previous_runtime_generation.as_deref(),
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
            interrupted_claim_generations: Vec::new(),
        }
    }

    #[test]
    fn interrupted_reopen_v3_matches_platform_vectors() {
        let current = "11111111-1111-4111-8111-111111111111";
        let previous = "22222222-2222-4222-8222-222222222222";
        let exchange = Exchange {
            generation: current.into(),
            lease_transcript_sha256: "33".repeat(32),
            admission_kind: Admission::InterruptedReopen,
            active_binding_sha256: "44".repeat(32),
            restore_proof: None,
            carried_restore_proof: None,
            previous_runtime_generation: Some(previous.into()),
        };
        for (kind, mac) in [
            (Kind::Admission, "6da0c39aa8e606b20b5d972440f070fb7bfeeb72d3ca6c0971bc03a080d04908"),
            (Kind::AdmissionAck, "15aae6811fc80d85b491fc997ae615b273538d5ac7e82d756755acdf10ce58ef"),
            (Kind::Stop, "5a59d72e0ccd2cbdbac233202e20286edd6d5561f6e9690beed0e42720776d19"),
            (Kind::Drained, "2738da7f70720fff8de6614c85f74ca071edecdb04ea33831ec1a54d645a6196"),
        ] {
            let wire = exchange.signed(&[0x55; 32], kind).unwrap();
            let expected = format!(
                r#"{{"version":3,"kind":"{}","runtimeGeneration":"{current}","leaseTranscriptSHA256":"{}","admissionKind":"interrupted-reopen","activeBindingSHA256":"{}","previousRuntimeGeneration":"{previous}","mac":"{mac}"}}"#,
                kind.name(), "33".repeat(32), "44".repeat(32),
            );
            assert_eq!(wire, expected.as_bytes());
            exchange.check_frame(&[0x55; 32], kind, &wire).unwrap();
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn interrupted_reopen_keeps_the_original_claim_across_a_second_interruption() {
        let recovery = |generation: &str| {
            let mut result = binding(generation);
            let (source, producer, manifest) = crate::native_payload::CATALOG47_FRESH_IMAGE_RECOVERY;
            result.source_commit = source.into();
            result.producer_sha256 = producer.into();
            result.payload_manifest_sha256 = manifest.into();
            result.catalog = 47;
            result
        };
        let initial = recovery(GENERATION);
        let mut attempt = recovery(NEXT);
        assert!(attempt.consume(&initial).is_err());
        attempt.consume_interrupted(&initial, true).unwrap();
        attempt.select_interrupted_claim_generation(GENERATION).unwrap();

        let mut retry = recovery(LATER);
        retry.consume_interrupted(&attempt, false).unwrap();
        let mut durable: Binding = serde_json::from_slice(&serde_json::to_vec(&retry).unwrap()).unwrap();
        assert_eq!(durable.previous_generation.as_deref(), Some(NEXT));
        assert!(durable.select_interrupted_claim_generation(&format!("{GENERATION}\n{NEXT}")).is_err());
        assert!(durable.select_interrupted_claim_generation("aaaaaaaa-0000-4000-8000-000000000001").is_err());
        durable.select_interrupted_claim_generation(GENERATION).unwrap();
        assert_eq!(Exchange::new(&durable, b"lease").unwrap().previous_runtime_generation.as_deref(), Some(GENERATION));
        durable.select_interrupted_claim_generation(NEXT).unwrap();
        assert_eq!(Exchange::new(&durable, b"lease").unwrap().previous_runtime_generation.as_deref(), Some(NEXT));
        let mut after_ready = recovery(LATER);
        after_ready.consume_interrupted(&attempt, true).unwrap();
        assert!(after_ready.select_interrupted_claim_generation(GENERATION).is_err());
        let mut wrong_physical = recovery(LATER);
        wrong_physical.physical.root_inode += 1;
        assert!(wrong_physical.consume_interrupted(&attempt, false).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn model_interruption_requires_its_own_exact_046_seal() {
        let model = |generation: &str, (source, producer, manifest): (&str, &str, &str)| {
            let mut result = binding(generation);
            result.source_commit = source.into();
            result.producer_sha256 = producer.into();
            result.payload_manifest_sha256 = manifest.into();
            result.catalog = 46;
            result
        };
        let clean = crate::native_payload::CATALOG46_LARGE_MODEL;
        let recovery = crate::native_payload::CATALOG46_LARGE_MODEL_RECOVERY;
        assert!(model(NEXT, clean).consume_interrupted(&model(GENERATION, clean), true).is_err());
        assert!(model(NEXT, recovery).consume_interrupted(&model(GENERATION, clean), true).is_err());
        let mut admitted = model(NEXT, recovery);
        admitted.consume_interrupted(&model(GENERATION, recovery), true).unwrap();
        assert!(admitted.select_interrupted_claim_generation("unbound").is_err());
        assert!(admitted.select_interrupted_claim_generation(&format!("{GENERATION}\n{LATER}")).is_err());
        admitted.select_interrupted_claim_generation(GENERATION).unwrap();
        assert_eq!(Exchange::new(&admitted, b"lease").unwrap().previous_runtime_generation.as_deref(), Some(GENERATION));
        let mut wrong_catalog = admitted;
        wrong_catalog.catalog = 47;
        assert!(wrong_catalog.check().is_err());
    }

    #[test]
    fn carried_restore_v4_authenticates_both_hops_in_all_four_frames() {
        let observation = serde_json::from_value(serde_json::json!({
            "version": 1, "sourceHandoffSha256": "e".repeat(64),
            "priorRestoreProof": {
                "restoreId": "10000000-0000-4000-8000-000000000007",
                "backupId": "10000000-0000-4000-8000-000000000008",
                "authenticatedManifestSha256": "c".repeat(64),
                "sourceDatabaseSystemId": "110", "sourceDatabaseOid": "16000",
                "targetDatabaseSystemId": "220", "targetDatabaseOid": "16002",
                "provenanceRowCount": 1, "provenanceTableSha256": "d".repeat(64)
            },
            "archiveSourceDatabaseSystemId": "220", "archiveSourceDatabaseOid": "16002",
            "targetDatabaseSystemId": "330", "targetDatabaseOid": "16004",
            "provenanceRowCount": 1, "provenanceTableSha256": "d".repeat(64)
        })).unwrap();
        let exchange = Exchange {
            generation: GENERATION.into(), lease_transcript_sha256: "a".repeat(64),
            admission_kind: Admission::VerifiedRestore, active_binding_sha256: "b".repeat(64),
            restore_proof: None, previous_runtime_generation: None,
            carried_restore_proof: Some(CarriedChannelProof {
                restore_id: "10000000-0000-4000-8000-000000000009".into(),
                backup_id: "10000000-0000-4000-8000-000000000010".into(),
                authenticated_manifest_sha256: "f".repeat(64), observation,
            }),
        };
        for (kind, mac) in [
            (Kind::Admission, "f838721f65b91d757ef843dc00e91a6b9cbee4c01301c02333795b6ed93ef407"),
            (Kind::AdmissionAck, "8d57528f12d6b2c8a7d30f2bcf7a1f4b6bff59debf36ea1984dd510bd4839a5e"),
            (Kind::Stop, "8ab42debe4f96a9672779249f2e5029e8f8b9263b7d46f538a73d9729864c072"),
            (Kind::Drained, "b0b5d3c538b5b2a7f91bd83f4a71f6b25e7985c51fac12b9a56608ea274979fe"),
        ] {
            let key = [7; 32];
            let bytes = exchange.signed(&key, kind).unwrap();
            assert!(bytes.len() < LIMIT);
            let mut frame: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(frame["version"], 4);
            assert_eq!(frame["mac"], mac);
            assert!(frame.get("restoreProof").is_none());
            exchange.check_frame(&key, kind, &bytes).unwrap();
            frame["carriedRestoreProof"]["observation"]["archiveSourceDatabaseOid"] = "16003".into();
            assert!(exchange.check_frame(&key, kind, &serde_json::to_vec(&frame).unwrap()).is_err());
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
            carried_restore_proof: None,
            previous_runtime_generation: None,
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
            carried_provenance: None,
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
        assert!(target.clone().consume_furnishing_transition(&prior, FurnishingTransition::White).is_err());
        prior.phase = Phase::Clean;
        assert!(target.clone().consume(&prior).is_err());
        let mut next = target.clone();
        next.consume_furnishing_transition(&prior, FurnishingTransition::White).unwrap();
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));

        let mut bad = prior.clone();
        bad.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_furnishing_transition(&bad, FurnishingTransition::White).is_err());
        bad = prior.clone();
        bad.physical.lock_inode += 1;
        assert!(target.clone().consume_furnishing_transition(&bad, FurnishingTransition::White).is_err());
        bad = prior.clone();
        bad.producer_sha256 = "a".repeat(64);
        assert!(target.clone().consume_furnishing_transition(&bad, FurnishingTransition::White).is_err());
        bad = target.clone();
        bad.payload_manifest_sha256 = "a".repeat(64);
        assert!(bad.consume_furnishing_transition(&prior, FurnishingTransition::White).is_err());
        bad = target;
        bad.catalog = 39;
        assert!(bad.consume_furnishing_transition(&prior, FurnishingTransition::White).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_transition_requires_the_exact_directed_tangent_step() {
        use crate::native_payload::{CATALOG40_FRAME_SOURCE, CATALOG40_FRAME_TARGET, CATALOG40_TANGENT_TARGET};
        let mut white = catalog40_binding(GENERATION, CATALOG40_FRAME_TARGET);
        white.phase = Phase::Clean;
        let tangent = catalog40_binding(NEXT, CATALOG40_TANGENT_TARGET);
        let step = FurnishingTransition::Tangent;
        assert_eq!(FurnishingTransition::from_manifests((
            &white.payload_manifest_sha256, &tangent.payload_manifest_sha256,
        )), Some(step));
        let mut next = tangent.clone();
        next.consume_furnishing_transition(&white, step).unwrap();
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert!(tangent.clone().consume(&white).is_err());
        assert!(tangent.clone().consume_furnishing_transition(&white, FurnishingTransition::White).is_err());

        let mut original = catalog40_binding(GENERATION, CATALOG40_FRAME_SOURCE);
        original.phase = Phase::Clean;
        assert!(tangent.clone().consume_furnishing_transition(&original, step).is_err());
        assert_eq!(FurnishingTransition::from_manifests((
            &original.payload_manifest_sha256, &tangent.payload_manifest_sha256,
        )), None);
        assert_eq!(FurnishingTransition::from_manifests((
            &tangent.payload_manifest_sha256, &white.payload_manifest_sha256,
        )), None);
        let mut bad = tangent;
        bad.producer_sha256 = white.producer_sha256.clone();
        assert!(bad.consume_furnishing_transition(&white, step).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog39_upgrade_uses_a_distinct_admission_and_reopens_normally_after_stop() {
        let transition = FurnishingTransition::test_upgrade();
        let mut prior = catalog40_binding(GENERATION, transition.source());
        prior.catalog = 39;
        prior.phase = Phase::Clean;
        let target = catalog40_binding(NEXT, transition.target());
        assert!(target.clone().consume(&prior).is_err());
        assert!(target.clone().consume_furnishing_transition(&prior, FurnishingTransition::Tangent).is_err());
        let mut active = target.clone();
        active.consume_furnishing_transition(&prior, transition).unwrap();
        assert_eq!(active.admission_kind, Admission::CatalogUpgrade039To040);
        assert_eq!(active.admission_kind.tag(), 4);
        assert_eq!(serde_json::to_value(active.admission_kind).unwrap(), "catalog-upgrade-039-to-040");
        assert_eq!(active.previous_generation.as_deref(), Some(GENERATION));
        assert!(catalog40_binding(LATER, transition.target()).consume(&active).is_err());
        active.phase = Phase::Clean;
        let mut reopened = catalog40_binding(LATER, transition.target());
        reopened.consume(&active).unwrap();
        assert_eq!(reopened.admission_kind, Admission::CleanReopen);
        assert!(catalog40_binding(LATER, transition.target()).consume_furnishing_transition(&active, transition).is_err());

        let mut invalid = prior;
        invalid.catalog = 40;
        assert!(target.clone().consume_furnishing_transition(&invalid, transition).is_err());
        invalid.catalog = 39;
        invalid.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_furnishing_transition(&invalid, transition).is_err());
        invalid.owner.principal_id = target.owner.principal_id.clone();
        invalid.physical.lock_inode += 1;
        assert!(target.clone().consume_furnishing_transition(&invalid, transition).is_err());
        invalid.physical = target.physical;
        invalid.phase = Phase::Active;
        assert!(target.clone().consume_furnishing_transition(&invalid, transition).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_to_44_requires_exact_clean_owner_and_reopens_after_stop() {
        let transition = FurnishingTransition::reviewed_catalog40_to_44();
        let mut source = catalog40_binding(GENERATION, transition.source());
        source.phase = Phase::Clean;
        let mut target = catalog40_binding(NEXT, transition.target());
        target.catalog = 44;
        assert!(target.check().is_err());
        assert!(target.clone().consume(&source).is_err());

        let mut wrong_source = source.clone();
        wrong_source.payload_manifest_sha256 = "d".repeat(64);
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());
        wrong_source = source.clone();
        wrong_source.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());
        wrong_source = source.clone();
        wrong_source.physical.lock_inode += 1;
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());
        wrong_source = source.clone();
        wrong_source.phase = Phase::Active;
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());

        let mut wrong_target = target.clone();
        wrong_target.producer_sha256 = "d".repeat(64);
        assert!(wrong_target.consume_furnishing_transition(&source, transition).is_err());
        target.consume_furnishing_transition(&source, transition).unwrap();
        assert_eq!(target.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(target.admission_kind, Admission::CatalogUpgrade040To044);
        assert_eq!(target.admission_kind.tag(), 5);
        assert_eq!(serde_json::to_value(target.admission_kind).unwrap(), "catalog-upgrade-040-to-044");

        target.phase = Phase::Clean;
        let mut reopen = catalog40_binding(LATER, transition.target());
        reopen.catalog = 44;
        assert!(reopen.clone().consume_furnishing_transition(&target, transition).is_err());
        reopen.consume(&target).unwrap();
        assert_eq!(reopen.previous_generation.as_deref(), Some(NEXT));
        assert_eq!(reopen.admission_kind, Admission::CleanReopen);
        assert!(catalog40_binding(LATER, transition.source()).consume(&target).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn furnishing_catalog44_to_45_requires_exact_clean_owner_and_reopens_after_stop() {
        let transition = FurnishingTransition::reviewed_catalog44_to_45();
        let mut source = catalog40_binding(GENERATION, transition.source());
        source.catalog = 44;
        source.phase = Phase::Clean;
        source.admission_kind = Admission::CleanReopen;
        source.previous_generation = Some(LATER.into());

        let mut target = catalog40_binding(NEXT, transition.target());
        target.catalog = 45;
        assert!(target.check().is_err());
        assert!(target.clone().consume(&source).is_err());

        let mut wrong_source = source.clone();
        wrong_source.owner.principal_id = NEXT.into();
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());
        wrong_source = source.clone();
        wrong_source.physical.lock_inode += 1;
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());
        wrong_source = source.clone();
        wrong_source.phase = Phase::Active;
        assert!(target.clone().consume_furnishing_transition(&wrong_source, transition).is_err());

        let mut wrong_target = target.clone();
        wrong_target.producer_sha256 = "d".repeat(64);
        assert!(wrong_target.consume_furnishing_transition(&source, transition).is_err());
        target.consume_furnishing_transition(&source, transition).unwrap();
        assert_eq!(target.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(target.admission_kind.tag(), 6);
        assert_eq!(serde_json::to_value(target.admission_kind).unwrap(), "catalog-upgrade-044-to-045");

        let mut reopen = catalog40_binding(LATER, transition.target());
        reopen.catalog = 45;
        assert!(reopen.clone().consume(&target).is_err());
        target.phase = Phase::Clean;
        assert!(reopen.clone().consume_furnishing_transition(&target, transition).is_err());
        reopen.consume(&target).unwrap();
        assert_eq!(reopen.previous_generation.as_deref(), Some(NEXT));
        assert_eq!(reopen.admission_kind, Admission::CleanReopen);
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn furnishing_default_frame_catalog45_admits_fresh_and_same_owner_reopen() {
        let (source, producer, manifest) = crate::native_payload::CATALOG45_DEFAULT_FRAME;
        let mut fresh = binding(GENERATION);
        fresh.catalog = 45;
        fresh.source_commit = source.into();
        fresh.producer_sha256 = producer.into();
        fresh.payload_manifest_sha256 = manifest.into();
        fresh.check().unwrap();
        fresh.phase = Phase::Clean;
        fresh.check().unwrap();

        let mut next = fresh.clone();
        next.generation = NEXT.into();
        next.phase = Phase::Active;
        let mut unrelated = next.clone();
        unrelated.payload_manifest_sha256 = "0".repeat(64);
        assert!(unrelated.check().is_err());
        assert!(unrelated.consume(&fresh).is_err());

        next.consume(&fresh).unwrap();
        assert_eq!(next.admission_kind, Admission::CleanReopen);
        next.admission_kind = Admission::CatalogUpgrade044To045;
        assert!(next.check().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn furnishing_catalog44_to_45_recovery_retains_the_failed_trial_and_restores_source() {
        check_furnishing_recovery(FurnishingTransition::reviewed_catalog44_to_45());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog45_recovery_requires_exact_failed_stop_and_consumed_predecessor() {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("catalog45-recovery-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let runs = directory(&root, "native-runs").unwrap();
        let predecessor = directory(&runs, GENERATION).unwrap();
        let failed_run = directory(&runs, NEXT).unwrap();
        let mut binding = catalog40_binding(NEXT, crate::native_payload::CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET);
        binding.catalog = 45;
        binding.admission_kind = Admission::CleanReopen;
        binding.previous_generation = Some(GENERATION.into());
        write_json(&root, "native-current-run.json", &Current {
            format: 1, id: NEXT.into(), furnishing: Some(binding.clone()),
        }).unwrap();
        let mut state = Context {
            format: 1, job_id: Some(GENERATION.into()), state: "stopped".into(),
            ports: Ports::default(), readiness: Readiness::default(), error_code: None, failure_code: None,
            postgres_pid: None, node_pid: None, auxiliary_pid: None,
            exits: [("postgres".into(), 0), ("node".into(), 0), ("auxiliary".into(), 0)].into(),
            log_truncated: false,
        };
        let predecessor_state = state.clone();
        let proof = |consumer: &str| Some(FurnishingCompletion {
            binding_sha256: "7".repeat(64), consumed_by: Some(consumer.into()),
        });
        write_run(&predecessor, &predecessor_state, proof(LATER)).unwrap();
        state.job_id = Some(NEXT.into());
        state.state = "failed".into();
        state.failure_code = Some(ERROR.into());
        state.error_code = Some(ERROR.into());
        write_run(&failed_run, &state, None).unwrap();
        assert!(catalog45_recovery_source_generation(&root, None).is_err());
        write_run(&predecessor, &predecessor_state, proof(NEXT)).unwrap();
        assert_eq!(catalog45_recovery_source_generation(&root, None).unwrap(), NEXT);
        let mut ordinary = catalog40_binding(LATER, crate::native_payload::CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET);
        ordinary.catalog = 45;
        assert!(ordinary.consume(&binding).is_err());
        state.node_pid = Some(std::process::id());
        write_run(&failed_run, &state, None).unwrap();
        assert!(catalog45_recovery_source_generation(&root, None).is_err());
        state.node_pid = None;
        write_run(&failed_run, &state, proof(LATER)).unwrap();
        assert!(catalog45_recovery_source_generation(&root, None).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog45_same_payload_recovery_is_explicit_and_bound_to_its_failed_generation() {
        let selected = "62200d7ccb47e7ede5d1910a65e0087ac171c595d76ca14df6a19290382e4288";
        let transition = FurnishingTransition::from_manifests((selected, selected)).unwrap();
        let old = crate::native_payload::CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET;
        assert!(FurnishingTransition::from_manifests((old.2, old.2)).is_none());
        assert!(FurnishingTransition::from_manifests((selected, old.2)).is_none());

        let mut failed = catalog40_binding(GENERATION, transition.source());
        failed.catalog = 45;
        failed.admission_kind = Admission::CleanReopen;
        failed.previous_generation = Some(LATER.into());
        let mut next = catalog40_binding(NEXT, transition.target());
        next.catalog = 45;

        assert!(next.clone().consume(&failed).is_err());
        let mut different_owner = next.clone();
        different_owner.owner.workspace_id = NEXT.into();
        assert!(different_owner.consume_furnishing_transition(&failed, transition).is_err());

        next.consume_furnishing_transition(&failed, transition).unwrap();
        assert_eq!(next.admission_kind, Admission::InterruptedReopen);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(next.interrupted_claim_generations, [GENERATION]);
        assert!(next.clone().consume_furnishing_transition(&failed, transition).is_err());

        next.phase = Phase::Clean;
        let mut reopened = catalog40_binding(LATER, transition.target());
        reopened.catalog = 45;
        assert!(reopened.clone().consume_furnishing_transition(&next, transition).is_err());
        reopened.consume(&next).unwrap();
        assert_eq!(reopened.admission_kind, Admission::CleanReopen);
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_to_44_recovery_retains_the_failed_trial_and_restores_source() {
        check_furnishing_recovery(FurnishingTransition::reviewed_catalog40_to_44());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_transition_recovers_only_its_own_consumption_and_keeps_trial_record() {
        for transition in [FurnishingTransition::White, FurnishingTransition::Tangent, FurnishingTransition::test_upgrade()] {
            check_furnishing_recovery(transition);
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn check_furnishing_recovery(transition: FurnishingTransition) {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("catalog40-transition-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let writer = furnishing_lifetime::WriterLifetime::acquire(&root).unwrap();
        let mut prior = catalog40_binding(GENERATION, transition.source());
        prior.catalog = transition.source_catalog().version();
        if matches!(transition, FurnishingTransition::Upgrade44 { .. }) {
            prior.admission_kind = Admission::CleanReopen;
            prior.previous_generation = Some(LATER.into());
        }
        prior.physical = writer.identity().unwrap();
        prior.phase = Phase::Clean;
        let source = Current { format: 1, id: GENERATION.into(), furnishing: Some(prior.clone()) };
        let trial = FurnishingTransitionTrial { source: source.clone(), generation: NEXT.into() };
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
        target.catalog = transition.target_catalog().version();
        target.physical = prior.physical;
        target.consume_furnishing_transition(&prior, transition).unwrap();
        write_json(&root, "native-current-run.json", &Current {
            format: 1, id: NEXT.into(), furnishing: Some(target.clone()),
        }).unwrap();

        // The helper crashed after publishing Active and before it could spawn.
        context.job_id = Some(NEXT.into());
        context.state = "starting".into();
        context.exits.clear();
        write_run(&trial_run, &context, None).unwrap();
        trial.check_recovery_root(&root, &writer, transition, None).unwrap();
        context.state = "starting_database".into();
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition, None).is_err());
        if transition.upgrades_catalog() {
            // This state covers a committed SQL transaction before Runtime Ready.
            // The PostgreSQL ledger alone cannot authorize an old-runtime reopen.
            context.state = "starting_services".into();
            write_run(&trial_run, &context, None).unwrap();
            assert!(trial.check_recovery_root(&root, &writer, transition, None).is_err());
            let mut next = catalog40_binding(LATER, transition.target());
            next.catalog = transition.target_catalog().version();
            assert!(next.consume_furnishing_transition(&target, transition).is_err());
        }

        // The helper drained the failed trial and exited before sealing the
        // supplemental checkpoint journal. Only the matching drain proof works.
        context.state = "failed".into();
        context.failure_code = Some("NATIVE_TEST_FAILURE".into());
        context.exits.insert("postgres".into(), 0);
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition, None).is_err());
        write_json(&trial_run, "catalog40-drained.json", &"a".repeat(64)).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition, None).is_err());
        write_json(&trial_run, "catalog40-drained.json", &target.sha256().unwrap()).unwrap();
        context.node_pid = Some(std::process::id());
        write_run(&trial_run, &context, None).unwrap();
        assert!(trial.check_recovery_root(&root, &writer, transition, None).is_err());
        context.node_pid = None;
        write_run(&trial_run, &context, None).unwrap();
        trial.check_recovery_root(&root, &writer, transition, None).unwrap();
        let trial_state = fs::read(trial_run.join("state.json")).unwrap();
        let trial_drain = fs::read(trial_run.join("catalog40-drained.json")).unwrap();
        trial.recover_source(&service, &writer, transition).unwrap();
        trial.recover_source(&service, &writer, transition).unwrap();
        assert_eq!(current_run(&root).unwrap(), Some(source));
        unconsumed_furnishing_completion(&root, &prior).unwrap();
        assert_eq!(fs::read(trial_run.join("state.json")).unwrap(), trial_state);
        assert_eq!(fs::read(trial_run.join("catalog40-drained.json")).unwrap(), trial_drain);
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
            carried_provenance: None,
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

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog45_sealed_restore_admits_activation_and_normal_reopens() {
        let pinned = |generation: &str| {
            let (source, producer, manifest) = crate::native_payload::CATALOG45_PLACED_EXIT_DOORS;
            let mut value = binding(generation);
            value.catalog = 45;
            value.source_commit = source.into();
            value.producer_sha256 = producer.into();
            value.payload_manifest_sha256 = manifest.into();
            value
        };
        let mut activated = pinned(GENERATION);
        activated.phase = Phase::ActivatedRestore;
        activated.admission_kind = Admission::VerifiedRestore;
        assert!(activated.sha256().is_err());
        activated.restore = Some(Restore {
            id: "10000000-0000-4000-8000-000000000007".into(),
            backup_id: "10000000-0000-4000-8000-000000000008".into(),
            archive_manifest_sha256: "5".repeat(64),
            target_system_identifier: "123".into(),
            provenance: None,
            carried_provenance: None,
        });
        activated.sha256().unwrap();
        let proof: RestoreProof = serde_json::from_value(serde_json::json!({
            "restoreId": "10000000-0000-4000-8000-000000000007",
            "backupId": "10000000-0000-4000-8000-000000000008",
            "authenticatedManifestSha256": "5".repeat(64),
            "sourceDatabaseSystemId": "122", "sourceDatabaseOid": "16000",
            "targetDatabaseSystemId": "123", "targetDatabaseOid": "16000",
            "provenanceRowCount": 1, "provenanceTableSha256": "6".repeat(64)
        })).unwrap();
        activated.restore.as_mut().unwrap().provenance = Some(proof.clone());
        activated.sha256().unwrap();

        let mut other_payload = activated.clone();
        other_payload.payload_manifest_sha256 = "4".repeat(64);
        assert!(other_payload.check().is_err());
        let mut malformed = activated.clone();
        malformed.restore.as_mut().unwrap().target_system_identifier = "0123".into();
        assert!(malformed.check().is_err());

        let mut other_owner = pinned(NEXT);
        other_owner.owner.owner_uid += 1;
        assert!(other_owner.consume(&activated).is_err());
        let mut other_root = pinned(NEXT);
        other_root.physical.root_inode += 1;
        assert!(other_root.consume(&activated).is_err());

        let mut first = pinned(NEXT);
        first.consume(&activated).unwrap();
        assert_eq!(first.admission_kind, Admission::VerifiedRestore);
        assert_eq!(first.restore, activated.restore);
        assert_eq!(Exchange::new(&first, b"lease").unwrap().restore_proof.as_ref(), Some(&proof));
        first.phase = Phase::Clean;
        first.sha256().unwrap();

        let mut reopened = pinned(LATER);
        reopened.consume(&first).unwrap();
        assert_eq!(reopened.admission_kind, Admission::CleanReopen);
        assert_eq!(reopened.restore, activated.restore);
        assert_eq!(Exchange::new(&reopened, b"new lease").unwrap().restore_proof.as_ref(), Some(&proof));
        reopened.phase = Phase::Clean;
        reopened.sha256().unwrap();
    }

    #[test]
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn restored_catalog45_backed_transition_preserves_exact_restore_authority() {
        let source = crate::native_payload::CATALOG45_PLACED_EXIT_DOORS;
        let target = crate::native_payload::CATALOG45_BACKED_BENCH;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        let pin = |generation: &str, tuple: (&str, &str, &str)| {
            let mut value = binding(generation);
            value.catalog = 45;
            value.source_commit = tuple.0.into();
            value.producer_sha256 = tuple.1.into();
            value.payload_manifest_sha256 = tuple.2.into();
            value
        };
        let mut prior = pin(GENERATION, source);
        prior.phase = Phase::Clean;
        prior.admission_kind = Admission::CleanReopen;
        prior.previous_generation = Some("10000000-0000-4000-8000-000000000004".into());
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
        let proof: RestoreProof = serde_json::from_value(serde_json::json!({
            "restoreId": "10000000-0000-4000-8000-000000000007",
            "backupId": "10000000-0000-4000-8000-000000000008",
            "authenticatedManifestSha256": "5".repeat(64),
            "sourceDatabaseSystemId": "122", "sourceDatabaseOid": "16000",
            "targetDatabaseSystemId": "123", "targetDatabaseOid": "16001",
            "provenanceRowCount": 1, "provenanceTableSha256": "6".repeat(64)
        })).unwrap();
        prior.restore = Some(Restore {
            id: "10000000-0000-4000-8000-000000000007".into(),
            backup_id: "10000000-0000-4000-8000-000000000008".into(),
            archive_manifest_sha256: "5".repeat(64),
            target_system_identifier: "123".into(),
            provenance: None,
            carried_provenance: None,
        });
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
        prior.restore.as_mut().unwrap().provenance = Some(proof.clone());
        let mut other_root = pin(NEXT, target);
        other_root.physical.root_inode += 1;
        assert!(other_root.consume_furnishing_transition(&prior, transition).is_err());
        let mut unrelated = pin(NEXT, target);
        unrelated.producer_sha256 = source.1.into();
        assert!(unrelated.consume_furnishing_transition(&prior, transition).is_err());
        let mut next = pin(NEXT, target);
        next.consume_furnishing_transition(&prior, transition).unwrap();
        assert_eq!(next.owner, prior.owner);
        assert_eq!(next.physical, prior.physical);
        assert_eq!(next.restore, prior.restore);
        assert_eq!(next.previous_generation.as_deref(), Some(GENERATION));
        assert_eq!(Exchange::new(&next, b"transition lease").unwrap().restore_proof, Some(proof));
        let mut direct_restore = next.clone();
        direct_restore.admission_kind = Admission::VerifiedRestore;
        assert!(direct_restore.check().is_err());
        next.phase = Phase::Clean;
        let mut reopened = pin(LATER, target);
        reopened.consume(&next).unwrap();
        assert_eq!(reopened.restore, next.restore);
    }

    #[test]
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn terminal_adoption_transition_preserves_none_or_exact_carried_restore() {
        let source = crate::native_payload::CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET;
        let target = crate::native_payload::CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        let pin = |generation: &str, tuple: (&str, &str, &str)| {
            let mut value = binding(generation);
            value.catalog = 45;
            value.source_commit = tuple.0.into();
            value.producer_sha256 = tuple.1.into();
            value.payload_manifest_sha256 = tuple.2.into();
            value
        };
        let mut prior = pin(GENERATION, source);
        prior.phase = Phase::Clean;
        prior.admission_kind = Admission::CleanReopen;
        prior.previous_generation = Some("10000000-0000-4000-8000-000000000004".into());
        let mut unrestored = pin(NEXT, target);
        unrestored.consume_furnishing_transition(&prior, transition).unwrap();
        assert!(unrestored.restore.is_none());
        assert_eq!(unrestored.owner, prior.owner);
        assert_eq!(unrestored.physical, prior.physical);
        let observation = serde_json::json!({
            "version":1,"sourceHandoffSha256":"60ed3615a4072856a1cb1449fa532acd307d8c237a09095b356566db4ed90b88",
            "priorRestoreProof":{
                "restoreId":"10000000-0000-4000-8000-000000000007",
                "backupId":"10000000-0000-4000-8000-000000000008",
                "authenticatedManifestSha256":"c".repeat(64),
                "sourceDatabaseSystemId":"110","sourceDatabaseOid":"16000",
                "targetDatabaseSystemId":"220","targetDatabaseOid":"16002",
                "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
            },
            "archiveSourceDatabaseSystemId":"220","archiveSourceDatabaseOid":"16002",
            "targetDatabaseSystemId":"330","targetDatabaseOid":"16004",
            "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
        });
        prior.restore = Some(Restore {
            id:"10000000-0000-4000-8000-000000000009".into(),
            backup_id:"10000000-0000-4000-8000-000000000010".into(),
            archive_manifest_sha256:"7a65e63f34d4bb036a346a611c8c9eacda4f02411caca320263e9f4b5ecb3e55".into(),
            target_system_identifier:"330".into(),provenance:None,
            carried_provenance:Some(serde_json::from_value(observation.clone()).unwrap()),
        });
        let mut restored = pin(NEXT, target);
        restored.consume_furnishing_transition(&prior, transition).unwrap();
        assert_eq!(restored.restore, prior.restore);
        assert_eq!(restored.owner, prior.owner);
        assert_eq!(restored.physical, prior.physical);
        restored.phase = Phase::Clean;
        let mut reopened = pin(LATER, target);
        reopened.consume(&restored).unwrap();
        assert_eq!(reopened.restore, prior.restore);
        let mut other_root = pin(NEXT, target);
        other_root.physical.root_inode += 1;
        assert!(other_root.consume_furnishing_transition(&prior, transition).is_err());
        let mut other_owner = pin(NEXT, target);
        other_owner.owner.principal_id = "10000000-0000-4000-8000-000000000011".into();
        assert!(other_owner.consume_furnishing_transition(&prior, transition).is_err());
        let mut forged = observation;
        forged["sourceHandoffSha256"] = "f".repeat(64).into();
        prior.restore.as_mut().unwrap().carried_provenance = Some(serde_json::from_value(forged).unwrap());
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
        prior.restore.as_mut().unwrap().carried_provenance = None;
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
    }

    #[test]
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn rendered_thumbnail_adoption_transition_preserves_none_or_exact_carried_restore() {
        let source = crate::native_payload::CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET;
        let target = crate::native_payload::CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        let pin = |generation: &str, tuple: (&str, &str, &str)| {
            let mut value = binding(generation);
            value.catalog = 45;
            value.source_commit = tuple.0.into();
            value.producer_sha256 = tuple.1.into();
            value.payload_manifest_sha256 = tuple.2.into();
            value
        };
        let mut prior = pin(GENERATION, source);
        prior.phase = Phase::Clean;
        prior.admission_kind = Admission::CleanReopen;
        prior.previous_generation = Some("10000000-0000-4000-8000-000000000004".into());
        let mut unrestored = pin(NEXT, target);
        unrestored.consume_furnishing_transition(&prior, transition).unwrap();
        assert!(unrestored.restore.is_none());
        assert_eq!(unrestored.owner, prior.owner);
        assert_eq!(unrestored.physical, prior.physical);
        let observation = serde_json::json!({
            "version":1,"sourceHandoffSha256":"60ed3615a4072856a1cb1449fa532acd307d8c237a09095b356566db4ed90b88",
            "priorRestoreProof":{
                "restoreId":"10000000-0000-4000-8000-000000000007",
                "backupId":"10000000-0000-4000-8000-000000000008",
                "authenticatedManifestSha256":"c".repeat(64),
                "sourceDatabaseSystemId":"110","sourceDatabaseOid":"16000",
                "targetDatabaseSystemId":"220","targetDatabaseOid":"16002",
                "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
            },
            "archiveSourceDatabaseSystemId":"220","archiveSourceDatabaseOid":"16002",
            "targetDatabaseSystemId":"330","targetDatabaseOid":"16004",
            "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
        });
        prior.restore = Some(Restore {
            id:"10000000-0000-4000-8000-000000000009".into(),
            backup_id:"10000000-0000-4000-8000-000000000010".into(),
            archive_manifest_sha256:"7a65e63f34d4bb036a346a611c8c9eacda4f02411caca320263e9f4b5ecb3e55".into(),
            target_system_identifier:"330".into(),provenance:None,
            carried_provenance:Some(serde_json::from_value(observation.clone()).unwrap()),
        });
        let mut restored = pin(NEXT, target);
        restored.consume_furnishing_transition(&prior, transition).unwrap();
        assert_eq!(restored.restore, prior.restore);
        assert_eq!(restored.owner, prior.owner);
        assert_eq!(restored.physical, prior.physical);
        restored.phase = Phase::Clean;
        let mut reopened = pin(LATER, target);
        reopened.consume(&restored).unwrap();
        assert_eq!(reopened.restore, prior.restore);
        let mut other_root = pin(NEXT, target);
        other_root.physical.root_inode += 1;
        assert!(other_root.consume_furnishing_transition(&prior, transition).is_err());
        let mut other_owner = pin(NEXT, target);
        other_owner.owner.principal_id = "10000000-0000-4000-8000-000000000011".into();
        assert!(other_owner.consume_furnishing_transition(&prior, transition).is_err());
        let mut forged = observation;
        forged["sourceHandoffSha256"] = "f".repeat(64).into();
        prior.restore.as_mut().unwrap().carried_provenance = Some(serde_json::from_value(forged).unwrap());
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
        prior.restore.as_mut().unwrap().carried_provenance = None;
        assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
    }

    #[test]
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn canonical_thumbnail_adoption_transition_preserves_none_or_exact_carried_restore() {
        for (source, target) in [
            (crate::native_payload::CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET,
             crate::native_payload::CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET),
            (crate::native_payload::CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET,
             crate::native_payload::CATALOG45_PORTAL_HAYES_CARD_TARGET),
            (crate::native_payload::CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET,
             crate::native_payload::CATALOG45_SCENE_AUTHORING_TARGET),
            (crate::native_payload::CATALOG45_PORTAL_HAYES_CARD_TARGET,
             crate::native_payload::CATALOG45_SCENE_AUTHORING_TARGET),
            (crate::native_payload::CATALOG45_SCENE_AUTHORING_TARGET,
             crate::native_payload::CATALOG45_THUMBNAIL_FRESHNESS_TARGET),
        ] {
            let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
            let pin = |generation: &str, tuple: (&str, &str, &str)| {
                let mut value = binding(generation);
                value.catalog = 45;
                value.source_commit = tuple.0.into();
                value.producer_sha256 = tuple.1.into();
                value.payload_manifest_sha256 = tuple.2.into();
                value
            };
            let mut prior = pin(GENERATION, source);
            prior.phase = Phase::Clean;
            prior.admission_kind = Admission::CleanReopen;
            prior.previous_generation = Some("10000000-0000-4000-8000-000000000004".into());
            let mut unrestored = pin(NEXT, target);
            unrestored.consume_furnishing_transition(&prior, transition).unwrap();
            assert!(unrestored.restore.is_none());
            assert_eq!(unrestored.owner, prior.owner);
            assert_eq!(unrestored.physical, prior.physical);
            let observation = serde_json::json!({
                "version":1,"sourceHandoffSha256":"60ed3615a4072856a1cb1449fa532acd307d8c237a09095b356566db4ed90b88",
                "priorRestoreProof":{
                    "restoreId":"10000000-0000-4000-8000-000000000007",
                    "backupId":"10000000-0000-4000-8000-000000000008",
                    "authenticatedManifestSha256":"c".repeat(64),
                    "sourceDatabaseSystemId":"110","sourceDatabaseOid":"16000",
                    "targetDatabaseSystemId":"220","targetDatabaseOid":"16002",
                    "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
                },
                "archiveSourceDatabaseSystemId":"220","archiveSourceDatabaseOid":"16002",
                "targetDatabaseSystemId":"330","targetDatabaseOid":"16004",
                "provenanceRowCount":1,"provenanceTableSha256":"d".repeat(64)
            });
            prior.restore = Some(Restore {
                id:"10000000-0000-4000-8000-000000000009".into(),
                backup_id:"10000000-0000-4000-8000-000000000010".into(),
                archive_manifest_sha256:"7a65e63f34d4bb036a346a611c8c9eacda4f02411caca320263e9f4b5ecb3e55".into(),
                target_system_identifier:"330".into(),provenance:None,
                carried_provenance:Some(serde_json::from_value(observation.clone()).unwrap()),
            });
            let mut restored = pin(NEXT, target);
            restored.consume_furnishing_transition(&prior, transition).unwrap();
            assert_eq!(restored.restore, prior.restore);
            assert_eq!(restored.owner, prior.owner);
            assert_eq!(restored.physical, prior.physical);
            restored.phase = Phase::Clean;
            let mut reopened = pin(LATER, target);
            reopened.consume(&restored).unwrap();
            assert_eq!(reopened.restore, prior.restore);
            let mut other_root = pin(NEXT, target);
            other_root.physical.root_inode += 1;
            assert!(other_root.consume_furnishing_transition(&prior, transition).is_err());
            let mut other_owner = pin(NEXT, target);
            other_owner.owner.principal_id = "10000000-0000-4000-8000-000000000011".into();
            assert!(other_owner.consume_furnishing_transition(&prior, transition).is_err());
            let mut forged = observation;
            forged["sourceHandoffSha256"] = "f".repeat(64).into();
            prior.restore.as_mut().unwrap().carried_provenance = Some(serde_json::from_value(forged).unwrap());
            assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
            prior.restore.as_mut().unwrap().carried_provenance = None;
            assert!(pin(NEXT, target).consume_furnishing_transition(&prior, transition).is_err());
        }
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
            carried_provenance: None,
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
