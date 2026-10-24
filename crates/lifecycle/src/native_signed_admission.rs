// SPDX-License-Identifier: Apache-2.0
//! Release payloads belong to this running, officially signed native app.
//! Manifest flags are declarations, not a substitute for either signature check.
//! Notarization, redistribution notices and installed/offline acceptance remain
//! release-pipeline evidence; this module neither submits nor changes trust.
use crate::{Result, err};
use std::{
    ffi::{OsStr, c_void},
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

type CFRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: CFRef);
    fn CFStringCreateWithBytes(
        allocator: CFRef,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external: u8,
    ) -> CFRef;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFRef,
        bytes: *const u8,
        length: isize,
        directory: u8,
    ) -> CFRef;
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut CFRef) -> i32;
    fn SecCodeCheckValidity(code: CFRef, flags: u32, requirement: CFRef) -> i32;
    fn SecRequirementCreateWithString(text: CFRef, flags: u32, requirement: *mut CFRef) -> i32;
    fn SecStaticCodeCreateWithPath(path: CFRef, flags: u32, code: *mut CFRef) -> i32;
    fn SecStaticCodeCheckValidity(code: CFRef, flags: u32, requirement: CFRef) -> i32;
}

// Public identity of the existing Manager distribution. This is not a signing
// credential. Apple Development/ad-hoc signatures cannot satisfy these rules.
const APP_ID: &str = "org.exhibitos.manager";
const HELPER_ID: &str = "org.exhibitos.manager.native";
const TEAM_ID: &str = "W563ZQD9G6";
const UTF8: u32 = 0x0800_0100;
const ALL_ARCHITECTURES: u32 = 1 << 0;
const NESTED_CODE: u32 = 1 << 3;
const STRICT_VALIDATE: u32 = 1 << 4;

struct Owned(CFRef);
impl Owned {
    fn checked(status: i32, value: CFRef) -> Result<Self> {
        let owned = Self(value);
        if status != 0 || value.is_null() {
            Err(err("NATIVE_RUNTIME_SIGNATURE_INVALID"))
        } else {
            Ok(owned)
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // Every non-null instance comes from a CoreFoundation Create/Copy
            // API with one retained reference and never escapes this call.
            unsafe { CFRelease(self.0) };
        }
    }
}
fn requirement(identifier: &str) -> Result<Owned> {
    let text = format!(
        "anchor apple generic and identifier \"{identifier}\" and certificate leaf[subject.OU] = \"{TEAM_ID}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
    );
    // The only identifiers passed here are the two fixed constants above.
    let string = Owned::checked(0, unsafe {
        CFStringCreateWithBytes(ptr::null(), text.as_ptr(), text.len() as isize, UTF8, 0)
    })?;
    let mut value = ptr::null();
    // The retained CFString stays alive throughout requirement compilation.
    let status = unsafe { SecRequirementCreateWithString(string.0, 0, &mut value) };
    Owned::checked(status, value)
}
fn application(payload: &Path, executable: &Path) -> Result<PathBuf> {
    let invalid = || err("NATIVE_RUNTIME_APP_REQUIRED");
    if !payload.is_absolute() || payload.file_name() != Some(OsStr::new("native-runtime")) {
        return Err(invalid());
    }
    let resources = payload.parent().ok_or_else(invalid)?;
    let contents = resources.parent().ok_or_else(invalid)?;
    let app = contents.parent().ok_or_else(invalid)?;
    if resources.file_name() != Some(OsStr::new("Resources"))
        || contents.file_name() != Some(OsStr::new("Contents"))
        || app.extension() != Some(OsStr::new("app"))
    {
        return Err(invalid());
    }
    if fs::canonicalize(payload).map_err(|_| invalid())? != payload {
        return Err(invalid());
    }
    let helpers = contents.join("Helpers");
    for path in [app, contents, resources, payload, helpers.as_path()] {
        let metadata = fs::symlink_metadata(path).map_err(|_| invalid())?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(invalid());
        }
    }
    let helper = helpers.join("exhibitos-native");
    let metadata = fs::symlink_metadata(&helper).map_err(|_| invalid())?;
    if metadata.is_symlink()
        || !metadata.is_file()
        || fs::canonicalize(&helper).map_err(|_| invalid())?
            != fs::canonicalize(executable).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    Ok(app.to_owned())
}

pub(crate) fn admit(payload: &Path) -> Result<()> {
    let executable = std::env::current_exe().map_err(|_| err("NATIVE_RUNTIME_APP_REQUIRED"))?;
    let app = application(payload, &executable)?;
    let helper_requirement = requirement(HELPER_ID)?;
    let app_requirement = requirement(APP_ID)?;
    let mut code = ptr::null();
    let status = unsafe { SecCodeCopySelf(0, &mut code) };
    let running = Owned::checked(status, code)?;
    // Validate the running helper, not just the file currently at its path.
    // https://developer.apple.com/documentation/security/seccodecopyself(_:_:)
    if unsafe { SecCodeCheckValidity(running.0, 0, helper_requirement.0) } != 0 {
        return Err(err("NATIVE_RUNTIME_SIGNATURE_INVALID"));
    }
    let bytes = app.as_os_str().as_bytes();
    let url = Owned::checked(0, unsafe {
        CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        )
    })?;
    let mut code = ptr::null();
    let status = unsafe { SecStaticCodeCreateWithPath(url.0, 0, &mut code) };
    let bundle = Owned::checked(status, code)?;
    // All slices, sealed resources and standard nested code are checked. No
    // do-not-validate flag or allow-network flag is supplied. This validates
    // the observed bundle; it does not freeze a concurrently modified disk.
    // https://developer.apple.com/documentation/security/static-code-validation-flags
    if unsafe {
        SecStaticCodeCheckValidity(
            bundle.0,
            ALL_ARCHITECTURES | NESTED_CODE | STRICT_VALIDATE,
            app_requirement.0,
        )
    } != 0
    {
        return Err(err("NATIVE_RUNTIME_SIGNATURE_INVALID"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn release_layout_binds_the_exact_helper_and_rejects_external_links() {
        let root =
            std::env::temp_dir().join(format!("native-signed-layout-{}", uuid::Uuid::new_v4()));
        let app = root.join("Manager.app");
        let payload = app.join("Contents/Resources/native-runtime");
        let helper = app.join("Contents/Helpers/exhibitos-native");
        fs::create_dir_all(&payload).unwrap();
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"synthetic").unwrap();
        let payload = fs::canonicalize(payload).unwrap();
        assert_eq!(
            application(&payload, &helper).unwrap(),
            fs::canonicalize(&app).unwrap()
        );
        let external = root.join("outside");
        fs::write(&external, b"not the app helper").unwrap();
        assert!(application(&payload, &external).is_err());
        fs::remove_file(&helper).unwrap();
        symlink(&external, &helper).unwrap();
        assert!(application(&payload, &external).is_err());
        assert!(application(&root, &external).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn requirements_compile_and_the_test_process_cannot_impersonate_the_pinned_helper() {
        let helper = requirement(HELPER_ID).unwrap();
        let _app = requirement(APP_ID).unwrap();
        let mut code = ptr::null();
        let status = unsafe { SecCodeCopySelf(0, &mut code) };
        let running = Owned::checked(status, code).unwrap();
        assert_ne!(unsafe { SecCodeCheckValidity(running.0, 0, helper.0) }, 0);
    }
}
