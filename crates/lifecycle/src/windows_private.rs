// SPDX-License-Identifier: Apache-2.0
//! Owner-only NTFS objects for the future managed Windows profile adapter.
//! Existing user directories/ACLs are inspected, never rewritten or adopted.
#[path = "windows_json_storage.rs"]
mod json_storage;
#[path = "windows_record_publication.rs"]
mod record_publication;
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
/// Ordinary OS-created parents may use this token's privileged default owner.
/// Accept only SYSTEM/Administrators, never an arbitrary shared owner group.
/// The current user's full grant and all nonprivileged-writer exclusions remain.
fn parent_acl(file: &File, sid: &Sid) -> Result<()> {
    let default_owner = Sid::default_owner()?;
    let trusted_default = matches!(
        Sid::text_ptr(default_owner.ptr())?.as_str(),
        "S-1-5-18" | "S-1-5-32-544"
    );
    acl_owner(file, sid, false, trusted_default.then_some(&default_owner))
}
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
            fsapi::READ_CONTROL
                | fsapi::FILE_READ_ATTRIBUTES
                // Metadata-only handles do not participate in the data/delete
                // sharing fence. Request directory enumeration so omitting
                // FILE_SHARE_DELETE also protects an empty candidate namespace.
                | if directory { fsapi::FILE_LIST_DIRECTORY } else { 0 },
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
/// Pins a current-user-owned, nonprivileged-writer-safe existing parent.
/// This type only creates protected anchor locks, never private state directories.
pub(crate) struct ParentDirectory {
    path: PathBuf,
    ancestors: Vec<(PathBuf, File, Identity)>,
    id: Identity,
    sid: Sid,
}
impl ParentDirectory {
    pub(crate) fn inspect(path: &Path) -> Result<Self> {
        let ancestors = pin_ancestors(path)?;
        let file = &ancestors
            .last()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
            .1;
        let sid = Sid::current()?;
        parent_acl(file, &sid)?;
        let id = identity(file, true)?;
        let path = path
            .canonicalize()
            .map_err(|_| err("PROFILE_PATH_INVALID"))?;
        let value = Self {
            path,
            ancestors,
            id,
            sid,
        };
        value.check()?;
        Ok(value)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn check(&self) -> Result<()> {
        for (path, file, id) in &self.ancestors {
            if identity(file, true)? != *id || identity(&open(path, true)?, true)? != *id {
                return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
            }
        }
        let file = &self
            .ancestors
            .last()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
            .1;
        parent_acl(file, &self.sid)?;
        if identity(&open(&self.path, true)?, true)? != self.id {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
    pub(crate) fn logical_child_key(&self, name: &str) -> Result<String> {
        valid_name(name)?;
        self.check()?;
        // Parent object identity is independent of drive/path spelling. ASCII
        // private leaf names are folded for Windows namespace case aliases.
        Ok(crate::digest(
            format!(
                "{}:{}:{}:{}",
                self.id.volume,
                self.id.index_high,
                self.id.index_low,
                name.to_ascii_lowercase()
            )
            .as_bytes(),
        ))
    }
    pub(crate) fn check_record(&self, file: &File, name: &str) -> Result<()> {
        valid_name(name)?;
        self.check()?;
        acl(file, &self.sid, true)?;
        if identity(file, false)? != identity(&open(&self.path.join(name), false)?, false)? {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
    /// Opens an existing lock without creating, changing ACLs or requesting writes.
    /// Shared readers and existing read/write lock owners remain compatible;
    /// native byte-range locks enforce exclusivity and delete sharing stays denied.
    pub(crate) fn inspect_lock_record(&self, name: &str) -> Result<File> {
        valid_name(name)?;
        self.check()?;
        let file = native_existing_lock_file(&self.path.join(name))?;
        self.check_record(&file, name)?;
        Ok(file)
    }
    pub(crate) fn lock_record(&self, name: &str) -> Result<File> {
        valid_name(name)?;
        self.check()?;
        let file = native_lock_file(&self.path.join(name), &self.sid)?;
        self.check_record(&file, name)?;
        Ok(file)
    }
}
fn native_existing_lock_file(path: &Path) -> Result<File> {
    let path = wide(path)?;
    let handle = unsafe {
        fsapi::CreateFileW(
            path.as_ptr(),
            0x80000000 | fsapi::READ_CONTROL,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE,
            ptr::null(),
            fsapi::OPEN_EXISTING,
            fsapi::FILE_ATTRIBUTE_NORMAL | fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(err("WINDOWS_PROFILE_RECORD_OPEN_REFUSED"));
    }
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}
fn native_lock_file(path: &Path, sid: &Sid) -> Result<File> {
    let path = wide(path)?;
    let sd = descriptor(sid, false)?;
    let attributes = sec::SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<sec::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        fsapi::CreateFileW(
            path.as_ptr(),
            0x80000000 | 0x40000000 | fsapi::READ_CONTROL,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE,
            &attributes,
            fsapi::OPEN_ALWAYS,
            fsapi::FILE_ATTRIBUTE_NORMAL | fsapi::FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(err("WINDOWS_PROFILE_RECORD_OPEN_REFUSED"));
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
        parent_acl(&parent, &sid)?;
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
    /// Create only missing private descendants, retaining each ancestor fence.
    /// Existing destination directories must already satisfy the private policy.
    pub(crate) fn ensure_tree(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        let mut missing = Vec::new();
        let mut existing = path.to_path_buf();
        loop {
            match std::fs::symlink_metadata(&existing) {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let name = existing
                        .file_name()
                        .and_then(|v| v.to_str())
                        .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
                        .to_owned();
                    valid_name(&name)?;
                    missing.push(name);
                    if missing.len() > 32 {
                        return Err(err("PROFILE_PATH_INVALID"));
                    }
                    existing = existing
                        .parent()
                        .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
                        .to_path_buf();
                }
                Err(_) => return Err(err("WINDOWS_PROFILE_HANDLE_UNAVAILABLE")),
            }
        }
        if missing.is_empty() {
            return Self::inspect(path);
        }
        // The first create pins and validates the existing parent's namespace/ACL;
        // every successor retains its predecessor in its own ancestor fence.
        let mut guard = None;
        for name in missing.into_iter().rev() {
            let next = Self::create(&existing.join(name))?;
            existing = next.path().to_path_buf();
            guard = Some(next);
        }
        guard.ok_or_else(|| err("PROFILE_PATH_INVALID"))
    }
    /// Never truncates or rewrites existing locks; validates both old and new ACLs.
    /// Opens an existing lock without creating, changing ACLs or requesting writes.
    /// Shared readers and existing read/write lock owners remain compatible;
    /// native byte-range locks enforce exclusivity and delete sharing stays denied.
    pub(crate) fn inspect_lock_record(&self, name: &str) -> Result<File> {
        valid_name(name)?;
        self.check()?;
        let file = native_existing_lock_file(&self.path.join(name))?;
        self.check_record(&file, name)?;
        Ok(file)
    }
    pub(crate) fn lock_record(&self, name: &str) -> Result<File> {
        self.check()?;
        valid_name(name)?;
        let file = native_lock_file(&self.path.join(name), &self.sid)?;
        self.check_record(&file, name)?;
        Ok(file)
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
        parent_acl(&parent, &sid)?;
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
        parent_acl(&self.parent, &self.sid)?;
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
    /// Validate an existing lock under this fence without reading or requesting write access.
    pub(crate) fn check_existing_record(&self, name: &str) -> Result<()> {
        valid_name(name)?;
        self.check_record(&open(&self.path.join(name), false)?, name)
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

/// Read an existing public document with pinned NTFS ancestors and native identity.
/// Denies concurrent writers/deletion and unrelated ACL writers; never adopts ACLs.
/// The bytes are stable input only, not proof of a trusted release or activation.
pub fn read_public_input(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if limit > 16 * 1024 * 1024 {
        return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
    }
    PublicRecord::open(path)?.read_bounded(limit)
}

/// Read an existing external owner-only 32-byte checkpoint key without adopting
/// ACLs. Existing/missing profile namespaces are pinned and rechecked; native
/// directory identities exclude keys inside a profile despite case aliases.
pub fn read_external_private_key(path: &Path, profile: &Path) -> Result<[u8; 32]> {
    let invalid = || err("WINDOWS_EXTERNAL_KEY_INVALID");
    if !path.is_absolute() || !profile.is_absolute() {
        return Err(invalid());
    }
    valid_name(
        profile
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(invalid)?,
    )?;
    let profile_parent = ParentDirectory::inspect(profile.parent().ok_or_else(invalid)?)?;
    let profile_guard = match std::fs::symlink_metadata(profile) {
        Ok(_) => Some(PrivateDirectory::inspect(profile)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(invalid()),
    };
    let external = PrivateDirectory::inspect(path.parent().ok_or_else(invalid)?)?;
    if let Some(guard) = &profile_guard {
        if external.id == guard.id || external.ancestors.iter().any(|(_, _, id)| *id == guard.id) {
            return Err(invalid());
        }
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(invalid)?;
    let mut record = external.read_record(name)?;
    let before = record.file.metadata().map_err(|_| invalid())?;
    if before.len() != 32 {
        return Err(invalid());
    }
    let mut key = [0u8; 32];
    let result = (|| {
        record.file.read_exact(&mut key).map_err(|_| invalid())?;
        record.check()?;
        external.check()?;
        profile_parent.check()?;
        match &profile_guard {
            Some(guard) => guard.check()?,
            None => match std::fs::symlink_metadata(profile) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                _ => return Err(invalid()),
            },
        }
        let after = record.file.metadata().map_err(|_| invalid())?;
        if after.len() != 32
            || before.modified().map_err(|_| invalid())?
                != after.modified().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        Ok(())
    })();
    if let Err(error) = result {
        key.fill(0);
        return Err(error);
    }
    Ok(key)
}

/// Existing public input, held without write/delete sharing or ACL changes.
/// This reader accepts only regular single-link files under pinned ancestors.
/// Public ownership may be this token's default owner; unrelated writers are refused.
pub(crate) struct PublicRecord {
    path: PathBuf,
    file: File,
    ancestors: Vec<(PathBuf, File, Identity)>,
    sid: Sid,
    id: Identity,
}
impl PublicRecord {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
        valid_name(name)?;
        let ancestors = pin_ancestors(parent)?;
        let sid = Sid::current()?;
        let parent_file = &ancestors
            .last()
            .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
            .1;
        public_acl(parent_file, &sid)?;
        let handle = unsafe {
            fsapi::CreateFileW(
                wide(path)?.as_ptr(),
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
        public_acl(&file, &sid)?;
        let id = identity(&file, false)?;
        fs2::FileExt::try_lock_shared(&file).map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
        let value = Self {
            path: path.into(),
            file,
            ancestors,
            sid,
            id,
        };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        for (path, held, expected) in &self.ancestors {
            if identity(held, true)? != *expected
                || identity(&open(path, true)?, true)? != *expected
            {
                return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
            }
        }
        public_acl(
            &self
                .ancestors
                .last()
                .ok_or_else(|| err("PROFILE_PATH_INVALID"))?
                .1,
            &self.sid,
        )?;
        public_acl(&self.file, &self.sid)?;
        if identity(&self.file, false)? != self.id
            || identity(&open(&self.path, false)?, false)? != self.id
        {
            return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
        }
        Ok(())
    }
    pub(crate) fn checked_metadata(&self) -> Result<std::fs::Metadata> {
        self.check()?;
        self.file
            .metadata()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))
    }
    pub(crate) fn read_bounded(&mut self, limit: usize) -> Result<Vec<u8>> {
        if limit > 16 * 1024 * 1024 {
            return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
        }
        self.check()?;
        let before = self
            .file
            .metadata()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        if before.len() > limit as u64 {
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
        let after = self
            .file
            .metadata()
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        if bytes.len() as u64 != before.len()
            || after.len() != before.len()
            || after
                .modified()
                .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?
                != before
                    .modified()
                    .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?
        {
            return Err(err("WINDOWS_PROFILE_RECORD_CHANGED"));
        }
        Ok(bytes)
    }
}
// Retain the native guard for the whole streaming consumer; no bare File escapes.
impl Read for PublicRecord {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.check()
            .map_err(|_| std::io::Error::other("WINDOWS_PUBLIC_INPUT_INVALID"))?;
        self.file.read(bytes)
    }
}
impl Drop for PublicRecord {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
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
    /// Retain a read-only native handle while the caller owns this directory fence.
    /// FILE_SHARE_READ denies writer/delete handles; no mutable handle escapes.
    pub(crate) fn retained_read_file(&self, name: &str) -> Result<File> {
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
        Ok(file)
    }
    /// Open only under this already-held directory fence; no unsafe namespace adoption.
    pub fn read_record<'a>(&'a self, name: &str) -> Result<BoundRecord<'a>> {
        let file = self.retained_read_file(name)?;
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
    fn windows_external_key_reopens_exact_private_bytes_for_existing_and_missing_profile() {
        let root = fresh();
        let profile = PrivateDirectory::create(&root.path().join("profile")).unwrap();
        let external = PrivateDirectory::create(&root.path().join("external")).unwrap();
        let key = external.path().join("key.bin");
        drop(external.write_new_record("key.bin", &[23; 32], 32).unwrap());
        assert_eq!(
            read_external_private_key(&key, profile.path()).unwrap(),
            [23; 32]
        );
        let missing = root.path().join("missing-profile");
        assert_eq!(read_external_private_key(&key, &missing).unwrap(), [23; 32]);
        assert!(!missing.exists());
        assert_eq!(std::fs::read(key).unwrap(), [23; 32]);
    }
    #[test]
    fn windows_external_key_native_identity_refuses_inside_case_alias_and_nested_profile() {
        let root = fresh();
        let profile = PrivateDirectory::create(&root.path().join("profile")).unwrap();
        drop(profile.write_new_record("key.bin", &[24; 32], 32).unwrap());
        let alias = profile.path().with_file_name("PROFILE");
        assert!(read_external_private_key(&profile.path().join("key.bin"), &alias).is_err());
        let nested = PrivateDirectory::create(&profile.path().join("keys")).unwrap();
        drop(nested.write_new_record("key.bin", &[25; 32], 32).unwrap());
        assert!(read_external_private_key(&nested.path().join("key.bin"), &alias).is_err());
        assert_eq!(
            std::fs::read(nested.path().join("key.bin")).unwrap(),
            [25; 32]
        );
    }
    #[test]
    fn windows_external_key_writer_hardlink_and_wrong_length_preserve_input() {
        let root = fresh();
        let external = PrivateDirectory::create(&root.path().join("external")).unwrap();
        let profile = root.path().join("missing-profile");
        let key = external.path().join("key.bin");
        drop(external.write_new_record("key.bin", &[26; 32], 32).unwrap());
        let writer = std::fs::OpenOptions::new().write(true).open(&key).unwrap();
        assert!(read_external_private_key(&key, &profile).is_err());
        drop(writer);
        drop(
            external
                .write_new_record("long.bin", &[27; 33], 33)
                .unwrap(),
        );
        assert!(read_external_private_key(&external.path().join("long.bin"), &profile).is_err());
        std::fs::hard_link(&key, external.path().join("alias.bin")).unwrap();
        assert!(read_external_private_key(&key, &profile).is_err());
        assert_eq!(std::fs::read(key).unwrap(), [26; 32]);
    }
    #[test]
    fn windows_external_key_shared_read_grant_is_refused_without_acl_repair() {
        let root = fresh();
        let external = PrivateDirectory::create(&root.path().join("external")).unwrap();
        let key = external.path().join("key.bin");
        drop(external.write_new_record("key.bin", &[28; 32], 32).unwrap());
        let changed = crate::process_window::background_command("icacls")
            .arg(&key)
            .args(["/grant", "*S-1-1-0:R"])
            .output()
            .unwrap();
        assert!(changed.status.success());
        assert!(read_external_private_key(&key, &root.path().join("missing-profile")).is_err());
        assert_eq!(std::fs::read(key).unwrap(), [28; 32]);
    }

    #[test]
    fn public_reader_pins_input_and_parent_without_adopting_private_acl() {
        let root = fresh();
        let bundle = root.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        let path = bundle.join("compose.yaml");
        std::fs::write(&path, b"synthetic: true\n").unwrap();
        assert!(PrivateDirectory::inspect(&bundle).is_err());
        let mut held = PublicRecord::open(&path).unwrap();
        assert_eq!(held.read_bounded(1024).unwrap(), b"synthetic: true\n");
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::rename(&path, bundle.join("moved.yaml")).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&bundle, root.path().join("moved")).is_err());
        assert_eq!(
            crate::installation_backup::source_bytes(
                root.path(),
                "bundle/compose.yaml",
                1024,
                false
            )
            .unwrap(),
            b"synthetic: true\n"
        );
        drop(held);
        assert!(PrivateDirectory::inspect(&bundle).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(bundle).unwrap();
        let path = root.path().to_owned();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn public_reader_refuses_existing_writer_hardlink_and_unrelated_parent_writer() {
        let root = fresh();
        let bundle = root.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        let path = bundle.join("compose.yaml");
        std::fs::write(&path, b"synthetic-only").unwrap();
        let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(PublicRecord::open(&path).is_err());
        drop(writer);
        let alias = bundle.join("alias.yaml");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(
            crate::installation_backup::source_bytes(
                root.path(),
                "bundle/compose.yaml",
                1024,
                false
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&alias).unwrap(), b"synthetic-only");
        std::fs::remove_file(alias).unwrap();
        let status = crate::process_window::background_command("icacls.exe")
            .arg(&bundle)
            .args(["/grant", "*S-1-1-0:W"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(PublicRecord::open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic-only");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(bundle).unwrap();
        let path = root.path().to_owned();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn public_source_limits_and_junctions_refuse_without_changing_input() {
        let root = fresh();
        let bundle = root.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        let path = bundle.join("compose.yaml");
        std::fs::write(&path, b"synthetic-only").unwrap();
        assert!(
            crate::installation_backup::source_bytes(root.path(), "bundle/compose.yaml", 4, false)
                .is_err()
        );
        assert!(
            PublicRecord::open(&path)
                .unwrap()
                .read_bounded(16 * 1024 * 1024 + 1)
                .is_err()
        );
        assert!(PublicRecord::open(&bundle.join("compose.yaml:stream")).is_err());
        let empty = bundle.join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(
            crate::installation_backup::source_bytes(root.path(), "bundle/empty", 1024, false)
                .is_err()
        );
        let junction = root.path().join("junction");
        let status = crate::process_window::background_command("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&bundle)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(PublicRecord::open(&junction.join("compose.yaml")).is_err());
        assert!(
            crate::installation_backup::source_bytes(
                root.path(),
                "junction/compose.yaml",
                1024,
                false
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic-only");
        std::fs::remove_dir(junction).unwrap();
        std::fs::remove_file(empty).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(bundle).unwrap();
        let path = root.path().to_owned();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn restore_stage_pins_new_private_candidates_and_never_overwrites_records() {
        let root = fresh();
        let stage_path = root.path().join("restore-candidate");
        let stage = crate::restoration::directory(&stage_path).unwrap();
        let stage_path = stage_path.canonicalize().unwrap();
        let renamed = root.path().join("retained-candidate");
        // No child record lock should be needed to retain the namespace.
        assert!(std::fs::rename(&stage_path, &renamed).is_err());
        assert!(std::fs::remove_dir(&stage_path).is_err());
        let input = stage_path.join("runtime.env");
        crate::restoration::private_bytes(&input, b"SYNTHETIC=retained\n").unwrap();
        assert_eq!(
            crate::installation_backup::source_bytes(&stage_path, "runtime.env", 1024, true)
                .unwrap(),
            b"SYNTHETIC=retained\n"
        );
        assert!(crate::restoration::private_bytes(&input, b"replacement").is_err());
        assert!(crate::restoration::directory(&stage_path).is_err());
        assert!(std::fs::rename(&stage_path, &renamed).is_err());
        assert_eq!(std::fs::read(&input).unwrap(), b"SYNTHETIC=retained\n");
        drop(stage);
        std::fs::rename(&stage_path, &renamed).unwrap();
        std::fs::remove_file(renamed.join("runtime.env")).unwrap();
        std::fs::remove_dir(renamed).unwrap();
        let path = root.path().to_owned();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn restore_stage_refuses_unprotected_existing_parent_and_unsafe_fresh_lock() {
        let root = fresh();
        let public = root.path().join("public");
        std::fs::create_dir(&public).unwrap();
        let candidate = public.join("restore-candidate");
        // The private new-file factory must not adopt an unprotected directory.
        assert!(
            crate::restoration::private_bytes(&public.join("runtime.env"), b"synthetic").is_err()
        );
        assert!(!public.join("runtime.env").exists());
        assert!(crate::restoration::directory(&public).is_err());
        assert!(!candidate.exists());
        std::fs::remove_dir(public).unwrap();
        let lock = root.path().join("operation.lock");
        std::fs::write(&lock, b"retained-unsafe-lock").unwrap();
        assert!(crate::restoration::fresh_root(root.path()).is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"retained-unsafe-lock");
        std::fs::remove_file(lock).unwrap();
        let guard = root.lock_record("operation.lock").unwrap();
        fs2::FileExt::try_lock_exclusive(&guard).unwrap();
        crate::restoration::fresh_root(root.path()).unwrap();
        drop(guard);
        std::fs::remove_file(root.path().join("operation.lock")).unwrap();
        let path = root.path().to_owned();
        drop(root);
        std::fs::remove_dir(path).unwrap();
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
    fn service_lifetime_pins_new_nested_roots_and_guarded_operation_locks() {
        let parent = fresh();
        let path = parent.path().join("service-parent").join("service-root");
        let service = crate::LifecycleService::new(path.clone()).unwrap();
        let other = crate::LifecycleService::bound_existing(service.root.clone()).unwrap();
        let lock = service.lock().unwrap();
        assert_eq!(other.lock().unwrap_err().code, "BUSY");
        assert!(std::fs::rename(&path, path.with_extension("moved")).is_err());
        assert!(std::fs::remove_file(path.join("operation.lock")).is_err());
        drop(lock);
        drop(other.lock().unwrap());
        let manifest = crate::tests::manifest_for_detection();
        service.runtime_env(&manifest).unwrap();
        let original =
            crate::installation_backup::source_bytes(&path, "runtime.env", 8192, true).unwrap();
        service.runtime_env(&manifest).unwrap();
        assert_eq!(
            crate::installation_backup::source_bytes(&path, "runtime.env", 8192, true).unwrap(),
            original
        );
        drop(other);
        drop(service);
        let reopened = crate::LifecycleService::new(path.clone()).unwrap();
        assert_eq!(
            crate::installation_backup::source_bytes(&path, "runtime.env", 8192, true).unwrap(),
            original
        );
        drop(reopened);
        std::fs::remove_file(path.join("operation.lock")).unwrap();
        std::fs::remove_file(path.join("runtime.env")).unwrap();
        std::fs::remove_dir(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
        let path = parent.path().to_path_buf();
        drop(parent);
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn service_refuses_unprotected_roots_and_unsafe_locks_without_repair() {
        let root = fresh();
        let public = root.path().join("unprotected");
        std::fs::create_dir(&public).unwrap();
        std::fs::write(public.join("runtime.env"), b"synthetic-preserve").unwrap();
        assert!(crate::LifecycleService::new(public.clone()).is_err());
        assert_eq!(
            std::fs::read(public.join("runtime.env")).unwrap(),
            b"synthetic-preserve"
        );
        assert!(!public.join("operation.lock").exists());
        let service = crate::LifecycleService::new(root.path().to_path_buf()).unwrap();
        let lock = root.path().join("operation.lock");
        std::fs::write(&lock, b"synthetic-lock-preserve").unwrap();
        let status = crate::process_window::background_command("icacls.exe")
            .arg(&lock)
            .args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            service.lock().unwrap_err().code,
            "WINDOWS_PROFILE_ACL_INVALID"
        );
        assert!(crate::LifecycleService::new(root.path().to_path_buf()).is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"synthetic-lock-preserve");
        drop(service);
        std::fs::remove_file(lock).unwrap();
        std::fs::remove_file(public.join("runtime.env")).unwrap();
        std::fs::remove_dir(public).unwrap();
        let path = root.path().to_path_buf();
        drop(root);
        std::fs::remove_dir(path).unwrap();
    }
    fn remove_owned_anchor_fixture(parent: PathBuf, profile: &Path) {
        if profile.exists() {
            std::fs::remove_file(profile.join("profile-session.lock")).unwrap();
            std::fs::remove_dir(profile).unwrap();
        }
        for entry in std::fs::read_dir(&parent).unwrap() {
            let entry = entry.unwrap();
            assert!(
                entry
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with(".exhibitos-profile-session-")
            );
            assert!(std::fs::symlink_metadata(entry.path()).unwrap().is_file());
            std::fs::remove_file(entry.path()).unwrap();
        }
        std::fs::remove_dir(parent).unwrap();
    }
    #[test]
    fn profile_anchor_precedes_creation_and_case_aliases_share_one_lock() {
        let parent = fresh();
        let parent_path = parent.path().to_path_buf();
        let profile = parent_path.join("managed-profile");
        let (target, anchor) = crate::profile_backup::anchor_lock(&profile, true).unwrap();
        assert!(!profile.exists());
        let alias = parent_path.join("MANAGED-PROFILE");
        assert_eq!(
            crate::profile_backup::anchor_lock(&alias, false)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        let clone = anchor.try_clone().unwrap();
        drop(anchor);
        assert_eq!(
            crate::profile_backup::anchor_lock(&profile, false)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        drop(parent);
        assert!(std::fs::rename(&parent_path, parent_path.with_extension("moved")).is_err());
        let root = PrivateDirectory::create(&target).unwrap();
        let profile = root.path().to_path_buf();
        let session = crate::profile_backup::anchored_session(&profile, clone, true).unwrap();
        drop(root);
        session.check_exclusive(&profile).unwrap();
        assert!(std::fs::rename(&profile, profile.with_extension("moved")).is_err());
        assert_eq!(
            crate::profile_backup::anchor_lock(&alias, false)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        drop(session);
        drop(crate::profile_backup::session_lock(&profile, true).unwrap());
        remove_owned_anchor_fixture(parent_path, &profile);
    }
    #[test]
    fn shared_profile_sessions_block_exclusive_and_refuse_hardlinked_legacy_lock() {
        let parent = fresh();
        let parent_path = parent.path().to_path_buf();
        let root = PrivateDirectory::create(&parent_path.join("profile")).unwrap();
        let profile = root.path().to_path_buf();
        let one = crate::profile_backup::session_lock(&profile, false).unwrap();
        let two = crate::profile_backup::session_lock(&profile, false).unwrap();
        assert_eq!(
            one.check_exclusive(&profile).unwrap_err().code,
            "PROFILE_LOCK_INVALID"
        );
        assert_eq!(
            crate::profile_backup::session_lock(&profile, true)
                .err()
                .unwrap()
                .code,
            "PROFILE_BUSY"
        );
        drop(one);
        drop(two);
        let original = profile.join("profile-session.lock");
        let alias = profile.join("legacy-alias");
        std::fs::hard_link(&original, &alias).unwrap();
        assert!(crate::profile_backup::session_lock(&profile, false).is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"");
        assert_eq!(std::fs::read(&alias).unwrap(), b"");
        std::fs::remove_file(alias).unwrap();
        drop(crate::profile_backup::session_lock(&profile, true).unwrap());
        let archive = parent_path.join("unqualified-profile-backup");
        assert_eq!(
            crate::profile_backup::backup(
                &profile,
                &parent_path.join("unused-key"),
                &archive,
                true
            )
            .unwrap_err()
            .code,
            "PROFILE_PLATFORM_UNVERIFIED"
        );
        assert!(!archive.exists());
        drop(root);
        drop(parent);
        remove_owned_anchor_fixture(parent_path, &profile);
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

// Full host checkpoints retain native permissions rather than synthesizing Unix modes.
pub(crate) use crate::profile_backup::HostWindowsPermissions as HostPermissions;
const HOST_ATTRIBUTES: u32 = fsapi::FILE_ATTRIBUTE_READONLY
    | fsapi::FILE_ATTRIBUTE_HIDDEN
    | fsapi::FILE_ATTRIBUTE_SYSTEM
    | fsapi::FILE_ATTRIBUTE_ARCHIVE
    | fsapi::FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
fn host_permissions(file: &File, sid: &Sid, directory: bool) -> Result<HostPermissions> {
    identity(file, directory)?;
    let (mut owner, mut dacl, mut sd) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    if unsafe { GetSecurityInfo(file.as_raw_handle().cast(), SE_FILE_OBJECT,
        sec::OWNER_SECURITY_INFORMATION | sec::DACL_SECURITY_INFORMATION,
        &mut owner, ptr::null_mut(), &mut dacl, ptr::null_mut(), &mut sd) } != 0 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
    }
    let _sd = Local(sd);
    host_validate_descriptor(sd, sid)?;
    let (mut text, mut length) = (ptr::null_mut(), 0);
    if unsafe { sec::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW(
        sd, SDDL_REVISION_1, sec::OWNER_SECURITY_INFORMATION | sec::DACL_SECURITY_INFORMATION,
        &mut text, &mut length) } == 0 || text.is_null() || length > 65536 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
    }
    let _text = Local(text.cast());
    let encoded = unsafe { std::slice::from_raw_parts(text, length as usize) };
    let encoded = encoded.strip_suffix(&[0]).unwrap_or(encoded);
    use std::os::windows::fs::MetadataExt;
    Ok(HostPermissions {
        security_descriptor: String::from_utf16(encoded).map_err(|_| err("WINDOWS_HOST_PERMISSIONS_INVALID"))?,
        attributes: file.metadata().map_err(|_| err("WINDOWS_HOST_IO"))?.file_attributes() & HOST_ATTRIBUTES,
    })
}
fn host_validate_descriptor(sd: sec::PSECURITY_DESCRIPTOR, sid: &Sid) -> Result<()> {
    let (mut owner, mut defaulted) = (ptr::null_mut(), 0);
    let (mut dacl, mut present) = (ptr::null_mut(), 0);
    if unsafe { sec::GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) } == 0
        || unsafe { sec::GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) } == 0
        || present == 0 { return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID")); }
    if owner.is_null() || dacl.is_null() || unsafe { sec::IsValidSid(owner) } == 0
        || unsafe { sec::EqualSid(owner, sid.ptr()) } == 0
        || unsafe { sec::IsValidAcl(dacl) } == 0 || unsafe { (*dacl).AceCount } > 1024 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
    }
    let mut owner_read = false;
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut raw = ptr::null_mut();
        if unsafe { sec::GetAce(dacl, index.into(), &mut raw) } == 0 || raw.is_null() {
            return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
        }
        let header = unsafe { &*raw.cast::<sec::ACE_HEADER>() };
        if header.AceType == 1 { continue; }
        if header.AceType != 0 || usize::from(header.AceSize) < mem::size_of::<sec::ACCESS_ALLOWED_ACE>() {
            return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
        }
        let ace = unsafe { &*raw.cast::<sec::ACCESS_ALLOWED_ACE>() };
        let grant: sec::PSID = ptr::addr_of!(ace.SidStart).cast_mut().cast();
        let available = usize::from(header.AceSize).saturating_sub(mem::offset_of!(sec::ACCESS_ALLOWED_ACE, SidStart));
        if available < 8 || 8 + 4 * usize::from(unsafe { *grant.cast::<u8>().add(1) }) > available
            || unsafe { sec::IsValidSid(grant) } == 0 {
            return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
        }
        if unsafe { sec::EqualSid(grant, sid.ptr()) } != 0 {
            if header.AceFlags & sec::INHERIT_ONLY_ACE as u8 == 0 {
                owner_read |= ace.Mask & (0x80000000 | 0x10000000 | fsapi::FILE_READ_DATA) != 0;
            }
        } else if !matches!(Sid::text_ptr(grant)?.as_str(), "S-1-5-18" | "S-1-5-32-544")
            && ace.Mask & 0x500d0156 != 0 {
            return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
        }
    }
    if !owner_read { return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID")); }
    Ok(())
}
fn host_open(path: &Path, directory: bool, access: u32, share: u32) -> Result<File> {
    let handle = unsafe { fsapi::CreateFileW(wide(path)?.as_ptr(), access | fsapi::READ_CONTROL,
        share, ptr::null(), fsapi::OPEN_EXISTING,
        fsapi::FILE_FLAG_OPEN_REPARSE_POINT | if directory { fsapi::FILE_FLAG_BACKUP_SEMANTICS } else { 0 }, ptr::null_mut()) };
    if handle == INVALID_HANDLE_VALUE { return Err(err("WINDOWS_HOST_OPEN_REFUSED")); }
    let file = unsafe { File::from_raw_handle(handle.cast()) };
    identity(&file, directory)?;
    Ok(file)
}
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostDirectorySync {
    pub(crate) directory_flush: bool,
    pub(crate) publication_namespace_checked: bool,
}
struct HostDirectoryInner {
    file: File,
    id: Identity,
    sid: Sid,
    ancestors: Vec<(PathBuf, File, Identity)>,
    parents: std::sync::Mutex<Vec<HostDirectory>>,
    children: std::sync::Mutex<std::collections::BTreeMap<String, std::sync::Weak<HostDirectoryInner>>>,
    created: bool,
    publishable: bool,
}
#[derive(Clone)]
pub(crate) struct HostDirectory {
    path: PathBuf,
    inner: std::sync::Arc<HostDirectoryInner>,
}
impl HostDirectory {
    pub(crate) fn inspect(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.canonicalize().ok().as_deref() != Some(path) {
            return Err(err("WINDOWS_HOST_PATH_INVALID"));
        }
        let ancestors = pin_ancestors(path.parent().ok_or_else(|| err("WINDOWS_HOST_PATH_INVALID"))?)?;
        let file = open(path, true)?;
        let sid = Sid::current()?;
        host_permissions(&file, &sid, true)?;
        let value = Self { path: path.into(), inner: std::sync::Arc::new(HostDirectoryInner {
            id: identity(&file, true)?, file, sid, ancestors, parents: std::sync::Mutex::new(Vec::new()), children: std::sync::Mutex::new(std::collections::BTreeMap::new()), created: false, publishable: false,
        }) };
        value.check()?;
        Ok(value)
    }
    /// The caller authenticates this inactive candidate before publication.
    /// This grants no existing-object permission mutation or byte-writing access.
    pub(crate) fn inspect_for_publication(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.canonicalize().ok().as_deref() != Some(path) {
            return Err(err("WINDOWS_HOST_PATH_INVALID"));
        }
        let ancestors = pin_ancestors(path.parent().ok_or_else(|| err("WINDOWS_HOST_PATH_INVALID"))?)?;
        let file = host_open(path, true, fsapi::FILE_LIST_DIRECTORY | fsapi::DELETE,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE)?;
        let sid = Sid::current()?;
        host_permissions(&file, &sid, true)?;
        let value = Self { path: path.into(), inner: std::sync::Arc::new(HostDirectoryInner {
            id: identity(&file, true)?, file, sid, ancestors,
            parents: std::sync::Mutex::new(Vec::new()),
            children: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            created: false, publishable: true,
        }) };
        value.check()?; Ok(value)
    }
    fn contains_identity(&self, wanted: &Identity) -> Result<bool> {
        if &self.inner.id == wanted { return Ok(true); }
        for parent in self.inner.parents.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?.iter() {
            if parent.contains_identity(wanted)? { return Ok(true); }
        }
        Ok(false)
    }
    pub(crate) fn path(&self) -> &Path { &self.path }
    pub(crate) fn metadata(&self) -> Result<std::fs::Metadata> {
        self.check()?;
        self.inner.file.metadata().map_err(|_| err("WINDOWS_HOST_IO"))
    }
    pub(crate) fn check(&self) -> Result<()> {
        for (path, file, expected) in &self.inner.ancestors {
            if identity(file, true)? != *expected || identity(&open(path, true)?, true)? != *expected {
                return Err(err("WINDOWS_HOST_IDENTITY_CHANGED"));
            }
        }
        for parent in self.inner.parents.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?.iter() { parent.check()?; }
        let named = host_open(&self.path, true, fsapi::FILE_READ_ATTRIBUTES,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE | fsapi::FILE_SHARE_DELETE)?;
        if identity(&self.inner.file, true)? != self.inner.id || identity(&named, true)? != self.inner.id {
            return Err(err("WINDOWS_HOST_IDENTITY_CHANGED"));
        }
        host_permissions(&self.inner.file, &self.inner.sid, true)?;
        Ok(())
    }
    pub(crate) fn permissions(&self) -> Result<HostPermissions> {
        self.check()?;
        host_permissions(&self.inner.file, &self.inner.sid, true)
    }
    pub(crate) fn check_private(&self) -> Result<()> {
        self.check()?;
        acl(&self.inner.file, &self.inner.sid, true)
    }

    pub(crate) fn child_directory(&self, name: &str) -> Result<Self> {
        host_name(name)?; self.check()?;
        let path = self.path.join(name);
        if let Some(inner) = self.inner.children.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?
            .get(name).and_then(std::sync::Weak::upgrade) {
            let value = Self { path, inner }; value.check()?; return Ok(value);
        }
        let file = host_open(&path, true, fsapi::FILE_LIST_DIRECTORY,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE)?;
        let sid = Sid::current()?;
        host_permissions(&file, &sid, true)?;
        let value = Self { path, inner: std::sync::Arc::new(HostDirectoryInner {
            id: identity(&file, true)?, file, sid, ancestors: Vec::new(), parents: std::sync::Mutex::new(vec![self.clone()]), children: std::sync::Mutex::new(std::collections::BTreeMap::new()), created: false, publishable: false,
        }) };
        value.check()?; Ok(value)
    }
    pub(crate) fn child_file(&self, name: &str) -> Result<HostFile> { HostFile::from_parent(self.clone(), name, false) }
    pub(crate) fn create_file(&self, name: &str) -> Result<HostFile> { HostFile::from_parent(self.clone(), name, true) }
    pub(crate) fn create_directory(&self, name: &str) -> Result<Self> {
        host_name(name)?; self.check()?;
        let path = self.path.join(name); let sid = Sid::current()?; let sd = descriptor(&sid, true)?;
        let attributes = sec::SECURITY_ATTRIBUTES { nLength: mem::size_of::<sec::SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0, bInheritHandle: 0 };
        if unsafe { fsapi::CreateDirectoryW(wide(&path)?.as_ptr(), &attributes) } == 0 {
            return Err(err("WINDOWS_HOST_CREATE_REFUSED"));
        }
        let file = host_open(&path, true, fsapi::FILE_LIST_DIRECTORY | fsapi::DELETE
            | fsapi::WRITE_DAC | fsapi::WRITE_OWNER | fsapi::FILE_WRITE_ATTRIBUTES,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE)?;
        let value = Self { path, inner: std::sync::Arc::new(HostDirectoryInner {
            id: identity(&file, true)?, file, sid, ancestors: Vec::new(), parents: std::sync::Mutex::new(vec![self.clone()]), children: std::sync::Mutex::new(std::collections::BTreeMap::new()), created: true, publishable: true,
        }) };
        value.check()?;
        self.inner.children.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?
            .insert(name.into(), std::sync::Arc::downgrade(&value.inner));
        Ok(value)
    }
    pub(crate) fn apply_permissions(&self, permissions: &HostPermissions) -> Result<()> {
        if !self.inner.created { return Err(err("WINDOWS_HOST_EXISTING_PERMISSIONS_IMMUTABLE")); }
        self.check()?;
        host_apply(&self.inner.file, &self.inner.sid, true, permissions)?;
        self.check()
    }
    pub(crate) fn sync_all(&self) -> Result<HostDirectorySync> {
        self.check()?;
        // These retained directory handles have no GENERIC_WRITE grant; Windows
        // does not provide Unix directory-fsync semantics through this handle.
        // NEW files use write-through handles and actual file FlushFileBuffers.
        // Callers retain this explicit evidence and perform final cold reopening.
        Ok(HostDirectorySync {
            directory_flush: false,
            publication_namespace_checked: true,
        })
    }
    pub(crate) fn publish_into(&mut self, parent: &HostDirectory, name: &str) -> Result<()> {
        if !self.inner.publishable { return Err(err("WINDOWS_HOST_EXISTING_PUBLICATION_REFUSED")); }
        self.check()?; parent.check()?; host_name(name)?;
        if parent.contains_identity(&self.inner.id)? { return Err(err("WINDOWS_HOST_PUBLICATION_REFUSED")); }
        let permissions = self.permissions()?;
        let target = parent.path.join(name);
        self.inner.parents.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?.push(parent.clone());
        host_publish(&self.inner.file, &target)?;
        self.path = target;
        parent.inner.children.lock().map_err(|_| err("WINDOWS_HOST_BUSY"))?
            .insert(name.into(), std::sync::Arc::downgrade(&self.inner));
        self.check().map_err(|_| err("WINDOWS_HOST_PUBLICATION_UNCERTAIN"))?;
        if self.permissions()? != permissions { return Err(err("WINDOWS_HOST_PUBLICATION_UNCERTAIN")); }
        parent.check().map_err(|_| err("WINDOWS_HOST_PUBLICATION_UNCERTAIN"))
    }
}
pub(crate) struct HostFile {
    path: PathBuf,
    file: File,
    parent: HostDirectory,
    id: Identity,
    created: bool,
    sealed: std::cell::Cell<bool>,
}
impl HostFile {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.canonicalize().ok().as_deref() != Some(path) {
            return Err(err("WINDOWS_HOST_PATH_INVALID"));
        }
        let parent = HostDirectory::inspect(path.parent().ok_or_else(|| err("WINDOWS_HOST_PATH_INVALID"))?)?;
        let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| err("WINDOWS_HOST_PATH_INVALID"))?;
        parent.child_file(name)
    }
    fn from_parent(parent: HostDirectory, name: &str, created: bool) -> Result<Self> {
        host_name(name)?; parent.check()?; let path = parent.path.join(name);
        let file = if created {
            let sd = descriptor(&parent.inner.sid, false)?;
            let attributes = sec::SECURITY_ATTRIBUTES { nLength: mem::size_of::<sec::SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0, bInheritHandle: 0 };
            let handle = unsafe { fsapi::CreateFileW(wide(&path)?.as_ptr(), 0x80000000 | 0x40000000
                | fsapi::READ_CONTROL | fsapi::WRITE_DAC | fsapi::WRITE_OWNER | fsapi::DELETE | fsapi::FILE_WRITE_ATTRIBUTES,
                fsapi::FILE_SHARE_READ, &attributes, fsapi::CREATE_NEW,
                fsapi::FILE_ATTRIBUTE_NORMAL | fsapi::FILE_FLAG_OPEN_REPARSE_POINT | fsapi::FILE_FLAG_WRITE_THROUGH, ptr::null_mut()) };
            if handle == INVALID_HANDLE_VALUE { return Err(err("WINDOWS_HOST_CREATE_REFUSED")); }
            unsafe { File::from_raw_handle(handle.cast()) }
        } else { host_open(&path, false, 0x80000000, fsapi::FILE_SHARE_READ)? };
        let value = Self { path, id: identity(&file, false)?, file, parent, created, sealed: std::cell::Cell::new(false) };
        value.check()?; Ok(value)
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.parent.check()?;
        let named = host_open(&self.path, false, fsapi::FILE_READ_ATTRIBUTES,
            fsapi::FILE_SHARE_READ | fsapi::FILE_SHARE_WRITE | fsapi::FILE_SHARE_DELETE)?;
        if identity(&self.file, false)? != self.id || identity(&named, false)? != self.id {
            return Err(err("WINDOWS_HOST_IDENTITY_CHANGED"));
        }
        host_permissions(&self.file, &self.parent.inner.sid, false)?; Ok(())
    }
    pub(crate) fn metadata(&self) -> Result<std::fs::Metadata> {
        self.check()?; self.file.metadata().map_err(|_| err("WINDOWS_HOST_IO"))
    }
    pub(crate) fn check_private(&self) -> Result<()> {
        self.check()?;
        acl(&self.file, &self.parent.inner.sid, true)
    }
    pub(crate) fn path(&self) -> &Path { &self.path }

    pub(crate) fn permissions(&self) -> Result<HostPermissions> {
        self.check()?; host_permissions(&self.file, &self.parent.inner.sid, false)
    }
    pub(crate) fn apply_permissions(&self, permissions: &HostPermissions) -> Result<()> {
        if !self.created { return Err(err("WINDOWS_HOST_EXISTING_PERMISSIONS_IMMUTABLE")); }
        self.check()?;
        host_apply(&self.file, &self.parent.inner.sid, false, permissions)?;
        self.sealed.set(true);
        self.check()
    }
    pub(crate) fn sync_all(&self) -> Result<()> {
        self.check()?; self.file.sync_all().map_err(|_| err("WINDOWS_HOST_IO"))?; self.check()
    }
    pub(crate) fn publish_into(&mut self, parent: &HostDirectory, name: &str) -> Result<()> {
        if !self.created { return Err(err("WINDOWS_HOST_EXISTING_PUBLICATION_REFUSED")); }
        self.check()?; parent.check()?; host_name(name)?;
        let permissions = self.permissions()?;
        self.sync_all()?;
        let target = parent.path.join(name); host_publish(&self.file, &target)?;
        self.path = target; self.parent = parent.clone();
        self.sync_all().map_err(|_| err("WINDOWS_HOST_PUBLICATION_UNCERTAIN"))?;
        self.check().map_err(|_| err("WINDOWS_HOST_PUBLICATION_UNCERTAIN"))?;
        if self.permissions()? != permissions { return Err(err("WINDOWS_HOST_PUBLICATION_UNCERTAIN")); }
        Ok(())
    }
}
impl Read for HostFile {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.check().map_err(|_| std::io::Error::other("WINDOWS_HOST_SOURCE_CHANGED"))?;
        self.file.read(bytes)
    }
}
impl Seek for HostFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.check().map_err(|_| std::io::Error::other("WINDOWS_HOST_SOURCE_CHANGED"))?;
        self.file.seek(position)
    }
}
impl Write for HostFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.created || self.sealed.get() { return Err(std::io::Error::other("WINDOWS_HOST_SOURCE_IMMUTABLE")); }
        self.check().map_err(|_| std::io::Error::other("WINDOWS_HOST_SOURCE_CHANGED"))?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> { self.file.flush() }
}
fn host_apply(file: &File, sid: &Sid, directory: bool, permissions: &HostPermissions) -> Result<()> {
    if permissions.security_descriptor.len() > 65536 || permissions.security_descriptor.contains('\0')
        || permissions.attributes & !HOST_ATTRIBUTES != 0 { return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID")); }
    let text: Vec<_> = permissions.security_descriptor.encode_utf16().chain([0]).collect();
    let mut sd = ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut sd, ptr::null_mut()) } == 0 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
    }
    let _sd = Local(sd); let (mut owner, mut defaulted) = (ptr::null_mut(), 0);
    let (mut dacl, mut present) = (ptr::null_mut(), 0); let (mut control, mut revision) = (0, 0);
    if unsafe { sec::GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) } == 0
        || unsafe { sec::GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) } == 0
        || unsafe { sec::GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0
        || owner.is_null() || dacl.is_null() || present == 0 || unsafe { sec::EqualSid(owner, sid.ptr()) } == 0 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_INVALID"));
    }
    host_validate_descriptor(sd, sid)?;
    let information = sec::OWNER_SECURITY_INFORMATION | sec::DACL_SECURITY_INFORMATION
        | if control & sec::SE_DACL_PROTECTED != 0 { sec::PROTECTED_DACL_SECURITY_INFORMATION } else { sec::UNPROTECTED_DACL_SECURITY_INFORMATION };
    if unsafe { sec::Authorization::SetSecurityInfo(file.as_raw_handle().cast(), SE_FILE_OBJECT,
        information, owner, ptr::null_mut(), dacl, ptr::null_mut()) } != 0 {
        return Err(err("WINDOWS_HOST_PERMISSIONS_APPLY_UNCERTAIN"));
    }
    let mut basic: fsapi::FILE_BASIC_INFO = unsafe { mem::zeroed() };
    basic.FileAttributes = if permissions.attributes == 0 { fsapi::FILE_ATTRIBUTE_NORMAL } else { permissions.attributes };
    if unsafe { fsapi::SetFileInformationByHandle(file.as_raw_handle().cast(), fsapi::FileBasicInfo,
        ptr::from_ref(&basic).cast(), mem::size_of::<fsapi::FILE_BASIC_INFO>() as u32) } == 0
        || host_permissions(file, sid, directory)? != *permissions {
        return Err(err("WINDOWS_HOST_PERMISSIONS_APPLY_UNCERTAIN"));
    }
    Ok(())
}
fn host_publish(file: &File, destination: &Path) -> Result<()> {
    let filename = wide(destination)?;
    if filename.len() > 32768 { return Err(err("WINDOWS_HOST_PATH_INVALID")); }
    let size = (mem::offset_of!(fsapi::FILE_RENAME_INFO, FileName) + filename.len() * 2)
        .max(mem::size_of::<fsapi::FILE_RENAME_INFO>());
    let mut buffer = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<fsapi::FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false; (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = ((filename.len() - 1) * 2) as u32;
        ptr::copy_nonoverlapping(filename.as_ptr(), ptr::addr_of_mut!((*info).FileName).cast::<u16>(), filename.len());
    }
    if unsafe { fsapi::SetFileInformationByHandle(file.as_raw_handle().cast(), fsapi::FileRenameInfo,
        buffer.as_ptr().cast(), size as u32) } == 0 { return Err(err("WINDOWS_HOST_PUBLICATION_REFUSED")); }
    Ok(())
}

fn host_name(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty() || name.encode_utf16().count() > 255 || name == "." || name == ".."
        || name.ends_with(['.', ' ']) || reserved
        || name.chars().any(|value| value.is_control() || "<>:\"/\\|?*".contains(value)) {
        return Err(err("WINDOWS_HOST_PATH_INVALID"));
    }
    Ok(())
}
#[link(name = "ntdll")]
unsafe extern "system" {
    #[link_name = "RtlUpcaseUnicodeChar"]
    fn host_nt_upcase_unicode_char(source: u16) -> u16;
}
/// NT's one-code-unit ordinal upper-case mapping does not expand names or use locale rules.
pub(crate) fn ordinal_case_key(relative: &str) -> Result<String> {
    let mut units = Vec::new();
    for (index, component) in relative.split('/').enumerate() {
        host_name(component)?;
        if index != 0 { units.push(u16::from(b'/')); }
        units.extend(component.encode_utf16().map(|unit| unsafe { host_nt_upcase_unicode_char(unit) }));
    }
    String::from_utf16(&units).map_err(|_| err("WINDOWS_HOST_PATH_INVALID"))
}

#[cfg(test)]
mod host_native_tests {
    use super::*;
    fn root() -> PrivateDirectory {
        PrivateDirectory::create(&std::env::temp_dir().join(format!("host-native-{}", uuid::Uuid::new_v4()))).unwrap()
    }
    #[test]
    fn host_restore_preserves_public_read_acl_attributes_and_exact_bytes() {
        let root = root();
        let path = root.path().to_owned();
        let source = path.join("source.bin");
        let mut writer = root.create_record("source.bin").unwrap();
        writer.write_all(b"original synthetic host bytes").unwrap();
        writer.sync_all().unwrap();
        drop(writer);
        let status = crate::process_window::background_command("icacls.exe")
            .arg(&source).args(["/grant", "*S-1-1-0:R"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
        assert!(status.success());
        let mut attributes = std::fs::metadata(&source).unwrap().permissions();
        attributes.set_readonly(true); std::fs::set_permissions(&source, attributes).unwrap();
        let mut original = HostFile::open(&source).unwrap();
        let permissions = original.permissions().unwrap();
        assert!(original.check_private().is_err());
        let parent = HostDirectory::inspect(&path).unwrap();
        let stage = parent.create_directory("stage").unwrap();
        let mut restored = stage.create_file("source.bin").unwrap();
        std::io::copy(&mut original, &mut restored).unwrap();
        restored.sync_all().unwrap(); restored.apply_permissions(&permissions).unwrap();
        assert_eq!(restored.permissions().unwrap(), permissions);
        assert!(restored.write_all(b"must refuse").is_err());
        assert_eq!(original.permissions().unwrap(), permissions);
        drop(restored);
        let mut observed = stage.child_file("source.bin").unwrap();
        let mut bytes = Vec::new(); observed.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"original synthetic host bytes");
        assert_eq!(observed.permissions().unwrap(), permissions);
        drop(observed); drop(original); drop(stage); drop(parent); drop(root);
        for file in [&source, &path.join("stage/source.bin")] {
            let mut attributes = std::fs::metadata(file).unwrap().permissions();
            attributes.set_readonly(false); std::fs::set_permissions(file, attributes).unwrap();
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(path.join("stage")).unwrap(); std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn host_publication_retains_identity_and_never_overwrites_existing_destination() {
        let root = root(); let path = root.path().to_owned();
        let parent = HostDirectory::inspect(&path).unwrap();
        let mut stage = parent.create_directory("stage").unwrap();
        let occupied = parent.create_directory("occupied").unwrap();
        let permissions = stage.permissions().unwrap();
        assert!(stage.publish_into(&parent, "occupied").is_err());
        stage.check().unwrap(); occupied.check().unwrap();
        assert_eq!(stage.permissions().unwrap(), permissions);
        stage.publish_into(&parent, "published").unwrap();
        assert_eq!(stage.permissions().unwrap(), permissions);
        assert!(!path.join("stage").exists());
        let held = parent.child_directory("published").unwrap(); held.check().unwrap(); drop(held);
        let evidence = stage.sync_all().unwrap();
        assert!(!evidence.directory_flush && evidence.publication_namespace_checked);
        drop(stage);
        let mut cold = HostDirectory::inspect_for_publication(&path.join("published")).unwrap();
        assert!(cold.apply_permissions(&permissions).is_err());
        cold.publish_into(&parent, "activated").unwrap(); cold.check().unwrap();
        drop(cold); drop(occupied); drop(parent); drop(root);
        std::fs::remove_dir(path.join("occupied")).unwrap();
        std::fs::remove_dir(path.join("activated")).unwrap(); std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn host_source_refuses_writer_hardlink_and_untrusted_write_grants_without_repair() {
        let root = root(); let path = root.path().to_owned(); let source = path.join("source.bin");
        let mut writer = root.create_record("source.bin").unwrap();
        writer.write_all(b"synthetic original").unwrap(); writer.sync_all().unwrap();
        assert!(HostFile::open(&source).is_err()); drop(writer);
        let alias = path.join("alias.bin"); std::fs::hard_link(&source, &alias).unwrap();
        assert!(HostFile::open(&source).is_err()); std::fs::remove_file(alias).unwrap();
        let status = crate::process_window::background_command("icacls.exe")
            .arg(&source).args(["/grant", "*S-1-1-0:W"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
        assert!(status.success()); assert!(HostFile::open(&source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"synthetic original");
        assert_eq!(ordinal_case_key("Ä.txt").unwrap(), ordinal_case_key("ä.txt").unwrap());
        drop(root); std::fs::remove_file(source).unwrap(); std::fs::remove_dir(path).unwrap();
    }
}

#[cfg(test)]
mod host_inherited_permissions_tests {
    use super::*;
    #[test]
    fn host_restore_keeps_inherited_read_acl_and_new_file_publication_never_replaces() {
        let root = PrivateDirectory::create(&std::env::temp_dir().join(format!("host-inherited-{}", uuid::Uuid::new_v4()))).unwrap();
        let path = root.path().to_owned();
        let source_dir = PrivateDirectory::create(&path.join("source")).unwrap();
        let mut original = source_dir.create_record("original.bin").unwrap();
        original.write_all(b"synthetic inherited permissions").unwrap(); original.sync_all().unwrap();
        drop(original); drop(source_dir);
        for (target, arguments) in [
            (path.join("source"), vec!["/grant", "*S-1-1-0:(OI)(CI)R"]),
            (path.join("source/original.bin"), vec!["/inheritance:e"]),
        ] {
            let status = crate::process_window::background_command("icacls.exe")
                .arg(target).args(arguments).stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()).status().unwrap();
            assert!(status.success());
        }
        let parent = HostDirectory::inspect(&path).unwrap();
        let source = parent.child_directory("source").unwrap();
        let mut input = source.child_file("original.bin").unwrap();
        let directory_permissions = source.permissions().unwrap();
        let permissions = input.permissions().unwrap();
        let stage = parent.create_directory("stage").unwrap();
        stage.apply_permissions(&directory_permissions).unwrap();
        let mut output = stage.create_file("original.bin").unwrap();
        std::io::copy(&mut input, &mut output).unwrap(); output.sync_all().unwrap();
        output.apply_permissions(&permissions).unwrap();
        assert_eq!(output.permissions().unwrap(), permissions);
        assert_eq!(input.permissions().unwrap(), permissions);
        drop(output); drop(input); drop(source); drop(stage);
        let mut existing = root.create_record("occupied.bin").unwrap();
        existing.write_all(b"preserved occupied bytes").unwrap(); drop(existing);
        let mut pending = parent.create_file("pending.bin").unwrap();
        pending.write_all(b"new synthetic ciphertext").unwrap(); pending.sync_all().unwrap();
        assert!(pending.publish_into(&parent, "occupied.bin").is_err());
        assert_eq!(std::fs::read(path.join("occupied.bin")).unwrap(), b"preserved occupied bytes");
        pending.check_private().unwrap();
        pending.publish_into(&parent, "ciphertext.bin").unwrap(); pending.check_private().unwrap();
        drop(pending);
        let cold = parent.child_file("ciphertext.bin").unwrap(); cold.check_private().unwrap(); drop(cold);
        assert!(!path.join("pending.bin").exists());
        drop(parent); drop(root);
        for file in ["source/original.bin", "stage/original.bin", "occupied.bin", "ciphertext.bin"] {
            std::fs::remove_file(path.join(file)).unwrap();
        }
        std::fs::remove_dir(path.join("source")).unwrap(); std::fs::remove_dir(path.join("stage")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
}
