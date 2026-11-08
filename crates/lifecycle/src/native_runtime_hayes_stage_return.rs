// SPDX-License-Identifier: Apache-2.0
//! Local return of the four retained stage copies under the original lock.

use super::{
    component, cvt, err, fs, io, Attempt, Binding, Physical, PrivateDirectory, Result, ACTIVE,
};
use crate::native_runtime::oex_read_fd::ObservedStage;
use serde::Serialize;
use std::os::{
    fd::{AsRawFd, IntoRawFd},
    unix::fs::MetadataExt,
};
use uuid::Uuid;

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StageIdentity {
    pub(super) id: Uuid,
    pub(super) physical: Physical,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

/// In order: private original copy, Generate stdout, selected GLB,
/// Validate stdout. Every handle is taken from the same live readback.
pub(super) struct RetainedStages([(Uuid, ObservedStage); 4]);

/// Local stages have returned. The active reservation and original lock
/// still belong to the Attempt. This cannot be deserialized on restart.
pub(super) struct ReturnedStages {
    binding: Binding,
    inventory: [StageIdentity; 4],
}

pub(super) struct ClosedCustody {
    journal: PrivateDirectory,
    inventory: [StageIdentity; 4],
}

impl ClosedCustody {
    pub(super) fn journal(&self) -> &PrivateDirectory {
        &self.journal
    }

    pub(super) fn inventory(&self) -> &[StageIdentity; 4] {
        &self.inventory
    }
}

impl RetainedStages {
    pub(super) fn new(stages: [(Uuid, ObservedStage); 4]) -> Self {
        Self(stages)
    }

    pub(super) fn inventory(&self, attempt: &Attempt) -> Result<[StageIdentity; 4]> {
        attempt.check()?;
        if self.0[0].0 != attempt.intent.binding.attempt
            || fs::read_dir(&attempt.stage.path)
                .map_err(|_| uncertain())?
                .take(5)
                .collect::<io::Result<Vec<_>>>()
                .map_err(|_| uncertain())?
                .len()
                != 4
        {
            return Err(uncertain());
        }
        for (index, (id, stage)) in self.0.iter().enumerate() {
            if self.0[..index].iter().any(|(prior_id, prior)| {
                prior_id == id || (prior.device, prior.inode) == (stage.device, stage.inode)
            }) {
                return Err(uncertain());
            }
            attempt.check_stage_file(*id, stage)?;
        }
        attempt.check()?;
        Ok(self.0.each_ref().map(|(id, stage)| StageIdentity {
            id: *id,
            physical: Physical {
                dev: stage.device,
                ino: stage.inode,
            },
            bytes: stage.bytes,
            sha256: stage.sha256.clone(),
        }))
    }

    /// The caller must have fenced the live Session and durably recorded its
    /// release intent. The callback checks that authority before each unlink.
    /// The original lock serializes cooperating writers. Any partial error
    /// leaves the active reservation in place for unresolved custody.
    pub(super) fn return_owned(
        self,
        attempt: &Attempt,
        check: &mut dyn FnMut(&Attempt) -> Result<()>,
    ) -> Result<ReturnedStages> {
        check(attempt)?;
        let inventory = self.inventory(attempt)?;
        for (id, stage) in self.0 {
            check(attempt)?;
            attempt.check()?;
            let before = stage.file().metadata().map_err(|_| uncertain())?;
            attempt.check_stage_file(id, &stage)?;
            let name = format!(".oex-stage-{id}");
            let component = component(&name);
            cvt(unsafe { libc::unlinkat(attempt.stage.fd.as_raw_fd(), component.as_ptr(), 0) })
                .map_err(|_| uncertain())?;
            let after = stage.file().metadata().map_err(|_| uncertain())?;
            if (after.dev(), after.ino(), after.nlink()) != (stage.device, stage.inode, 0)
                || (after.len(), after.mtime(), after.mtime_nsec())
                    != (stage.bytes, before.mtime(), before.mtime_nsec())
                || attempt.stage.exists(&name)?
            {
                return Err(uncertain());
            }
            stage.close_owned()?;
        }
        attempt.stage.fd.sync_all().map_err(|_| uncertain())?;
        if fs::read_dir(&attempt.stage.path)
            .map_err(|_| uncertain())?
            .next()
            .is_some()
        {
            return Err(uncertain());
        }
        check(attempt)?;
        attempt.check()?;
        Ok(ReturnedStages {
            binding: attempt.intent.binding,
            inventory,
        })
    }
}

impl ReturnedStages {
    pub(super) fn close_custody(
        self,
        attempt: Attempt,
        check: &mut dyn FnMut(&Attempt) -> Result<()>,
    ) -> Result<ClosedCustody> {
        check(&attempt)?;
        attempt.check()?;
        if self.binding != attempt.intent.binding
            || fs::read_dir(&attempt.stage.path)
                .map_err(|_| uncertain())?
                .next()
                .is_some()
        {
            return Err(uncertain());
        }
        let name = component(ACTIVE);
        cvt(unsafe { libc::unlinkat(attempt.lease.root.fd.as_raw_fd(), name.as_ptr(), 0) })
            .map_err(|_| uncertain())?;
        attempt.lease.root.fd.sync_all().map_err(|_| uncertain())?;
        attempt.lease.check()?;
        if attempt.lease.root.exists(ACTIVE)? {
            return Err(uncertain());
        }
        let Attempt { lease, journal, .. } = attempt;
        let fd = lease.lock.into_raw_fd();
        cvt(unsafe { libc::close(fd) }).map_err(|_| uncertain())?;
        Ok(ClosedCustody {
            journal,
            inventory: self.inventory,
        })
    }
}

fn uncertain() -> crate::LifecycleError {
    err("HAYES_STAGE_RETURN_UNCONFIRMED")
}
