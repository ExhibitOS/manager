// SPDX-License-Identifier: Apache-2.0
//! Private exact-package qualification fixtures, never a product entry point.

use super::*;
use crate::{
    native_payload::Payload,
    native_runtime::hayes::{
        tests::{binding, wait_for, Fixture},
        transfer::Transfer,
        Lease,
    },
};
use std::{
    env,
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    process::{Command, ExitStatus, Stdio},
};

fn path(name: &str) -> std::path::PathBuf {
    env::var_os(name)
        .map(Into::into)
        .unwrap_or_else(|| panic!("{name} required for selected probe"))
}

fn new_custody(name: &str) -> std::mem::ManuallyDrop<Fixture> {
    std::mem::ManuallyDrop::new(Fixture::provision(path(name)))
}

fn stage_exact(
    attempt: &Attempt,
    source: &mut impl Read,
    stage_id: Uuid,
    bytes: u64,
    sha256: &str,
) -> ObservedStage {
    let stage = Stage::create(&attempt.stage.fd, &stage_id.to_string()).unwrap();
    let mut limited = source.take(bytes + 1);
    assert_eq!(
        io::copy(&mut limited, &mut stage.materializer_write()).unwrap(),
        bytes
    );
    let observed = observe_stage(
        stage,
        bytes,
        sha256,
        &AtomicBool::new(false),
        &mut || Ok(()),
    )
    .unwrap();
    attempt.check_stage_file(stage_id, &observed).unwrap();
    observed
}

fn selected_source(attempt: &Attempt) -> SourceStage {
    let binding = attempt.intent.binding;
    let correlation = Uuid::new_v4();
    let flow = Transfer::source(binding.attempt, correlation).unwrap();
    let mut source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path("HAYES_SELECTED_SOURCE"))
        .unwrap();
    assert!(source.metadata().unwrap().is_file());
    let observed = stage_exact(
        attempt,
        &mut source,
        binding.attempt,
        flow.bytes(),
        flow.sha256(),
    );
    SourceStage {
        binding,
        correlation,
        observed,
    }
}

fn start_generate(attempt: &mut Attempt, payload: &Payload) -> RunningGeneration {
    let source = selected_source(attempt);
    let command = payload.hayes_read_only_generate().unwrap();
    attempt
        .prepare_generation(source)
        .unwrap()
        .spawn_fixed_generate(command)
        .unwrap()
}

fn reap(child: &mut SpawnedWorker) -> ExitStatus {
    wait_for(|| child.try_wait().unwrap().is_some());
    child.try_wait().unwrap().unwrap()
}

fn proof(parent: &Path, name: &str, value: serde_json::Value) {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(parent.join(name))
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &value).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}

#[test]
#[ignore]
fn selected_package_generate_and_validate_retains_exact_private_stages() {
    let root = new_custody("HAYES_NOMINAL_CUSTODY");
    let expected = binding();
    let mut attempt = root.acquire().begin(expected).unwrap();
    let payload = Payload::open(&path("HAYES_FIXED_PACKAGE")).unwrap();
    let mut run = start_generate(&mut attempt, &payload);
    let cancellation = AtomicBool::new(false);
    let generated = attempt.observe_generation(
        &mut run,
        Instant::now() + Duration::from_secs(16),
        &cancellation,
        &mut |_| Ok(()),
    );
    assert!(reap(&mut run.child).success());
    let generated = generated.unwrap();
    let diagnostics: serde_json::Value = attempt.journal.read(OBSERVED).unwrap();
    assert_eq!(
        diagnostics["watched_direct_exit_without_fork_or_exec"],
        true
    );
    let selected = generated
        .stage_selected_body(&attempt, &cancellation, &mut || Ok(()))
        .unwrap();
    let mut validation = attempt
        .spawn_retained_validation(&payload, selected, &cancellation, &mut || Ok(()))
        .unwrap();
    let validated = attempt.observe_retained_validation(
        &mut validation,
        Instant::now() + Duration::from_secs(16),
        &cancellation,
        &mut |_| Ok(()),
    );
    assert!(reap(&mut validation.child).success());
    let validated = validated.unwrap();
    let body_id = validated.selected.id;
    let body = &validated.selected.observed;
    let telemetry = &validated.selected.telemetry;
    let validate_rss = validated.peak_rss_kib;
    let validate_stdout_sha = &validated.stdout.sha256;
    attempt.check().unwrap();
    let body_stage = format!(".oex-stage-{body_id}");
    drop(attempt);
    assert_eq!(
        root.acquire().check_interrupted(expected).unwrap_err().code,
        "HAYES_DRAIN_UNCONFIRMED"
    );
    proof(
        root.path(),
        "selected-nominal.json",
        serde_json::json!({
            "binding": expected, "generateRaw": diagnostics["stdout"],
            "body": StageBinding::from(body), "bodyStage": body_stage,
            "simplifierRelativeError": telemetry.relative_error, "generatePeakRssKiB": telemetry.peak_rss_kib,
            "validatePeakRssKiB": validate_rss, "validateStdoutSha256": validate_stdout_sha,
            "scope": "Exact package, both direct exits/EOF observed; held private custody; no product authority."
        }),
    );
}

