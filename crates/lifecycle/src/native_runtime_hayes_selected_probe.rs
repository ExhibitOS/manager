// SPDX-License-Identifier: Apache-2.0
//! Private exact-package qualification fixtures, never a product entry point.

use super::*;
use crate::{
    native_payload::{
        hayes::{HayesPhase, ReadOnlyHayesGenerate},
        Payload,
    },
    native_runtime::hayes::{
        frame::{inspect_selected_frame, BODY_BYTES, BODY_SHA},
        tests::{binding, wait_for, Fixture},
        transfer::Transfer,
        Lease, Worker,
    },
};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom},
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

fn validate_exact(
    attempt: &mut Attempt,
    payload: &Payload,
    pin: &ReadOnlyHayesGenerate,
    body: &ObservedStage,
    body_id: Uuid,
) -> Result<(u32, String)> {
    pin.check()?;
    attempt.check_stage_file(body_id, body)?;
    let command = payload.hayes_fixed_command(HayesPhase::Validate)?;
    let args: Vec<_> = command.args.iter().map(|arg| arg.as_os_str()).collect();
    let output_id = Uuid::new_v4();
    let output = Stage::create(&attempt.stage.fd, &output_id.to_string())?;
    let (dev, ino) = output.identity()?;
    let staged = serde_json::json!({
        "binding": attempt.intent.binding, "role": "validate",
        "bodyStageId": body_id, "body": StageBinding::from(body),
        "stdoutStageId": output_id, "stdoutPhysical": Physical { dev, ino }, "stdoutLimit":256
    });
    attempt
        .journal
        .create("validate-qualification-io.json", &staged)?;
    // Fixture-only successor. It is reachable only from the exact Generate
    // readback below, while the original Attempt and OFD remain held.
    let worker = Worker {
        binding: attempt.intent.binding,
        role: Role::Validate,
        nonce: Uuid::new_v4(),
    };
    attempt.journal.create(Role::Validate.files().0, &worker)?;
    let permit = LaunchPermit::new(
        worker,
        attempt
            .lease
            .lock
            .try_clone()
            .map_err(|_| custody_error())?,
    );
    let (mut child, mut stdio) = permit.spawn_staged(&command.program, &args, None)?;
    let deadline = Instant::now() + Duration::from_secs(16);
    let mut progress = || {
        if Instant::now() >= deadline {
            return Err(err("HAYES_WORKER_TIMEOUT"));
        }
        pin.check()
    };
    let result = (|| {
        attempt.register_ready_until(&mut child, deadline, &mut progress)?;
        let watch = DirectWatch::arm(&mut child)?;
        attempt.check()?;
        attempt.check_stage_file(body_id, body)?;
        let named = attempt
            .stage
            .open_file(&format!(".oex-stage-{output_id}"), libc::O_RDONLY)
            .map_err(|_| custody_error())?;
        if private_file(&named)? != (Physical { dev, ino })
            || output.identity()? != (dev, ino)
            || attempt
                .journal
                .read::<serde_json::Value>("validate-qualification-io.json")?
                != staged
        {
            return Err(custody_error());
        }
        stdio.check_quiet_output()?;
        progress()?;
        watch.poll()?;
        attempt.send_go(&mut child)?;
        let (bytes, sha256) = capture(
            &mut child,
            &mut stdio,
            body,
            output.materializer_write(),
            256,
            &mut || {
                progress()?;
                watch.poll()
            },
        )?;
        watch.require_exit()?;
        Ok((bytes, sha256))
    })();
    child.owner.take();
    drop(stdio);
    output
        .materializer_write()
        .sync_all()
        .map_err(|_| custody_error())?;
    let status = reap(&mut child);
    let (bytes, sha256) = result?;
    if !status.success() {
        return Err(io_error());
    }
    let stdout = observe_stage(
        output,
        bytes,
        &sha256,
        &AtomicBool::new(false),
        &mut progress,
    )?;
    attempt.check_stage_file(output_id, &stdout)?;
    let mut response = Vec::new();
    let mut reader = stdout.file();
    reader.read_to_end(&mut response).map_err(|_| io_error())?;
    let rss = std::str::from_utf8(&response)
        .ok()
        .and_then(|text| text.strip_prefix("VALID\nRSS "))
        .and_then(|text| text.strip_suffix('\n'))
        .and_then(|text| {
            text.parse::<u32>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == text)
        })
        .ok_or_else(io_error)?;
    attempt.journal.create("validate-qualification-observed.json", &serde_json::json!({
        "binding": attempt.intent.binding, "stdout": StageBinding::from(&stdout), "peakRssKiB": rss,
        "directExitWithoutObservedForkOrExec": true
    }))?;
    Ok((rss, sha256))
}

#[test]
#[ignore]
fn selected_package_generate_and_validate_retains_exact_private_stages() {
    let root = new_custody("HAYES_NOMINAL_CUSTODY");
    let expected = binding();
    let mut attempt = root.acquire().begin(expected).unwrap();
    let payload = Payload::open(&path("HAYES_FIXED_PACKAGE")).unwrap();
    let mut run = start_generate(&mut attempt, &payload);
    let observed = attempt.drive_generation(
        &mut run,
        Instant::now() + Duration::from_secs(16),
        &AtomicBool::new(false),
        &mut || Ok(()),
    );
    let status = reap(&mut run.child);
    assert_eq!(observed.unwrap_err().code, "HAYES_DRAIN_UNCONFIRMED");
    assert!(status.success());
    let diagnostics: serde_json::Value = attempt.journal.read(OBSERVED).unwrap();
    assert_eq!(
        diagnostics["watched_direct_exit_without_fork_or_exec"],
        true
    );
    let prepared: Prepared = attempt.journal.read(PREPARED).unwrap();
    let name = format!(".oex-stage-{}", prepared.output_stage_id);
    let mut raw = attempt.stage.open_file(&name, libc::O_RDONLY).unwrap();
    assert_eq!(private_file(&raw).unwrap(), prepared.output);
    assert_eq!(raw.metadata().unwrap().len(), BODY_BYTES as u64 + 8);
    let telemetry = inspect_selected_frame(&mut raw).unwrap();
    raw.seek(SeekFrom::Start(8)).unwrap();
    let body_id = Uuid::new_v4();
    let body = stage_exact(&attempt, &mut raw, body_id, BODY_BYTES.into(), BODY_SHA);

    let pin = run.pinned_generate.as_ref().unwrap();
    let (validate_rss, validate_stdout_sha) =
        validate_exact(&mut attempt, &payload, pin, &body, body_id).unwrap();
    pin.check().unwrap();
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
            "body": StageBinding::from(&body), "bodyStage": body_stage,
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
