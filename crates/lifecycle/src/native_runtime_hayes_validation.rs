// SPDX-License-Identifier: Apache-2.0
//! One private Validate successor under the same retained native attempt.

use super::{super::Worker, *};
use crate::native_payload::Payload;

const PREPARED: &str = "validate-qualification-io.json";
const OBSERVED: &str = "validate-qualification-observed.json";
const STDOUT_LIMIT: u64 = 256;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ValidateIo {
    binding: Binding,
    role: Role,
    body_stage_id: Uuid,
    body: StageBinding,
    stdout_stage_id: Uuid,
    stdout_physical: Physical,
    stdout_limit: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidateObservation {
    binding: Binding,
    stdout: StageBinding,
    peak_rss_kib: u32,
    direct_exit_without_observed_fork_or_exec: bool,
}

/// The caller owns this handle even if READY, GO, watch or stdout fails.
/// Dropping the worker channel requests cancellation, without proving drain.
pub(super) struct RunningValidation {
    pub(super) child: SpawnedWorker,
    work: Option<ValidateWork>,
}

struct ValidateWork {
    selected: SelectedBody,
    prepared: ValidateIo,
    output: Stage,
    stdio: WorkerStdio,
}

pub(super) struct ValidatedReadback {
    pub(super) selected: SelectedBody,
    pub(super) stdout: ObservedStage,
    pub(super) peak_rss_kib: u32,
}

impl Attempt {
    pub(super) fn spawn_retained_validation(
        &mut self,
        payload: &Payload,
        selected: SelectedBody,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut() -> Result<()>,
    ) -> Result<RunningValidation> {
        let mut progress = || {
            if cancelled.load(Ordering::SeqCst) || super::super::super::cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            check_owner()
        };
        progress()?;
        selected.readback.check(self)?;
        self.check_stage_file(selected.id, &selected.observed)?;
        let pin = selected
            .readback
            .worker_pin
            .as_ref()
            .ok_or_else(custody_error)?;
        let command = pin.validate_command(payload)?;
        if self.journal.exists(PREPARED)? || self.journal.exists(Role::Validate.files().0)? {
            return Err(err("HAYES_WORKER_ALREADY_LAUNCHED"));
        }
        let stdout_stage_id = Uuid::new_v4();
        let output = Stage::create(&self.stage.fd, &stdout_stage_id.to_string())?;
        let (dev, ino) = output.identity()?;
        let prepared = ValidateIo {
            binding: self.intent.binding,
            role: Role::Validate,
            body_stage_id: selected.id,
            body: StageBinding::from(&selected.observed),
            stdout_stage_id,
            stdout_physical: Physical { dev, ino },
            stdout_limit: STDOUT_LIMIT,
        };
        self.journal.create(PREPARED, &prepared)?;
        let worker = Worker {
            binding: prepared.binding,
            role: Role::Validate,
            nonce: Uuid::new_v4(),
        };
        self.journal.create(Role::Validate.files().0, &worker)?;
        let lock = self.lease.lock.try_clone().map_err(|_| custody_error())?;
        let args: Vec<_> = command.args.iter().map(|arg| arg.as_os_str()).collect();
        pin.check()?;
        progress()?;
        let (child, stdio) =
            LaunchPermit::new(worker, lock).spawn_staged(&command.program, &args, None)?;
        Ok(RunningValidation {
            child,
            work: Some(ValidateWork {
                selected,
                prepared,
                output,
                stdio,
            }),
        })
    }

    pub(super) fn observe_retained_validation(
        &mut self,
        run: &mut RunningValidation,
        deadline: Instant,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ValidatedReadback> {
        let ValidateWork {
            selected,
            prepared,
            output,
            mut stdio,
        } = run.work.take().ok_or_else(channel_error)?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(16));
        let mut progress = || {
            if cancelled.load(Ordering::SeqCst) || super::super::super::cancelled() {
                return Err(err("NATIVE_CANCELLED"));
            }
            if Instant::now() >= deadline {
                return Err(err("HAYES_WORKER_TIMEOUT"));
            }
            check_owner()?;
            selected
                .readback
                .worker_pin
                .as_ref()
                .ok_or_else(custody_error)?
                .check()
        };
        let result = (|| {
            progress()?;
            self.register_ready_until(&mut run.child, deadline, &mut progress)?;
            let watch = DirectWatch::arm(&mut run.child)?;
            selected.readback.check(self)?;
            self.check_stage_file(selected.id, &selected.observed)?;
            let named = self
                .stage
                .open_file(
                    &format!(".oex-stage-{}", prepared.stdout_stage_id),
                    libc::O_RDONLY,
                )
                .map_err(|_| custody_error())?;
            let (dev, ino) = output.identity()?;
            if private_file(&named)? != prepared.stdout_physical
                || (Physical { dev, ino }) != prepared.stdout_physical
                || self.journal.read::<ValidateIo>(PREPARED)? != prepared
            {
                return Err(custody_error());
            }
            stdio.check_quiet_output()?;
            progress()?;
            watch.poll()?;
            self.send_go(&mut run.child)?;
            let output = capture(
                &mut run.child,
                &mut stdio,
                &selected.observed,
                output.materializer_write(),
                STDOUT_LIMIT,
                &mut || {
                    progress()?;
                    watch.poll()
                },
            )?;
            watch.require_exit()?;
            if !run
                .child
                .try_wait()
                .map_err(|_| io_error())?
                .is_some_and(|status| status.success())
            {
                return Err(io_error());
            }
            Ok(output)
        })();
        run.child.owner.take();
        drop(stdio);
        output
            .materializer_write()
            .sync_all()
            .map_err(|_| custody_error())?;
        let (bytes, sha256) = result?;
        let stdout = observe_stage(output, bytes, &sha256, cancelled, &mut progress)?;
        selected.readback.check(self)?;
        self.check_stage_file(selected.id, &selected.observed)?;
        self.check_stage_file(prepared.stdout_stage_id, &stdout)?;
        let mut response = Vec::new();
        stdout
            .file()
            .read_to_end(&mut response)
            .map_err(|_| io_error())?;
        let peak_rss_kib = std::str::from_utf8(&response)
            .ok()
            .and_then(|text| text.strip_prefix("VALID\nRSS "))
            .and_then(|text| text.strip_suffix('\n'))
            .and_then(|text| {
                text.parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0 && n.to_string() == text)
            })
            .ok_or_else(io_error)?;
        progress()?;
        self.journal.create(
            OBSERVED,
            &ValidateObservation {
                binding: self.intent.binding,
                stdout: StageBinding::from(&stdout),
                peak_rss_kib,
                direct_exit_without_observed_fork_or_exec: true,
            },
        )?;
        Ok(ValidatedReadback {
            selected,
            stdout,
            peak_rss_kib,
        })
    }
}