#[test]
#[ignore]
fn selected_package_owner_loss_after_observed_libvips_mapping_retains_custody() {
    let root = new_custody("HAYES_FAULT_CUSTODY");
    let expected = binding();
    let mut attempt = root.acquire().begin(expected).unwrap();
    let mount = path("HAYES_FIXED_PACKAGE");
    let payload = Payload::open(&mount).unwrap();
    let probe = path("HAYES_LIBPROC_PROBE");
    let dylib = mount
        .join("platform/node_modules/@img/sharp-libvips-darwin-arm64/lib/libvips-cpp.8.18.6.dylib");
    let mut run = start_generate(&mut attempt, &payload);
    let pid = run.child.diagnostic_pid().to_string();
    let mut mapped = false;
    let mut next = Instant::now();
    let observed = attempt.drive_generation(
        &mut run,
        Instant::now() + Duration::from_secs(16),
        &AtomicBool::new(false),
        &mut || {
            if Instant::now() < next {
                return Ok(());
            }
            next = Instant::now() + Duration::from_millis(10);
            let result = Command::new(&probe)
                .env_clear()
                .arg(&pid)
                .arg(&dylib)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|_| io_error())?;
            match result.code() {
                Some(0) => {
                    mapped = true;
                    Err(err("HAYES_QUALIFICATION_OWNER_LOSS"))
                }
                Some(2) => Ok(()),
                _ => Err(err("HAYES_QUALIFICATION_MAPPING_UNKNOWN")),
            }
        },
    );
    let outcome = observed.unwrap_err().code;
    drop(attempt);
    let busy_after_owner_drop = match Lease::acquire(root.path(), root.authority()) {
        Ok(_) => false,
        Err(error) if error.code == "HAYES_RESERVATION_BUSY" => true,
        Err(error) => panic!("fault lock observation: {error}"),
    };
    let status = reap(&mut run.child);
    let watch_observation = run
        .watch
        .as_ref()
        .map(|watch| watch.require_exit().err().map(|error| error.code));
    run.pinned_generate.as_ref().unwrap().check().unwrap();
    assert_eq!(
        root.acquire().check_interrupted(expected).unwrap_err().code,
        "HAYES_DRAIN_UNCONFIRMED"
    );
    proof(
        root.path(),
        "selected-fault.json",
        serde_json::json!({
            "binding": expected, "mappedBeforeOwnerClose": mapped, "result": outcome,
            "exitCode": status.code(), "busyAfterOwnerDrop": busy_after_owner_drop,
            "watchedTerminal": watch_observation == Some(None), "watchError": watch_observation.flatten(),
            "scope": "Mapping observed before owner close is not proof of a codec call in progress. Custody retained."
        }),
    );
}
