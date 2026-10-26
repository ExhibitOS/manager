// SPDX-License-Identifier: Apache-2.0
// Debug-only local-validation Payload fixtures reuse the existing payload and
// trust guards. No production signature bypass, process or database is used.
use super::*;
use crate::{native_payload::Payload, signed_release::native};
use ed25519_dalek::Signer;
use std::{os::unix::fs::PermissionsExt, sync::Arc};

fn payload(parent: &Path) -> (PathBuf, Arc<Payload>) {
    let root = parent.join("native-payload");
    fs::create_dir(&root).unwrap();
    let mut files = Vec::new();
    for (path, executable) in [
        ("bin/node", true),
        ("platform/scripts/local-runtime.mjs", false),
        ("postgres/bin/postgres", true),
        ("postgres/bin/initdb", true),
    ] {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, path).unwrap();
        fs::set_permissions(
            &full,
            fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )
        .unwrap();
        files.push(serde_json::json!({"path":path,"bytes":path.len(),"sha256":hash(path.as_bytes()),"executable":executable}));
    }
    let value = serde_json::json!({"format":"exhibitos-native-runtime-v1","mode":"local-validation","platform":"darwin-arm64","nodeVersion":"24.21.0","postgresVersion":"18.6","platformVersion":"0.1.0","protocolVersion":"1","sourceCommit":"a".repeat(40),"producerSHA256":"a".repeat(64),"entry":{"node":"bin/node","script":"platform/scripts/local-runtime.mjs","postgres":"postgres/bin/postgres","initdb":"postgres/bin/initdb"},"files":files,"redistributionQualified":false,"officialSigned":false,"remaining":"Existing synthetic byte-admission fixture. Never executed."});
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let admitted = Arc::new(Payload::open(&root).unwrap());
    (root, admitted)
}

pub(super) fn fixture() -> (
    PathBuf,
    Store,
    Arc<Payload>,
    Vec<u8>,
    native::StagedNativeArtifact,
) {
    let (profile, key, mut policy, oci_release) = super::tests::fixture();
    policy.format = 2;
    policy.target = "macos-arm64".into();
    let parent = profile.parent().unwrap();
    let (_, payload) = payload(parent);
    let release = native::NativeRelease {
        format: 2,
        kind: "native-payload".into(),
        product: oci_release.product,
        channel: policy.channel.clone(),
        target: policy.target.clone(),
        version: oci_release.version,
        sequence: oci_release.sequence,
        issued_at: oci_release.issued_at,
        expires_at: oci_release.expires_at,
        protocol_version: 1,
        source_schemas: oci_release.source_schemas,
        artifact: native::NativeArtifact {
            name: "native-runtime.tar".into(),
            bytes: 7,
            sha256: hash(b"fixture"),
            native_manifest_sha256: payload.context().manifest_sha256,
            schema_sha256: "c".repeat(64),
        },
    };
    let text = serde_json::to_string(&release).unwrap();
    let envelope = serde_json::to_vec(&Envelope {
        format: 1,
        algorithm: "ed25519".into(),
        key_id: hash(key.verifying_key().as_bytes()),
        signature: key
            .sign(&message(&text))
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        payload: text,
    })
    .unwrap();
    let store = Store::provision(&profile, "default", policy, 10).unwrap();
    let source = parent.join(&release.artifact.name);
    fs::write(&source, b"fixture").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
    let mut verified = store.verify_native(&envelope, 20).unwrap();
    let staged = native::stage(&mut verified, &source, parent, 20).unwrap();
    (profile, store, payload, envelope, staged)
}

#[test]
fn native_artifact_admission_is_read_only_and_never_attests_schema_or_execution() {
    let (profile, store, payload, envelope, staged) = fixture();
    let before = store.current_sha256.clone();
    let generation = store.receipt().generation;
    let mut admitted = store
        .admit_native_payload(&envelope, staged, payload, 20)
        .unwrap();
    store.recheck_native_admission(&mut admitted, 21).unwrap();
    let receipt = serde_json::to_value(admitted.receipt()).unwrap();
    assert_eq!(receipt["artifactAndPayloadAdmitted"], true);
    assert_eq!(receipt["schemaObserved"], false);
    assert_eq!(receipt["executionPermitted"], false);
    assert_eq!(receipt["publisherSchemaSha256"], "c".repeat(64));
    assert_eq!(store.current_sha256, before);
    assert_eq!(store.receipt().generation, generation);
    assert!(store.intent().is_none());
    assert!(store.verify(&envelope, 20).is_err());
    drop(admitted);
    drop(store);
    fs::remove_dir_all(profile.parent().unwrap()).unwrap();
}

#[test]
fn stale_authority_or_selected_payload_clears_native_admission() {
    let (profile, mut store, payload, envelope, staged) = fixture();
    let mut admitted = store
        .admit_native_payload(&envelope, staged, payload, 20)
        .unwrap();
    let policy = store.policy().clone();
    store.replace_policy(policy, 1, 21).unwrap();
    assert_eq!(
        store
            .recheck_native_admission(&mut admitted, 22)
            .unwrap_err(),
        Error::TrustStalePolicy
    );
    assert_eq!(
        serde_json::to_value(admitted.receipt()).unwrap()["artifactAndPayloadAdmitted"],
        false
    );
    drop(admitted);
    drop(store);
    fs::remove_dir_all(profile.parent().unwrap()).unwrap();

    let (profile, store, payload, envelope, staged) = fixture();
    let mut admitted = store
        .admit_native_payload(&envelope, staged, payload, 20)
        .unwrap();
    fs::write(
        profile.parent().unwrap().join("native-payload/bin/node"),
        b"changed",
    )
    .unwrap();
    assert_eq!(
        store
            .recheck_native_admission(&mut admitted, 21)
            .unwrap_err(),
        Error::ArtifactMismatch
    );
    assert_eq!(
        serde_json::to_value(admitted.receipt()).unwrap()["artifactAndPayloadAdmitted"],
        false
    );
    drop(admitted);
    drop(store);
    fs::remove_dir_all(profile.parent().unwrap()).unwrap();
}

#[test]
fn different_fresh_payload_manifest_is_not_the_signed_selected_payload() {
    let (profile, store, _, envelope, staged) = fixture();
    let parent = profile.parent().unwrap();
    let path = parent.join("native-payload/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["remaining"] =
        serde_json::json!("Different independently valid Payload; not the signed manifest.");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let different = Arc::new(Payload::open(path.parent().unwrap()).unwrap());
    assert!(matches!(
        store.admit_native_payload(&envelope, staged, different, 20),
        Err(Error::ArtifactMismatch)
    ));
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

// One explicit opt-in entry in this existing TEST authority/admission target.
#[path = "native_r5_copy_actual_tests.rs"]
mod r5_copy_actual;
