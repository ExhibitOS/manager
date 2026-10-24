// SPDX-License-Identifier: Apache-2.0
#![cfg(unix)]
use exhibitos_lifecycle::installations::{
    InstallationController,
    native_ui::{Mode, Step},
};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::symlink,
    process::{Command, Stdio},
};

fn root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("exhibitos-native-ui-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

#[test]
fn mode_and_step_survive_relaunch_without_touching_registry_or_jobs() {
    let parent = root();
    let profile = parent.join("profile");
    let c = InstallationController::new(profile.clone(), None).unwrap();
    let registry = fs::read(profile.join("installation-selection.json")).unwrap();
    let initial = c.presentation().unwrap();
    assert_eq!(initial.mode, Mode::Beginner);
    let mut expert = initial.clone();
    expert.mode = Mode::Expert;
    expert.step = Step::Preview;
    expert.text_scale = 200;
    let saved = c.save_presentation(expert).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        c.save_presentation(initial).unwrap_err().code,
        "NATIVE_UI_STATE_CHANGED"
    );
    assert_eq!(
        fs::read(profile.join("installation-selection.json")).unwrap(),
        registry
    );
    let context = c.context().unwrap();
    assert!(
        c.with_current(&context.selection_token, |s| s.jobs())
            .unwrap()
            .is_empty()
    );
    drop(c);
    let reopened = InstallationController::new(profile.clone(), None).unwrap();
    assert_eq!(reopened.presentation().unwrap(), saved);
    // Non-owner stale selection cannot query this new session's lifecycle.
    assert!(
        reopened
            .with_current(&context.selection_token, |s| s.status())
            .is_err()
    );
    drop(reopened);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn state_corruption_and_symlink_are_refused_without_overwrite() {
    let parent = root();
    let profile = parent.join("profile");
    let c = InstallationController::new(profile.clone(), None).unwrap();
    let saved = c.save_presentation(c.presentation().unwrap()).unwrap();
    let path = profile.join("native-ui.json");
    fs::write(&path, b"broken").unwrap();
    assert!(c.save_presentation(saved.clone()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"broken");
    fs::remove_file(&path).unwrap();
    let original = parent.join("original");
    fs::write(&original, b"preserve").unwrap();
    symlink(&original, &path).unwrap();
    assert!(c.presentation().is_err());
    assert!(c.save_presentation(saved).is_err());
    assert_eq!(fs::read(original).unwrap(), b"preserve");
    drop(c);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn protocol_is_bounded_and_does_not_accept_arbitrary_commands_or_fields() {
    let parent = root();
    let profile = parent.join("profile");
    let mut child = Command::new(env!("CARGO_BIN_EXE_exhibitos-native"))
        .args(["--profile", profile.to_str().unwrap()])
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for (request, success) in [
        (r#"{"version":1,"id":1,"command":"presentation"}"#, true),
        (r#"{"version":1,"id":2,"command":"context"}"#, true),
        (r#"{"version":2,"id":3,"command":"presentation"}"#, false),
        (
            r#"{"version":1,"id":4,"command":"shell","path":"/bin/sh"}"#,
            false,
        ),
        (
            r#"{"version":1,"id":5,"command":"presentation","token":"never echo"}"#,
            false,
        ),
        (
            r#"{"version":1,"id":6,"command":"status","selection_token":"stale"}"#,
            false,
        ),
        (
            r#"{"version":1,"id":7,"command":"native_prepare","selection_token":"stale","account":{"subject":"synthetic","password":"never echo this password"}}"#,
            false,
        ),
        (
            r#"{"version":1,"id":8,"command":"native_start","selection_token":"stale","account":{"subject":"synthetic","password":"never echo this password"}}"#,
            false,
        ),
        (
            r#"{"version":1,"id":9,"command":"presentation","account":{"subject":"synthetic","password":"never echo this password"}}"#,
            false,
        ),
    ] {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert!(!line.contains("never echo"));
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response.get("result").is_some(), success, "{response}");
    }
    stdin.write_all(&vec![b'x'; 3_145_729]).unwrap();
    stdin.flush().unwrap();
    drop(stdin);
    assert_eq!(child.wait().unwrap().code(), Some(2));
    fs::remove_dir_all(parent).unwrap();
}
