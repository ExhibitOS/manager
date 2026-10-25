// SPDX-License-Identifier: Apache-2.0
//! Additive native artifact admission in the existing release signature/trust
//! domain. A publisher's schema digest is NOT an observed compatibility proof.
//! No native type converts to VerifiedRelease, Preflight or an OCI permit.
use super::*;
#[cfg(target_os = "macos")]
use crate::native_payload::Payload;
#[cfg(target_os = "macos")]
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeArtifact {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub native_manifest_sha256: String,
    pub schema_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeRelease {
    pub format: u8,
    pub kind: String,
    pub product: String,
    pub channel: String,
    pub target: String,
    pub version: String,
    pub sequence: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub protocol_version: u32,
    pub source_schemas: Vec<String>,
    pub artifact: NativeArtifact,
}

/// Cannot be deserialized or constructed from a feed's success flags.
#[derive(Debug)]
pub struct VerifiedNativeRelease {
    pub(super) release: NativeRelease,
    pub(super) key_id: String,
    pub(super) payload_sha256: String,
    pub(super) source_schema_sha256: String,
    artifact_verified: bool,
}

pub fn verify(bytes: &[u8], policy: &Policy, now: u64) -> Result<VerifiedNativeRelease, Error> {
    if policy.format != 2 || policy.target != "macos-arm64" {
        return Err(Error::InvalidPolicy);
    }
    let envelope = super::verified_envelope(bytes, policy)?;
    let release: NativeRelease =
        serde_json::from_str(&envelope.payload).map_err(|_| Error::InvalidRelease)?;
    if release.format != 2
        || release.kind != "native-payload"
        || release.product != "ExhibitOS/runtime"
        || !channel(&release.channel)
        || release.target != "macos-arm64"
        || !version(&release.version)
        || release.sequence == 0
        || release.expires_at <= release.issued_at
        || release.expires_at - release.issued_at > MAX_LIFETIME
        || release.protocol_version != 1
        || release.source_schemas.is_empty()
        || release.source_schemas.len() > 16
        || release
            .source_schemas
            .iter()
            .any(|v| hex::<32>(v).is_none())
        || release.source_schemas.windows(2).any(|v| v[0] >= v[1])
        || !name(&release.artifact.name)
        || release.artifact.bytes == 0
        || release.artifact.bytes > MAX_ARTIFACT
        || hex::<32>(&release.artifact.sha256).is_none()
        || hex::<32>(&release.artifact.native_manifest_sha256).is_none()
        || hex::<32>(&release.artifact.schema_sha256).is_none()
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
    Ok(VerifiedNativeRelease {
        release,
        key_id: envelope.key_id,
        payload_sha256: hash(envelope.payload.as_bytes()),
        source_schema_sha256: policy.source_schema_sha256.clone(),
        artifact_verified: false,
    })
}

impl VerifiedNativeRelease {
    pub fn release(&self) -> &NativeRelease {
        &self.release
    }
}

impl artifact::StageVerification for VerifiedNativeRelease {
    fn artifact_name(&self) -> &str {
        &self.release.artifact.name
    }
    fn artifact_bytes(&self) -> u64 {
        self.release.artifact.bytes
    }
    fn reset_artifact(&mut self) {
        self.artifact_verified = false;
    }
    fn valid_time(&self, now: u64) -> Result<(), Error> {
        if now < self.release.issued_at || now >= self.release.expires_at {
            Err(Error::Expired)
        } else {
            Ok(())
        }
    }
    fn verify_file(&mut self, file: &mut std::fs::File) -> Result<(), Error> {
        self.artifact_verified = false;
        verify_artifact_bytes(
            file,
            self.release.artifact.bytes,
            &self.release.artifact.sha256,
        )?;
        self.artifact_verified = true;
        Ok(())
    }
}

/// Retains the original signed declaration, including schemaSha256, so a staged
/// file cannot be reused under a different declaration with identical bytes.
/// Its inner OCI-shaped file guard is private and never exposed to executors.
pub struct StagedNativeArtifact {
    inner: artifact::StagedArtifact,
    payload_sha256: String,
    key_id: String,
    source_schema_sha256: String,
}

#[cfg(target_os = "macos")]
pub fn stage(
    release: &mut VerifiedNativeRelease,
    source: &Path,
    parent: &Path,
    now: u64,
) -> Result<StagedNativeArtifact, Error> {
    let inner = artifact::stage_bound(release, source, parent, now)?;
    Ok(StagedNativeArtifact {
        inner,
        payload_sha256: release.payload_sha256.clone(),
        key_id: release.key_id.clone(),
        source_schema_sha256: release.source_schema_sha256.clone(),
    })
}

impl StagedNativeArtifact {
    pub(super) fn reverify(
        &mut self,
        release: &mut VerifiedNativeRelease,
        now: u64,
    ) -> Result<(), Error> {
        release.artifact_verified = false;
        if self.payload_sha256 != release.payload_sha256
            || self.key_id != release.key_id
            || self.source_schema_sha256 != release.source_schema_sha256
        {
            return Err(Error::ArtifactMismatch);
        }
        self.inner.reverify_bound(release, now)
    }
}

/// Signature + stable staged bytes + freshly re-admitted selected Payload.
/// Read-only, not Clone/Deserialize and deliberately NOT an execution permit.
///
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::{VerifiedRelease, native::AdmittedNativePayload};
/// fn oci_only(_: &VerifiedRelease) {}
/// fn cannot_cross_use(native: &AdmittedNativePayload) { oci_only(native); }
/// ```
#[cfg(target_os = "macos")]
pub struct AdmittedNativePayload {
    pub(super) envelope: Vec<u8>,
    pub(super) release: VerifiedNativeRelease,
    pub(super) staged: StagedNativeArtifact,
    pub(super) payload: Arc<Payload>,
    pub(super) scope: String,
    pub(super) generation: u64,
    pub(super) policy_generation: u64,
    pub(super) authority_sha256: String,
    pub(super) admitted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAdmissionReceipt<'a> {
    operation: &'static str,
    version: &'a str,
    sequence: u64,
    artifact_sha256: &'a str,
    native_manifest_sha256: &'a str,
    publisher_schema_sha256: &'a str,
    artifact_and_payload_admitted: bool,
    schema_observed: bool,
    execution_permitted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn fixture() -> (SigningKey, Policy, NativeRelease) {
        // Existing public-test seed pattern, never a signing credential.
        let key = SigningKey::from_bytes(&[31; 32]);
        let policy = Policy {
            format: 2,
            channel: "development".into(),
            target: "macos-arm64".into(),
            protocol_version: 1,
            source_schema_sha256: "a".repeat(64),
            minimum_sequence: 10,
            minimum_issued_at: 100,
            public_keys: vec![
                key.verifying_key()
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            ],
        };
        let release = NativeRelease {
            format: 2,
            kind: "native-payload".into(),
            product: "ExhibitOS/runtime".into(),
            channel: policy.channel.clone(),
            target: policy.target.clone(),
            version: "0.2.0-dev.1".into(),
            sequence: 11,
            issued_at: 100,
            expires_at: 200,
            protocol_version: 1,
            source_schemas: vec![policy.source_schema_sha256.clone()],
            artifact: NativeArtifact {
                name: "native-runtime.tar".into(),
                bytes: 7,
                sha256: hash(b"fixture"),
                native_manifest_sha256: "b".repeat(64),
                schema_sha256: "c".repeat(64),
            },
        };
        (key, policy, release)
    }
    fn seal_value(key: &SigningKey, value: serde_json::Value) -> Vec<u8> {
        let payload = serde_json::to_string(&value).unwrap();
        let signature = key.sign(&message(&payload));
        serde_json::to_vec(&Envelope {
            format: 1,
            algorithm: "ed25519".into(),
            key_id: hash(&key.verifying_key().to_bytes()),
            payload,
            signature: signature
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
        .unwrap()
    }
    fn seal(key: &SigningKey, release: &NativeRelease) -> Vec<u8> {
        seal_value(key, serde_json::to_value(release).unwrap())
    }
    #[test]
    fn native_declaration_uses_existing_signature_domain_without_oci_cross_use() {
        let (key, policy, release) = fixture();
        let bytes = seal(&key, &release);
        let verified = verify(&bytes, &policy, 150).unwrap();
        assert_eq!(
            verified.release().artifact.native_manifest_sha256,
            "b".repeat(64)
        );
        assert!(!verified.artifact_verified);
        assert_eq!(
            super::super::verify(&bytes, &policy, 150).unwrap_err(),
            Error::InvalidPolicy
        );
        let mut oci_policy = policy.clone();
        oci_policy.format = 1;
        oci_policy.target = "linux-arm64".into();
        assert_eq!(
            verify(&bytes, &oci_policy, 150).unwrap_err(),
            Error::InvalidPolicy
        );
        assert!(super::super::verify(&bytes, &oci_policy, 150).is_err());
        let mut changed: Envelope = serde_json::from_slice(&bytes).unwrap();
        changed.payload.push(' ');
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &policy, 150).unwrap_err(),
            Error::InvalidSignature
        );
    }
    #[test]
    fn wrong_target_kind_fields_schema_and_stale_declarations_refuse() {
        let (key, policy, release) = fixture();
        for (field, value) in [
            ("format", serde_json::json!(1)),
            ("kind", serde_json::json!("oci")),
            ("target", serde_json::json!("linux-arm64")),
            ("officialSigned", serde_json::json!(true)),
        ] {
            let mut raw = serde_json::to_value(&release).unwrap();
            raw[field] = value;
            assert_eq!(
                verify(&seal_value(&key, raw), &policy, 150).unwrap_err(),
                Error::InvalidRelease
            );
        }
        let mut raw = serde_json::to_value(&release).unwrap();
        raw["artifact"]["runtimeImageSha256"] = serde_json::json!("d".repeat(64));
        assert_eq!(
            verify(&seal_value(&key, raw), &policy, 150).unwrap_err(),
            Error::InvalidRelease
        );
        let mut changed = release.clone();
        changed.artifact.schema_sha256 = "not-a-schema".into();
        assert_eq!(
            verify(&seal(&key, &changed), &policy, 150).unwrap_err(),
            Error::InvalidRelease
        );
        changed = release.clone();
        changed.source_schemas = vec!["d".repeat(64)];
        assert_eq!(
            verify(&seal(&key, &changed), &policy, 150).unwrap_err(),
            Error::PolicyMismatch
        );
        changed = release.clone();
        changed.sequence = 10;
        assert_eq!(
            verify(&seal(&key, &changed), &policy, 150).unwrap_err(),
            Error::Replay
        );
        assert_eq!(
            verify(&seal(&key, &release), &policy, 200).unwrap_err(),
            Error::Expired
        );
        assert_eq!(
            verify(&seal(&key, &release), &policy, 99).unwrap_err(),
            Error::Expired
        );
    }
    #[cfg(target_os = "macos")]
    fn private_directory() -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("native-release-admission-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::canonicalize(root).unwrap()
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn staged_native_bytes_keep_identity_and_signed_schema_declaration_bound() {
        use std::os::unix::fs::PermissionsExt;
        let (key, policy, release) = fixture();
        let root = private_directory();
        let input = root.join(&release.artifact.name);
        std::fs::write(&input, b"fixture").unwrap();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut verified = verify(&seal(&key, &release), &policy, 150).unwrap();
        let mut staged = stage(&mut verified, &input, &root, 150).unwrap();
        assert!(verified.artifact_verified);
        let mut changed = release.clone();
        changed.artifact.schema_sha256 = "d".repeat(64);
        let mut wrong = verify(&seal(&key, &changed), &policy, 150).unwrap();
        assert_eq!(
            staged.reverify(&mut wrong, 150).unwrap_err(),
            Error::ArtifactMismatch
        );
        assert!(!wrong.artifact_verified);
        let path = staged.inner.path().to_path_buf();
        std::fs::rename(&path, path.with_extension("preserved")).unwrap();
        std::fs::write(&path, b"fixture").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(
            staged.reverify(&mut verified, 150).unwrap_err(),
            Error::ArtifactMismatch
        );
        assert!(!verified.artifact_verified);
        // Only this small synthetic source and copies are removed.
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn staged_native_artifact_cannot_switch_its_pinned_source_schema_policy() {
        use std::os::unix::fs::PermissionsExt;
        let (key, policy, mut release) = fixture();
        release.source_schemas.push("d".repeat(64));
        let envelope = seal(&key, &release);
        let root = private_directory();
        let input = root.join(&release.artifact.name);
        std::fs::write(&input, b"fixture").unwrap();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut original = verify(&envelope, &policy, 150).unwrap();
        let mut staged = stage(&mut original, &input, &root, 150).unwrap();
        let mut changed_policy = policy;
        changed_policy.source_schema_sha256 = "d".repeat(64);
        let mut changed = verify(&envelope, &changed_policy, 150).unwrap();
        assert_eq!(
            staged.reverify(&mut changed, 150).unwrap_err(),
            Error::ArtifactMismatch
        );
        assert!(!changed.artifact_verified);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(target_os = "macos")]
impl AdmittedNativePayload {
    pub fn receipt(&self) -> NativeAdmissionReceipt<'_> {
        NativeAdmissionReceipt {
            operation: "native-artifact-admitted-not-execution",
            version: &self.release.release.version,
            sequence: self.release.release.sequence,
            artifact_sha256: &self.release.release.artifact.sha256,
            native_manifest_sha256: &self.release.release.artifact.native_manifest_sha256,
            publisher_schema_sha256: &self.release.release.artifact.schema_sha256,
            artifact_and_payload_admitted: self.admitted,
            schema_observed: false,
            execution_permitted: false,
        }
    }
}
