// SPDX-License-Identifier: Apache-2.0
//! Persistent local custody chosen from the effective user's OS account.

use super::*;
use crate::native_runtime::LocalAuthority;
use std::{
    os::unix::fs::DirBuilderExt,
};

const CONTAINER: &str = "ExhibitOS Hayes Custody";
const ROOT: &str = "custody";
const AUTHORITY: &str = "authority.json";
const PROVISION_LOCK: &str = "provision.lock";

pub(in crate::native_runtime) fn acquire_user_lease(owner: Option<&LocalAuthority>) -> Result<Lease> {
    let support = crate::native_account_home::effective_home().ok_or_else(custody_error)?.join("Library/Application Support");
    let pin = DirectoryPin::open(&support).map_err(|_| custody_error())?;
    let base = support.join(CONTAINER);
    match fs::DirBuilder::new().mode(0o700).create(&base) {
        Ok(()) => pin.try_clone_file().map_err(|_| custody_error())?.sync_all().map_err(|_| custody_error())?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(custody_error()),
    }
    pin.check().map_err(|_| custody_error())?;
    let base = custody_scope(PrivateDirectory::open(&base)?, owner, true)?;
    acquire_in(&base)
}

pub(in crate::native_runtime) fn open_user_lease(owner: Option<&LocalAuthority>) -> Result<Lease> {
    let base = PrivateDirectory::open(&crate::native_account_home::effective_home().ok_or_else(custody_error)?.join("Library/Application Support").join(CONTAINER))?;
    let base = custody_scope(base, owner, false)?;
    let authority = base.read(AUTHORITY)?;
    Lease::acquire(&base.path.join(ROOT), authority)
}

fn custody_scope(base: PrivateDirectory, owner: Option<&LocalAuthority>, create: bool) -> Result<PrivateDirectory> {
    let Some(owner) = owner else { return Ok(base); };
    let ids = [&owner.workspace_id, &owner.tenant_id, &owner.principal_id];
    if owner.owner_uid != effective_uid() || ids.iter().any(|id| {
        !Uuid::parse_str(id).is_ok_and(|uuid| !uuid.is_nil() && uuid.to_string() == **id)
    }) {
        return Err(custody_error());
    }
    let name = format!("owner-052-{}", crate::digest(ids.map(String::as_str).join("/").as_bytes()));
    if create && !base.exists(&name)? {
        return base.mkdir(&name);
    }
    PrivateDirectory::open(&base.path.join(name))
}

fn acquire_in(base: &PrivateDirectory) -> Result<Lease> {
    let lock = base.open_file(PROVISION_LOCK, libc::O_RDWR | libc::O_CREAT)
        .map_err(|_| custody_error())?;
    let identity = private_file(&lock)?;
    match cvt(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }) {
        Ok(_) => (),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(err("HAYES_RESERVATION_BUSY")),
        Err(_) => return Err(custody_error()),
    }
    let named = base.open_file(PROVISION_LOCK, libc::O_RDONLY).map_err(|_| custody_error())?;
    if private_file(&named)? != identity {
        return Err(custody_error());
    }
    let authority = if base.exists(AUTHORITY)? {
        base.read(AUTHORITY)?
    } else {
        // A partial provision is ambiguous. Preserve it for diagnosis.
        let root = base.mkdir(ROOT)?;
        let lock = root.open_file(LOCK, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
            .map_err(|_| custody_error())?;
        let authority = Authority {
            uid: effective_uid(),
            root: root.identity()?,
            lock: private_file(&lock)?,
        };
        lock.sync_all().and_then(|()| root.fd.sync_all()).map_err(|_| custody_error())?;
        base.create(AUTHORITY, &authority)?;
        authority
    };
    Lease::acquire(&base.path.join(ROOT), authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog52_owner_custody_is_independent_and_reopens_the_same_authority() {
        let path = fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("hayes-scoped-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let base = || PrivateDirectory::open(&path).unwrap();
        let owner = |workspace: Uuid| LocalAuthority {
            format: 1, workspace_id: workspace.to_string(), tenant_id: Uuid::from_u128(3).to_string(),
            principal_id: Uuid::from_u128(4).to_string(), owner_uid: effective_uid(),
            session_token: String::new(), csrf_token: String::new(),
        };
        let first = owner(Uuid::from_u128(1));
        let second = owner(Uuid::from_u128(2));
        let legacy = acquire_in(&base()).unwrap();
        let old = base().read_bytes(AUTHORITY).unwrap();
        assert!(custody_scope(base(), Some(&first), false).is_err());
        let first_scope = custody_scope(base(), Some(&first), true).unwrap();
        let second_scope = custody_scope(base(), Some(&second), true).unwrap();
        assert_ne!(first_scope.path, second_scope.path);
        let first_lease = acquire_in(&first_scope).unwrap();
        let second_lease = acquire_in(&second_scope).unwrap();
        assert_eq!(acquire_in(&first_scope).err().unwrap().code, "HAYES_RESERVATION_BUSY");
        let original_scope_authority = first_scope.read_bytes(AUTHORITY).unwrap();
        let authority_digest = crate::digest(&original_scope_authority);
        first_lease.check_preopen_custody(&authority_digest).unwrap();
        assert!(first_lease.check_preopen_custody(&"a".repeat(64)).is_err());
        let attempt_dir = first_scope.path.join(ROOT).join(Uuid::new_v4().to_string());
        fs::DirBuilder::new().mode(0o700).create(&attempt_dir).unwrap();
        assert!(first_lease.check_preopen_custody(&authority_digest).is_err());
        fs::remove_dir(attempt_dir).unwrap();
        drop(first_lease);
        let reopened = custody_scope(base(), Some(&first), false).unwrap();
        let lease = Lease::acquire(&reopened.path.join(ROOT), reopened.read(AUTHORITY).unwrap()).unwrap();
        assert_eq!(reopened.read_bytes(AUTHORITY).unwrap(), original_scope_authority);
        assert_eq!(base().read_bytes(AUTHORITY).unwrap(), old);
        drop((lease, legacy, second_lease));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn ordinary_hayes_provision_reopens_only_the_original_locked_custody() {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap()
            .join(format!("hayes-custody-provision-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let base = PrivateDirectory::open(&root).unwrap();
        let lease = acquire_in(&base).unwrap();
        let authority = base.read_bytes(AUTHORITY).unwrap();
        assert_eq!(acquire_in(&base).err().unwrap().code, "HAYES_RESERVATION_BUSY");
        drop(lease);
        drop(acquire_in(&base).unwrap());
        assert_eq!(base.read_bytes(AUTHORITY).unwrap(), authority);

        let custody = root.join(ROOT);
        fs::rename(custody.join(LOCK), custody.join("old.lock")).unwrap();
        fs::write(custody.join(LOCK), []).unwrap();
        assert!(acquire_in(&base).is_err());
        assert_eq!(base.read_bytes(AUTHORITY).unwrap(), authority);
        drop(base);
        fs::remove_dir_all(root).unwrap();
    }
}
