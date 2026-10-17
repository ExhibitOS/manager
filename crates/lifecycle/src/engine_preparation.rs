// SPDX-License-Identifier: Apache-2.0
//! Closed official Docker Desktop preparation. No VM reset, installer redistribution or license acceptance.
use super::*;
use std::process::{Child, Command};
const VERSION: &str = "4.95.0";
const URL: &str = "https://desktop.docker.com/mac/main/arm64/242831/Docker.dmg";
const SHA: &str = "7857222d3c009d787e7f8347b12e24bfee44e49a68cf455cb4b612991d2492c1";
const SIZE: u64 = 589_939_097;
const ARTIFACT: &str = "Docker-4.95.0-arm64.dmg";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub job_id: Option<String>,
    pub stage: String,
    pub saved_bytes: u64,
    pub total_bytes: u64,
    pub version: String,
    pub error_code: Option<String>,
}
fn fail(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:match code{
 "ENGINE_PREP_OS_PERMISSION"=>"응용 프로그램에 자동 설치할 권한이 없어요. 공식 설치 창에서 macOS 승인을 마친 뒤 다시 확인하세요. 기존 설치와 받은 파일은 유지됩니다.",
 "ENGINE_PREP_COPY"=>"실행 도구 앱을 복사하지 못했어요. 받은 파일과 기존 설치는 유지됩니다. 응용 프로그램 폴더의 권한과 저장 공간을 확인한 뒤 다시 준비하세요.",
 "ENGINE_PREP_STORAGE"=>"실행 도구 설치 파일을 보관할 공간이 부족해요. 기존 파일은 유지됩니다. 여유 공간을 확보한 뒤 다시 시도하세요.",
 "ENGINE_PREP_HASH"|"ENGINE_PREP_SIGNATURE"=>"설치 파일을 확인하지 못해 열지 않았어요. 기존 파일을 보존했습니다. 공식 설치 파일을 다시 준비하세요.",
 "ENGINE_PREP_CANCELLED"=>"설치 파일 준비를 멈췄어요. 받은 부분은 보존되며 다시 준비할 수 있어요. 기존 실행 도구는 변경하지 않았어요.",
 "ENGINE_PREP_DOWNLOAD"=>"설치 파일을 다 받지 못했어요. 받은 부분은 보존됩니다. 연결을 확인한 뒤 다시 준비하세요.",
 "ENGINE_PREP_BUSY"=>"실행 도구를 준비하고 있어요. 진행 상태를 확인하세요.",
 "ENGINE_PREP_UNSUPPORTED"=>"현재 이 설치 파일의 자동 준비는 Apple 칩 Mac용 Docker Desktop에 한해 연결돼 있어요. 기존 실행 도구와 데이터는 변경하지 않았어요.",
 _=>"실행 도구 준비 상태를 확인하지 못했어요. 기존 설치와 파일은 보존됩니다. 상태를 다시 확인하세요.",}.into()}
}
fn safe_file(root: &Path, name: &str) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(root.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(fail("ENGINE_PREP_STATE")),
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(fail("ENGINE_PREP_STATE"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 || m.mode() & 0o077 != 0
                {
                    return Err(fail("ENGINE_PREP_STATE"));
                }
            }
            Ok(Some(m))
        }
    }
}
fn directory(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    if !root.exists() {
        fs::create_dir_all(root).map_err(|_| fail("ENGINE_PREP_STATE"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .map_err(|_| fail("ENGINE_PREP_STATE"))?;
        }
    }
    let m = fs::symlink_metadata(root).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(fail("ENGINE_PREP_STATE"));
        }
    }
    Ok(())
}
fn lock(root: &Path) -> Result<File> {
    directory(root)?;
    safe_file(root, "preparation.lock")?;
    let f = private_options()
        .create_new(false)
        .create(true)
        .truncate(false)
        .read(true)
        .open(root.join("preparation.lock"))
        .map_err(|_| fail("ENGINE_PREP_STATE"))?;
    f.try_lock_exclusive()
        .map_err(|_| fail("ENGINE_PREP_BUSY"))?;
    Ok(f)
}
fn initial() -> Context {
    Context {
        job_id: None,
        stage: "not-started".into(),
        saved_bytes: 0,
        total_bytes: SIZE,
        version: VERSION.into(),
        error_code: None,
    }
}
fn validate(v: &Context) -> Result<()> {
    if v.version != VERSION
        || v.total_bytes != SIZE
        || v.saved_bytes > SIZE
        || !matches!(
            v.stage.as_str(),
            "not-started"
                | "downloading"
                | "verifying"
                | "installer-ready"
                | "installing"
                | "awaiting-installation"
                | "awaiting-engine"
                | "ready"
                | "cancelled"
                | "interrupted"
                | "failed"
        )
        || v.job_id
            .as_ref()
            .is_some_and(|id| Uuid::parse_str(id).is_err())
        || v.error_code.as_ref().is_some_and(|c| {
            !matches!(
                c.as_str(),
                "ENGINE_PREP_COPY"
                    | "ENGINE_PREP_OS_PERMISSION"
                    | "ENGINE_PREP_STATE"
                    | "ENGINE_PREP_STORAGE"
                    | "ENGINE_PREP_DOWNLOAD"
                    | "ENGINE_PREP_HASH"
                    | "ENGINE_PREP_SIGNATURE"
                    | "ENGINE_PREP_CANCELLED"
            )
        })
    {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    Ok(())
}
fn read_state(root: &Path) -> Result<Context> {
    let Some(m) = safe_file(root, "preparation.json")? else {
        return Ok(initial());
    };
    if m.len() > 8192 {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    let v: Context =
        read_json(&root.join("preparation.json")).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    validate(&v)?;
    Ok(v)
}
fn save(root: &Path, v: &Context) -> Result<()> {
    validate(v)?;
    safe_file(root, "preparation.json")?;
    write_json(root, "preparation.json", v).map_err(|_| fail("ENGINE_PREP_STATE"))
}
pub fn context(root: &Path) -> Result<Context> {
    if !root.exists() {
        return Ok(initial());
    }
    directory(root)?;
    let mut v = read_state(root)?;
    if let Some(m) = safe_file(root, "download.part")? {
        if m.len() > SIZE {
            return Err(fail("ENGINE_PREP_STATE"));
        }
        if v.stage == "downloading" {
            v.saved_bytes = m.len();
        }
    }
    if matches!(v.stage.as_str(), "downloading" | "verifying" | "installing") && lock(root).is_ok()
    {
        v.stage = "interrupted".into();
    }
    Ok(v)
}
pub fn ready() -> Context {
    let mut v = initial();
    v.stage = "ready".into();
    v
}
pub fn awaiting_engine() -> Context {
    let mut v = initial();
    v.stage = "awaiting-engine".into();
    v
}
pub fn running(root: &Path) -> Result<bool> {
    if !root.exists() {
        return Ok(false);
    }
    match lock(root) {
        Ok(_) => Ok(false),
        Err(e) if e.code == "ENGINE_PREP_BUSY" => Ok(true),
        Err(e) => Err(e),
    }
}
pub fn cancel(root: &Path) -> Result<Context> {
    directory(root)?;
    let v = read_state(root)?;
    if v.stage == "installer-ready"
        || matches!(v.stage.as_str(), "downloading" | "verifying" | "installing") && running(root)?
    {
        safe_file(root, "cancel.json")?;
        write_json(root, "cancel.json", &v.job_id).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    }
    context(root)
}
fn cancelled(root: &Path, id: &Option<String>) -> Result<bool> {
    let Some(m) = safe_file(root, "cancel.json")? else {
        return Ok(false);
    };
    if m.len() > 128 {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    let request: Option<String> =
        read_json(&root.join("cancel.json")).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    Ok(request.is_some() && request == *id)
}
fn verify_bytes(root: &Path, name: &str, expected_size: u64, expected_hash: &str) -> Result<()> {
    let m = safe_file(root, name)?.ok_or_else(|| fail("ENGINE_PREP_HASH"))?;
    if m.len() != expected_size {
        return Err(fail("ENGINE_PREP_HASH"));
    }
    let mut f = File::open(root.join(name)).map_err(|_| fail("ENGINE_PREP_HASH"))?;
    let mut hash = Sha256::new();
    let mut n = 0u64;
    let mut b = [0; 65536];
    loop {
        let read = f.read(&mut b).map_err(|_| fail("ENGINE_PREP_HASH"))?;
        if read == 0 {
            break;
        }
        n += read as u64;
        if n > expected_size {
            return Err(fail("ENGINE_PREP_HASH"));
        }
        hash.update(&b[..read]);
    }
    if n != expected_size || format!("{:x}", hash.finalize()) != expected_hash {
        return Err(fail("ENGINE_PREP_HASH"));
    }
    Ok(())
}
fn command_success(program: &str, args: &[&std::ffi::OsStr], seconds: u64) -> Result<()> {
    command_with_code(program, args, seconds, "ENGINE_PREP_SIGNATURE")
}
fn command_with_code(
    program: &str,
    args: &[&std::ffi::OsStr],
    seconds: u64,
    code: &str,
) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| fail(code))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|_| fail(code))? {
            return if status.success() {
                Ok(())
            } else {
                Err(fail(code))
            };
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(fail(code));
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn signature(root: &Path) -> Result<()> {
    let path = root.join(ARTIFACT);
    command_success(
        "/usr/bin/hdiutil",
        &["verify".as_ref(), path.as_os_str()],
        120,
    )?;
    command_success(
        "/usr/sbin/spctl",
        &[
            "--assess".as_ref(),
            "--type".as_ref(),
            "open".as_ref(),
            "--context".as_ref(),
            "context:primary-signature".as_ref(),
            path.as_os_str(),
        ],
        120,
    )
}
struct DownloadChild(Child);
impl Drop for DownloadChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn download(root: &Path, v: &mut Context) -> Result<()> {
    let path = root.join("download.part");
    let bytes = safe_file(root, "download.part")?.map_or(0, |m| m.len());
    if bytes > SIZE {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    if bytes == SIZE {
        return Ok(());
    }
    if fs2::available_space(root).map_err(|_| fail("ENGINE_PREP_STORAGE"))? < SIZE - bytes {
        return Err(fail("ENGINE_PREP_STORAGE"));
    }
    if bytes == 0 && safe_file(root, "download.part")?.is_none() {
        write_private_new(root, "download.part", &[], 1).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    }
    let child = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "30",
            "--max-time",
            "3600",
            "--max-filesize",
            "589939097",
            "--continue-at",
            "-",
            "--output",
        ])
        .arg(&path)
        .arg(URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| fail("ENGINE_PREP_DOWNLOAD"))?;
    let mut child = DownloadChild(child);
    let start = Instant::now();
    loop {
        if cancelled(root, &v.job_id)? {
            return Err(fail("ENGINE_PREP_CANCELLED"));
        }
        v.saved_bytes = safe_file(root, "download.part")?.map_or(0, |m| m.len());
        if v.saved_bytes > SIZE {
            return Err(fail("ENGINE_PREP_HASH"));
        }
        save(root, v)?;
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|_| fail("ENGINE_PREP_DOWNLOAD"))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(fail("ENGINE_PREP_DOWNLOAD"))
            };
        }
        if start.elapsed() > Duration::from_secs(3610) {
            return Err(fail("ENGINE_PREP_DOWNLOAD"));
        }
        thread::sleep(Duration::from_millis(500));
    }
}
fn prepare_inner(root: &Path, v: &mut Context) -> Result<()> {
    let previous_hash_failure = v.error_code.as_deref() == Some("ENGINE_PREP_HASH");
    if safe_file(root, ARTIFACT)?.is_some() {
        v.stage = "verifying".into();
        v.error_code = None;
        save(root, v)?;
        if cancelled(root, &v.job_id)? {
            return Err(fail("ENGINE_PREP_CANCELLED"));
        }
        if verify_bytes(root, ARTIFACT, SIZE, SHA).is_ok() {
            signature(root)?;
            if cancelled(root, &v.job_id)? {
                return Err(fail("ENGINE_PREP_CANCELLED"));
            }
            v.saved_bytes = SIZE;
            v.stage = "installer-ready".into();
            return save(root, v);
        }
        fs::rename(
            root.join(ARTIFACT),
            root.join(format!("rejected-{}.dmg", Uuid::new_v4())),
        )
        .map_err(|_| fail("ENGINE_PREP_STATE"))?;
    }
    if safe_file(root, "download.part")?.is_some() && previous_hash_failure {
        fs::rename(
            root.join("download.part"),
            root.join(format!("rejected-{}.part", Uuid::new_v4())),
        )
        .map_err(|_| fail("ENGINE_PREP_STATE"))?;
    }
    v.stage = "downloading".into();
    v.error_code = None;
    save(root, v)?;
    download(root, v)?;
    v.stage = "verifying".into();
    v.saved_bytes = safe_file(root, "download.part")?.map_or(0, |m| m.len());
    save(root, v)?;
    if cancelled(root, &v.job_id)? {
        return Err(fail("ENGINE_PREP_CANCELLED"));
    }
    verify_bytes(root, "download.part", SIZE, SHA)?;
    fs::rename(root.join("download.part"), root.join(ARTIFACT))
        .map_err(|_| fail("ENGINE_PREP_STATE"))?;
    signature(root)?;
    if cancelled(root, &v.job_id)? {
        return Err(fail("ENGINE_PREP_CANCELLED"));
    }
    v.stage = "installer-ready".into();
    save(root, v)
}
/// A narrowly scoped helper owns this lock until transfer/verification ends, including after GUI exit.
pub fn run(root: &Path) -> Result<()> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err(fail("ENGINE_PREP_UNSUPPORTED"));
    }
    let _lock = lock(root)?;
    let previous = read_state(root)?;
    let mut v = initial();
    v.job_id = Some(Uuid::new_v4().to_string());
    v.error_code = previous.error_code;
    let result = prepare_inner(root, &mut v);
    if let Err(e) = &result {
        v.stage = if e.code == "ENGINE_PREP_CANCELLED" {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        v.error_code = Some(e.code.clone());
        v.saved_bytes =
            safe_file(root, "download.part")?.map_or(v.saved_bytes, |m| m.len().min(SIZE));
        save(root, &v)?;
    }
    result
}
pub fn ensure_automatic_continuation(root: &Path) -> Result<()> {
    let _guard = lock(root)?;
    let mut v = read_state(root)?;
    if cancelled(root, &v.job_id)? {
        v.stage = "cancelled".into();
        v.error_code = Some("ENGINE_PREP_CANCELLED".into());
        save(root, &v)?;
        return Err(fail("ENGINE_PREP_CANCELLED"));
    }
    Ok(())
}
pub fn mark_awaiting_engine(root: &Path) -> Result<Context> {
    let _guard = lock(root)?;
    let mut v = read_state(root)?;
    v.stage = "awaiting-engine".into();
    v.error_code = None;
    save(root, &v)?;
    Ok(v)
}
pub fn installer(root: &Path) -> Result<PathBuf> {
    let _lock = lock(root)?;
    verify_bytes(root, ARTIFACT, SIZE, SHA)?;
    signature(root)?;
    let mut v = read_state(root)?;
    v.stage = "awaiting-installation".into();
    save(root, &v)?;
    Ok(root.join(ARTIFACT))
}
pub fn verify_desktop_app() -> Result<PathBuf> {
    let path = PathBuf::from("/Applications/Docker.app");
    if !path.is_dir() {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    verify_app(&path)?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn publish_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    let to =
        CString::new(destination.as_os_str().as_bytes()).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
        return Err(fail("ENGINE_PREP_OS_PERMISSION"));
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn publish_without_replacing(_source: &Path, _destination: &Path) -> Result<()> {
    Err(fail("ENGINE_PREP_UNSUPPORTED"))
}
fn verify_app(path: &Path) -> Result<()> {
    let info = path.join("Contents/Info.plist");
    let metadata = fs::symlink_metadata(&info).map_err(|_| fail("ENGINE_PREP_SIGNATURE"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(fail("ENGINE_PREP_SIGNATURE"));
    }
    let identifier = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print:CFBundleIdentifier"])
        .arg(&info)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| fail("ENGINE_PREP_SIGNATURE"))?;
    if !identifier.status.success()
        || identifier.stdout.len() > 128
        || identifier.stdout != b"com.docker.docker\n"
    {
        return Err(fail("ENGINE_PREP_SIGNATURE"));
    }

    command_success(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            path.as_os_str(),
        ],
        120,
    )?;
    command_success(
        "/usr/sbin/spctl",
        &[
            "--assess".as_ref(),
            "--type".as_ref(),
            "execute".as_ref(),
            path.as_os_str(),
        ],
        120,
    )
}
fn application_bytes(source: &Path) -> Result<u64> {
    let mut stack = vec![source.to_path_buf()];
    let mut total = 0u64;
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).map_err(|_| fail("ENGINE_PREP_COPY"))? {
            let entry = entry.map_err(|_| fail("ENGINE_PREP_COPY"))?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|_| fail("ENGINE_PREP_COPY"))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or_else(|| fail("ENGINE_PREP_STORAGE"))?;
            }
        }
    }
    Ok(total)
}
/// Same app-copy installation documented by Docker; no privileged flags or existing app replacement.
pub fn install_desktop(root: &Path) -> Result<PathBuf> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err(fail("ENGINE_PREP_UNSUPPORTED"));
    }
    let _guard = lock(root)?;
    let target = PathBuf::from("/Applications/Docker.app");
    if fs::symlink_metadata(&target).is_ok() {
        return Err(fail("ENGINE_PREP_STATE"));
    }
    verify_bytes(root, ARTIFACT, SIZE, SHA)?;
    signature(root)?;
    let mut v = read_state(root)?;
    v.stage = "installing".into();
    v.error_code = None;
    save(root, &v)?;
    let mount = root.join(format!("mount-{}", Uuid::new_v4()));
    fs::create_dir(&mount).map_err(|_| fail("ENGINE_PREP_STATE"))?;
    let artifact = root.join(ARTIFACT);
    let result = (|| {
        if cancelled(root, &v.job_id)? {
            return Err(fail("ENGINE_PREP_CANCELLED"));
        }
        command_success(
            "/usr/bin/hdiutil",
            &[
                "attach".as_ref(),
                "-readonly".as_ref(),
                "-nobrowse".as_ref(),
                "-mountpoint".as_ref(),
                mount.as_os_str(),
                artifact.as_os_str(),
            ],
            120,
        )?;
        let source = mount.join("Docker.app");
        verify_app(&source)?;
        if fs2::available_space("/Applications").map_err(|_| fail("ENGINE_PREP_STORAGE"))?
            < application_bytes(&source)?
        {
            return Err(fail("ENGINE_PREP_STORAGE"));
        }
        // A private temporary destination is atomically published only after a complete signed app copy.
        let stage = PathBuf::from("/Applications")
            .join(format!(".ExhibitOS-Docker-{}.app", Uuid::new_v4()));
        fs::create_dir(&stage).map_err(|_| fail("ENGINE_PREP_OS_PERMISSION"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
                .map_err(|_| fail("ENGINE_PREP_OS_PERMISSION"))?;
        }
        command_with_code(
            "/usr/bin/ditto",
            &[
                "--rsrc".as_ref(),
                "--extattr".as_ref(),
                source.as_os_str(),
                stage.as_os_str(),
            ],
            600,
            "ENGINE_PREP_COPY",
        )?;
        verify_app(&stage)?;
        if cancelled(root, &v.job_id)? {
            return Err(fail("ENGINE_PREP_CANCELLED"));
        }
        publish_without_replacing(&stage, &target)?;
        Ok(target)
    })();
    // Detach only the uniquely created mount; no forced detach or application/data deletion.
    let _ = command_success(
        "/usr/bin/hdiutil",
        &["detach".as_ref(), mount.as_os_str()],
        60,
    );
    match &result {
        Ok(_) => {
            v.stage = "awaiting-engine".into();
        }
        Err(e) => {
            v.stage = if e.code == "ENGINE_PREP_CANCELLED" {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            v.error_code = Some(e.code.clone());
        }
    }
    save(root, &v)?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("engine-prep-synthetic-{}", Uuid::new_v4()))
    }
    #[test]
    fn missing_status_does_not_create_files() {
        let p = root();
        assert_eq!(context(&p).unwrap().stage, "not-started");
        assert!(!p.exists());
    }
    #[test]
    fn helper_lock_preserves_work_across_gui_restart_and_cancel_targets_only_current_job() {
        let p = root();
        let guard = lock(&p).unwrap();
        let mut v = initial();
        v.job_id = Some(Uuid::new_v4().to_string());
        v.stage = "downloading".into();
        save(&p, &v).unwrap();
        write_private_new(&p, "download.part", b"partial", 16).unwrap();
        assert_eq!(context(&p).unwrap().stage, "downloading");
        assert_eq!(context(&p).unwrap().saved_bytes, 7);
        cancel(&p).unwrap();
        assert!(cancelled(&p, &v.job_id).unwrap());
        assert!(!cancelled(&p, &Some(Uuid::new_v4().to_string())).unwrap());
        drop(guard);
        assert_eq!(context(&p).unwrap().stage, "interrupted");
        assert_eq!(fs::read(p.join("download.part")).unwrap(), b"partial");
    }
    #[test]
    fn hash_failure_does_not_execute_or_remove_candidate() {
        let p = root();
        directory(&p).unwrap();
        write_private_new(&p, "download.part", b"synthetic", 16).unwrap();
        assert_eq!(
            verify_bytes(&p, "download.part", 9, &"0".repeat(64))
                .unwrap_err()
                .code,
            "ENGINE_PREP_HASH"
        );
        assert_eq!(fs::read(p.join("download.part")).unwrap(), b"synthetic");
        assert!(verify_bytes(&p, "download.part", 9, &digest(b"synthetic")).is_ok());
    }
    #[test]
    fn stored_state_cannot_change_provider_url_version_or_size() {
        let mut v = initial();
        v.total_bytes += 1;
        assert!(validate(&v).is_err());
        v = initial();
        v.version = "latest".into();
        assert!(validate(&v).is_err());
        v = initial();
        v.saved_bytes = SIZE + 1;
        assert!(validate(&v).is_err());
        let mut value = serde_json::to_value(initial()).unwrap();
        value["url"] = Value::String("https://other.invalid".into());
        assert!(serde_json::from_value::<Context>(value).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_candidate_and_shared_files_are_never_used() {
        use std::os::unix::fs::symlink;
        let p = root();
        directory(&p).unwrap();
        let original = p.join("original");
        fs::write(&original, b"preserve").unwrap();
        symlink(&original, p.join("download.part")).unwrap();
        assert!(safe_file(&p, "download.part").is_err());
        assert_eq!(fs::read(&original).unwrap(), b"preserve");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(&original, p.join("shared.part")).unwrap();
        assert!(safe_file(&p, "shared.part").is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn application_publish_never_replaces_existing_destination() {
        let p = root();
        directory(&p).unwrap();
        let source = p.join("staged.app");
        let target = p.join("existing.app");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("retained"), b"existing").unwrap();
        assert!(publish_without_replacing(&source, &target).is_err());
        assert_eq!(fs::read(target.join("retained")).unwrap(), b"existing");
        assert!(source.exists());
        let fresh = p.join("fresh.app");
        publish_without_replacing(&source, &fresh).unwrap();
        assert!(fresh.is_dir());
    }
    #[test]
    fn cancellation_between_verified_download_and_installation_is_not_lost() {
        let p = root();
        directory(&p).unwrap();
        let mut v = initial();
        v.job_id = Some(Uuid::new_v4().to_string());
        v.stage = "installer-ready".into();
        save(&p, &v).unwrap();
        cancel(&p).unwrap();
        assert_eq!(
            ensure_automatic_continuation(&p).unwrap_err().code,
            "ENGINE_PREP_CANCELLED"
        );
        assert_eq!(context(&p).unwrap().stage, "cancelled");
    }
}
