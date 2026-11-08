// SPDX-License-Identifier: Apache-2.0
//! Inactive independent owner observation. This does not prove a live lease.
use super::*;
use std::os::unix::fs::MetadataExt;

fn invalid() -> LifecycleError { err("NATIVE_OEX_ATTEMPT_INVALID") }
pub(super) const DATABASE_IDENTITY_SQL: &str = "SELECT system_identifier::text AS system_id,(SELECT oid::text FROM pg_database WHERE datname=current_database()) AS database_oid FROM pg_control_system()";

fn require(valid: bool, failure: &'static str, observe: &mut dyn FnMut(&'static str)) -> Result<()> {
    if !valid {
        observe(failure);
        return Err(invalid());
    }
    Ok(())
}

fn parse_database_identity(value: &str) -> Result<(String, String)> {
    let (system, oid) = value.split_once('|').ok_or_else(invalid)?;
    let canonical = |v: &str| v.parse::<u64>().ok().is_some_and(|n| n != 0 && n.to_string() == v);
    if !canonical(system) || !canonical(oid) || oid.parse::<u32>().is_err() {
        return Err(invalid());
    }
    Ok((system.into(), oid.into()))
}

#[allow(dead_code)]
pub(super) struct Owner {
    pin: crate::maintenance_source::DirectoryPin,
    path: PathBuf,
    device: u64,
    inode: u64,
    runtime: String,
    node: u32,
    postgres: u32,
    identity: (String, String, String),
    pub(super) system_id: String,
    pub(super) database_oid: String,
}

#[allow(dead_code)]
impl Owner {
    #[cfg(test)]
    pub(super) fn test_only(path: &Path) -> Self {
        let pin=crate::maintenance_source::DirectoryPin::open(path).unwrap();
        let stat=pin.try_clone_file().unwrap().metadata().unwrap();
        Self {pin,path:path.into(),device:stat.dev(),inode:stat.ino(),runtime:String::new(),node:0,postgres:0,
            identity:(String::new(),String::new(),String::new()),system_id:"123".into(),database_oid:"456".into()}
    }
    pub(super) fn store_identity(&self) -> (&Path, u64, u64) {
        (&self.path, self.device, self.inode)
    }
    /// A failed or timed-out query leaves any owned auxiliary in the Session;
    /// the caller must retain the fence and drain it using the existing lifecycle.
    pub(super) fn capture(session: &mut Session) -> Result<Self> {
        Self::capture_observed(session, &mut |_| {})
    }

