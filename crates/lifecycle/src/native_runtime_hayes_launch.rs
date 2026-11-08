// SPDX-License-Identifier: Apache-2.0
//! Private direct-child transport for the inactive Hayes custody owner.

use super::{custody_error, cvt, err, private_file, Result, Worker};
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, net::UnixStream, process::ExitStatusExt},
    },
    path::Path,
    process::ExitStatus,
    ptr,
    time::{Duration, Instant},
};

const OWNER_FD: libc::c_int = 3;
const LOCK_FD: libc::c_int = 4;
const READY_DEADLINE: Duration = Duration::from_secs(5);

/// The native owner must supply the fixed, verified leaf program and argv
/// from its admitted payload. No public request may provide a command here.
pub(in super::super) struct LaunchPermit {
    pub(super) worker: Worker,
    lock: File,
}

impl LaunchPermit {
    pub(super) fn new(worker: Worker, lock: File) -> Self {
        Self { worker, lock }
    }

    /// Consumes the permit and gives exactly one spawned child the peer socket
    /// and a duplicate of the leased open description. Child stdio is inert
    /// in this draft. The child inherits the owner's process group.
    // A production caller must construct fixed argv from an admitted payload.
    // Keep this implementation private until that constructor exists.
    fn spawn(self, program: &Path, args: &[&OsStr]) -> Result<SpawnedWorker> {
        if !program.is_absolute() {
            return Err(err("HAYES_WORKER_START_FAILED"));
        }
        let (owner, child) = UnixStream::pair().map_err(|_| channel_error())?;
        let lock_identity = private_file(&self.lock)?;
        let lock_arg = format!("{}:{}", lock_identity.dev, lock_identity.ino);
        let nonce = self.worker.nonce.simple().to_string();
        let argv = std::iter::once(program.as_os_str())
            .chain(args.iter().copied())
            .chain([OsStr::new(&lock_arg), OsStr::new(&nonce)])
            .map(|arg| CString::new(arg.as_bytes()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| err("HAYES_WORKER_START_FAILED"))?;
        let pid = spawn_leaf(&argv, &child, &self.lock)?;
        Ok(SpawnedWorker {
            worker: self.worker,
            pid,
            phase: Phase::Awaiting,
            owner: Some(owner),
            status: None,
        })
    }

    #[cfg(test)]
    pub(in super::super) fn spawn_fixture(
        self,
        program: &Path,
        args: &[&OsStr],
    ) -> Result<SpawnedWorker> {
        self.spawn(program, args)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Phase {
    Awaiting,
    Registered,
    Authorized,
}

pub(in super::super) struct SpawnedWorker {
    pub(super) worker: Worker,
    pid: libc::pid_t,
    pub(super) phase: Phase,
    pub(super) owner: Option<UnixStream>,
    status: Option<ExitStatus>,
}

impl SpawnedWorker {
    /// For the durable record only. This number grants no recovery authority.
    pub(super) fn diagnostic_pid(&self) -> libc::pid_t {
        self.pid
    }

    /// Reap this direct child only. The caller must retain and reap the
    /// handle even when registration or GO fails. No group/drain inference.
    pub(in super::super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            let mut status = 0;
            let waited = cvt(unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) })?;
            if waited == self.pid {
                self.status = Some(ExitStatus::from_raw(status));
            }
        }
        Ok(self.status)
    }

    pub(super) fn require_live(&mut self) -> Result<()> {
        match self.try_wait() {
            Ok(None) => Ok(()),
            _ => Err(channel_error()),
        }
    }
}

pub(super) fn channel_error() -> crate::LifecycleError {
    err("HAYES_WORKER_CHANNEL_UNAVAILABLE")
}

pub(super) fn receive_ready(owner: &mut UnixStream, worker: Worker) -> Result<()> {
    let expected = format!("READY {}\n", worker.nonce.simple());
    // Read up to one extra byte, with one absolute deadline for all fragments.
    let deadline = Instant::now() + READY_DEADLINE;
    let mut frame = [0; 40]; // 6 byte prefix, 32 hex digits, LF, extra byte
    let mut length = 0;
    while length < expected.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(channel_error());
        }
        owner
            .set_read_timeout(Some(remaining))
            .map_err(|_| channel_error())?;
        let n = owner
            .read(&mut frame[length..])
            .map_err(|_| channel_error())?;
        if n == 0 {
            return Err(channel_error());
        }
        length += n;
        if !expected.as_bytes().starts_with(&frame[..length]) {
            return Err(err("HAYES_READY_INVALID"));
        }
    }
    require_idle(owner)
}

