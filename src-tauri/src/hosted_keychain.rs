// SPDX-License-Identifier: Apache-2.0
//! Fixed-provider generic passwords in the macOS login Keychain. Never shell out with secrets.
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
        if !backup && !matches!(provider, "cloudflare" | "render" | "hosted-work")
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
    pub fn store(provider: &str, token: &[u8]) -> Result<(), ()> {
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
        let query = query(provider)?;
        match unsafe { SecItemDelete(query.0) } {
            0 | -25300 => Ok(()),
            _ => Err(()),
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::{contains, load, random, remove, store};
#[cfg(not(target_os = "macos"))]
pub fn contains(_: &str) -> Result<bool, ()> {
    Err(())
}
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
