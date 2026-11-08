// SPDX-License-Identifier: Apache-2.0
//! Same-owner observation of one already spawned direct child. Darwin does
//! not attach this watch to descendants or to unrelated delegated work.

use super::{cvt, err, launch::SpawnedWorker, Result};
use std::{
    cell::Cell,
    fs::File,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd},
    ptr,
};

const NOTES: u32 = libc::NOTE_FORK | libc::NOTE_EXEC | libc::NOTE_EXIT;
const NO_WAIT: libc::timespec = libc::timespec {
    tv_sec: 0,
    tv_nsec: 0,
};

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Live,
    Exited,
    Unknown,
}

pub(super) struct DirectWatch {
    queue: File,
    pid: libc::pid_t,
    state: Cell<State>,
}

impl DirectWatch {
    pub(super) fn arm(child: &mut SpawnedWorker) -> Result<Self> {
        // The exact direct-child handle is unreaped during registration.
        child.require_live()?;
        let pid = child.diagnostic_pid();
        let fd = cvt(unsafe { libc::kqueue() }).map_err(|_| unknown())?;
        let queue = unsafe { File::from_raw_fd(fd) };
        cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) }).map_err(|_| unknown())?;
        let registration = libc::kevent64_s {
            ident: pid as u64,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR | libc::EV_RECEIPT,
            fflags: NOTES,
            data: 0,
            udata: 0,
            ext: [0; 2],
        };
        let mut receipt = MaybeUninit::<libc::kevent64_s>::uninit();
        if unsafe { libc::kevent64(fd, &registration, 1, receipt.as_mut_ptr(), 1, 0, &NO_WAIT) }
            != 1
        {
            return Err(unknown());
        }
        let receipt = unsafe { receipt.assume_init() };
        if receipt.ident != pid as u64
            || receipt.filter != libc::EVFILT_PROC
            || receipt.flags & libc::EV_ERROR == 0
            || receipt.data != 0
        {
            return Err(unknown());
        }
        child.require_live()?;
        let watch = Self {
            queue,
            pid,
            state: Cell::new(State::Live),
        };
        watch.poll()?;
        if watch.state.get() != State::Live {
            return Err(unknown());
        }
        Ok(watch)
    }

    pub(super) fn poll(&self) -> Result<()> {
        if self.state.get() == State::Unknown {
            return Err(unknown());
        }
        let mut event = MaybeUninit::<libc::kevent64_s>::uninit();
        let result = unsafe {
            libc::kevent64(
                self.queue.as_raw_fd(),
                ptr::null(),
                0,
                event.as_mut_ptr(),
                1,
                0,
                &NO_WAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }
        if result != 1 {
            self.state.set(State::Unknown);
            return Err(unknown());
        }
        let event = unsafe { event.assume_init() };
        if event.ident != self.pid as u64
            || event.filter != libc::EVFILT_PROC
            || event.flags & libc::EV_ERROR != 0
            || self.state.get() != State::Live
        {
            self.state.set(State::Unknown);
            return Err(unknown());
        }
        // The kernel can coalesce FORK or EXEC with the terminal EXIT bit.
        // Handle both vetoes before accepting that terminal observation.
        if event.fflags & libc::NOTE_FORK != 0 {
            self.state.set(State::Unknown);
            return Err(err("HAYES_WORKER_FORK_OBSERVED"));
        }
        if event.fflags & libc::NOTE_EXEC != 0 {
            self.state.set(State::Unknown);
            return Err(err("HAYES_WORKER_EXEC_OBSERVED"));
        }
        if event.fflags != libc::NOTE_EXIT || event.flags & libc::EV_EOF == 0 {
            self.state.set(State::Unknown);
            return Err(unknown());
        }
        self.state.set(State::Exited);
        Ok(())
    }

    pub(super) fn require_exit(&self) -> Result<()> {
        self.poll()?;
        if self.state.get() != State::Exited {
            return Err(unknown());
        }
        Ok(())
    }
}

fn unknown() -> crate::LifecycleError {
    err("HAYES_WORKER_WATCH_UNKNOWN")
}
