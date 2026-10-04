// SPDX-License-Identifier: Apache-2.0
//! Offline signed runtime release verification. No trust-on-first-use, download,
//! policy persistence, migration, engine mutation or backup attestation.
use crate::update::{Plan, Preflight};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

pub const DOMAIN: &[u8] = b"ExhibitOS-runtime-release-v1\0";
pub const MAX_ENVELOPE: usize = 32 * 1024;
pub const MAX_PAYLOAD: usize = 12 * 1024;
const MAX_ARTIFACT: u64 = 64 * 1024 * 1024 * 1024;
const MAX_LIFETIME: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidPolicy,
    InvalidEnvelope,
    UntrustedKey,
    InvalidSignature,
    InvalidRelease,
    PolicyMismatch,
    Expired,
    Replay,
    ArtifactMismatch,
    ArtifactUnavailable,
    PlanMismatch,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy => "UPDATE_POLICY_INVALID",
            Self::InvalidEnvelope => "UPDATE_ENVELOPE_INVALID",
            Self::UntrustedKey => "UPDATE_KEY_UNTRUSTED",
            Self::InvalidSignature => "UPDATE_SIGNATURE_INVALID",
            Self::InvalidRelease => "UPDATE_RELEASE_INVALID",
            Self::PolicyMismatch => "UPDATE_POLICY_MISMATCH",
            Self::Expired => "UPDATE_RELEASE_EXPIRED",
            Self::Replay => "UPDATE_RELEASE_REPLAY",
            Self::ArtifactMismatch => "UPDATE_ARTIFACT_MISMATCH",
            Self::ArtifactUnavailable => "UPDATE_ARTIFACT_UNAVAILABLE",
            Self::PlanMismatch => "UPDATE_PLAN_MISMATCH",
        }
    }
}

/// Provision separately from the feed in trusted private storage. Accepted
/// sequence/time floors must be advanced durably after complete verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub format: u8,
    pub channel: String,
    pub target: String,
    pub protocol_version: u32,
    pub source_schema_sha256: String,
    pub minimum_sequence: u64,
    pub minimum_issued_at: u64,
    pub public_keys: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub runtime_image_sha256: String,
    pub schema_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub format: u8,
    pub product: String,
    pub channel: String,
    pub target: String,
    pub version: String,
    pub sequence: u64,
    /// UTC Unix seconds, not milliseconds. Trusted caller supplies current time.
    pub issued_at: u64,
    pub expires_at: u64,
    pub protocol_version: u32,
    pub source_schemas: Vec<String>,
    pub artifact: Artifact,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub format: u8,
    pub algorithm: String,
    /// SHA256 of the pinned raw 32-byte public key, never a feed-supplied key.
    pub key_id: String,
    /// Sign DOMAIN || u64be(UTF8 length) || exact UTF8 payload bytes.
    pub payload: String,
    pub signature: String,
}

