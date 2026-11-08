// SPDX-License-Identifier: Apache-2.0
//! Inactive custody for the native Hayes helper. No runtime entry point is wired.
//! Production must provision one canonical root per effective UID and retain its
//! identity independently of the namespace opened here. Profile paths and HOME
//! are not authority for choosing a root. Open never provisions or repairs it.

use crate::{err, maintenance_source::DirectoryPin, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::{self, File, Metadata},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Instant,
};
use uuid::Uuid;

#[path = "native_runtime_hayes_frame.rs"]
mod frame;
#[path = "native_runtime_hayes_generation.rs"]
mod generation;
#[path = "native_runtime_hayes_launch.rs"]
mod launch;
#[path = "native_runtime_hayes_operation.rs"]
pub(super) mod operation;
#[path = "native_runtime_hayes_transfer.rs"]
mod transfer;
#[path = "native_runtime_hayes_watch.rs"]
mod watch;
use launch::{channel_error, LaunchPermit, Phase, SpawnedWorker};

pub(super) const RESERVATION_BYTES: u64 = 512 * 1024 * 1024;
const LOCK: &str = "hayes.lock";
const ACTIVE: &str = "active.json";
const MAX_RECORD: u64 = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Physical {
    dev: u64,
    ino: u64,
}

impl From<&Metadata> for Physical {
    fn from(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }
}

/// A trusted provisioning receipt, stored outside the namespace it identifies.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Authority {
    uid: u32,
    root: Physical,
    lock: Physical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub(super) attempt: Uuid,
    pub(super) generation: Uuid,
    pub(super) owner: Uuid,
    pub(super) job: Uuid,
}

