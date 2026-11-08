// SPDX-License-Identifier: Apache-2.0
//! Persistent local custody chosen from the effective user's OS account.

use super::*;
use std::{
    ffi::{CStr, OsStr},
    os::unix::{ffi::OsStrExt, fs::DirBuilderExt},
};

const CONTAINER: &str = "ExhibitOS Hayes Custody";
const ROOT: &str = "custody";
const AUTHORITY: &str = "authority.json";
const PROVISION_LOCK: &str = "provision.lock";

fn account_home() -> Result<PathBuf> {
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut record = std::ptr::null_mut();
    let mut buffer = [0u8; 16 * 1024];
    let uid = effective_uid();
    let status = unsafe {
        libc::getpwuid_r(uid, account.as_mut_ptr(), buffer.as_mut_ptr().cast(), buffer.len(), &mut record)
    };
    if status != 0 || record.is_null() || uid == 0 {
        return Err(custody_error());
    }
    let home = unsafe { account.assume_init() }.pw_dir;
    if home.is_null() {
        return Err(custody_error());
    }
    let path = PathBuf::from(OsStr::from_bytes(unsafe { CStr::from_ptr(home) }.to_bytes()));
    if !path.is_absolute() || fs::canonicalize(&path).ok().as_ref() != Some(&path)
        || fs::metadata(&path).map_err(|_| custody_error())?.uid() != uid
    {
        return Err(custody_error());
    }
    Ok(path)
}

pub(in crate::native_runtime) fn acquire_user_lease() -> Result<Lease> {
    let support = account_home()?.join("Library/Application Support");
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