/// Not Deserialize: network objects cannot directly claim verification.
#[derive(Debug)]
pub struct VerifiedRelease {
    release: Release,
    key_id: String,
    payload_sha256: String,
    source_schema_sha256: String,
    artifact_verified: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReceipt<'a> {
    pub operation: &'static str,
    pub version: &'a str,
    pub sequence: u64,
    pub key_id: &'a str,
    pub payload_sha256: &'a str,
    pub artifact_sha256: &'a str,
    pub artifact_bytes: u64,
    pub signature_verified: bool,
    pub artifact_verified: bool,
    pub backup_restore_verified: bool,
    pub activated: bool,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let nibble = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        out[i] = nibble(chunk[0]) * 16 + nibble(chunk[1]);
    }
    Some(out)
}
fn channel(s: &str) -> bool {
    matches!(s, "development" | "stable")
}
fn target(s: &str) -> bool {
    matches!(s, "linux-arm64" | "linux-amd64")
}
fn version(s: &str) -> bool {
    if s.len() > 64 {
        return false;
    }
    let (core, pre) = s.split_once('-').map_or((s, None), |(c, p)| (c, Some(p)));
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && (p.len() == 1 || !p.starts_with('0'))
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u32>().is_ok()
        })
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.split('.').all(|s| {
                    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
        })
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn message(payload: &str) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    m.extend_from_slice(payload.as_bytes());
    m
}
impl Policy {
    fn keys(&self) -> Result<Vec<(String, VerifyingKey)>, Error> {
        if self.format != 1
            || !channel(&self.channel)
            || !target(&self.target)
            || self.protocol_version != 1
            || hex::<32>(&self.source_schema_sha256).is_none()
            || self.public_keys.is_empty()
            || self.public_keys.len() > 8
        {
            return Err(Error::InvalidPolicy);
        }
        let mut keys = Vec::new();
        for raw in &self.public_keys {
            let b = hex::<32>(raw).ok_or(Error::InvalidPolicy)?;
            let k = VerifyingKey::from_bytes(&b).map_err(|_| Error::InvalidPolicy)?;
            let id = hash(&b);
            if k.is_weak() || keys.iter().any(|(old, _)| old == &id) {
                return Err(Error::InvalidPolicy);
            }
            keys.push((id, k));
        }
        Ok(keys)
    }
}
pub fn verify(bytes: &[u8], policy: &Policy, now: u64) -> Result<VerifiedRelease, Error> {
    let keys = policy.keys()?;
    if bytes.is_empty() || bytes.len() > MAX_ENVELOPE {
        return Err(Error::InvalidEnvelope);
    }
    let e: Envelope = serde_json::from_slice(bytes).map_err(|_| Error::InvalidEnvelope)?;
    if e.format != 1
        || e.algorithm != "ed25519"
        || e.payload.is_empty()
        || e.payload.len() > MAX_PAYLOAD
        || hex::<32>(&e.key_id).is_none()
    {
        return Err(Error::InvalidEnvelope);
    }
    let key = keys
        .iter()
        .find(|(id, _)| id == &e.key_id)
        .ok_or(Error::UntrustedKey)?;
    let sig = hex::<64>(&e.signature).ok_or(Error::InvalidSignature)?;
    key.1
        .verify_strict(&message(&e.payload), &Signature::from_bytes(&sig))
        .map_err(|_| Error::InvalidSignature)?;
    let release: Release = serde_json::from_str(&e.payload).map_err(|_| Error::InvalidRelease)?;
    if release.format != 1
        || release.product != "ExhibitOS/runtime"
        || !channel(&release.channel)
        || !target(&release.target)
        || !version(&release.version)
        || release.sequence == 0
        || release.expires_at <= release.issued_at
        || release.expires_at - release.issued_at > MAX_LIFETIME
        || release.source_schemas.is_empty()
        || release.source_schemas.len() > 16
        || release
            .source_schemas
            .iter()
            .any(|s| hex::<32>(s).is_none())
        || release.source_schemas.windows(2).any(|w| w[0] >= w[1])
        || !name(&release.artifact.name)
        || release.artifact.bytes == 0
        || release.artifact.bytes > MAX_ARTIFACT
        || hex::<32>(&release.artifact.sha256).is_none()
        || hex::<32>(&release.artifact.runtime_image_sha256).is_none()
        || hex::<32>(&release.artifact.schema_sha256).is_none()
        || release.protocol_version != 1
    {
        return Err(Error::InvalidRelease);
    }
    if release.channel != policy.channel
        || release.target != policy.target
        || release.protocol_version != policy.protocol_version
        || !release
            .source_schemas
            .contains(&policy.source_schema_sha256)
        || policy.channel == "stable" && release.version.contains('-')
    {
        return Err(Error::PolicyMismatch);
    }
    if release.sequence <= policy.minimum_sequence || release.issued_at < policy.minimum_issued_at {
        return Err(Error::Replay);
    }
    if now < release.issued_at || now >= release.expires_at {
        return Err(Error::Expired);
    }
    Ok(VerifiedRelease {
        release,
        key_id: e.key_id,
        payload_sha256: hash(e.payload.as_bytes()),
        source_schema_sha256: policy.source_schema_sha256.clone(),
        artifact_verified: false,
    })
}
impl VerifiedRelease {
    pub fn release(&self) -> &Release {
        &self.release
    }
    /// Streaming size/hash check. Failed/repeated checks clear previous proof.
    /// Caller must hold a stable file handle until import and verify its identity;
    /// passing this check does not import or authenticate runtime bundle internals.
    pub fn verify_artifact(&mut self, source: &mut impl Read) -> Result<(), Error> {
        self.artifact_verified = false;
        let mut h = Sha256::new();
        let mut n = 0u64;
        let mut b = [0u8; 65536];
        loop {
            let got = source
                .read(&mut b)
                .map_err(|_| Error::ArtifactUnavailable)?;
            if got == 0 {
                break;
            }
            n = n
                .checked_add(got as u64)
                .filter(|n| *n <= self.release.artifact.bytes)
                .ok_or(Error::ArtifactMismatch)?;
            h.update(&b[..got]);
        }
        if n != self.release.artifact.bytes
            || format!("{:x}", h.finalize()) != self.release.artifact.sha256
        {
            return Err(Error::ArtifactMismatch);
        }
        self.artifact_verified = true;
        Ok(())
    }
    pub fn receipt(&self) -> VerificationReceipt<'_> {
        VerificationReceipt {
            operation: "signed-runtime-release-verified-not-activated",
            version: &self.release.version,
            sequence: self.release.sequence,
            key_id: &self.key_id,
            payload_sha256: &self.payload_sha256,
            artifact_sha256: &self.release.artifact.sha256,
            artifact_bytes: self.release.artifact.bytes,
            signature_verified: true,
            artifact_verified: self.artifact_verified,
            backup_restore_verified: false,
            activated: false,
        }
    }
    fn binds(&self, p: &Plan) -> bool {
        p.target_image == self.release.artifact.runtime_image_sha256
            && p.target_schema == self.release.artifact.schema_sha256
            && p.source_schema == self.source_schema_sha256
    }
    /// Supplies only actual signature/artifact observations to an existing plan.
    /// Compatibility, source inventory, backup/restore, space and rollback flags
    /// remain the responsibility of independently verified trusted adapters.
    pub fn bind_preflight(&self, evidence: &mut Preflight, now: u64) -> Result<(), Error> {
        evidence.signature_verified = false;
        evidence.artifact_verified = false;
        if now < self.release.issued_at || now >= self.release.expires_at {
            return Err(Error::Expired);
        }
        if !self.artifact_verified || !self.binds(&evidence.plan) {
            return Err(Error::PlanMismatch);
        }
        evidence.signature_verified = true;
        evidence.artifact_verified = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn fixture() -> (SigningKey, Policy, Release) {
        // Public test seed only, never a deployed signing root.
        let k = SigningKey::from_bytes(&[31; 32]);
        let policy = Policy {
            format: 1,
            channel: "development".into(),
            target: "linux-arm64".into(),
            protocol_version: 1,
            source_schema_sha256: "a".repeat(64),
            minimum_sequence: 10,
            minimum_issued_at: 100,
            public_keys: vec![
                k.verifying_key()
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            ],
        };
        let release = Release {
            format: 1,
            product: "ExhibitOS/runtime".into(),
            channel: policy.channel.clone(),
            target: policy.target.clone(),
            version: "0.2.0-dev.1".into(),
            sequence: 11,
            issued_at: 100,
            expires_at: 200,
            protocol_version: 1,
            source_schemas: vec![policy.source_schema_sha256.clone()],
            artifact: Artifact {
                name: "runtime.tar".into(),
                bytes: 7,
                sha256: hash(b"fixture"),
                runtime_image_sha256: "b".repeat(64),
                schema_sha256: "c".repeat(64),
            },
        };
        (k, policy, release)
    }
    fn seal_payload(k: &SigningKey, payload: String) -> Vec<u8> {
        let e = Envelope {
            format: 1,
            algorithm: "ed25519".into(),
            key_id: hash(k.verifying_key().as_bytes()),
            signature: k
                .sign(&message(&payload))
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            payload,
        };
        serde_json::to_vec(&e).unwrap()
    }
    fn seal(k: &SigningKey, r: &Release) -> Vec<u8> {
        seal_payload(k, serde_json::to_string(r).unwrap())
    }
    #[test]
    fn exact_signature_artifact_and_preflight_do_not_attest_backup_or_activation() {
        let (k, p, mut r) = fixture();
        r.source_schemas.push("d".repeat(64));
        let mut v = verify(&seal(&k, &r), &p, 150).unwrap();
        assert!(!v.receipt().artifact_verified);
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let plan = Plan {
            operation_id: "update-1".into(),
            source_instance: "old".into(),
            target_instance: "new".into(),
            source_image: "d".repeat(64),
            target_image: r.artifact.runtime_image_sha256.clone(),
            source_schema: p.source_schema_sha256.clone(),
            target_schema: r.artifact.schema_sha256.clone(),
            backup_id: "backup-1".into(),
            backup_manifest: "e".repeat(64),
            source_inventory: "f".repeat(64),
            required_free_bytes: 100,
        };
        let mut e = Preflight {
            plan: plan.clone(),
            signature_verified: false,
            artifact_verified: false,
            compatibility_verified: false,
            backup_restore_verified: false,
            current_source_matches_backup: false,
            available_free_bytes: 0,
            image_only_rollback_verified: false,
        };
        v.bind_preflight(&mut e, 150).unwrap();
        assert!(e.signature_verified && e.artifact_verified);
        assert!(
            !e.backup_restore_verified
                && !e.compatibility_verified
                && !e.current_source_matches_backup
                && !e.image_only_rollback_verified
        );
        let mut u = crate::update::Update::new(plan).unwrap();
        assert_eq!(
            u.begin_update(e.clone()),
            Err(crate::update::Error::CompatibilityUnverified)
        );
        e.plan.target_schema = "d".repeat(64);
        assert_eq!(v.bind_preflight(&mut e, 150), Err(Error::PlanMismatch));
        assert!(!e.signature_verified && !e.artifact_verified);
        e.plan.target_schema = r.artifact.schema_sha256.clone();
        e.plan.source_schema = "d".repeat(64);
        assert_eq!(v.bind_preflight(&mut e, 150), Err(Error::PlanMismatch));
        assert!(!e.signature_verified && !e.artifact_verified);
        assert!(!v.receipt().activated);
        assert_eq!(v.bind_preflight(&mut e, 200), Err(Error::Expired));
    }
    #[test]
    fn wrong_key_tamper_domain_and_noncanonical_signature_reject() {
        let (k, p, r) = fixture();
        let bytes = seal(&k, &r);
        let other = SigningKey::from_bytes(&[32; 32]);
        assert_eq!(
            verify(&seal(&other, &r), &p, 150).unwrap_err(),
            Error::UntrustedKey
        );
        let mut e: Envelope = serde_json::from_slice(&bytes).unwrap();
        e.payload.push(' ');
        assert_eq!(
            verify(&serde_json::to_vec(&e).unwrap(), &p, 150).unwrap_err(),
            Error::InvalidSignature
        );
        e.payload = serde_json::to_string(&r).unwrap();
        e.signature = k
            .sign(e.payload.as_bytes())
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            verify(&serde_json::to_vec(&e).unwrap(), &p, 150).unwrap_err(),
            Error::InvalidSignature
        );
        e.signature = "F".repeat(128);
        assert_eq!(
            verify(&serde_json::to_vec(&e).unwrap(), &p, 150).unwrap_err(),
            Error::InvalidSignature
        );
    }
    #[test]
    fn replay_expiry_future_and_lifetime_reject() {
        let (k, p, r) = fixture();
        for now in [99, 200, 201] {
            assert_eq!(verify(&seal(&k, &r), &p, now).unwrap_err(), Error::Expired);
        }
        let mut q = p.clone();
        q.minimum_sequence = 11;
        assert_eq!(verify(&seal(&k, &r), &q, 150).unwrap_err(), Error::Replay);
        q = p.clone();
        q.minimum_issued_at = 101;
        assert_eq!(verify(&seal(&k, &r), &q, 150).unwrap_err(), Error::Replay);
        let mut r = r;
        r.expires_at = r.issued_at + MAX_LIFETIME + 1;
        assert_eq!(
            verify(&seal(&k, &r), &p, 150).unwrap_err(),
            Error::InvalidRelease
        );
    }
    #[test]
    fn closed_schema_duplicate_fields_path_escape_and_policy_binding_reject() {
        let (k, p, r) = fixture();
        let payload = serde_json::to_string(&r).unwrap();
        for raw in [
            payload.replacen('{', "{\"format\":1,", 1),
            payload.replacen('{', "{\"extra\":true,", 1),
        ] {
            assert_eq!(
                verify(&seal_payload(&k, raw), &p, 150).unwrap_err(),
                Error::InvalidRelease
            );
        }
        let mut q = p.clone();
        q.channel = "stable".into();
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::PolicyMismatch
        );
        q = p.clone();
        q.target = "linux-amd64".into();
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::PolicyMismatch
        );
        q = p.clone();
        q.source_schema_sha256 = "d".repeat(64);
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::PolicyMismatch
        );
        for path in ["../runtime.tar", "/runtime.tar", "a\\b", "a:b", ".."] {
            let mut bad = r.clone();
            bad.artifact.name = path.into();
            assert_eq!(
                verify(&seal(&k, &bad), &p, 150).unwrap_err(),
                Error::InvalidRelease
            );
        }
        let mut bad = r.clone();
        bad.source_schemas.push(bad.source_schemas[0].clone());
        assert_eq!(
            verify(&seal(&k, &bad), &p, 150).unwrap_err(),
            Error::InvalidRelease
        );
    }
    #[test]
    fn weak_duplicate_revoked_keys_and_envelope_bounds_refuse() {
        let (k, p, r) = fixture();
        let mut q = p.clone();
        q.public_keys.push(q.public_keys[0].clone());
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::InvalidPolicy
        );
        q = p.clone();
        q.public_keys = vec!["0".repeat(64)];
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::InvalidPolicy
        );
        q = p.clone();
        q.public_keys.clear();
        assert_eq!(
            verify(&seal(&k, &r), &q, 150).unwrap_err(),
            Error::InvalidPolicy
        );
        assert_eq!(
            verify(&vec![b' '; MAX_ENVELOPE + 1], &p, 150).unwrap_err(),
            Error::InvalidEnvelope
        );
        let bytes = seal(&k, &r);
        assert_eq!(
            verify(&bytes[..bytes.len() - 1], &p, 150).unwrap_err(),
            Error::InvalidEnvelope
        );
    }
    #[test]
    fn artifact_size_hash_io_failure_and_recheck_clear_proof() {
        let (k, p, r) = fixture();
        let mut v = verify(&seal(&k, &r), &p, 150).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        assert!(v.receipt().artifact_verified);
        for mut data in [b"fixtur".as_slice(), b"fixtureX", b"changed"] {
            assert_eq!(v.verify_artifact(&mut data), Err(Error::ArtifactMismatch));
            assert!(!v.receipt().artifact_verified);
        }
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic"))
            }
        }
        assert_eq!(
            v.verify_artifact(&mut Broken),
            Err(Error::ArtifactUnavailable)
        );
        assert!(!v.receipt().artifact_verified);
    }
}