impl Binding {
    fn validate(self) -> Result<()> {
        if [self.attempt, self.generation, self.owner, self.job]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(err("HAYES_BINDING_INVALID"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Role {
    Generate,
    Validate,
}

impl Role {
    fn files(self) -> (&'static str, &'static str) {
        match self {
            Self::Generate => ("generate-launch.json", "generate-ready.json"),
            Self::Validate => ("validate-launch.json", "validate-ready.json"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u8,
    reservation_bytes: u64,
    binding: Binding,
    authority: Authority,
    journal: Physical,
    stage: Physical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Worker {
    binding: Binding,
    role: Role,
    nonce: Uuid,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    worker: Worker,
    // Diagnostic observation from the direct child handle, never re-adopted.
    child_pid: libc::pid_t,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct UnstartedTerminal {
    intent: Intent,
    outcome: &'static str,
}

struct PrivateDirectory {
    path: PathBuf,
    pin: DirectoryPin,
    fd: File,
}

impl PrivateDirectory {
    fn open(path: &Path) -> Result<Self> {
        let pin = DirectoryPin::open(path).map_err(|_| custody_error())?;
        let fd = pin.try_clone_file().map_err(|_| custody_error())?;
        let dir = Self {
            path: path.into(),
            pin,
            fd,
        };
        dir.identity()?;
        Ok(dir)
    }

    fn identity(&self) -> Result<Physical> {
        self.pin.check().map_err(|_| custody_error())?;
        let m = self.fd.metadata().map_err(|_| custody_error())?;
        if m.uid() != effective_uid() || m.mode() & 0o7777 != 0o700 {
            return Err(custody_error());
        }
        Ok(Physical::from(&m))
    }

    fn mkdir(&self, name: &str) -> Result<Self> {
        self.identity()?;
        let child = component(name);
        // The name is fixed or a UUID rendered locally.
        cvt(unsafe { libc::mkdirat(self.fd.as_raw_fd(), child.as_ptr(), 0o700) })
            .map_err(|_| custody_error())?;
        self.fd.sync_all().map_err(|_| custody_error())?;
        Self::open(&self.path.join(name))
    }

    fn open_file(&self, name: &str, flags: i32) -> io::Result<File> {
        let name = component(name);
        let fd = cvt(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn exists(&self, name: &str) -> Result<bool> {
        self.identity()?;
        let name = component(name);
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        match cvt(unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        }) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(custody_error()),
        }
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        serde_json::from_slice(&self.read_bytes(name)?).map_err(|_| custody_error())
    }

    fn read_bytes(&self, name: &str) -> Result<Vec<u8>> {
        self.identity()?;
        let file = self
            .open_file(name, libc::O_RDONLY)
            .map_err(|_| custody_error())?;
        private_file(&file)?;
        let mut bytes = Vec::new();
        file.take(MAX_RECORD + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| custody_error())?;
        if bytes.len() as u64 > MAX_RECORD {
            return Err(custody_error());
        }
        Ok(bytes)
    }

    fn create<T: Serialize>(&self, name: &str, record: &T) -> Result<()> {
        self.identity()?;
        let bytes = serde_json::to_vec(record).map_err(|_| custody_error())?;
        if bytes.len() as u64 > MAX_RECORD {
            return Err(custody_error());
        }
        let mut file = self
            .open_file(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
            .map_err(|_| custody_error())?;
        private_file(&file)?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| self.fd.sync_all())
            .map_err(|_| custody_error())?;
        self.identity()?;
        Ok(())
    }
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn component(name: &str) -> CString {
    debug_assert!(!name.is_empty() && !name.contains('/') && name != "." && name != "..");
    CString::new(name).expect("fixed Hayes file name or UUID")
}

fn cvt(value: i32) -> io::Result<i32> {
    if value == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

fn custody_error() -> crate::LifecycleError {
    err("HAYES_CUSTODY_UNAVAILABLE")
}

fn private_file(file: &File) -> Result<Physical> {
    let m = file.metadata().map_err(|_| custody_error())?;
    if !m.is_file() || m.nlink() != 1 || m.uid() != effective_uid() || m.mode() & 0o7777 != 0o600 {
        return Err(custody_error());
    }
    Ok(Physical::from(&m))
}

pub(super) struct Lease {
    authority: Authority,
    root: PrivateDirectory,
    // Only close this open description. LOCK_UN would unlock inherited copies.
    lock: File,
}

impl Lease {
    /// Each call, including restart, independently opens and locks the stable inode.
    pub(super) fn acquire(path: &Path, authority: Authority) -> Result<Self> {
        let root = PrivateDirectory::open(path)?;
        if authority.uid != effective_uid() || root.identity()? != authority.root {
            return Err(custody_error());
        }
        let lock = root
            .open_file(LOCK, libc::O_RDWR)
            .map_err(|_| custody_error())?;
        if private_file(&lock)? != authority.lock {
            return Err(custody_error());
        }
        match cvt(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(err("HAYES_RESERVATION_BUSY"));
            }
            Err(_) => return Err(custody_error()),
        }
        let lease = Self {
            authority,
            root,
            lock,
        };
        lease.check()?;
        Ok(lease)
    }

    fn check(&self) -> Result<()> {
        let named = self
            .root
            .open_file(LOCK, libc::O_RDONLY)
            .map_err(|_| custody_error())?;
        if self.root.identity()? != self.authority.root
            || private_file(&named)? != self.authority.lock
            || private_file(&self.lock)? != self.authority.lock
        {
            return Err(custody_error());
        }
        Ok(())
    }

    pub(super) fn begin(self, binding: Binding) -> Result<Attempt> {
        binding.validate()?;
        self.check()?;
        if self.root.exists(ACTIVE)? {
            return Err(err("HAYES_RESERVATION_RETAINED"));
        }
        let journal = self.root.mkdir(&binding.attempt.to_string())?;
        let stage = journal.mkdir("stage")?;
        let intent = Intent {
            version: 1,
            reservation_bytes: RESERVATION_BYTES,
            binding,
            authority: self.authority,
            journal: journal.identity()?,
            stage: stage.identity()?,
        };
        self.root.create(ACTIVE, &intent)?;
        let attempt = Attempt {
            lease: self,
            intent,
            journal,
            stage,
            launch_possible: false,
            source_started: false,
        };
        attempt.check()?;
        Ok(attempt)
    }

    /// Even a successful new flock cannot prove that native descendants drained.
    /// Never reconstruct GO, inspect PIDs, touch outputs or retire this reservation.
    pub(super) fn check_interrupted(&self, expected: Binding) -> Result<()> {
        expected.validate()?;
        self.check()?;
        let intent: Intent = self.root.read(ACTIVE)?;
        if intent.version != 1
            || intent.reservation_bytes != RESERVATION_BYTES
            || intent.binding != expected
            || intent.authority != self.authority
        {
            return Err(custody_error());
        }
        let journal = PrivateDirectory::open(&self.root.path.join(expected.attempt.to_string()))?;
        let stage = PrivateDirectory::open(&journal.path.join("stage"))?;
        if journal.identity()? != intent.journal || stage.identity()? != intent.stage {
            return Err(custody_error());
        }
        Err(err("HAYES_DRAIN_UNCONFIRMED"))
    }
}

pub(super) struct Attempt {
    lease: Lease,
    intent: Intent,
    journal: PrivateDirectory,
    stage: PrivateDirectory,
    launch_possible: bool,
    source_started: bool,
}

/// Pinned Manager-only input. The API cannot supply its path or open handle.
pub(super) struct SourceStage {
    binding: Binding,
    correlation: Uuid,
    observed: super::oex_read_fd::ObservedStage,
}

impl Attempt {
    fn check(&self) -> Result<()> {
        self.lease.check()?;
        if self.lease.root.read::<Intent>(ACTIVE)? != self.intent
            || self.journal.identity()? != self.intent.journal
            || self.stage.identity()? != self.intent.stage
        {
            return Err(custody_error());
        }
        Ok(())
    }

    /// The caller must own the authenticated operation and live DB lease.
    /// The same correlation and absolute deadline bind both data directions.
    pub(super) fn receive_source(
        &mut self,
        connection: &mut super::owner_channel::Connection,
        correlation: Uuid,
        deadline: Instant,
        cancelled: &AtomicBool,
        check_owner: &mut dyn FnMut() -> Result<()>,
    ) -> Result<SourceStage> {
        let mut frames = transfer::OwnerFrames::new(connection, deadline, cancelled, check_owner);
        let result = self.capture_source(&mut frames, correlation, cancelled);
        if result.is_err() {
            frames.invalidate();
        }
        result
    }

    fn capture_source(
        &mut self,
        frames: &mut impl transfer::Frames,
        correlation: Uuid,
        cancelled: &AtomicBool,
    ) -> Result<SourceStage> {
        self.check()?;
        if self.source_started || self.launch_possible {
            return Err(err("HAYES_TRANSFER_INVALID"));
        }
        self.source_started = true;
        let binding = self.intent.binding;
        let stage_id = binding.attempt.to_string();
        let flow = transfer::Transfer::source(binding.attempt, correlation)?;
        frames.check()?;
        let stage = super::oex_read_fd::Stage::create(&self.stage.fd, &stage_id)?;
        flow.receive(frames, &mut stage.materializer_write())?;
        let observed = super::oex_read_fd::observe_stage(
            stage,
            flow.bytes(),
            flow.sha256(),
            cancelled,
            &mut || frames.check(),
        )?;
        self.check()?;
        self.check_stage_file(binding.attempt, &observed)?;
        frames.check()?;
        flow.accept(frames)?;
        Ok(SourceStage {
            binding,
            correlation,
            observed,
        })
    }

    fn check_stage_file(
        &self,
        stage_id: Uuid,
        observed: &super::oex_read_fd::ObservedStage,
    ) -> Result<()> {
        let name = format!(".oex-stage-{stage_id}");
        let named = self
            .stage
            .open_file(&name, libc::O_RDONLY)
            .map_err(|_| custody_error())?;
        let identity = Physical {
            dev: observed.device,
            ino: observed.inode,
        };
        if private_file(&named)? != identity {
            return Err(custody_error());
        }
        observed.check_unchanged()
    }

    pub(super) fn before_spawn(&mut self, role: Role) -> Result<LaunchPermit> {
        self.check()?;
        if role == Role::Validate {
            // Generation drain has no trusted producer in this inactive branch.
            return Err(err("HAYES_DRAIN_UNCONFIRMED"));
        }
        if self.launch_possible {
            return Err(err("HAYES_WORKER_ALREADY_LAUNCHED"));
        }
        // Treat an incomplete write as a possible launch in this attempt.
        self.launch_possible = true;
        let worker = Worker {
            binding: self.intent.binding,
            role,
            nonce: Uuid::new_v4(),
        };
        self.journal.create(role.files().0, &worker)?;
        let lock_fd = self.lease.lock.try_clone().map_err(|_| custody_error())?;
        Ok(LaunchPermit::new(worker, lock_fd))
    }

    fn worker_record(&self, child: &SpawnedWorker) -> Result<Registration> {
        self.check()?;
        let worker = child.worker;
        if worker.binding != self.intent.binding
            || self.journal.read::<Worker>(worker.role.files().0)? != worker
        {
            return Err(err("HAYES_READY_INVALID"));
        }
        Ok(Registration {
            worker,
            child_pid: child.diagnostic_pid(),
        })
    }

    /// Reads from the exact spawn's private channel and durably registers
    /// it. A transport or storage error consumes the channel without GO.
    pub(super) fn register_ready(&mut self, child: &mut SpawnedWorker) -> Result<()> {
        self.register_ready_until(
            child,
            Instant::now() + std::time::Duration::from_secs(5),
            &mut || Ok(()),
        )
    }

    fn register_ready_until(
        &mut self,
        child: &mut SpawnedWorker,
        deadline: Instant,
        check: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        if child.phase != Phase::Awaiting {
            return Err(channel_error());
        }
        let owner = child.owner.take().ok_or_else(channel_error)?;
        let record = self.worker_record(child)?;
        child.require_live()?;
        launch::receive_ready(&owner, record.worker, deadline, check)?;
        child.require_live()?;
        self.journal.create(record.worker.role.files().1, &record)?;
        self.check()?;
        child.phase = Phase::Registered;
        child.owner = Some(owner);
        Ok(())
    }

    /// GO stays on that same channel and is attempted at most once, even
    /// when send fails. No raw authorization frame is returned to a caller.
    pub(super) fn send_go(&mut self, child: &mut SpawnedWorker) -> Result<()> {
        self.send_go_checked(child, &mut || Ok(()))
    }

    fn send_go_checked(
        &mut self,
        child: &mut SpawnedWorker,
        check: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        if child.phase != Phase::Registered {
            return Err(channel_error());
        }
        let owner = child.owner.take().ok_or_else(channel_error)?;
        let record = self.worker_record(child)?;
        if self
            .journal
            .read::<Registration>(record.worker.role.files().1)?
            != record
        {
            return Err(custody_error());
        }
        child.require_live()?;
        check()?;
        launch::send_go(&owner, record.worker)?;
        child.phase = Phase::Authorized;
        child.owner = Some(owner);
        Ok(())
    }

    /// The only terminal this inactive implementation can prove. Once any
    /// launch is possible, an OS containment and stage-custody adapter must
    /// supply the missing proof before a terminal or retirement can be added.
    pub(super) fn retire_unstarted(self) -> Result<()> {
        self.check()?;
        if self.launch_possible
            || self.journal.exists(Role::Generate.files().0)?
            || self.journal.exists(Role::Validate.files().0)?
            || fs::read_dir(&self.stage.path)
                .map_err(|_| custody_error())?
                .next()
                .is_some()
        {
            return Err(err("HAYES_DRAIN_UNCONFIRMED"));
        }
        self.journal.create(
            "terminal.json",
            &UnstartedTerminal {
                intent: self.intent,
                outcome: "aborted-before-launch",
            },
        )?;
        self.check()?;
        let name = component(ACTIVE);
        cvt(unsafe { libc::unlinkat(self.lease.root.fd.as_raw_fd(), name.as_ptr(), 0) })
            .map_err(|_| custody_error())?;
        self.lease.root.fd.sync_all().map_err(|_| custody_error())
    }
}

#[cfg(test)]
#[path = "native_runtime_hayes_tests.rs"]
pub(super) mod tests;
