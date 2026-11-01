// SPDX-License-Identifier: Apache-2.0
//! Inactive OEX direct-child primitive. This is not an operation drain proof:
//! source-materialization ownership, a qualified worker closure, and the
//! platform's packed-output verifier must be supplied before any route uses it.
use super::*;
use std::os::unix::{fs::{MetadataExt, OpenOptionsExt, FileExt}, process::CommandExt};
use std::os::fd::AsRawFd;

const OUTPUT_LIMIT: u64 = 512 * 1024 * 1024;
const REPLY_LIMIT: u64 = 4 * 1024 * 1024;

/// Never abandon our retained direct child on an error. The parent fence must
/// remain held until wait succeeds; this does not prove descendant quiescence.
struct WaitOwned(std::process::Child);
impl Drop for WaitOwned {
    fn drop(&mut self) {
        loop {
            match self.0.wait() {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OexWorkerIdentity {
    pub attempt_id: String,
    pub runtime_generation: String,
    pub reservation_generation: String,
    /// Digest of the complete component receipt, including source/job/user,
    /// chunk identities, stage, reservation and requested output version.
    pub receipt_sha256: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
}
impl OexWorkerIdentity {
    fn valid(&self) -> bool {
        [&self.attempt_id, &self.runtime_generation, &self.reservation_generation]
            .iter().all(|s| Uuid::parse_str(s).is_ok_and(|v| v.to_string() == **s && v.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&v.get_version_num())))
            && hash_valid(&self.receipt_sha256) && hash_valid(&self.archive_sha256)
            && (1..=OUTPUT_LIMIT).contains(&self.archive_bytes)
    }
}

/// Created only after waiting on the Child that this process actually spawned.
/// It is neither serializable nor cloneable and cannot authorize claim release,
/// prove descendants drained, or establish that the owning Session is still live.
#[derive(Debug)]
struct ReapedDirectChild {
    attempt_id: String,
    runtime_generation: String,
    reservation_generation: String,
    receipt_sha256: String,
    child_id: u32,
}
impl ReapedDirectChild {
    fn matches(&self, identity: &OexWorkerIdentity) -> bool {
        self.child_id != 0 && self.attempt_id == identity.attempt_id
            && self.runtime_generation == identity.runtime_generation
            && self.reservation_generation == identity.reservation_generation
            && self.receipt_sha256 == identity.receipt_sha256
    }
}
#[derive(Debug)]
pub(super) struct OwnedWorkerSuccess { ack: Vec<u8>, reaped: ReapedDirectChild }
impl OwnedWorkerSuccess {
    pub(super) fn ack(&self) -> &[u8] { &self.ack }
}

/// The intent is exclusively created and synced before launch. Refusal across
/// restart additionally requires the caller to bind a stable journal namespace;
/// that session authority is not yet supplied by this inactive primitive.
pub(super) fn reserve(directory: &File, identity: &OexWorkerIdentity) -> Result<()> {
    if !identity.valid() { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
    let held = directory.metadata().map_err(|_| err("STATE_UNAVAILABLE"))?;
    if !held.is_dir() || held.uid() != unsafe { libc::geteuid() } || held.mode() & 0o777 != 0o700 { return Err(err("STATE_UNAVAILABLE")); }
    let name = format!("oex-attempt-{}.json", identity.attempt_id);
    let bytes = serde_json::to_vec(identity).map_err(|_| err("NATIVE_OEX_ATTEMPT_INVALID"))?;
    let filename = std::ffi::CString::new(name.as_str()).map_err(|_| err("STATE_UNAVAILABLE"))?;
    let fd = unsafe { libc::openat(directory.as_raw_fd(), filename.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(err("STATE_UNAVAILABLE")); }
    use std::os::fd::FromRawFd;
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
    directory.sync_all().map_err(|_| err("STATE_UNAVAILABLE"))?;
    Ok(())
}

/// Returns only a direct-child result, never authorization to release the
/// reservation or commit. A worker must be a fixed admitted payload entry.
/// `archive` and `packed` must already be pinned and exclusively owned by the
/// caller; caller-side materialization drain is intentionally not inferred.
/// The fixed worker must read the archive using explicit positional reads.
#[allow(dead_code)]
pub(super) fn run_direct(
    payload: &Payload, journal: &File, identity: &OexWorkerIdentity,
    archive: &File, packed: &File, cancelled: &AtomicBool, expected: &super::oex_ack::Expected<'_>, request: &[u8],
    check_owner: &mut dyn FnMut() -> Result<()>,
) -> Result<OwnedWorkerSuccess> {
    check_owner()?;
    if !super::oex_ack::valid_expected(expected) || expected.attempt_id != identity.attempt_id || expected.runtime_generation != identity.runtime_generation
        || expected.reservation_generation != identity.reservation_generation || expected.receipt_sha256 != identity.receipt_sha256
        || request.is_empty() || request.len() > 8192 || digest(request) != identity.receipt_sha256 {
        return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
    }
    let before = packed.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
    if (before.dev(),before.ino(),before.len()) != (expected.output_device,expected.output_inode,0) {
        return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
    }
    let (node, script, root) = payload.oex_packed_worker()?;
    let result = run_fixed_with_limit(node, script, root, journal, identity, archive, packed, cancelled, expected.capacity, request, check_owner)?;
    let output = packed.metadata().map_err(|_| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
    if (output.dev(),output.ino(),output.len()) != (expected.output_device,expected.output_inode,expected.actual_output_bytes) {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    if !result.reaped.matches(identity) { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
    super::oex_ack::check(&result.ack, expected)?;
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    Ok(result)
}

fn run_fixed(
    node: PathBuf, script: PathBuf, root: PathBuf, journal: &File,
    identity: &OexWorkerIdentity, archive: &File, packed: &File, cancelled: &AtomicBool, request: &[u8],
) -> Result<Vec<u8>> {
    run_fixed_with_limit(node, script, root, journal, identity, archive, packed, cancelled, OUTPUT_LIMIT, request, &mut || Ok(())).map(|result| result.ack)
}

fn run_fixed_with_limit(
    node: PathBuf, script: PathBuf, root: PathBuf, journal: &File,
    identity: &OexWorkerIdentity, archive: &File, packed: &File, cancelled: &AtomicBool, file_limit: u64, request: &[u8],
    check_owner: &mut dyn FnMut() -> Result<()>,
) -> Result<OwnedWorkerSuccess> {
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    let src = archive.metadata().map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
    let dst = packed.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
    let access = |file: &File| unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if !identity.valid() || request.is_empty() || request.len() > 8192 || digest(request) != identity.receipt_sha256
        || !src.is_file() || src.len() != identity.archive_bytes || src.nlink() != 1
        || !dst.is_file() || dst.len() != 0 || dst.nlink() != 1
        || src.uid() != unsafe { libc::geteuid() } || dst.uid() != unsafe { libc::geteuid() }
        || src.mode() & 0o077 != 0 || dst.mode() & 0o077 != 0
        || src.dev() == dst.dev() && src.ino() == dst.ino()
        || access(archive) & libc::O_ACCMODE != libc::O_RDONLY
        || access(packed) & libc::O_ACCMODE != libc::O_RDWR || access(packed) & libc::O_APPEND != 0 {
        return Err(err("NATIVE_OEX_INPUT_INVALID"));
    }
    let check = archive.try_clone().map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
    let mut hasher = Sha256::new(); let mut buf = [0u8; 65536];
    let mut offset = 0u64;
    while offset < identity.archive_bytes { check_owner()?; if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
        let amount = ((identity.archive_bytes - offset) as usize).min(buf.len());
        let n = archive.read_at(&mut buf[..amount], offset).map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
        if n == 0 { return Err(err("NATIVE_OEX_INPUT_INVALID")); } hasher.update(&buf[..n]); offset += n as u64;
    }
    if format!("{:x}", hasher.finalize()) != identity.archive_sha256 { return Err(err("NATIVE_OEX_INPUT_INVALID")); }
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    reserve(journal, identity)?;
    let output_owned = packed.try_clone().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
    use std::os::unix::net::UnixStream;
    use std::os::fd::FromRawFd;
    let (mut control_send, control_receive) = UnixStream::pair().map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?;
    // Both original ends must close on exec; only the duplicate read end is
    // selectively made inheritable in the one forked child below.
    for stream in [&control_send, &control_receive] {
        let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
        if flags < 0 || unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(err("NATIVE_PROCESS_START_FAILED"));
        }
    }
    control_send.set_write_timeout(Some(Duration::from_secs(5))).map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?;
    let control_fd = unsafe { libc::fcntl(control_receive.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if control_fd < 0 { return Err(err("NATIVE_PROCESS_START_FAILED")); }
    let control_owned = unsafe { File::from_raw_fd(control_fd) };
    if control_fd > 1023 { return Err(err("NATIVE_PROCESS_START_FAILED")); }
    drop(control_receive);
    let mut command = Command::new(node);
    command.arg("--permission").arg(format!("--allow-fs-read={}", root.display()))
        .arg(script)
        .env_clear().env("PATH", "/usr/bin:/bin").env("LANG", "C").env("EXHIBITOS_NATIVE_OEX_CONTROL_FD", control_fd.to_string())
        .stdin(Stdio::from(check)).stdout(Stdio::from(output_owned)).stderr(Stdio::piped());
    // Bound actual file growth, not just the post-exit stat. This runs only
    // setrlimit in the forked child and never changes the Manager's limit.
    if file_limit == 0 || file_limit > OUTPUT_LIMIT { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
    unsafe { command.pre_exec(move || {
        let limit = libc::rlimit { rlim_cur: file_limit as libc::rlim_t, rlim_max: file_limit as libc::rlim_t };
        if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 { return Err(std::io::Error::last_os_error()); }
        let flags = libc::fcntl(control_fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(control_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    }); }
    let mut child = WaitOwned(command.spawn().map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?);
    let child_id = child.0.id();
    drop(control_owned);
    if control_send.write_all(&(request.len() as u32).to_be_bytes()).and_then(|_| control_send.write_all(request)).and_then(|_| control_send.shutdown(std::net::Shutdown::Write)).is_err() {
        let _ = child.0.kill(); return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    drop(control_send);
    let mut stderr = child.0.stderr.take().ok_or_else(|| err("NATIVE_PROCESS_UNAVAILABLE"))?;
    let flags = unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        let _ = child.0.kill(); return Err(err("NATIVE_PROCESS_UNAVAILABLE"));
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut interrupted = false; let mut reply = Vec::new(); let mut eof = false; let mut observed_exit = false;
    let status = loop {
        if let Err(error) = check_owner() {
            let _ = child.0.kill();
            return Err(error);
        }
        let mut block = [0u8; 8192];
        loop {
            match stderr.read(&mut block) {
                Ok(0) => { eof = true; break; }
                Ok(n) => { reply.extend_from_slice(&block[..n]); if reply.len() as u64 > REPLY_LIMIT { interrupted = true; break; } }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => { let _ = child.0.kill(); return Err(err("NATIVE_PROCESS_UNAVAILABLE")); }
            }
        }
        if let Some(status) = child.0.try_wait().map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))? {
            if eof || observed_exit { break status; }
            observed_exit = true; continue;
        }
        if interrupted || cancelled.load(Ordering::SeqCst) || Instant::now() >= deadline {
            interrupted = true;
            if let Err(e) = child.0.kill() {
                if e.raw_os_error() != Some(libc::ESRCH) { return Err(err("NATIVE_STOP_UNAVAILABLE")); }
            }
            break child.0.wait().map_err(|_| err("NATIVE_PROCESS_UNAVAILABLE"))?;
        }
        thread::sleep(Duration::from_millis(50));
    };
    if interrupted || !eof || !status.success() || status.signal().is_some() || reply.len() as u64 > REPLY_LIMIT {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    check_owner()?;
    let result: Value = serde_json::from_slice(&reply).map_err(|_| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
    if result.get("receiptSha256").and_then(Value::as_str) != Some(&identity.receipt_sha256)
        || result.get("status").and_then(Value::as_str) != Some("complete") {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    let after_src = archive.metadata().map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
    let after_dst = packed.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
    if (src.dev(), src.ino(), src.len(), src.mtime(), src.mtime_nsec(), src.ctime(), src.ctime_nsec()) !=
       (after_src.dev(), after_src.ino(), after_src.len(), after_src.mtime(), after_src.mtime_nsec(), after_src.ctime(), after_src.ctime_nsec())
       || (dst.dev(), dst.ino()) != (after_dst.dev(), after_dst.ino()) || after_dst.len() > file_limit {
        return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
    }
    packed.sync_all().map_err(|_| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    // The private observation records only this Child's successful OS exit.
    // It is not a drain receipt; the durable intent remains for reconciliation.
    Ok(OwnedWorkerSuccess { ack: reply, reaped: ReapedDirectChild {
        attempt_id: identity.attempt_id.clone(), runtime_generation: identity.runtime_generation.clone(),
        reservation_generation: identity.reservation_generation.clone(), receipt_sha256: identity.receipt_sha256.clone(), child_id,
    } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture() -> (PathBuf, OexWorkerIdentity, File, File, PathBuf) {
        let base = std::env::temp_dir().join(format!("native-oex-owned-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap(); fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(base).unwrap();
        write_private_new(&root, "archive", b"abc", 10).unwrap();
        let archive = File::open(root.join("archive")).unwrap();
        let output = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(root.join("packed")).unwrap();
        let identity = OexWorkerIdentity { attempt_id: Uuid::new_v4().to_string(), runtime_generation: Uuid::new_v4().to_string(), reservation_generation: Uuid::new_v4().to_string(), receipt_sha256: digest(b"{\"version\":1}"), archive_sha256: digest(b"abc"), archive_bytes: 3 };
        let node = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NODE").expect("EXHIBITOS_TEST_NODE is required for this focused test"));
        (root, identity, archive, output, node)
    }
    #[test]
    #[ignore = "requires EXHIBITOS_TEST_NODE and a coordinated native test slot"]
    fn actual_fixed_child_writes_fd_and_duplicate_attempt_is_refused() {
        use std::io::{Seek, SeekFrom};
        let (root, identity, archive, packed, node) = fixture();
        let journal = File::open(&root).unwrap();
        let mut offset = archive.try_clone().unwrap(); offset.seek(SeekFrom::Start(2)).unwrap();
        let script = root.join("worker.cjs");
        fs::write(&script, b"const fs=require('node:fs'); const c=fs.readFileSync(Number(process.env.EXHIBITOS_NATIVE_OEX_CONTROL_FD)); if(c.readUInt32BE(0)!==c.length-4 || c.subarray(4).toString()!=='{\"version\":1}')process.exit(4); const b=Buffer.alloc(3); fs.readSync(0,b,0,3,0); fs.writeSync(1,b); fs.writeSync(2,JSON.stringify({receiptSha256:require('node:crypto').createHash('sha256').update(c.subarray(4)).digest('hex'),status:'complete'}));").unwrap();
        assert!(run_fixed(node.clone(), script.clone(), root.clone(), &journal, &identity, &archive, &packed, &AtomicBool::new(false), b"wrong").is_err());
        assert!(!root.join(format!("oex-attempt-{}.json", identity.attempt_id)).exists());
        assert_eq!(packed.metadata().unwrap().len(), 0);
        let reply = run_fixed(node.clone(), script.clone(), root.clone(), &journal, &identity, &archive, &packed, &AtomicBool::new(false), b"{\"version\":1}").unwrap();
        assert!(!reply.is_empty()); assert_eq!(fs::read(root.join("packed")).unwrap(), b"abc");
        assert_eq!(offset.stream_position().unwrap(), 2);
        let second = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(root.join("packed-second")).unwrap();
        let error = run_fixed(node, script, root.clone(), &journal, &identity, &archive, &second, &AtomicBool::new(false), b"{\"version\":1}").unwrap_err();
        assert_eq!(error.code, "STATE_UNAVAILABLE"); assert_eq!(second.metadata().unwrap().len(), 0);
        assert!(root.join(format!("oex-attempt-{}.json", identity.attempt_id)).exists());
        drop(packed); drop(archive); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires EXHIBITOS_TEST_NODE and a coordinated native test slot"]
    fn cancellation_waits_for_owned_child_and_retains_intent() {
        let (root, identity, archive, packed, node) = fixture();
        let journal = File::open(&root).unwrap();
        let script = root.join("worker.cjs"); fs::write(&script, b"require('node:fs').writeSync(1,String(process.pid));setInterval(()=>{},1000);").unwrap();
        let cancel = Arc::new(AtomicBool::new(false)); let trigger = cancel.clone();
        let marker = root.join("packed");
        let signal = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if fs::metadata(&marker).is_ok_and(|m| m.len() > 0) { trigger.store(true, Ordering::SeqCst); return true; }
                thread::sleep(Duration::from_millis(20));
            }
            trigger.store(true, Ordering::SeqCst); false
        });
        let result = run_fixed(node, script, root.clone(), &journal, &identity, &archive, &packed, &cancel, b"{\"version\":1}");
        assert!(signal.join().unwrap()); assert_eq!(result.unwrap_err().code, "NATIVE_OEX_WORKER_UNCERTAIN");
        let pid: i32 = fs::read_to_string(root.join("packed")).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert!(root.join(format!("oex-attempt-{}.json", identity.attempt_id)).exists());
        drop(packed); drop(archive); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires EXHIBITOS_TEST_NODE and a coordinated native test slot"]
    fn lost_owner_stops_and_waits_for_direct_child_and_retains_intent() {
        let (root, identity, archive, packed, node) = fixture(); let journal=File::open(&root).unwrap();
        let script=root.join("worker.cjs");fs::write(&script,b"require('node:fs').writeSync(1,String(process.pid));setInterval(()=>{},1000);").unwrap();
        let mut check_owner=|| {if packed.metadata().unwrap().len()>0 {Err(err("NATIVE_PROCESS_EXITED"))} else {Ok(())}};
        let result=run_fixed_with_limit(node,script,root.clone(),&journal,&identity,&archive,&packed,&AtomicBool::new(false),OUTPUT_LIMIT,b"{\"version\":1}",&mut check_owner);
        assert_eq!(result.unwrap_err().code,"NATIVE_PROCESS_EXITED");
        let pid:i32=fs::read_to_string(root.join("packed")).unwrap().parse().unwrap();
        assert_eq!(unsafe{libc::kill(pid,0)},-1);assert_eq!(std::io::Error::last_os_error().raw_os_error(),Some(libc::ESRCH));
        assert!(root.join(format!("oex-attempt-{}.json",identity.attempt_id)).exists());
        drop(journal);drop(packed);drop(archive);fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires EXHIBITOS_TEST_NODE and a coordinated native test slot"]
    fn owned_child_cannot_grow_packed_file_beyond_limit() {
        let (root, identity, archive, packed, node) = fixture();
        let journal = File::open(&root).unwrap();
        let script = root.join("worker.cjs");
        fs::write(&script, b"const fs=require('node:fs'); fs.writeSync(1,Buffer.alloc(8192));").unwrap();
        let error = run_fixed_with_limit(node, script, root.clone(), &journal, &identity, &archive, &packed, &AtomicBool::new(false), 4096, b"{\"version\":1}", &mut || Ok(())).unwrap_err();
        assert_eq!(error.code, "NATIVE_OEX_WORKER_UNCERTAIN");
        assert!(packed.metadata().unwrap().len() <= 4096);
        assert!(root.join(format!("oex-attempt-{}.json", identity.attempt_id)).exists());
        drop(packed); drop(archive); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires independently admitted isolated payload and synthetic fixture under heavy guard"]
    fn actual_admitted_payload_worker_matches_independent_fixture() {
        let payload_path = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PAYLOAD").expect("isolated payload required"));
        let source_path = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_ARCHIVE").expect("synthetic archive required"));
        let fixture_path = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_FIXTURE").expect("independent fixture receipt required"));
        let fixture_bytes = fs::read(fixture_path).unwrap();
        let fixture: Value = serde_json::from_slice(&fixture_bytes).unwrap();
        let base = std::env::temp_dir().join(format!("native-oex-real-worker-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap(); fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(base).unwrap(); let journal = File::open(&root).unwrap();
        fs::copy(source_path, root.join("archive")).unwrap(); fs::set_permissions(root.join("archive"), fs::Permissions::from_mode(0o600)).unwrap();
        let archive = File::open(root.join("archive")).unwrap();
        let packed = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(root.join("packed")).unwrap();
        let input = archive.metadata().unwrap(); let output = packed.metadata().unwrap();
        let payload = Payload::open(&payload_path).unwrap();
        let id = || Uuid::new_v4().to_string();
        let attempt=id(); let runtime=id(); let reservation=id(); let generation=id();
        let archive_sha=fixture["archiveSha256"].as_str().unwrap().to_string();
        let manifest_sha=fixture["manifestSha256"].as_str().unwrap().to_string();
        let archive_bytes=fixture["archiveBytes"].as_u64().unwrap();
        let total=fixture["totalAssetBytes"].as_u64().unwrap();
        assert!(total > 0);
        let request=serde_json::to_vec(&serde_json::json!({"version":1,"kind":"oex-packed-v1","tenantId":id(),"resourceId":id(),"jobId":id(),"userId":id(),"requestId":id(),"leaseId":id(),"attemptId":attempt,"runtimeGeneration":runtime,"stageId":id(),"reservationId":reservation,"reservationGeneration":generation,"archiveBytes":archive_bytes,"archiveSha256":archive_sha,"manifestSha256":manifest_sha,"capacity":total,"inputDevice":input.dev().to_string(),"inputInode":input.ino().to_string(),"outputDevice":output.dev().to_string(),"outputInode":output.ino().to_string(),"reservationDigest":"a".repeat(64),"sourceDigest":"b".repeat(64),"workerDigest":payload.context().manifest_sha256,"profile":"bounded-file-v1","format":"1.0.0-draft.9"})).unwrap();
        let request_sha=digest(&request);
        let identity=OexWorkerIdentity {attempt_id:attempt.clone(),runtime_generation:runtime.clone(),reservation_generation:generation.clone(),receipt_sha256:request_sha.clone(),archive_sha256:archive_sha,archive_bytes};
        let expected=super::super::oex_ack::Expected {attempt_id:&attempt,runtime_generation:&runtime,reservation_id:&reservation,reservation_generation:&generation,receipt_sha256:&request_sha,manifest_sha256:&manifest_sha,output_device:output.dev(),output_inode:output.ino(),actual_output_bytes:total,capacity:total};
        let result=run_direct(&payload,&journal,&identity,&archive,&packed,&AtomicBool::new(false),&expected,&request,&mut || Ok(())).unwrap();
        assert!(result.reaped.matches(&identity));
        let mut wrong_generation=identity.clone();wrong_generation.reservation_generation=Uuid::new_v4().to_string();
        assert!(!result.reaped.matches(&wrong_generation));
        let ack:Value=serde_json::from_slice(result.ack()).unwrap(); let entries=ack["entries"].as_array().unwrap(); let trusted=fixture["assets"].as_array().unwrap();
        assert_eq!(entries.len(),trusted.len());
        for entry in entries { let path=entry["path"].as_str().unwrap(); let reference=trusted.iter().find(|v|v["path"].as_str()==Some(path)).unwrap();
            let len=entry["bytes"].as_u64().unwrap() as usize; assert_eq!(reference["bytes"].as_u64().unwrap() as usize,len);
            assert_eq!(entry["sha256"].as_str().unwrap(),reference["sha256"].as_str().unwrap());
            let mut data=vec![0;len]; let mut got=0; while got<len {let n=packed.read_at(&mut data[got..],entry["offset"].as_u64().unwrap()+got as u64).unwrap(); assert!(n>0);got+=n;}
            assert_eq!(digest(&data),reference["sha256"].as_str().unwrap());
        }
        println!("payload_manifest_sha256={} archive_sha256={} fixture_sha256={} ack_sha256={} output_bytes={}",payload.context().manifest_sha256,fixture["archiveSha256"].as_str().unwrap(),digest(&fixture_bytes),digest(result.ack()),total);
        assert!(root.join(format!("oex-attempt-{attempt}.json")).exists());
        drop(journal); drop(archive); drop(packed); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn intent_is_durable_one_shot_and_invalid_identity_refuses() {
        let root = std::env::temp_dir().join(format!("native-oex-intent-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let journal = File::open(&root).unwrap();
        let mut id = OexWorkerIdentity { attempt_id: Uuid::new_v4().to_string(), runtime_generation: Uuid::new_v4().to_string(), reservation_generation: Uuid::new_v4().to_string(), receipt_sha256: "a".repeat(64), archive_sha256: "b".repeat(64), archive_bytes: 1 };
        id.archive_bytes = 0;
        assert!(reserve(&journal, &id).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        id.archive_bytes = 1;
        reserve(&journal, &id).unwrap();
        let path = root.join(format!("oex-attempt-{}.json", id.attempt_id));
        let saved = fs::read(&path).unwrap();
        assert!(reserve(&journal, &id).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        let observed: OexWorkerIdentity = serde_json::from_slice(&saved).unwrap();
        assert_eq!(observed.receipt_sha256, id.receipt_sha256);
        fs::remove_file(path).unwrap(); fs::remove_dir(root).unwrap();
    }
}