fn require_idle(owner: &UnixStream) -> Result<()> {
    let mut byte = 0u8;
    let received = unsafe {
        libc::recv(
            owner.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_DONTWAIT | libc::MSG_PEEK,
        )
    };
    match received {
        0 => Err(channel_error()),
        1 => Err(err("HAYES_READY_INVALID")),
        _ if io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock => Ok(()),
        _ => Err(channel_error()),
    }
}

pub(super) fn send_go(owner: &UnixStream, worker: Worker) -> Result<()> {
    require_idle(owner)?;
    let frame = format!("GO {}\n", worker.nonce.simple());
    let sent = unsafe {
        libc::send(
            owner.as_raw_fd(),
            frame.as_ptr().cast(),
            frame.len(),
            libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
        )
    };
    if sent != frame.len() as isize {
        return Err(channel_error());
    }
    Ok(())
}

struct SpawnOptions {
    actions: libc::posix_spawn_file_actions_t,
    attributes: libc::posix_spawnattr_t,
}

impl SpawnOptions {
    fn new() -> Result<Self> {
        let mut options = Self {
            actions: ptr::null_mut(),
            attributes: ptr::null_mut(),
        };
        spawn_ok(unsafe { libc::posix_spawn_file_actions_init(&mut options.actions) })?;
        spawn_ok(unsafe { libc::posix_spawnattr_init(&mut options.attributes) })?;
        spawn_ok(unsafe {
            libc::posix_spawnattr_setflags(
                &mut options.attributes,
                libc::POSIX_SPAWN_CLOEXEC_DEFAULT as libc::c_short,
            )
        })?;
        for fd in [0, 1, 2] {
            spawn_ok(unsafe {
                libc::posix_spawn_file_actions_addopen(
                    &mut options.actions,
                    fd,
                    c"/dev/null".as_ptr(),
                    libc::O_RDWR,
                    0,
                )
            })?;
        }
        Ok(options)
    }
}

impl Drop for SpawnOptions {
    fn drop(&mut self) {
        unsafe {
            if !self.actions.is_null() {
                libc::posix_spawn_file_actions_destroy(&mut self.actions);
            }
            if !self.attributes.is_null() {
                libc::posix_spawnattr_destroy(&mut self.attributes);
            }
        }
    }
}

fn spawn_ok(status: libc::c_int) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(err("HAYES_WORKER_START_FAILED"))
    }
}

fn spawn_leaf(argv: &[CString], peer: &UnixStream, lock: &File) -> Result<libc::pid_t> {
    let mut options = SpawnOptions::new()?;
    // Keep both sources above all target FDs until posix_spawn returns.
    // dup2 only happens in the child; unrelated inherited FDs are closed.
    let duplicate = |fd| -> Result<File> {
        let copy = cvt(unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, LOCK_FD + 1) })
            .map_err(|_| custody_error())?;
        Ok(unsafe { File::from_raw_fd(copy) })
    };
    let sources = [duplicate(peer.as_raw_fd())?, duplicate(lock.as_raw_fd())?];
    for (source, target) in sources.iter().zip([OWNER_FD, LOCK_FD]) {
        spawn_ok(unsafe {
            libc::posix_spawn_file_actions_adddup2(&mut options.actions, source.as_raw_fd(), target)
        })?;
    }
    let mut args: Vec<_> = argv.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    args.push(ptr::null_mut());
    let env = [ptr::null_mut()];
    let mut pid = 0;
    spawn_ok(unsafe {
        libc::posix_spawn(
            &mut pid,
            argv[0].as_ptr(),
            &options.actions,
            &options.attributes,
            args.as_ptr(),
            env.as_ptr(),
        )
    })?;
    Ok(pid)
}
