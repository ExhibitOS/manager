// SPDX-License-Identifier: Apache-2.0
//! Real native child-process fixtures; not evidence of actual Docker/Podman faults.
use super::*;
use std::sync::OnceLock;

fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let root =
                std::env::temp_dir().join(format!("exhibitos-engine-error-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let source = root.join("fixture.rs");
            fs::write(
                &source,
                r#"
fn main() {
    let mode = std::env::args().nth(1).unwrap();
    match mode.as_str() {
        "permission" => eprintln!("permission denied SYNTHETIC_PRIVATE_OUTPUT"),
        "port" => eprintln!("port is already allocated SYNTHETIC_PRIVATE_OUTPUT"),
        "address" => eprintln!("address already in use SYNTHETIC_PRIVATE_OUTPUT"),
        "network-capacity" => eprintln!("all predefined address pools have been fully subnetted SYNTHETIC_PRIVATE_OUTPUT"),
        "unknown" => eprintln!("SYNTHETIC_PRIVATE_OUTPUT"),
        "success" => { print!("exact-output"); eprintln!("SYNTHETIC_PRIVATE_OUTPUT"); return; },
        "timeout" => std::thread::sleep(std::time::Duration::from_secs(10)),
        _ => panic!("unsupported fixture mode"),
    }
    std::process::exit(7);
}
"#,
            )
            .unwrap();
            let executable = root.join(if cfg!(windows) {
                "fixture.exe"
            } else {
                "fixture"
            });
            let compiled = process_window::background_command("rustc")
                .args([
                    "--edition=2024",
                    "--crate-name",
                    "exhibitos_engine_error_fixture",
                ])
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                compiled.status.success(),
                "native synthetic fixture compilation failed"
            );
            executable
        })
        .as_path()
}

#[test]
fn engine_failure_native_permission_and_port_codes_never_repeat_private_output() {
    for (mode, code) in [
        ("permission", "ENGINE_PERMISSION"),
        ("port", "PORT_IN_USE"),
        ("address", "PORT_IN_USE"),
        ("network-capacity", "ENGINE_NETWORK_CAPACITY"),
        ("unknown", "ENGINE_OPERATION_FAILED"),
    ] {
        let error = run(fixture().to_str().unwrap(), &[mode.into()], None, 5).unwrap_err();
        assert_eq!(error.code, code);
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("SYNTHETIC_PRIVATE_OUTPUT")
        );
        assert!(!error.guidance.is_empty());
    }
}
#[test]
fn engine_failure_native_success_keeps_stdout_separate_from_stderr() {
    let actual = run(fixture().to_str().unwrap(), &["success".into()], None, 5).unwrap();
    assert_eq!(actual, b"exact-output");
}
#[test]
fn engine_failure_native_timeout_waits_and_missing_tool_are_distinct() {
    let error = run(fixture().to_str().unwrap(), &["timeout".into()], None, 1).unwrap_err();
    assert_eq!(error.code, "ENGINE_TIMEOUT");
    let missing = fixture().with_file_name("nonexistent-owned-fixture-command");
    let error = run(missing.to_str().unwrap(), &[], None, 1).unwrap_err();
    assert_eq!(error.code, "RUNTIME_MISSING");
}

#[test]
#[ignore = "explicit local Docker exhausted address pool diagnosis; fresh empty probe only, no service or data volume changes"]
fn engine_network_capacity_actual_docker_returns_safe_code() {
    let name = format!("exhibitos-network-capacity-check-{}", Uuid::new_v4());
    let result = run("docker", &["network".into(), "create".into(), "--label".into(), "com.exhibitos.probe=network-capacity".into(), name.clone()], None, 30);
    if let Ok(output) = &result {
        let id = std::str::from_utf8(output).unwrap().trim();
        assert!(hash_valid(id));
        let proof = backup_creation::inspected("docker", &["network".into(), "inspect".into(), id.into()]).unwrap();
        assert_eq!(proof["Name"], name);
        assert_eq!(proof["Labels"]["com.exhibitos.probe"], "network-capacity");
        assert!(proof["Containers"].as_object().unwrap().is_empty());
        run("docker", &["network".into(), "rm".into(), id.into()], None, 30).unwrap();
    }
    let error = result.expect_err("address pool is available; empty probe cleaned, exhaustion not reproduced");
    assert_eq!(error.code, "ENGINE_NETWORK_CAPACITY");
    assert!(!error.guidance.contains(&name));
    println!("PASS_ACTUAL_DOCKER_NETWORK_CAPACITY_CODE_NO_SERVICE_MUTATION");
}
