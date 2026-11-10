// SPDX-License-Identifier: Apache-2.0
//! Canonical home bound to the current effective macOS account.
use std::{
    ffi::{CStr, OsStr},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::PathBuf,
};

pub(crate) fn effective_home() -> Option<PathBuf> {
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut record = std::ptr::null_mut();
    let mut buffer = [0u8; 16 * 1024];
    let uid = unsafe { libc::geteuid() };
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            account.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut record,
        )
    };
    if status != 0 || record.is_null() || uid == 0 {
        return None;
    }
    let home = unsafe { account.assume_init() }.pw_dir;
    if home.is_null() {
        return None;
    }
    let path = PathBuf::from(OsStr::from_bytes(
        unsafe { CStr::from_ptr(home) }.to_bytes(),
    ));
    if !path.is_absolute()
        || fs::canonicalize(&path).ok().as_ref() != Some(&path)
        || fs::metadata(&path).ok()?.uid() != uid
    {
        return None;
    }
    Some(path)
}
