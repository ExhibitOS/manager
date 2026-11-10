// SPDX-License-Identifier: Apache-2.0
//! Persistent local custody chosen from the effective user's OS account.

use super::*;
use std::{
    os::unix::fs::DirBuilderExt,
};

const CONTAINER: &str = "ExhibitOS Hayes Custody";
const ROOT: &str = "custody";
const AUTHORITY: &str = "authority.json";
const PROVISION_LOCK: &str = "provision.lock";

pub(in crate::native_runtime) fn acquire_user_lease() -> Result<Lease> {
    let support = crate::native_account_home::effective_home().ok_or_else(custody_error)?.join("Library/Application Support");
    let pin = DirectoryPin::open(&support).map_err(|_| custody_error())?;
    let base = support.join(CONTAINER);
    match fs::DirBuilder::new().mode(0o700).create(&base) {
        Ok(()) => pin.try_clone_file().map_err(|_| custody_error())?.sync_all().map_err(|_| custody_error())?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(custody_error()),
    }
    pin.check().map_err(|_| custody_error())?;
    acquire_in(&PrivateDirectory::open(&base)?)
}

pub(in crate::native_runtime) fn open_user_lease() -> Result<Lease> {
    let base = PrivateDirectory::open(&crate::native_account_home::effective_home().ok_or_else(custody_error)?.join("Library/Application Support").join(CONTAINER))?;
    let authority = base.read(AUTHORITY)?;
    Lease::acquire(&base.path.join(ROOT), authority)
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
