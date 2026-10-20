// SPDX-License-Identifier: Apache-2.0
//! Fixed-provider generic passwords in the macOS login Keychain. Never shell out with secrets.
// These closed namespaces hold immutable job material, not provider tokens.
pub(crate) fn canonical_backup_uuid(id: &str) -> bool {
    let bytes=id.as_bytes();
    bytes.len()==36 && bytes.iter().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){*b==b'-'}else{b.is_ascii_digit()||(b'a'..=b'f').contains(b)})
        && (b'1'..=b'8').contains(&bytes[14]) && b"89ab".contains(&bytes[19])
}
fn backup_namespace(account: &str) -> bool {
    account.starts_with("render-remote-configuration-") || account.starts_with("render-backup-configuration-") || account.starts_with("render-backup-key-")
}
fn add_backup_only_with(account: &str, bytes: &[u8], add: impl FnOnce(&str,&[u8])->Result<i32,()>) -> Result<(),()> {
    let valid=if let Some(id)=account.strip_prefix("render-backup-configuration-"){canonical_backup_uuid(id) && (1..=65536).contains(&bytes.len())}
        else if let Some(id)=account.strip_prefix("render-remote-configuration-"){canonical_backup_uuid(id) && (1..=65536).contains(&bytes.len())}
        else if let Some(id)=account.strip_prefix("render-backup-key-"){canonical_backup_uuid(id) && bytes.len()==32}else{false};
    if !valid{return Err(());}
    // Duplicate, denial and every other non-success are terminal. In particular,
    // there is no lookup/update/delete/recreate or random-key fallback here.
    if add(account,bytes)?==0{Ok(())}else{Err(())}
}
#[derive(Debug,PartialEq,Eq)]
pub(crate) enum BackupConfigurationReadError { Missing, Unavailable }
fn configuration_read_status(status:i32,has_result:bool)->Result<(),BackupConfigurationReadError>{
    match (status,has_result){(0,true)=>Ok(()),(-25300,false)=>Err(BackupConfigurationReadError::Missing),_=>Err(BackupConfigurationReadError::Unavailable)}
}
#[cfg(target_os = "macos")]
mod native {
    use std::{
        ffi::{CString, c_char, c_void},
        ptr,
    };
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
        fn CFDictionaryCreateMutable(
            allocator: Ref,
            capacity: isize,
            keys: Ref,
            values: Ref,
        ) -> *mut c_void;
        fn CFDictionarySetValue(dictionary: *mut c_void, key: Ref, value: Ref);
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFDataGetLength(data: Ref) -> isize;
        fn CFDataGetBytePtr(data: Ref) -> *const u8;
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFDataGetTypeID() -> usize;
        fn CFRelease(value: Ref);
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFBooleanTrue: Ref;
    }
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecItemAdd(query: Ref, result: *mut Ref) -> i32;
        fn SecRandomCopyBytes(random: Ref, count: usize, bytes: *mut u8) -> i32;
        fn SecItemUpdate(query: Ref, changes: Ref) -> i32;
        fn SecItemCopyMatching(query: Ref, result: *mut Ref) -> i32;
        fn SecItemDelete(query: Ref) -> i32;
        static kSecClass: Ref;
        static kSecClassGenericPassword: Ref;
        static kSecAttrService: Ref;
        static kSecAttrAccount: Ref;
        static kSecValueData: Ref;
        static kSecReturnData: Ref;
        static kSecMatchLimit: Ref;
        static kSecMatchLimitOne: Ref;
    }
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CFRelease(self.0);
                }
            }
        }
    }
    fn string(value: &str) -> Result<Owned, ()> {
        let text = CString::new(value).map_err(|_| ())?;
        let value = unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), 0x08000100) };
        if value.is_null() {
            Err(())
        } else {
            Ok(Owned(value))
        }
    }
    fn dictionary() -> Result<Owned, ()> {
        let value = unsafe {
            CFDictionaryCreateMutable(
                ptr::null(),
                0,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            )
        };
        if value.is_null() {
            Err(())
        } else {
            Ok(Owned(value))
        }
    }
    fn set(dictionary: &Owned, key: Ref, value: Ref) {
        unsafe {
            CFDictionarySetValue(dictionary.0.cast_mut(), key, value);
        }
    }
    fn query(provider: &str) -> Result<Owned, ()> {
        let backup=provider.strip_prefix("render-backup-key-").is_some_and(|id|id.len()==36&&id.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_digit()||(b'a'..=b'f').contains(&b)}));
        let configuration=provider.strip_prefix("render-backup-configuration-").is_some_and(super::canonical_backup_uuid)||provider.strip_prefix("render-remote-configuration-").is_some_and(super::canonical_backup_uuid);
        if !backup && !configuration && !matches!(provider, "cloudflare" | "render" | "hosted-work" | "render-recovery-source")
            && !(provider.starts_with("render-runtime-")
                && provider.len() == 79
                && provider[15..].bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(());
        }
        let query = dictionary()?;
        let service = string("org.exhibitos.manager.hosted")?;
        let account = string(provider)?;
        unsafe {
            set(&query, kSecClass, kSecClassGenericPassword);
            set(&query, kSecAttrService, service.0);
            set(&query, kSecAttrAccount, account.0);
        }
        Ok(query)
    }
    pub fn random(bytes: &mut [u8]) -> Result<(), ()> {
        if unsafe { SecRandomCopyBytes(ptr::null(), bytes.len(), bytes.as_mut_ptr()) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn contains(provider: &str) -> Result<bool, ()> {
        let query = query(provider)?;
        match unsafe { SecItemCopyMatching(query.0, ptr::null_mut()) } {
            0 => Ok(true),
            -25300 => Ok(false),
            _ => Err(()),
        }
    }
    pub fn load(provider: &str) -> Result<Vec<u8>, ()> {
        let query = query(provider)?;
        unsafe {
            set(&query, kSecReturnData, kCFBooleanTrue);
            set(&query, kSecMatchLimit, kSecMatchLimitOne);
        }
        let mut result = ptr::null();
        if unsafe { SecItemCopyMatching(query.0, &mut result) } != 0 || result.is_null() {
            return Err(());
        }
        let result = Owned(result);
        if unsafe { CFGetTypeID(result.0) != CFDataGetTypeID() } {
            return Err(());
        }
        let size = unsafe { CFDataGetLength(result.0) };
        if !(1..=131072).contains(&size) {
            return Err(());
        }
        Ok(
            unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(result.0), size as usize) }
                .to_vec(),
        )
    }
    pub fn load_backup_configuration(account:&str)->Result<Vec<u8>,super::BackupConfigurationReadError>{
        use super::BackupConfigurationReadError as E;
        if !account.strip_prefix("render-backup-configuration-").or_else(||account.strip_prefix("render-remote-configuration-")).is_some_and(super::canonical_backup_uuid){return Err(E::Unavailable);}
        let query=query(account).map_err(|_|E::Unavailable)?;
        unsafe {set(&query,kSecReturnData,kCFBooleanTrue);set(&query,kSecMatchLimit,kSecMatchLimitOne);}
        let mut value=ptr::null();let status=unsafe{SecItemCopyMatching(query.0,&mut value)};
        let value=Owned(value);super::configuration_read_status(status,!value.0.is_null())?;
        if unsafe{CFGetTypeID(value.0)!=CFDataGetTypeID()}{return Err(E::Unavailable);}
        let size=unsafe{CFDataGetLength(value.0)};
        if !(1..=65536).contains(&size){return Err(E::Unavailable);}
        Ok(unsafe{std::slice::from_raw_parts(CFDataGetBytePtr(value.0),size as usize)}.to_vec())
    }
    pub fn add_backup_only(account: &str, bytes: &[u8]) -> Result<(), ()> {
        super::add_backup_only_with(account,bytes,|account,bytes|{
            let query=query(account)?;
            let data=Owned(unsafe { CFDataCreate(ptr::null(),bytes.as_ptr(),bytes.len() as isize) });
            if data.0.is_null(){return Err(());}
            unsafe { set(&query,kSecValueData,data.0); }
            Ok(unsafe { SecItemAdd(query.0,ptr::null_mut()) })
        })
    }
    pub fn store(provider: &str, token: &[u8]) -> Result<(), ()> {
        if super::backup_namespace(provider) { return Err(()); }
        if token.is_empty() || token.len() > 131072 {
            return Err(());
        }
        let query = query(provider)?;
        let data =
            Owned(unsafe { CFDataCreate(ptr::null(), token.as_ptr(), token.len() as isize) });
        if data.0.is_null() {
            return Err(());
        }
        let changes = dictionary()?;
        unsafe {
            set(&changes, kSecValueData, data.0);
        }
        match unsafe { SecItemUpdate(query.0, changes.0) } {
            0 => Ok(()),
            -25300 => {
                unsafe {
                    set(&query, kSecValueData, data.0);
                }
                if unsafe { SecItemAdd(query.0, ptr::null_mut()) } == 0 {
                    Ok(())
                } else {
                    Err(())
                }
            }
            _ => Err(()),
        }
    }
    pub fn remove(provider: &str) -> Result<(), ()> {
        if super::backup_namespace(provider) { return Err(()); }
        let query = query(provider)?;
        match unsafe { SecItemDelete(query.0) } {
            0 | -25300 => Ok(()),
            _ => Err(()),
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::{add_backup_only, contains, load, load_backup_configuration, random, remove, store};
#[cfg(not(target_os = "macos"))]
pub fn contains(_: &str) -> Result<bool, ()> {
    Err(())
}
#[cfg(not(target_os = "macos"))]
pub fn load_backup_configuration(_: &str) -> Result<Vec<u8>,BackupConfigurationReadError> { Err(BackupConfigurationReadError::Unavailable) }
#[cfg(not(target_os = "macos"))]
pub fn load(_: &str) -> Result<Vec<u8>, ()> {
    Err(())
}
#[cfg(not(target_os = "macos"))]
pub fn store(_: &str, _: &[u8]) -> Result<(), ()> {
    Err(())
}
#[cfg(not(target_os = "macos"))]
pub fn remove(_: &str) -> Result<(), ()> {
    Err(())
}

#[cfg(not(target_os = "macos"))]
pub fn random(_: &mut [u8]) -> Result<(), ()> {
    Err(())
}

#[cfg(not(target_os = "macos"))]
pub fn add_backup_only(_: &str, _: &[u8]) -> Result<(), ()> { Err(()) }

#[cfg(test)]
mod tests {
    use super::*;
    const ID:&str="c30e227b-e771-43dc-b315-023e0469d4d2";
    #[test]
    fn only_exact_not_found_without_data_can_use_archived_authority(){
        assert_eq!(configuration_read_status(-25300,false),Err(BackupConfigurationReadError::Missing));
        assert_eq!(configuration_read_status(0,true),Ok(()));
        for status in [-128,-25293,-25308,-36,0]{assert_eq!(configuration_read_status(status,false),Err(BackupConfigurationReadError::Unavailable));}
        assert_eq!(configuration_read_status(-25300,true),Err(BackupConfigurationReadError::Unavailable));
    }
    #[test]
    fn immutable_job_material_uses_exactly_one_add_and_preserves_input() {
        for (prefix,bytes) in [("render-backup-key-",vec![23u8;32]),("render-backup-configuration-",b"synthetic private configuration".to_vec()),("render-backup-configuration-",vec![1u8;65536]),("render-remote-configuration-",b"synthetic original connection".to_vec())] {
            let account=format!("{prefix}{ID}");let original=bytes.clone();let mut calls=0;
            assert_eq!(add_backup_only_with(&account,&bytes,|name,value|{calls+=1;assert_eq!(name,account);assert_eq!(value,original);Ok(0)}),Ok(()));
            assert_eq!(calls,1);assert_eq!(bytes,original);
        }
    }
    #[test]
    fn duplicate_or_denied_add_never_overwrites_or_retries_existing_material() {
        for status in [-25299,-25308,-25293,-1] {
            let account=format!("render-remote-configuration-{ID}");let existing=b"retained original".to_vec();let before=existing.clone();let mut calls=0;
            assert_eq!(add_backup_only_with(&account,b"new candidate",|_,_|{calls+=1;Ok(status)}),Err(()));
            assert_eq!(calls,1);assert_eq!(existing,before);
        }
        assert_eq!(add_backup_only_with(&format!("render-backup-key-{ID}"),&[1u8;32],|_,_|Err(())),Err(()));
    }
    #[test]
    fn invalid_namespace_uuid_and_size_refuse_before_any_os_operation() {
        for account in ["render".to_owned(),format!("render-runtime-{ID}"),format!("render-backup-key-{}",ID.to_uppercase()),"render-backup-key-00000000-0000-0000-0000-000000000000".into(),format!("render-backup-configuration-{ID}-extra"),format!("render-backup-configuration-{ID}/other")] {
            assert_eq!(add_backup_only_with(&account,&[1u8;32],|_,_|panic!("invalid input must not call OS")),Err(()));
        }
        for (prefix,n) in [("render-backup-key-",0),("render-backup-key-",31),("render-backup-key-",33),("render-backup-configuration-",0),("render-backup-configuration-",65537),("render-remote-configuration-",0),("render-remote-configuration-",65537)] {
            assert_eq!(add_backup_only_with(&format!("{prefix}{ID}"),&vec![1u8;n],|_,_|panic!("invalid size must not call OS")),Err(()));
        }
        for account in ["cloudflare","render","hosted-work","render-runtime-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] { assert!(!backup_namespace(account)); }
        assert!(backup_namespace(&format!("render-remote-configuration-{ID}")));assert!(backup_namespace(&format!("render-backup-key-{ID}")));assert!(backup_namespace(&format!("render-backup-configuration-{ID}")));
    }
}
