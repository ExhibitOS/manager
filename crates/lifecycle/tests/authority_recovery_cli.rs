// SPDX-License-Identifier: Apache-2.0
#![cfg(unix)]
use exhibitos_lifecycle::{installations::InstallationController, signed_release::Policy};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
    process::{Command, Output},
};
fn cli(profile: &Path, command: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exhibitos-update"))
        .args([command, "--profile"])
        .arg(profile)
        .args(["--installation", "default"])
        .args(extra)
        .arg("--apps-closed")
        .output()
        .unwrap()
}
fn success(out: Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn records(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let m = fs::symlink_metadata(&p).unwrap();
            assert!(m.is_file() && !m.is_symlink());
            assert_eq!(m.permissions().mode() & 0o777, 0o600);
            (
                p.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(p).unwrap(),
            )
        })
        .collect()
}
#[test]
fn native_cli_recovers_latest_authority_and_refuses_existing_or_stale_primary() {
    let parent =
        std::env::temp_dir().join(format!("exhibitos-authority-cli-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let profile = parent.join("profile");
    drop(InstallationController::new(profile.clone(), None).unwrap());
    let public_key = |byte| {
        ed25519_dalek::SigningKey::from_bytes(&[byte; 32])
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let mut policy = Policy {
        format: 1,
        channel: "development".into(),
        target: "linux-arm64".into(),
        protocol_version: 1,
        source_schema_sha256: "a".repeat(64),
        minimum_sequence: 1,
        minimum_issued_at: 10,
        public_keys: vec![public_key(31)],
    };
    let policy_path = parent.join("policy.json");
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let policy_arg = policy_path.to_str().unwrap();
    assert_eq!(
        success(cli(&profile, "trust-provision", &["--policy", policy_arg]))["generation"],
        1
    );
    let vault = parent.join("vault");
    let enrolled = success(cli(
        &profile,
        "enroll-authority-recovery",
        &["--vault", vault.to_str().unwrap()],
    ));
    assert_eq!(enrolled["generation"], 2);
    assert_eq!(enrolled["liveAuthorityRestored"], false);
    policy.public_keys = vec![public_key(32)];
    policy.minimum_sequence = 20;
    policy.minimum_issued_at = 20;
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let latest = success(cli(
        &profile,
        "trust-policy",
        &["--policy", policy_arg, "--expected-generation", "1"],
    ));
    assert_eq!(latest["generation"], 3);
    let root = fs::read_dir(&parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(".exhibitos-release-trust-")
        })
        .unwrap();
    let exact = records(&root);
    assert_eq!(exact.len(), 3);
    assert_eq!(exact, records(&vault.join("records")));
    let existing = cli(&profile, "restore-missing-authority", &[]);
    assert!(!existing.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&existing.stdout).unwrap()["code"],
        "UPDATE_TRUST_EXISTS"
    );
    assert_eq!(records(&root), exact);
    let last = root.join("00000000000000000003.json");
    let stale_retained = parent.join("retained-latest.json");
    fs::rename(&last, &stale_retained).unwrap();
    assert!(!cli(&profile, "trust-status", &[]).status.success());
    fs::rename(&stale_retained, &last).unwrap();
    // Replay an exact interrupted publication boundary from real CLI policy bytes:
    // the candidate is durable in the vault; primary and completion are retained
    // separately as crash evidence. Reconciliation selects no caller head.
    let completion = vault.join("completed/00000000000000000003.json");
    fs::rename(&completion, parent.join("retained-completion.json")).unwrap();
    fs::rename(&last, &stale_retained).unwrap();
    assert!(!cli(&profile, "trust-status", &[]).status.success());
    let reconciled = success(cli(&profile, "reconcile-authority", &[]));
    assert_eq!(reconciled["generation"], 3);
    assert_eq!(reconciled["primaryRecordPublished"], true);
    assert_eq!(reconciled["completionMarkerPublished"], true);
    assert_eq!(reconciled["updateExecuted"], false);
    assert_eq!(fs::read(&last).unwrap(), fs::read(&stale_retained).unwrap());
    assert_eq!(records(&root), exact);
    let no_op = success(cli(&profile, "reconcile-authority", &[]));
    assert_eq!(no_op["primaryRecordPublished"], false);
    assert_eq!(no_op["completionMarkerPublished"], false);
    let retained = parent.join("retained-original");
    fs::rename(&root, &retained).unwrap();
    assert!(
        !cli(&profile, "trust-provision", &["--policy", policy_arg])
            .status
            .success()
    );
    assert!(!root.exists());
    let restored = success(cli(&profile, "restore-missing-authority", &[]));
    assert_eq!(restored["generation"], 3);
    assert_eq!(restored["liveAuthorityRestored"], true);
    for field in ["hostRestored", "servicesRestored", "updateExecuted"] {
        assert_eq!(restored[field], false);
    }
    assert_eq!(records(&root), exact);
    assert_eq!(records(&retained), exact);
    assert_eq!(success(cli(&profile, "trust-status", &[])), latest);
    // Only this fresh, fully verified synthetic scope is retired; failures preserve it.
    fs::remove_dir_all(parent).unwrap();
}
