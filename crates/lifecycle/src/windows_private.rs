// SPDX-License-Identifier: Apache-2.0
//! Owner-only NTFS objects for the future managed Windows profile adapter.
//! Existing user directories/ACLs are inspected, never rewritten or adopted.
#[path = "windows_json_storage.rs"]
mod json_storage;
use crate::{Result, err};
pub(crate) use json_storage::{read_json_path, write_json_root};
use std::{
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        self as sec,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        },
    },
    Storage::FileSystem::{self as fsapi, BY_HANDLE_FILE_INFORMATION},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Sid(Vec<u32>);
impl Sid {
    fn ptr(&self) -> sec::PSID {
        self.0.as_ptr().cast_mut().cast()
    }
    fn current() -> Result<Self> {
        Self::from_token(sec::TokenUser, mem::size_of::<sec::TOKEN_USER>())
    }
    fn default_owner() -> Result<Self> {
        Self::from_token(sec::TokenOwner, mem::size_of::<sec::TOKEN_OWNER>())
    }
    fn from_token(class: sec::TOKEN_INFORMATION_CLASS, minimum: usize) -> Result<Self> {
        let mut token: HANDLE = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), sec::TOKEN_QUERY, &mut token) } == 0 {
            return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
        }
        let result = (|| {
            let mut bytes = 0;
            unsafe {
                sec::GetTokenInformation(token, class, ptr::null_mut(), 0, &mut bytes);
            }
            if bytes < minimum as u32 || bytes > 65536 {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            let capacity = bytes;
            let mut storage = vec![0usize; (bytes as usize).div_ceil(mem::size_of::<usize>())];
            if unsafe {
                sec::GetTokenInformation(
                    token,
                    class,
                    storage.as_mut_ptr().cast(),
                    bytes,
                    &mut bytes,
                )
            } == 0
            {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            if bytes < minimum as u32 || bytes > capacity {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            // Both TOKEN_USER and TOKEN_OWNER begin with a SID pointer; the
            // class-specific minimum and bounded SID tail are checked first.
            let token_sid = unsafe { *storage.as_ptr().cast::<sec::PSID>() };
            let start = storage.as_ptr() as usize;
            let sid_start = token_sid as usize;
            if sid_start < start
                || sid_start
                    .checked_add(8)
                    .is_none_or(|end| end > start + bytes as usize)
            {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            let prefix = token_sid.cast::<u8>();
            let required = 8 + 4 * usize::from(unsafe { *prefix.add(1) });
            if sid_start
                .checked_add(required)
                .is_none_or(|end| end > start + bytes as usize)
            {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            if unsafe { sec::IsValidSid(token_sid) } == 0 {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            let len = unsafe { sec::GetLengthSid(token_sid) };
            if len == 0 || len > 256 {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            let mut sid = Self(vec![0; (len as usize).div_ceil(4)]);
            if unsafe { sec::CopySid(len, sid.0.as_mut_ptr().cast(), token_sid) } == 0 {
                return Err(err("WINDOWS_PROFILE_TOKEN_UNAVAILABLE"));
            }
            Ok(sid)
        })();
        unsafe {
            CloseHandle(token);
        }
        result
    }
    fn text_ptr(sid: sec::PSID) -> Result<String> {
        let mut text = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        let _memory = Local(text.cast());
        let mut len = 0;
        while len < 256 && unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        if len == 256 {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) })
            .map_err(|_| err("WINDOWS_PROFILE_ACL_INVALID"))
    }
}
fn wide(path: &Path) -> Result<Vec<u16>> {
    if !path.is_absolute() {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    value.push(0);
    Ok(value)
}
fn descriptor(sid: &Sid, directory: bool) -> Result<Local> {
    let text = format!(
        "O:{}D:P(A;{};FA;;;{})",
        Sid::text_ptr(sid.ptr())?,
        if directory { "OICI" } else { "" },
        Sid::text_ptr(sid.ptr())?
    );
    let text: Vec<_> = text.encode_utf16().chain([0]).collect();
    let mut sd = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    Ok(Local(sd))
}
fn acl(file: &File, sid: &Sid, private: bool) -> Result<()> {
    acl_owner(file, sid, private, None)
}
/// Public input may use this token's default group owner, unlike private state.
fn public_acl(file: &File, sid: &Sid) -> Result<()> {
    let default_owner = Sid::default_owner()?;
    acl_owner(file, sid, false, Some(&default_owner))
}
fn acl_owner(file: &File, sid: &Sid, private: bool, default_owner: Option<&Sid>) -> Result<()> {
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut sd = ptr::null_mut();
    if unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            sec::OWNER_SECURITY_INFORMATION | sec::DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut sd,
        )
    } != 0
    {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    let _memory = Local(sd);
    if owner.is_null()
        || dacl.is_null()
        || unsafe { sec::IsValidSid(owner) } == 0
        || (unsafe { sec::EqualSid(owner, sid.ptr()) } == 0
            && !default_owner
                .is_some_and(|value| unsafe { sec::EqualSid(owner, value.ptr()) } != 0))
        || unsafe { sec::IsValidAcl(dacl) } == 0
    {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { sec::GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0
        || private && control & sec::SE_DACL_PROTECTED == 0
    {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    let count = unsafe { (*dacl).AceCount };
    if count > 1024 {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    let mut owner_full = false;
    for i in 0..count {
        let mut raw = ptr::null_mut();
        if unsafe { sec::GetAce(dacl, i.into(), &mut raw) } == 0 || raw.is_null() {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        let header = unsafe { &*raw.cast::<sec::ACE_HEADER>() };
        if header.AceFlags & sec::INHERIT_ONLY_ACE as u8 != 0 || header.AceType == 1 {
            continue;
        }
        if header.AceType != 0
            || usize::from(header.AceSize) < mem::size_of::<sec::ACCESS_ALLOWED_ACE>()
        {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        let ace = unsafe { &*raw.cast::<sec::ACCESS_ALLOWED_ACE>() };
        let grant: sec::PSID = ptr::addr_of!(ace.SidStart).cast_mut().cast();
        // SID prefix is eight bytes; check its variable tail before native inspection.
        let offset = mem::offset_of!(sec::ACCESS_ALLOWED_ACE, SidStart);
        let available = usize::from(header.AceSize).saturating_sub(offset);
        if available < 8 {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        let sid_bytes = grant.cast::<u8>();
        let sid_len = 8 + 4 * usize::from(unsafe { *sid_bytes.add(1) });
        if sid_len > available {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        if unsafe { sec::IsValidSid(grant) } == 0 {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        }
        if unsafe { sec::EqualSid(grant, sid.ptr()) } != 0 {
            owner_full |= ace.Mask & fsapi::FILE_ALL_ACCESS == fsapi::FILE_ALL_ACCESS
                || ace.Mask & 0x10000000 != 0;
        } else if private {
            return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
        } else {
            let privileged = matches!(Sid::text_ptr(grant)?.as_str(), "S-1-5-18" | "S-1-5-32-544");
            // Includes DELETE_CHILD, writes, DELETE, WRITE_DAC/OWNER and generic writes/all.
            if !privileged && ace.Mask & 0x500d0156 != 0 {
                return Err(err("WINDOWS_PROFILE_PARENT_UNSAFE"));
            }
        }
    }
    if !owner_full {
        return Err(err("WINDOWS_PROFILE_ACL_INVALID"));
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    volume: u32,
    index_high: u32,
    index_low: u32,
}
fn identity(file: &File, directory: bool) -> Result<Identity> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    if unsafe { fsapi::GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0
        || info.dwFileAttributes & fsapi::FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & fsapi::FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || !directory && info.nNumberOfLinks != 1
    {
        return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
    }
    Ok(Identity {
        volume: info.dwVolumeSerialNumber,
        index_high: info.nFileIndexHigh,
        index_low: info.nFileIndexLow,
    })
}
fn open(path: &Path, directory: bool) -> Result<File> {
    let path = wide(path)?;
    let handle = unsafe {
        fsapi::CreateFileW(
            path.as_ptr(),
            fsapi::READ_CONTROL | fsapi::FILE_READ_ATTRIBUTES,
            fsapi::FILE_SHARE_READ
                | fsapi::FILE_SHARE_WRITE
                | if directory {
                    0
                } else {
                    fsapi::FILE_SHARE_DELETE
                },
            ptr::null(),
            fsapi::OPEN_EXISTING,
            fsapi::FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    fsapi::FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(err("WINDOWS_PROFILE_HANDLE_UNAVAILABLE"));
    }
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}
/// Pins parent and new private directory without FILE_SHARE_DELETE. No existing ACL edits.
pub struct PrivateDirectory {
    path: PathBuf,
    file: File,
    parent: File,
    parent_path: PathBuf,
    id: Identity,
    parent_id: Identity,
    sid: Sid,
    ancestors: Vec<(PathBuf, File, Identity)>,
}
impl PrivateDirectory {
    pub fn create(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        let original_parent = path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let ancestors = pin_ancestors(original_parent)?;
        let parent_path = path
            .parent()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
            .canonicalize()
            .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let leaf = path
            .file_name()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let leaf_text = leaf.to_str().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        valid_name(leaf_text)?;
        let path = parent_path.join(leaf);
        let parent = open(&parent_path, true)?;
        let sid = Sid::current()?;
        acl(&parent, &sid, false)?;
        let parent_id = identity(&parent, true)?;
        let sd = descriptor(&sid, true)?;
        let attributes = sec::SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<sec::SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        if unsafe { fsapi::CreateDirectoryW(wide(&path)?.as_ptr(), &attributes) } == 0 {
            return Err(err("WINDOWS_PROFILE_CREATE_REFUSED"));
        }
        let path = path
            .canonicalize()
            .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let file = open(&path, true)?;
        acl(&file, &sid, true)?;
        let id = identity(&file, true)?;
        let result = Self {
            path,
            file,
            parent,
            parent_path,
            id,
            parent_id,
            sid,
            ancestors,
        };
        result.check()?;
        Ok(result)
    }
    /// Inspect an existing protected owner-only directory without changing permissions.
    /// This is not an identity witness across backup restore or namespace replacement.
    pub(crate) fn inspect(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        let original_parent = path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let ancestors = pin_ancestors(original_parent)?;
        let parent_path = original_parent
            .canonicalize()
            .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let leaf = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        valid_name(leaf)?;
        let path = parent_path.join(leaf);
        let parent = open(&parent_path, true)?;
        let sid = Sid::current()?;
        acl(&parent, &sid, false)?;
        let parent_id = identity(&parent, true)?;
        // Open lexical leaf before canonicalizing: junctions cannot be silently resolved.
        let file = open(&path, true)?;
        acl(&file, &sid, true)?;
        let id = identity(&file, true)?;
        let path = path
            .canonicalize()
            .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let value = Self {
            path,
            file,
            parent,
            parent_path,
            id,
            parent_id,
            sid,
            ancestors,
        };
        value.check()?;
        Ok(value)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn check(&self) -> Result<()> {
        for (path, held, expected) in &self.ancestors {
            if identity(held, true)? != *expected
                || identity(&open(path, true)?, true)? != *expected
            {
                return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
            }
        }
        acl(&self.parent, &self.sid, false)?;
        acl(&self.file, &self.sid, true)?;
        if identity(&self.parent, true)? != self.parent_id
            || identity(&self.file, true)? != self.id
            || identity(&open(&self.parent_path, true)?, true)? != self.parent_id
            || identity(&open(&self.path, true)?, true)? != self.id
        {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
    pub fn create_record(&self, name: &str) -> Result<File> {
        self.create_record_sharing(name, fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE)
    }
    fn create_record_sharing(&self, name: &str, sharing: u32) -> Result<File> {
        self.create_record_access(name, sharing, 0)
    }
    fn create_record_access(&self, name: &str, sharing: u32, extra_access: u32) -> Result<File> {
        self.check()?;
        valid_name(name)?;
        let path = self.path.join(name);
        let sd = descriptor(&self.sid, false)?;
        let attributes = sec::SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<sec::SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let handle = unsafe {
            fsapi::CreateFileW(
                wide(&path)?.as_ptr(),
                0x80000000 | 0x40000000 | fsapi::READ_CONTROL | extra_access,
                sharing,
                &attributes,
                fsapi::CREATE_NEW,
                fsapi::FILE_ATTRIBUTE_NORMAL | fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(err("WINDOWS_PROFILE_CREATE_REFUSED"));
        }
        let file = unsafe { File::from_raw_handle(handle.cast()) };
        self.check_record(&file, name)?;
        Ok(file)
    }
    pub fn check_record(&self, file: &File, name: &str) -> Result<()> {
        valid_name(name)?;
        self.check()?;
        acl(file, &self.sid, true)?;
        if identity(file, false)? != identity(&open(&self.path.join(name), false)?, false)? {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
}

/// A private immutable-generation record with its directory fence alive for every access.
/// No raw handle escapes; holding it denies new write/delete handles on Windows.
pub struct BoundRecord<'a> {
    directory: &'a PrivateDirectory,
    file: File,
    name: String,
    id: Identity,
}
impl PrivateDirectory {
    pub fn write_new_record<'a>(
        &'a self,
        name: &str,
        bytes: &[u8],
        limit: usize,
    ) -> Result<BoundRecord<'a>> {
        if limit > 16 * 1024 * 1024 || bytes.len() > limit {
            return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
        }
        let file = self.create_record_sharing(name, fsapi::FILE_SHARE_READ)?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
        let mut record = BoundRecord {
            directory: self,
            id: identity(&file, false)?,
            file,
            name: name.into(),
        };
        record.check()?;
        record
            .file
            .write_all(bytes)
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        record
            .file
            .sync_all()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        record.check()?;
        Ok(record)
    }
    /// Open only under this already-held directory fence; no unsafe namespace adoption.
    pub fn read_record<'a>(&'a self, name: &str) -> Result<BoundRecord<'a>> {
        valid_name(name)?;
        self.check()?;
        let path = wide(&self.path.join(name))?;
        let handle = unsafe {
            fsapi::CreateFileW(
                path.as_ptr(),
                0x80000000 | fsapi::READ_CONTROL,
                fsapi::FILE_SHARE_READ,
                ptr::null(),
                fsapi::OPEN_EXISTING,
                fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(err("WINDOWS_PROFILE_RECORD_OPEN_REFUSED"));
        }
        let file = unsafe { File::from_raw_handle(handle.cast()) };
        self.check_record(&file, name)?;
        fs2::FileExt::try_lock_shared(&file).map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
        let record = BoundRecord {
            directory: self,
            id: identity(&file, false)?,
            file,
            name: name.into(),
        };
        record.check()?;
        Ok(record)
    }
}
impl BoundRecord<'_> {
    pub fn check(&self) -> Result<()> {
        self.directory.check_record(&self.file, &self.name)?;
        if identity(&self.file, false)? != self.id {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
    pub fn read_bounded(&mut self, limit: usize) -> Result<Vec<u8>> {
        if limit > 16 * 1024 * 1024 {
            return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
        }
        self.check()?;
        let before = self
            .file
            .metadata()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?
            .len();
        if before > limit as u64 {
            return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
        }
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        self.check()?;
        if bytes.len() > limit
            || bytes.len() as u64 != before
            || self
                .file
                .metadata()
                .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?
                .len()
                != before
        {
            return Err(err("WINDOWS_PROFILE_RECORD_CHANGED"));
        }
        Ok(bytes)
    }
}
impl Drop for BoundRecord<'_> {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn valid_name(name: &str) -> Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || device
    {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    Ok(())
}

// Pin every lexical directory before resolving the path: junctions/symlinks are refused.
// Retained handles omit FILE_SHARE_DELETE, so ancestors cannot be renamed under the guard.
fn pin_ancestors(path: &Path) -> Result<Vec<(PathBuf, File, Identity)>> {
    let mut current = PathBuf::new();
    let mut result = Vec::new();
    let mut rooted = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) {
                    return Err(err("PROFILE_PATH_INVALID"));
                }
                current.push(prefix.as_os_str());
                // A verbatim drive prefix alone can report is_absolute(), but
                // opening \\?\C: addresses a volume, not its root directory.
                // Retain the root and each ancestor only after an explicit root.
                continue;
            }
            Component::RootDir => {
                if current.as_os_str().is_empty() {
                    return Err(err("PROFILE_PATH_INVALID"));
                }
                current.push(component.as_os_str());
                rooted = true;
            }
            Component::Normal(value) => {
                if !rooted {
                    return Err(err("PROFILE_PATH_INVALID"));
                }
                let text = value.to_str().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
                if text.contains(':') || text.ends_with(['.', ' ']) {
                    return Err(err("PROFILE_PATH_INVALID"));
                }
                current.push(value);
            }
            _ => return Err(err("PROFILE_PATH_INVALID")),
        }
        if rooted {
            let file = open(&current, true)?;
            let id = identity(&file, true)?;
            result.push((current.clone(), file, id));
        }
    }
    if result.is_empty() {
        return Err(err("PROFILE_PATH_INVALID"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    fn fresh() -> PrivateDirectory {
        let parent =
            PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA required"));
        PrivateDirectory::create(
            &parent.join(format!("exhibitos-acl-test-{}", uuid::Uuid::new_v4())),
        )
        .unwrap()
    }
    #[test]
    fn canonical_verbatim_ancestors_reopen_the_same_private_directory() {
        let dir = fresh();
        let lexical_parent = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        let lexical = lexical_parent.join(dir.path().file_name().unwrap());
        assert!(matches!(
            dir.path().components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_))
        ));
        let canonical = PrivateDirectory::inspect(dir.path()).unwrap();
        let ordinary = PrivateDirectory::inspect(&lexical).unwrap();
        assert_eq!(canonical.id, dir.id);
        assert_eq!(ordinary.id, dir.id);
        assert_eq!(canonical.ancestors.len(), ordinary.ancestors.len());
        for ((path, _, canonical_id), (_, _, ordinary_id)) in
            canonical.ancestors.iter().zip(&ordinary.ancestors)
        {
            assert!(path.components().any(|part| part == Component::RootDir));
            assert_eq!(canonical_id, ordinary_id);
        }
        // Never accept a drive-relative path, bare volume or network namespace.
        for invalid in [
            r"C:relative",
            r"\\?\C:",
            r"\\server\share\folder",
            r"\\.\C:",
        ] {
            assert!(pin_ancestors(Path::new(invalid)).is_err());
        }
        let path = dir.path().to_path_buf();
        drop(ordinary);
        drop(canonical);
        drop(dir);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn installed_private_folder_and_raw_records_refuse_adoption_and_rewrite() {
        let root = fresh();
        let path = root.path().join("selection-history");
        crate::installations::new_directory(&path).unwrap();
        crate::installations::private_directory(&path).unwrap();
        assert!(crate::installations::new_directory(&path).is_err());
        crate::write_private_new(&path, "generation.json", b"synthetic-history", 64).unwrap();
        assert!(crate::write_private_new(&path, "generation.json", b"replacement", 64).is_err());
        assert!(crate::write_private_new(&path, "oversized", b"too-long", 1).is_err());
        assert!(!path.join("oversized").exists());
        assert_eq!(
            crate::installation_backup::source_bytes(
                root.path(),
                "selection-history/generation.json",
                64,
                true
            )
            .unwrap(),
            b"synthetic-history"
        );
        assert!(
            crate::installation_backup::source_bytes(
                root.path(),
                "selection-history/generation.json",
                1,
                true
            )
            .is_err()
        );
        crate::write_private_new(&path, "empty", b"", 64).unwrap();
        assert!(
            crate::installation_backup::source_bytes(
                root.path(),
                "selection-history/empty",
                64,
                true
            )
            .is_err()
        );
        // An inherited public directory must be refused, without ACL adoption.
        let public = root.path().join("public-child");
        std::fs::create_dir(&public).unwrap();
        assert!(crate::installations::private_directory(&public).is_err());
        assert!(crate::write_private_new(&public, "secret", b"synthetic-only", 64).is_err());
        assert!(!public.join("secret").exists());
        std::fs::remove_dir(public).unwrap();
        std::fs::remove_file(path.join("generation.json")).unwrap();
        std::fs::remove_file(path.join("empty")).unwrap();
        std::fs::remove_dir(path).unwrap();
        let path = root.path().to_path_buf();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn private_records_refuse_existing_names_aliases_and_rename() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let mut file = dir.create_record("synthetic.json").unwrap();
        file.write_all(b"synthetic-only").unwrap();
        file.sync_all().unwrap();
        dir.check_record(&file, "synthetic.json").unwrap();
        assert!(dir.create_record("synthetic.json").is_err());
        assert!(PrivateDirectory::create(&path).is_err());
        for name in ["../outside", "x:y", "NUL", "CON.txt", "LPT1.json", "x."] {
            assert!(dir.create_record(name).is_err());
            assert!(dir.check_record(&file, name).is_err());
        }
        assert!(std::fs::rename(&path, path.with_extension("moved")).is_err());
        assert!(std::fs::rename(path.join("synthetic.json"), path.join("renamed.json")).is_err());
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut data = String::new();
        file.read_to_string(&mut data).unwrap();
        assert_eq!(data, "synthetic-only");
        drop(file);
        drop(dir);
        std::fs::remove_file(path.join("synthetic.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn hardlinked_private_record_is_refused_without_rewriting_acl() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let file = dir.create_record("synthetic.json").unwrap();
        std::fs::hard_link(path.join("synthetic.json"), path.join("alias.json")).unwrap();
        assert!(dir.check_record(&file, "synthetic.json").is_err());
        drop(file);
        drop(dir);
        std::fs::remove_file(path.join("alias.json")).unwrap();
        std::fs::remove_file(path.join("synthetic.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn added_everyone_read_grant_is_rejected() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let file = dir.create_record("synthetic.json").unwrap();
        let status = crate::process_window::background_command("icacls.exe")
            .arg(path.join("synthetic.json"))
            .args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            dir.check_record(&file, "synthetic.json").unwrap_err().code,
            "WINDOWS_PROFILE_ACL_INVALID"
        );
        drop(file);
        drop(dir);
        std::fs::remove_file(path.join("synthetic.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn native_record_lock_refuses_another_open_handle() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let file = dir.create_record("synthetic.json").unwrap();
        fs2::FileExt::try_lock_exclusive(&file).unwrap();
        let second = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("synthetic.json"))
            .unwrap();
        dir.check_record(&second, "synthetic.json").unwrap();
        assert!(fs2::FileExt::try_lock_exclusive(&second).is_err());
        drop(second);
        fs2::FileExt::unlock(&file).unwrap();
        drop(file);
        drop(dir);
        std::fs::remove_file(path.join("synthetic.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn bound_record_reopens_exact_bytes_without_allowing_writer_handles() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let mut created = dir
            .write_new_record("generation-1.json", b"synthetic-only", 1024)
            .unwrap();
        assert_eq!(created.read_bounded(1024).unwrap(), b"synthetic-only");
        assert!(
            dir.write_new_record("generation-1.json", b"replacement", 1024)
                .is_err()
        );
        assert!(dir.read_record("generation-1.json").is_err());
        drop(created);
        let mut first = dir.read_record("generation-1.json").unwrap();
        let mut second = dir.read_record("generation-1.json").unwrap();
        assert_eq!(first.read_bounded(1024).unwrap(), b"synthetic-only");
        assert_eq!(second.read_bounded(1024).unwrap(), b"synthetic-only");
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.join("generation-1.json"))
                .is_err()
        );
        assert!(std::fs::remove_file(path.join("generation-1.json")).is_err());
        assert!(first.read_bounded(1).is_err());
        assert!(dir.write_new_record("too-large", b"xx", 1).is_err());
        assert!(!path.join("too-large").exists());
        drop(second);
        drop(first);
        drop(dir);
        std::fs::remove_file(path.join("generation-1.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn bound_reader_refuses_existing_writer_without_modifying_it() {
        let dir = fresh();
        let path = dir.path().to_owned();
        let file = dir.create_record("generation-1.json").unwrap();
        assert!(dir.read_record("generation-1.json").is_err());
        drop(file);
        drop(dir);
        std::fs::remove_file(path.join("generation-1.json")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
}
