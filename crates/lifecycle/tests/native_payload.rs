// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::native_payload::Payload;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

fn fixture() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("exhibitos-native-payload-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
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
        files.push(json!({"path":path,"bytes":path.len(),"sha256":format!("{:x}",Sha256::digest(path.as_bytes())),"executable":executable}));
    }
    let manifest = json!({"format":"exhibitos-native-runtime-v1","mode":"local-validation","platform":"darwin-arm64","nodeVersion":"24.21.0","postgresVersion":"18.6","platformVersion":"0.1.0","protocolVersion":"1","sourceCommit":"a".repeat(40),"producerSHA256":"a".repeat(64),"entry":{"node":"bin/node","script":"platform/scripts/local-runtime.mjs","postgres":"postgres/bin/postgres","initdb":"postgres/bin/initdb"},"files":files,"redistributionQualified":false,"officialSigned":false,"remaining":"Synthetic byte-binding fixture; never executed."});
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    root
}
#[test]
fn bytes_and_executable_identity_are_checked_without_distribution_claims() {
    let root = fixture();
    let payload = Payload::open(&root).unwrap();
    let context = payload.context();
    assert_eq!(context.state, "bytes_verified");
    assert!(!context.official_signed);
    assert!(!context.redistribution_qualified);
    fs::write(root.join("bin/unlisted"), "unlisted executable candidate").unwrap();
    assert!(Payload::open(&root).is_err());
    fs::remove_file(root.join("bin/unlisted")).unwrap();
    fs::write(root.join("bin/node"), "changed!").unwrap();
    assert!(Payload::open(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn backup_capability_requires_bundled_exact_tools_and_preserves_older_payloads() {
    let root = fixture();
    assert_eq!(
        Payload::open(&root)
            .unwrap()
            .backup_resources()
            .err()
            .unwrap()
            .code,
        "NATIVE_BACKUP_UNAVAILABLE"
    );
    let manifest_path = root.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    for (path, executable) in [
        ("platform/scripts/service-backup.mjs", false),
        ("platform/scripts/native-service-backup.mjs", false),
        ("postgres/bin/pg_dump", true),
        ("postgres/bin/pg_restore", true),
    ] {
        let full = root.join(path);
        fs::write(&full, path).unwrap();
        fs::set_permissions(
            &full,
            fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )
        .unwrap();
        manifest["files"].as_array_mut().unwrap().push(json!({
            "path":path,"bytes":path.len(),
            "sha256":format!("{:x}",Sha256::digest(path.as_bytes())),
            "executable":executable
        }));
    }
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let resources = Payload::open(&root).unwrap().backup_resources().unwrap();
    assert_eq!(
        resources.script,
        root.join("platform/scripts/native-service-backup.mjs")
    );
    assert_eq!(resources.pg_dump, root.join("postgres/bin/pg_dump"));
    assert_eq!(resources.pg_restore, root.join("postgres/bin/pg_restore"));
    // A regular, hashed file without executable permission is still not a tool.
    let restore = manifest["files"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    restore["executable"] = json!(false);
    fs::set_permissions(&resources.pg_restore, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        Payload::open(&root)
            .unwrap()
            .backup_resources()
            .err()
            .unwrap()
            .code,
        "NATIVE_BACKUP_UNAVAILABLE"
    );
    // Replacing admitted bytes fails payload admission before maintenance.
    fs::write(&resources.pg_dump, b"foreign tool").unwrap();
    assert!(Payload::open(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn external_links_and_traversal_are_refused() {
    let root = fixture();
    let external = root.with_extension("outside");
    fs::write(&external, "bin/node").unwrap();
    fs::remove_file(root.join("bin/node")).unwrap();
    symlink(&external, root.join("bin/node")).unwrap();
    assert!(Payload::open(&root).is_err());
    fs::remove_file(external).unwrap();
    fs::remove_dir_all(root).unwrap();
    let root = fixture();
    let path = root.join("manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    m["files"][0]["path"] = json!("../outside");
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(Payload::open(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn release_flags_cannot_substitute_for_actual_signed_distribution_admission() {
    let root = fixture();
    let path = root.join("manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    m["mode"] = json!("release");
    m["officialSigned"] = json!(true);
    m["redistributionQualified"] = json!(true);
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert_eq!(
        Payload::open(&root).err().unwrap().code,
        "NATIVE_RUNTIME_APP_REQUIRED"
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "Requires the exact operator-owned packaged runtime via EXHIBITOS_NATIVE_TEST_PAYLOAD"]
fn actual_packaged_runtime_bytes_are_admitted() {
    let path = std::env::var_os("EXHIBITOS_NATIVE_TEST_PAYLOAD")
        .expect("explicit isolated payload required");
    let payload = Payload::open(&PathBuf::from(path)).unwrap();
    assert_eq!(payload.context().state, "bytes_verified");
    assert!(payload.node().is_file());
    assert!(payload.postgres().is_file());
}
