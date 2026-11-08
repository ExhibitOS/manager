// SPDX-License-Identifier: Apache-2.0
//! Inactive generate-phase I/O and private output custody.

use super::{
    channel_error, custody_error, cvt, err,
    frame::{inspect_selected_frame, GenerateTelemetry, BODY_BYTES, BODY_SHA},
    launch::{LaunchPermit, SpawnedWorker, WorkerStdio},
    private_file,
    watch::DirectWatch,
    Attempt, Binding, Physical, Result, Role, SourceStage,
};
use crate::{
    native_payload::hayes::ReadOnlyHayesGenerate,
    native_runtime::oex_read_fd::{observe_stage, ObservedStage, Stage},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    net::Shutdown,
    os::{fd::AsRawFd, unix::fs::FileExt},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const STDOUT_LIMIT: u64 = 16 * 1024 * 1024 + 8; // GLB + binary error/RSS telemetry
const PREPARED: &str = "generate-io.json";
const OBSERVED: &str = "generate-direct-observed.json";

#[derive(Clone, Copy)]
pub(super) enum WorkerCheckpoint<'a> {
    Observe,
    BeforeGo(&'a Attempt),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StageBinding {
    physical: Physical,
    bytes: u64,
    sha256: String,
}

impl From<&ObservedStage> for StageBinding {
    fn from(value: &ObservedStage) -> Self {
        Self {
            physical: Physical {
                dev: value.device,
                ino: value.inode,
            },
            bytes: value.bytes,
            sha256: value.sha256.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    binding: Binding,
    correlation_id: Uuid,
    source: StageBinding,
    output_stage_id: Uuid,
    output: Physical,
    stdout_limit: u64,
}

#[derive(Serialize)]
struct DirectObservation<'a> {
    prepared: &'a Prepared,
    stdout: StageBinding,
    watched_direct_exit_without_fork_or_exec: bool,
}

/// Direct-child readback under the retained Attempt. Its descriptors bind
/// the original and raw stdout until the next local qualification step.
pub(super) struct GenerationReadback {
    prepared: Prepared,
    source: SourceStage,
    stdout: ObservedStage,
    watched: bool,
    worker_pin: Option<ReadOnlyHayesGenerate>,
}

pub(super) struct SelectedBody {
    readback: GenerationReadback,
    pub(super) id: Uuid,
    pub(super) observed: ObservedStage,
    pub(super) telemetry: GenerateTelemetry,
}

impl GenerationReadback {
    fn check(&self, attempt: &Attempt) -> Result<()> {
        if !self.watched
            || attempt.journal.read::<Prepared>(PREPARED)? != self.prepared
            || self.prepared.binding != attempt.intent.binding
            || self.source.binding != self.prepared.binding
            || self.source.correlation != self.prepared.correlation_id
            || StageBinding::from(&self.source.observed) != self.prepared.source
        {
            return Err(custody_error());
        }
        attempt.check()?;
        attempt.check_stage_file(self.source.binding.attempt, &self.source.observed)?;
        attempt.check_stage_file(self.prepared.output_stage_id, &self.stdout)?;
        Ok(())
    }

    pub(super) fn stage_selected_body(
        self,
        attempt: &Attempt,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut() -> Result<()>,
    ) -> Result<SelectedBody> {
        let mut progress = || {
            if cancelled.load(Ordering::SeqCst) || super::super::cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            check_owner()
        };
        progress()?;
        self.check(attempt)?;

        let mut raw = self.stdout.file();
        raw.seek(SeekFrom::Start(0)).map_err(|_| io_error())?;
        let telemetry = inspect_selected_frame(&mut raw)?;
        progress()?;
        let id = Uuid::new_v4();
        let stage = Stage::create(&attempt.stage.fd, &id.to_string())?;
        let mut writer = stage.materializer_write();
        let mut buffer = [0; 16 * 1024];
        let copied: Result<()> = (|| {
            let mut offset = 0;
            while offset < u64::from(BODY_BYTES) {
                progress()?;
                let size = (u64::from(BODY_BYTES) - offset).min(buffer.len() as u64) as usize;
                raw.read_exact_at(&mut buffer[..size], 8 + offset)
                    .map_err(|_| io_error())?;
                writer.write_all(&buffer[..size]).map_err(|_| io_error())?;
                offset += size as u64;
            }
            Ok(())
        })();
        buffer.fill(0);
        copied?;
        let observed = observe_stage(stage, BODY_BYTES.into(), BODY_SHA, cancelled, &mut progress)?;
        self.check(attempt)?;
        attempt.check_stage_file(id, &observed)?;
        Ok(SelectedBody {
            readback: self,
            id,
            observed,
            telemetry,
        })
    }
}

pub(super) struct GenerationPermit {
    launch: LaunchPermit,
    staging: Staging,
}

struct Staging {
    prepared: Prepared,
    source: SourceStage,
    output: Stage,
}

pub(super) struct RunningGeneration {
    pub(super) child: SpawnedWorker,
    work: Option<(Staging, WorkerStdio)>,
    pinned_generate: Option<ReadOnlyHayesGenerate>,
    watch: Option<DirectWatch>,
    watch_required: bool,
}

impl RunningGeneration {
    fn arm_watch(&mut self) -> Result<()> {
        if self.watch.is_some() || self.child.phase != super::Phase::Registered {
            return Err(channel_error());
        }
        self.watch = Some(DirectWatch::arm(&mut self.child)?);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn require_fixture_watch(&mut self) {
        self.watch_required = true;
    }
}

impl GenerationPermit {
    /// The private exact-candidate lane keeps its read-only mount and
    /// derived VIPSHOME pinned through launch and authorization.
    pub(super) fn spawn_fixed_generate(
        self,
        candidate: ReadOnlyHayesGenerate,
    ) -> Result<RunningGeneration> {
        if self.launch.worker.role != Role::Generate {
            return Err(custody_error());
        }
        candidate.check()?;
        let command = candidate.command();
        let args: Vec<_> = command.args.iter().map(|arg| arg.as_os_str()).collect();
        let (child, stdio) =
            self.launch
                .spawn_staged(&command.program, &args, Some(candidate.environment()))?;
        Ok(RunningGeneration {
            child,
            work: Some((self.staging, stdio)),
            pinned_generate: Some(candidate),
            watch: None,
            watch_required: true,
        })
    }

    #[cfg(test)]
    pub(super) fn spawn_fixture(
        self,
        program: &Path,
        args: &[&OsStr],
    ) -> Result<RunningGeneration> {
        let (child, stdio) = self.launch.spawn_staged(program, args, None)?;
        Ok(RunningGeneration {
            child,
            work: Some((self.staging, stdio)),
            pinned_generate: None,
            watch: None,
            watch_required: false,
        })
    }
}

impl Attempt {
    pub(super) fn prepare_generation(&mut self, source: SourceStage) -> Result<GenerationPermit> {
        self.check()?;
        if source.binding != self.intent.binding || self.launch_possible {
            return Err(custody_error());
        }
        self.check_stage_file(source.binding.attempt, &source.observed)?;
        let output_stage_id = Uuid::new_v4();
        let output = Stage::create(&self.stage.fd, &output_stage_id.to_string())?;
        let (dev, ino) = output.identity()?;
        let prepared = Prepared {
            binding: source.binding,
            correlation_id: source.correlation,
            source: (&source.observed).into(),
            output_stage_id,
            output: Physical { dev, ino },
            stdout_limit: STDOUT_LIMIT,
        };
        self.journal.create(PREPARED, &prepared)?;
        let launch = self.before_spawn(Role::Generate)?;
        Ok(GenerationPermit {
            launch,
            staging: Staging {
                prepared,
                source,
                output,
            },
        })
    }

    fn check_generation(&self, staging: &Staging) -> Result<()> {
        self.check()?;
        if staging.prepared.binding != self.intent.binding
            || self.journal.read::<Prepared>(PREPARED)? != staging.prepared
        {
            return Err(custody_error());
        }
        self.check_stage_file(staging.source.binding.attempt, &staging.source.observed)?;
        let name = format!(".oex-stage-{}", staging.prepared.output_stage_id);
        let named = self
            .stage
            .open_file(&name, libc::O_RDONLY)
            .map_err(|_| custody_error())?;
        let (dev, ino) = staging.output.identity()?;
        if private_file(&named)? != staging.prepared.output
            || (Physical { dev, ino }) != staging.prepared.output
        {
            return Err(custody_error());
        }
        Ok(())
    }

    /// Drives the exact child through READY and GO. The caller retains the
    /// direct child handle even on failure. Positive descendant containment
    /// is unavailable, so even an exact private stdout readback stays held.
    pub(super) fn drive_generation(
        &mut self,
        run: &mut RunningGeneration,
        deadline: Instant,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        let mut readback =
            self.observe_generation(run, deadline, cancelled, &mut |_| check_owner())?;
        run.pinned_generate = readback.worker_pin.take();
        Err(err("HAYES_DRAIN_UNCONFIRMED"))
    }

    pub(super) fn observe_generation(
        &mut self,
        run: &mut RunningGeneration,
        deadline: Instant,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut(WorkerCheckpoint<'_>) -> Result<()>,
    ) -> Result<GenerationReadback> {
        let (staging, mut stdio) = run.work.take().ok_or_else(channel_error)?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(16));
        let mut progress = |checkpoint: WorkerCheckpoint<'_>| {
            if cancelled.load(Ordering::SeqCst) || super::super::cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            if Instant::now() >= deadline {
                return Err(err("HAYES_WORKER_TIMEOUT"));
            }
            check_owner(checkpoint)
        };
        let result = (|| {
            progress(WorkerCheckpoint::Observe)?;
            self.check_generation(&staging)?;
            self.register_ready_until(&mut run.child, deadline, &mut || {
                progress(WorkerCheckpoint::Observe)
            })?;
            // The trusted inert entry has executed and completed READY. Darwin
            // can still queue the initial exec after posix_spawn returns.
            // Start the witness here, before the first possible GO.
            if run.watch_required {
                run.arm_watch()?;
            }
            let watch = run.watch.as_ref();
            let mut watched_progress = |checkpoint: WorkerCheckpoint<'_>| {
                progress(checkpoint)?;
                if let Some(watch) = watch {
                    watch.poll()?;
                }
                Ok(())
            };
            self.check_generation(&staging)?;
            if let Some(candidate) = &run.pinned_generate {
                candidate.check()?;
            }
            stdio.check_quiet_output()?;
            watched_progress(WorkerCheckpoint::BeforeGo(self))?;
            self.check_generation(&staging)?;
            if let Some(candidate) = &run.pinned_generate {
                candidate.check()?;
            }
            stdio.check_quiet_output()?;
            self.send_go_checked(&mut run.child, &mut || {
                watched_progress(WorkerCheckpoint::Observe)
            })?;
            capture(
                &mut run.child,
                &mut stdio,
                &staging.source.observed,
                staging.output.materializer_write(),
                staging.prepared.stdout_limit,
                &mut || watched_progress(WorkerCheckpoint::Observe),
            )
        })();
        // Closing the dedicated owner channel is the cancellation request.
        // Reaping, when possible, is only an observation of the direct child.
        run.child.owner.take();
        drop(stdio);
        staging
            .output
            .materializer_write()
            .sync_all()
            .map_err(|_| custody_error())?;
        let (bytes, sha256) = result?;
        let watch = run.watch.as_ref();
        if let Some(watch) = watch {
            watch.require_exit()?;
        }
        if let Some(candidate) = &run.pinned_generate {
            candidate.check()?;
        }

        // Only Manager has written this inode. This establishes the current
        // bytes, and confers no release or descendant-drain authority.
        let observed = observe_stage(staging.output, bytes, &sha256, cancelled, &mut || {
            progress(WorkerCheckpoint::Observe)
        })?;
        self.check()?;
        self.check_stage_file(staging.prepared.output_stage_id, &observed)?;
        if Physical::from(&observed.file().metadata().map_err(|_| custody_error())?)
            != staging.prepared.output
        {
            return Err(custody_error());
        }
        self.journal.create(
            OBSERVED,
            &DirectObservation {
                prepared: &staging.prepared,
                stdout: (&observed).into(),
                watched_direct_exit_without_fork_or_exec: watch.is_some(),
            },
        )?;
        Ok(GenerationReadback {
            prepared: staging.prepared,
            source: staging.source,
            stdout: observed,
            watched: watch.is_some(),
            worker_pin: run.pinned_generate.take(),
        })
    }
}

fn capture(
    child: &mut SpawnedWorker,
    stdio: &mut WorkerStdio,
    source: &ObservedStage,
    mut stage: &File,
    stdout_limit: u64,
    progress: &mut dyn FnMut() -> Result<()>,
) -> Result<(u64, String)> {
    let mut input = [0; 16 * 1024];
    let mut output = [0; 16 * 1024];
    let mut pending = 0..0;
    let mut sent = 0;
    let mut captured = 0;
    let mut input_eof = false;
    let mut output_eof = false;
    let mut hash = Sha256::new();
    let result = (|| {
        loop {
            progress()?;
            let status = child.try_wait().map_err(|_| io_error())?;
            if status.is_some_and(|status| !status.success() || !input_eof) {
                return Err(io_error());
            }
            if status.is_some() && output_eof {
                break;
            }
            let mut advanced = false;
            if !input_eof {
                if pending.is_empty() && sent < source.bytes {
                    source.check_unchanged()?;
                    let n = (source.bytes - sent).min(input.len() as u64) as usize;
                    source
                        .file()
                        .read_exact_at(&mut input[..n], sent)
                        .map_err(|_| io_error())?;
                    pending = 0..n;
                }
                if !pending.is_empty() {
                    let chunk = &input[pending.clone()];
                    let written = unsafe {
                        libc::send(
                            stdio.input.as_raw_fd(),
                            chunk.as_ptr().cast(),
                            chunk.len(),
                            libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                        )
                    };
                    match cvt(written as i32) {
                        Ok(n) if n > 0 => {
                            sent += n as u64;
                            pending.start += n as usize;
                            advanced = true;
                        }
                        Err(error) if retry(&error) => {}
                        _ => return Err(io_error()),
                    }
                }
                if sent == source.bytes {
                    source.check_unchanged()?;
                    stdio
                        .input
                        .shutdown(Shutdown::Write)
                        .map_err(|_| io_error())?;
                    input_eof = true;
                    advanced = true;
                }
            }
            if !output_eof {
                match stdio.output.read(&mut output) {
                    Ok(0) => {
                        output_eof = true;
                        advanced = true;
                    }
                    Ok(n) => {
                        captured += n as u64;
                        if captured > stdout_limit {
                            return Err(err("HAYES_WORKER_OUTPUT_LIMIT"));
                        }
                        stage.write_all(&output[..n]).map_err(|_| io_error())?;
                        hash.update(&output[..n]);
                        advanced = true;
                    }
                    Err(error) if retry(&error) => {}
                    Err(_) => return Err(io_error()),
                }
            }
            if !advanced {
                thread::sleep(Duration::from_millis(2));
            }
        }
        if captured <= 8 {
            return Err(io_error());
        }
        Ok((captured, format!("{:x}", hash.finalize())))
    })();
    input.fill(0);
    output.fill(0);
    result
}

fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn io_error() -> crate::LifecycleError {
    err("HAYES_WORKER_IO_INVALID")
}

#[path = "native_runtime_hayes_validation.rs"]
pub(super) mod validation;

#[cfg(test)]
#[path = "native_runtime_hayes_selected_probe.rs"]
mod selected_probe;
