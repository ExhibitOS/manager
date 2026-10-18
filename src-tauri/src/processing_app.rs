// SPDX-License-Identifier: Apache-2.0
//! Optional user-launched processing application. Never starts a reconstruction or requires Platform.
use exhibitos_lifecycle::LifecycleError;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
const BUNDLE: &str = "org.exhibitos.processing";
static ACTION: Mutex<()> = Mutex::new(());
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Status,
    Choose,
    OpenDetected,
    OpenSelected,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub path: String,
    pub name: String,
    pub version: String,
    pub bundle_id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    supported: bool,
    detected_count: usize,
    detected: Option<Application>,
    selected: Option<Application>,
    selection_unavailable: bool,
    store_available: bool,
}
fn fail(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:"선택 도구를 확인하지 못했어요. 앱을 다시 선택하거나 상태를 확인하세요. Studio와 기존 파일은 그대로 사용할 수 있습니다.".into()}
}
#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::{CString, c_char, c_void},
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        ptr,
    };
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFURLGetTypeID() -> usize;
        fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
        fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> bool;
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFPropertyListCreateWithData(
            allocator: Ref,
            data: Ref,
            options: usize,
            format: *mut isize,
            error: *mut Ref,
        ) -> Ref;
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        fn CFArrayGetCount(array: Ref) -> isize;
        fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
        fn CFURLCopyFileSystemPath(url: Ref, style: isize) -> Ref;
        fn CFErrorGetCode(error: Ref) -> isize;
    }
    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyApplicationURLsForBundleIdentifier(bundle: Ref, error: *mut Ref) -> Ref;
    }
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }
    fn owned(value: Ref) -> Result<Owned, LifecycleError> {
        if value.is_null() {
            Err(fail("PROCESSING_APP_INVALID"))
        } else {
            Ok(Owned(value))
        }
    }
    fn string(value: &str) -> Result<Owned, LifecycleError> {
        let text = CString::new(value).map_err(|_| fail("PROCESSING_APP_INVALID"))?;
        owned(unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), 0x08000100) })
    }
    fn text(value: Ref) -> Result<String, LifecycleError> {
        if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let mut bytes = [0u8; 4096];
        if !unsafe {
            CFStringGetCString(
                value,
                bytes.as_mut_ptr().cast(),
                bytes.len() as isize,
                0x08000100,
            )
        } {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let size = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| fail("PROCESSING_APP_INVALID"))?;
        String::from_utf8(bytes[..size].to_vec()).map_err(|_| fail("PROCESSING_APP_INVALID"))
    }
    pub(super) fn inspect(path: &Path) -> Result<Application, LifecycleError> {
        if !path.is_absolute() {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let canonical = fs::canonicalize(path).map_err(|_| fail("PROCESSING_APP_UNAVAILABLE"))?;
        let value = canonical
            .to_str()
            .ok_or_else(|| fail("PROCESSING_APP_INVALID"))?;
        if !canonical.is_absolute()
            || canonical.extension().and_then(|x| x.to_str()) != Some("app")
            || value.len() > 2048
            || value.chars().any(char::is_control)
            || !fs::metadata(&canonical)
                .map_err(|_| fail("PROCESSING_APP_INVALID"))?
                .is_dir()
        {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let plist = canonical.join("Contents/Info.plist");
        if fs::canonicalize(&plist).ok().as_deref() != Some(plist.as_path()) {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&plist)
            .map_err(|_| fail("PROCESSING_APP_INVALID"))?;
        if !file
            .metadata()
            .map_err(|_| fail("PROCESSING_APP_INVALID"))?
            .is_file()
        {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let mut bytes = Vec::new();
        file.take(262145)
            .read_to_end(&mut bytes)
            .map_err(|_| fail("PROCESSING_APP_INVALID"))?;
        if bytes.is_empty() || bytes.len() > 262144 {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let data =
            owned(unsafe { CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize) })?;
        let mut error = ptr::null();
        let dictionary = owned(unsafe {
            CFPropertyListCreateWithData(ptr::null(), data.0, 0, ptr::null_mut(), &mut error)
        });
        let _error = Owned(error);
        let dictionary = dictionary?;
        if unsafe { CFGetTypeID(dictionary.0) != CFDictionaryGetTypeID() } {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let field = |name: &str| -> Result<String, LifecycleError> {
            let key = string(name)?;
            text(unsafe { CFDictionaryGetValue(dictionary.0, key.0) })
        };
        if field("CFBundlePackageType")? != "APPL" {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let bundle_id = field("CFBundleIdentifier")?;
        let executable = field("CFBundleExecutable")?;
        if bundle_id.is_empty()
            || bundle_id.len() > 256
            || !bundle_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            || executable.is_empty()
            || executable.len() > 256
            || executable.contains('/')
            || executable.chars().any(char::is_control)
            || executable == "."
            || executable == ".."
        {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let exe = canonical.join("Contents/MacOS").join(executable);
        if fs::canonicalize(&exe).ok().as_deref() != Some(exe.as_path()) {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let metadata = fs::symlink_metadata(exe).map_err(|_| fail("PROCESSING_APP_INVALID"))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(fail("PROCESSING_APP_INVALID"));
        }
        let clean = |v: String| {
            v.chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect::<String>()
        };
        let name = field("CFBundleDisplayName")
            .or_else(|_| field("CFBundleName"))
            .unwrap_or_else(|_| {
                canonical
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            });
        let version = field("CFBundleShortVersionString").unwrap_or_default();
        Ok(Application {
            path: value.into(),
            name: clean(name),
            version: clean(version),
            bundle_id,
        })
    }
    fn detected() -> Result<Vec<Application>, LifecycleError> {
        let bundle = string(BUNDLE)?;
        let mut error = ptr::null();
        let urls = unsafe { LSCopyApplicationURLsForBundleIdentifier(bundle.0, &mut error) };
        let error = Owned(error);
        if urls.is_null() {
            return if error.0.is_null() || unsafe { CFErrorGetCode(error.0) } == -10814 {
                Ok(Vec::new())
            } else {
                Err(fail("PROCESSING_DETECTION_FAILED"))
            };
        }
        let urls = Owned(urls);
        let count = unsafe { CFArrayGetCount(urls.0) };
        if !(0..=128).contains(&count) {
            return Err(fail("PROCESSING_DETECTION_FAILED"));
        }
        let mut applications = std::collections::BTreeMap::new();
        for index in 0..count {
            let url = unsafe { CFArrayGetValueAtIndex(urls.0, index) };
            if url.is_null() || unsafe { CFGetTypeID(url) != CFURLGetTypeID() } {
                continue;
            }
            let path = owned(unsafe { CFURLCopyFileSystemPath(url, 0) })?;
            if let Ok(path) = text(path.0) {
                if let Ok(app) = inspect(Path::new(&path)) {
                    if app.bundle_id == BUNDLE {
                        applications.insert(app.path.clone(), app);
                    }
                }
            }
        }
        Ok(applications.into_values().collect())
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Preference {
        version: u8,
        path: String,
    }
    fn setting(config: &Path) -> Result<Option<PathBuf>, LifecycleError> {
        let path = config.join("processing-application.json");
        let m = match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(fail("PROCESSING_SELECTION_UNAVAILABLE")),
            Ok(m) => m,
        };
        if !m.is_file()
            || m.is_symlink()
            || m.nlink() != 1
            || m.mode() & 0o7777 != 0o600
            || m.uid() != unsafe { libc::geteuid() }
            || m.len() > 4096
        {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        let mut bytes = Vec::new();
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        let pref: Preference =
            serde_json::from_slice(&bytes).map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        if pref.version != 1
            || !Path::new(&pref.path).is_absolute()
            || !pref.path.ends_with(".app")
            || pref.path.len() > 2048
            || pref.path.chars().any(char::is_control)
        {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        Ok(Some(pref.path.into()))
    }
    pub(super) fn save(config: &Path, app: &Application) -> Result<(), LifecycleError> {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStringExt,
        };
        if !config.exists() {
            fs::create_dir_all(config).map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
            fs::set_permissions(config, fs::Permissions::from_mode(0o700))
                .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(config)
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        let check = || -> Result<(), LifecycleError> {
            let meta = directory
                .metadata()
                .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
            let current = fs::symlink_metadata(config)
                .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
            let mut path = [0u8; libc::PATH_MAX as usize];
            if unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) } < 0
            {
                return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
            }
            let len = path
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
            let actual = PathBuf::from(std::ffi::OsString::from_vec(path[..len].to_vec()));
            if actual != config
                || meta.uid() != unsafe { libc::geteuid() }
                || current.is_symlink()
                || meta.dev() != current.dev()
                || meta.ino() != current.ino()
                || fs::canonicalize(config).ok().as_deref() != Some(config)
            {
                return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
            }
            Ok(())
        };
        check()?;
        let target = config.join("processing-application.json");
        match fs::symlink_metadata(&target) {
            Ok(m) => {
                if !m.is_file()
                    || m.is_symlink()
                    || m.nlink() != 1
                    || m.mode() & 0o7777 != 0o600
                    || m.uid() != unsafe { libc::geteuid() }
                {
                    return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(fail("PROCESSING_SELECTION_UNAVAILABLE")),
        }
        let stage = format!(
            ".processing-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?
                .as_nanos()
        );
        let stage =
            std::ffi::CString::new(stage).map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        let target = std::ffi::CString::new("processing-application.json").unwrap();
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                stage.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let m = file
            .metadata()
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        if m.mode() & 0o7777 != 0o600 || m.nlink() != 1 {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        check()?;
        let bytes = serde_json::to_vec(&Preference {
            version: 1,
            path: app.path.clone(),
        })
        .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        if bytes.len() > 4096 {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        check()?;
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                stage.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
            )
        } != 0
        {
            return Err(fail("PROCESSING_SELECTION_UNAVAILABLE"));
        }
        directory
            .sync_all()
            .map_err(|_| fail("PROCESSING_SELECTION_UNAVAILABLE"))?;
        check()?;
        Ok(())
    }
    pub(super) fn context(config: &Path) -> Result<Context, LifecycleError> {
        let all = detected()?;
        let pref = setting(config);
        let unavailable = pref.is_err();
        let selected = pref.ok().flatten();
        let exists = selected.is_some();
        let selected = selected.and_then(|p| inspect(&p).ok());
        Ok(Context {
            supported: true,
            detected_count: all.len(),
            detected: if all.len() == 1 {
                all.into_iter().next()
            } else {
                None
            },
            selection_unavailable: unavailable || (exists && selected.is_none()),
            selected,
            store_available: false,
        })
    }
    pub(super) fn open(config: &Path, selected: bool) -> Result<(), LifecycleError> {
        let app = if selected {
            inspect(&setting(config)?.ok_or_else(|| fail("PROCESSING_APP_UNAVAILABLE"))?)?
        } else {
            let mut all = detected()?;
            if all.len() != 1 {
                return Err(fail("PROCESSING_APP_CHOICE_REQUIRED"));
            }
            all.remove(0)
        };
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-a", &app.path])
            .status()
            .map_err(|_| fail("PROCESSING_OPEN_FAILED"))?;
        if !status.success() {
            return Err(fail("PROCESSING_OPEN_FAILED"));
        }
        Ok(())
    }
}
pub fn operation(config: &Path, action: Action) -> Result<Context, LifecycleError> {
    let _guard = ACTION.try_lock().map_err(|_| fail("PROCESSING_BUSY"))?;
    #[cfg(target_os = "macos")]
    {
        match action {
            Action::Status => {}
            Action::Choose => {
                if let Some(path) =
                    super::maintenance_picker::choose(super::maintenance_picker::Kind::Application)
                        .map_err(|error| fail(&error.code))?
                {
                    let app = native::inspect(Path::new(&path))?;
                    native::save(config, &app)?
                }
            }
            Action::OpenDetected => native::open(config, false)?,
            Action::OpenSelected => native::open(config, true)?,
        }
        native::context(config)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (config, action);
        Ok(Context {
            supported: false,
            detected_count: 0,
            detected: None,
            selected: None,
            selection_unavailable: false,
            store_available: false,
        })
    }
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "processing-app-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let app = root.join("Synthetic.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"),br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleIdentifier</key><string>test.synthetic</string><key>CFBundleName</key><string>Synthetic</string><key>CFBundleExecutable</key><string>synthetic</string></dict></plist>"#).unwrap();
        fs::write(
            app.join("Contents/MacOS/synthetic"),
            b"synthetic fixture, never executed",
        )
        .unwrap();
        fs::set_permissions(
            app.join("Contents/MacOS/synthetic"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        (root, app)
    }
    #[test]
    fn app_metadata_and_private_preference_roundtrip_do_not_launch() {
        let (root, app) = fixture();
        let info = native::inspect(&app).unwrap();
        assert_eq!(info.bundle_id, "test.synthetic");
        let config = root.join("settings");
        native::save(&config, &info).unwrap();
        assert_eq!(
            fs::metadata(config.join("processing-application.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        let value = fs::read_to_string(config.join("processing-application.json")).unwrap();
        assert!(value.contains(&info.path) && !value.contains("password"));
        fs::remove_dir_all(root).unwrap()
    }
    #[test]
    fn invalid_or_linked_executable_is_not_a_selected_application() {
        let (root, app) = fixture();
        let exe = app.join("Contents/MacOS/synthetic");
        fs::remove_file(&exe).unwrap();
        symlink("/bin/sh", &exe).unwrap();
        assert!(native::inspect(&app).is_err());
        assert!(native::inspect(&root).is_err());
        fs::remove_dir_all(root).unwrap()
    }
    #[test]
    fn missing_or_deleted_selection_never_initializes_settings_or_platform() {
        let (root, app) = fixture();
        let config = root.join("settings");
        let state = native::context(&config).unwrap();
        assert!(state.selected.is_none());
        assert!(!config.exists());
        let info = native::inspect(&app).unwrap();
        native::save(&config, &info).unwrap();
        fs::remove_dir_all(&app).unwrap();
        let state = native::context(&config).unwrap();
        assert!(state.selection_unavailable && state.selected.is_none());
        assert!(config.join("processing-application.json").is_file());
        fs::remove_dir_all(root).unwrap()
    }
    #[test]
    fn explicit_new_choice_repairs_malformed_path_only_setting_without_following_links() {
        let (root, app) = fixture();
        let info = native::inspect(&app).unwrap();
        let config = root.join("settings");
        native::save(&config, &info).unwrap();
        fs::write(
            config.join("processing-application.json"),
            b"malformed old preference",
        )
        .unwrap();
        native::save(&config, &info).unwrap();
        assert!(native::context(&config).unwrap().selected.is_some());
        let outside = root.join("outside");
        fs::write(&outside, b"preserve").unwrap();
        fs::remove_file(config.join("processing-application.json")).unwrap();
        symlink(&outside, config.join("processing-application.json")).unwrap();
        assert!(native::save(&config, &info).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap()
    }
    #[test]
    fn registered_bundle_read_only_detection_reports_identity_without_launch() {
        let root = std::env::temp_dir().join("exhibitos-unused-processing-readonly-settings");
        assert!(!root.exists());
        let state = native::context(&root).unwrap();
        println!(
            "registeredProcessingCount={} selected=false storeVerified=false launched=false",
            state.detected_count
        );
        if let Some(app) = state.detected {
            assert_eq!(app.bundle_id, BUNDLE);
        }
        assert!(!root.exists());
    }
}
