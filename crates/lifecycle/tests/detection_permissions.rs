// SPDX-License-Identifier: Apache-2.0
//! Real scoped subprocess EACCES proof; does not change host tools or VM state.
#![cfg(unix)]
use exhibitos_lifecycle::EngineProbe;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use uuid::Uuid;
#[test]
fn non_executable_path_fixture_reports_permission_without_touching_installed_engines() {
    let fixture =
        std::env::temp_dir().join(format!("exhibitos-detection-denied-{}", Uuid::new_v4()));
    let binaries = fixture.join("bin");
    fs::create_dir_all(&binaries).unwrap();
    for kind in ["podman", "docker"] {
        let file = binaries.join(kind);
        fs::write(&file, b"private non-executable fixture").unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let original = std::env::var_os("PATH");
    let output = Command::new(env!("CARGO_BIN_EXE_exhibitos-manager"))
        .env("PATH", &binaries)
        .args(["--root", fixture.join("state").to_str().unwrap(), "detect"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let probes: Vec<EngineProbe> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(probes.len(), 2);
    for probe in probes {
        assert!(probe.installed);
        assert!(!probe.available);
        assert_eq!(probe.error_code.as_deref(), Some("ENGINE_PERMISSION"));
        assert!(probe.guidance.unwrap().contains("권한"));
        assert!(probe.engine_version.is_none());
    }
    assert_eq!(std::env::var_os("PATH"), original);
}