    pub(super) fn capture_observed(session: &mut Session, observe: &mut dyn FnMut(&'static str)) -> Result<Self> {
        session.poll().inspect_err(|_| observe("capture-poll-failed"))?;
        session.check_native_observation_fence().inspect_err(|_| observe("capture-root-fence-failed"))?;
        require(session.state == "ready", "capture-session-not-ready", observe)?;
        require(session.auxiliary.is_none(), "capture-auxiliary-present", observe)?;
        require(!session.cancelled(), "capture-cancelled", observe)?;
        let runtime = session.job_id.clone().ok_or_else(|| { observe("capture-generation-missing"); invalid() })?;
        require(Uuid::parse_str(&runtime).is_ok_and(|id| id.to_string() == runtime),
            "capture-generation-invalid", observe)?;
        let node = session.node.as_ref().ok_or_else(|| { observe("capture-node-missing"); invalid() })?.child.id();
        let postgres = session.postgres.as_ref().ok_or_else(|| { observe("capture-postgres-missing"); invalid() })?.child.id();
        let identity = session.service.personal_identity().inspect_err(|_| observe("capture-personal-identity-failed"))?;
        let path = session.service.root.join("blobs");
        let pin = crate::maintenance_source::DirectoryPin::open(&path)
            .inspect_err(|_| observe("capture-blob-pin-failed"))?;
        let stat = pin.try_clone_file().and_then(|file| file.metadata().map_err(|_| invalid()))
            .inspect_err(|_| observe("capture-blob-metadata-failed"))?;
        let mut owner = Self { pin, path, device: stat.dev(), inode: stat.ino(), runtime, node, postgres, identity,
            system_id: String::new(), database_oid: String::new() };
        owner.check_observed(session, observe)?;
        observe("capture-database-query-start");
        let result = session.query_database("exhibitos", DATABASE_IDENTITY_SQL)
            .inspect_err(|_| observe("capture-database-query-failed"))?;
        (owner.system_id, owner.database_oid) = parse_database_identity(&result)
            .inspect_err(|_| observe("capture-database-identity-invalid"))?;
        observe("capture-database-identity-accepted");
        owner.check_observed(session, observe)?;
        Ok(owner)
    }

    /// Recheck live database identity at admission and settlement boundaries;
    /// a process ID alone does not exclude database replacement.
    pub(super) fn recheck_database_identity(&self, session: &mut Session) -> Result<()> {
        self.check(session)?;
        let value = session.query_database("exhibitos", DATABASE_IDENTITY_SQL)?;
        self.check_database_identity_result(&value)?;
        self.check(session)
    }

    pub(super) fn check_database_identity_result(&self, value: &str) -> Result<()> {
        if parse_database_identity(value)? != (self.system_id.clone(), self.database_oid.clone()) {
            return Err(invalid());
        }
        Ok(())
    }

    /// Checks the retained runtime and directory pins, not current DB identity.
    pub(super) fn check(&self, session: &mut Session) -> Result<()> {
        self.check_observed(session, &mut |_| {})
    }

    pub(super) fn check_observed(&self, session: &mut Session, observe: &mut dyn FnMut(&'static str)) -> Result<()> {
        session.poll().inspect_err(|_| observe("check-poll-failed"))?;
        session.check_native_observation_fence().inspect_err(|_| observe("check-root-fence-failed"))?;
        if let Some(binding) = session.active_furnishing_binding() {
            let writer = session.furnishing_writer_lifetime.as_ref().ok_or_else(|| {
                observe("check-writer-missing"); invalid()
            })?;
            let physical = writer.identity().inspect_err(|_| observe("check-writer-identity-failed"))?;
            require(physical == binding.physical, "check-writer-identity-mismatch", observe)?;
        }
        self.pin.check_path(&self.path).inspect_err(|_| observe("check-blob-pin-failed"))?;
        let stat = self.pin.try_clone_file().and_then(|file| file.metadata().map_err(|_| invalid()))
            .inspect_err(|_| observe("check-blob-metadata-failed"))?;
        let authority = session.local_authority.as_ref().ok_or_else(|| {
            observe("check-local-authority-missing"); invalid()
        })?;
        let identity = session.service.personal_identity().inspect_err(|_| observe("check-personal-identity-failed"))?;
        require(session.state == "ready", "check-session-not-ready", observe)?;
        require(session.auxiliary.is_none(), "check-auxiliary-present", observe)?;
        require(!session.cancelled(), "check-cancelled", observe)?;
        require(session.job_id.as_deref() == Some(self.runtime.as_str()), "check-generation-mismatch", observe)?;
        require(session.node.as_ref().map(|p| p.child.id()) == Some(self.node), "check-node-mismatch", observe)?;
        require(session.postgres.as_ref().map(|p| p.child.id()) == Some(self.postgres), "check-postgres-mismatch", observe)?;
        require(identity == self.identity, "check-personal-identity-mismatch", observe)?;
        require(authority.owner_uid == unsafe { libc::geteuid() }, "check-uid-mismatch", observe)?;
        require((&authority.workspace_id, &authority.tenant_id, &authority.principal_id)
            == (&identity.0, &identity.1, &identity.2), "check-local-scope-mismatch", observe)?;
        require(stat.is_dir() && (stat.dev(), stat.ino()) == (self.device, self.inode),
            "check-blob-identity-mismatch", observe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_identity_is_exact_and_bounded() {
        assert_eq!(parse_database_identity("18446744073709551615|4294967295").unwrap(), ("18446744073709551615".into(), "4294967295".into()));
        for bad in ["", "0|1", "1|0", "01|1", "1|01", "1|4294967296", "18446744073709551616|1", "1|2\n3|4", "1|2|3", " 1|2", "1|2 ", "-1|2"] {
            assert!(parse_database_identity(bad).is_err(), "accepted {bad:?}");
        }
    }
}
