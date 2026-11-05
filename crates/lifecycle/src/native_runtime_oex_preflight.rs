// SPDX-License-Identifier: Apache-2.0
//! Correlation of a bounded preflight child's exact bytes. This does not grant
//! provider reservation, input seal, child drain, authorization, or commit.
use super::*;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{fs::MetadataExt, net::UnixStream, process::CommandExt};

const FRAME_LIMIT: usize = 4096;
fn invalid() -> LifecycleError { err("NATIVE_OEX_PREFLIGHT_INVALID") }
fn id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|v| v.to_string() == value && v.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&v.get_version_num()))
}
fn decimal(value: &str, nonzero: bool) -> bool {
    value.parse::<u64>().is_ok_and(|n| n.to_string() == value && (!nonzero || n != 0))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Control {
    pub version: u32, pub kind: String, pub attempt_id: String, pub runtime_generation: String,
    pub stage_id: String, pub reservation_id: String, pub reservation_generation: String,
    pub reservation_digest: String, pub archive_bytes: u64, pub archive_sha256: String,
    pub input_device: String, pub input_inode: String,
}
#[allow(dead_code)]
impl Control {
    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        if self.version != 1 || self.kind != "oex-preflight-v1"
            || [&self.attempt_id, &self.runtime_generation, &self.stage_id, &self.reservation_id, &self.reservation_generation].iter().any(|v| !id(v))
            || !hash_valid(&self.reservation_digest) || !hash_valid(&self.archive_sha256)
            || !(1..=512*1024*1024).contains(&self.archive_bytes)
            || !decimal(&self.input_device, false) || !decimal(&self.input_inode, true) { return Err(invalid()); }
        let bytes = serde_json::to_vec(self).map_err(|_| invalid())?;
        if !(2..=FRAME_LIMIT).contains(&bytes.len()) { return Err(invalid()); }
        Ok(bytes)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack {
    version: u32, kind: String, attempt_id: String, runtime_generation: String, stage_id: String,
    reservation_id: String, reservation_generation: String, control_sha256: String,
    archive_bytes: u64, expanded_bytes: u64, packed_bytes: u64, manifest_sha256: String,
    entry_count: usize, archive_sha256: String, input_device: String, input_inode: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    archive_format:Option<String>,
}
/// Observed child output only; the owner must still revalidate the held file,
/// provider leases and actual child drain before it can be used for admission.
#[allow(dead_code)]
pub(super) struct Inventory { pub expanded_bytes: u64, pub packed_bytes: u64, pub manifest_sha256: String, pub entry_count: usize, pub archive_format:Option<String> }
impl Inventory {
    pub(super) fn check_format(&self, selected:Option<&str>)->Result<()> {
        if self.archive_format.as_deref()!=selected
            || selected.is_some_and(|f|!matches!(f,"1.0.0-draft.9"|"1.0.0-draft.10")){return Err(invalid());} Ok(())
    }
}
#[allow(dead_code)]
pub(super) fn check(control: &Control, reply: &[u8]) -> Result<Inventory> {
    let encoded = control.encode()?;
    if reply.is_empty() || reply.len() > FRAME_LIMIT { return Err(invalid()); }
    let value: Ack = serde_json::from_slice(reply).map_err(|_| invalid())?;
    if serde_json::to_vec(&value).map_err(|_| invalid())?.as_slice() != reply
        || value.version != 1 || value.kind != "preflight-complete"
        || value.attempt_id != control.attempt_id || value.runtime_generation != control.runtime_generation
        || value.stage_id != control.stage_id || value.reservation_id != control.reservation_id
        || value.reservation_generation != control.reservation_generation || value.control_sha256 != digest(&encoded)
        || value.archive_bytes != control.archive_bytes || value.archive_sha256 != control.archive_sha256
        || value.input_device != control.input_device || value.input_inode != control.input_inode
        || value.expanded_bytes > 512*1024*1024
        || value.packed_bytes > value.expanded_bytes || value.packed_bytes > 512*1024*1024
        || value.entry_count < 2 || value.entry_count > 256
        || value.archive_format.as_deref().is_some_and(|f| !matches!(f,"1.0.0-draft.9"|"1.0.0-draft.10"))
        || !hash_valid(&value.manifest_sha256) { return Err(invalid()); }
    Ok(Inventory { expanded_bytes: value.expanded_bytes, packed_bytes: value.packed_bytes,
        manifest_sha256: value.manifest_sha256, entry_count: value.entry_count, archive_format:value.archive_format })
}

/// Correlated bytes and actual direct-child reap, still not provider authority
/// or a proof of descendant/decoder drain. This candidate is not wired into an
/// import until the provider and resource-limit contracts are implemented.
#[allow(dead_code)]
pub(super) struct Prepared { inventory: Inventory, control: Vec<u8>, ack: Vec<u8>, _child: super::oex_materializer::ReapedChild }
#[allow(dead_code)]
impl Prepared {
    pub(super) fn inventory(&self)->&Inventory { &self.inventory }
    #[cfg(test)]
    pub(super) fn test_only_raw(request:&Control,ack:Vec<u8>)->Self {
        let control=request.encode().unwrap();let inventory=check(request,&ack).unwrap();
        Self{inventory,control,ack,_child:super::oex_materializer::ReapedChild::test_only()}
    }
    #[cfg(test)]
    pub(super) fn test_only(request:&Control,expanded_bytes:u64,packed_bytes:u64)->Self {
        let control=request.encode().unwrap();
        let ack=Ack{version:1,kind:"preflight-complete".into(),attempt_id:request.attempt_id.clone(),runtime_generation:request.runtime_generation.clone(),
            stage_id:request.stage_id.clone(),reservation_id:request.reservation_id.clone(),reservation_generation:request.reservation_generation.clone(),
            control_sha256:digest(&control),archive_bytes:request.archive_bytes,expanded_bytes,packed_bytes,manifest_sha256:"c".repeat(64),
            entry_count:2,archive_sha256:request.archive_sha256.clone(),input_device:request.input_device.clone(),input_inode:request.input_inode.clone(),archive_format:None};
        let ack=serde_json::to_vec(&ack).unwrap();let inventory=check(request,&ack).unwrap();
        Self{inventory,control,ack,_child:super::oex_materializer::ReapedChild::test_only()}
    }
    /// Correlation only; callers must independently recheck the live FD and
    /// both provider leases at the admission and settlement boundaries.
    pub(super) fn matches(&self, request: &Control) -> bool {
        request.encode().is_ok_and(|bytes| bytes == self.control)
    }
    pub(super) fn report(&self, request:&Control, challenge:&super::owner_claim_frames::ClaimChallenge) -> Result<Vec<u8>> {
        use base64::{engine::general_purpose::STANDARD,Engine as _};
        #[derive(Serialize)] #[serde(rename_all="camelCase")]
        struct Report<'a> {version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,runtime_generation:&'a str,
            stage_id:&'a str,control_base64:String,control_sha256:String,ack_base64:String,ack_sha256:String}
        if challenge.version!=1 || challenge.kind!="claim-challenge-v3" || !id(&challenge.correlation_id) || !hash_valid(&challenge.claim_digest) || !self.matches(request)
            || challenge.attempt_id!=request.attempt_id || challenge.runtime_generation!=request.runtime_generation
            || challenge.stage_id!=request.stage_id || challenge.reservation_id!=request.reservation_id
            || challenge.reservation_generation!=request.reservation_generation || challenge.receipt_digest!=request.reservation_digest
            || self.control.len()>2048 || self.ack.len()>2048 {return Err(invalid());}
        let value=Report{version:1,kind:"preflight-report-v3",correlation_id:&challenge.correlation_id,attempt_id:&request.attempt_id,
            runtime_generation:&request.runtime_generation,stage_id:&request.stage_id,control_base64:STANDARD.encode(&self.control),
            control_sha256:digest(&self.control),ack_base64:STANDARD.encode(&self.ack),ack_sha256:digest(&self.ack)};
        let bytes=serde_json::to_vec(&value).map_err(|_|invalid())?;
        if bytes.is_empty() || bytes.len()>8192{return Err(invalid());}Ok(bytes)
    }
}

#[allow(dead_code)]
pub(super) fn run_direct(payload: &Payload, input: &super::oex_read_fd::ObservedStage,
    request: &Control, expected_format:Option<&str>, cancelled: &AtomicBool, check_owner: &mut dyn FnMut() -> Result<()>) -> Result<Prepared> {
    if expected_format.is_some()!=payload.normal_oex_028_selected_input()?{return Err(invalid());}
    let (node, script, root) = payload.oex_preflight()?;
    let prepared=run_owned(&node, &script, &root, input, request, expected_format, cancelled, check_owner)?;
    prepared.inventory().check_format(expected_format)?;
    Ok(prepared)
}

/// Executable paths come only from the already-verified payload in production.
fn run_owned(node: &Path, script: &Path, root: &Path, input: &super::oex_read_fd::ObservedStage,
    request: &Control, expected_format:Option<&str>, cancelled: &AtomicBool, check_owner: &mut dyn FnMut() -> Result<()>) -> Result<Prepared> {
    if expected_format.is_some_and(|f|!matches!(f,"1.0.0-draft.9"|"1.0.0-draft.10")){return Err(invalid());}
    let uncertain = || err("NATIVE_OEX_WORKER_UNCERTAIN");
    let encoded = request.encode()?;
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    let source = input.file();
    let before = source.metadata().map_err(|_| uncertain())?;
    let flags = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_GETFL) };
    if !before.is_file() || before.uid() != unsafe { libc::geteuid() } || before.mode() & 0o777 != 0o600
        || before.nlink() != 1 || (before.dev(), before.ino(), before.len()) != (input.device, input.inode, input.bytes)
        || request.input_device != before.dev().to_string() || request.input_inode != before.ino().to_string()
        || request.archive_bytes != input.bytes || request.archive_sha256 != input.sha256
        || flags < 0 || flags & libc::O_ACCMODE != libc::O_RDONLY { return Err(uncertain()); }
    let (mut control, control_child) = UnixStream::pair().map_err(|_| uncertain())?;
    let (mut ack, ack_child) = UnixStream::pair().map_err(|_| uncertain())?;
    for socket in [&control, &control_child, &ack, &ack_child] { socket.set_nonblocking(true).map_err(|_| uncertain())?; }
    let copies = vec![super::oex_materializer_fds::duplicate(control_child.as_raw_fd())?,
        super::oex_materializer_fds::duplicate(ack_child.as_raw_fd())?,
        super::oex_materializer_fds::duplicate(source.as_raw_fd())?];
    drop(control_child); drop(ack_child);
    let fds: Vec<_> = copies.iter().map(AsRawFd::as_raw_fd).collect();
    let list = fds.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
    let mut command = Command::new(node);
    command.arg("--permission").arg(format!("--allow-fs-read={}", root.display())).arg("--no-addons").arg(script)
        .env_clear().env("PATH", "/usr/bin:/bin").env("LANG", "C").env("EXHIBITOS_OEX_PREFLIGHT_FDS", list)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(format)=expected_format {command.env("EXHIBITOS_OEX_PREFLIGHT_FORMAT",format);}
    unsafe { command.pre_exec(move || {
        // No archive/output file is written by preflight.
        let limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 { return Err(std::io::Error::last_os_error()); }
        for fd in &fds {
            let flags = libc::fcntl(*fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 { return Err(std::io::Error::last_os_error()); }
        }
        Ok(())
    }); }
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    let child = command.spawn().map_err(|_| err("NATIVE_PROCESS_START_FAILED"))?;
    drop(copies);
    let mut child = super::oex_materializer::OwnedChild::new(child);
    let mut frame = Vec::with_capacity(encoded.len() + 4);
    frame.extend_from_slice(&(encoded.len() as u32).to_be_bytes()); frame.extend_from_slice(&encoded);
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut sent = 0usize; let mut closed = false; let mut reply = Vec::new(); let mut eof = false; let mut status = None;
    loop {
        check_owner()?;
        if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
        if Instant::now() >= deadline { return Err(uncertain()); }
        let mut progressed = false;
        if !closed { match control.write(&frame[sent..]) {
            Ok(0) => return Err(uncertain()),
            Ok(n) => { progressed = true; sent += n; if sent == frame.len() { control.shutdown(std::net::Shutdown::Write).map_err(|_| uncertain())?; closed = true; } },
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted || e.kind() == std::io::ErrorKind::WouldBlock => {},
            Err(_) => return Err(uncertain()),
        }}
        if !eof { let mut block = [0u8; 4096]; match ack.read(&mut block) {
            Ok(0) => eof = true,
            Ok(n) => { progressed = true; if reply.len() + n > FRAME_LIMIT { return Err(uncertain()); } reply.extend_from_slice(&block[..n]); },
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted || e.kind() == std::io::ErrorKind::WouldBlock => {},
            Err(_) => return Err(uncertain()),
        }}
        if status.is_none() { status = child.poll().map_err(|_| uncertain())?; }
        if let Some(exit) = status {
            if !exit.success() { return Err(uncertain()); }
            if eof && closed { break; }
        }
        if !progressed { thread::sleep(Duration::from_millis(5)); }
    }
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    let after = source.metadata().map_err(|_| uncertain())?;
    if !after.is_file() || after.uid() != before.uid() || after.mode() & 0o777 != 0o600 || after.nlink() != 1
        || (after.dev(), after.ino(), after.len(), after.mtime(), after.mtime_nsec(), after.ctime(), after.ctime_nsec())
            != (before.dev(), before.ino(), before.len(), before.mtime(), before.mtime_nsec(), before.ctime(), before.ctime_nsec()) {
        return Err(uncertain());
    }
    let inventory = check(request, &reply)?;
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    Ok(Prepared { inventory, control: encoded, ack: reply, _child: child.reaped_token()? })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_preflight_reply_is_correlated_and_bounded() {
        let next = || Uuid::new_v4().to_string();
        let control = Control { version: 1, kind: "oex-preflight-v1".into(), attempt_id: next(), runtime_generation: next(),
            stage_id: next(), reservation_id: next(), reservation_generation: next(), reservation_digest: "a".repeat(64),
            archive_bytes: 128, archive_sha256: "b".repeat(64), input_device: "1".into(), input_inode: "2".into() };
        let mut ack = Ack { version: 1, kind: "preflight-complete".into(), attempt_id: control.attempt_id.clone(),
            runtime_generation: control.runtime_generation.clone(), stage_id: control.stage_id.clone(), reservation_id: control.reservation_id.clone(),
            reservation_generation: control.reservation_generation.clone(), control_sha256: digest(&control.encode().unwrap()), archive_bytes: 128,
            expanded_bytes: 100, packed_bytes: 80, manifest_sha256: "c".repeat(64), entry_count: 3,
            archive_sha256: control.archive_sha256.clone(), input_device: "1".into(), input_inode: "2".into(),archive_format:None };
        let legacy=check(&control,&serde_json::to_vec(&ack).unwrap()).unwrap();
        assert!(legacy.check_format(None).is_ok());assert!(legacy.check_format(Some("1.0.0-draft.10")).is_err());
        for format in ["1.0.0-draft.9","1.0.0-draft.10"] {
            ack.archive_format=Some(format.into());
            let actual=check(&control,&serde_json::to_vec(&ack).unwrap()).unwrap();
            assert!(actual.check_format(Some(format)).is_ok());assert!(actual.check_format(None).is_err());
            assert!(actual.check_format(Some(if format.ends_with("10"){"1.0.0-draft.9"}else{"1.0.0-draft.10"})).is_err());
        }
        ack.archive_format=Some("1.0.0-draft.11".into());assert!(check(&control,&serde_json::to_vec(&ack).unwrap()).is_err());
        ack.archive_format=None;
        let accepted = check(&control, &serde_json::to_vec(&ack).unwrap()).unwrap();
        assert_eq!((accepted.expanded_bytes, accepted.packed_bytes, accepted.entry_count), (100, 80, 3));
        let challenge=super::super::owner_claim_frames::ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:next(),workspace_id:next(),tenant_id:next(),principal_id:next(),runtime_generation:control.runtime_generation.clone(),job_id:next(),attempt_id:control.attempt_id.clone(),generation:next(),stage_id:control.stage_id.clone(),reservation_id:control.reservation_id.clone(),reservation_generation:control.reservation_generation.clone(),claim_digest:"d".repeat(64),receipt_digest:control.reservation_digest.clone()};
        let prepared=Prepared{inventory:accepted,control:control.encode().unwrap(),ack:serde_json::to_vec(&ack).unwrap(),_child:super::super::oex_materializer::ReapedChild::test_only()};
        let report=prepared.report(&control,&challenge).unwrap();let value:Value=serde_json::from_slice(&report).unwrap();
        assert_eq!(value["kind"],"preflight-report-v3");assert_eq!(value["controlSha256"],digest(&prepared.control));
        let mut changed=challenge.clone();changed.stage_id=next();assert!(prepared.report(&control,&changed).is_err());
        changed=challenge.clone();changed.kind="claim-challenge".into();assert!(prepared.report(&control,&changed).is_err());
        ack.input_inode = "3".into(); assert!(check(&control, &serde_json::to_vec(&ack).unwrap()).is_err());
        ack.input_inode = "2".into(); ack.packed_bytes = 101; assert!(check(&control, &serde_json::to_vec(&ack).unwrap()).is_err());
        ack.packed_bytes = 80; ack.entry_count = 1; assert!(check(&control, &serde_json::to_vec(&ack).unwrap()).is_err());
        ack.entry_count = 3; ack.control_sha256 = "d".repeat(64); assert!(check(&control, &serde_json::to_vec(&ack).unwrap()).is_err());
        ack.control_sha256 = digest(&control.encode().unwrap()); let mut bytes = serde_json::to_vec(&ack).unwrap(); bytes.push(b' ');
        assert!(check(&control, &bytes).is_err());
    }

    #[test]
    #[ignore = "requires explicitly owned Node child and synthetic fixture"]
    fn owned_node_child_uses_same_file_and_reaps() {
        use std::os::unix::fs::{FileExt, PermissionsExt};
        let node = std::env::var("EXHIBITOS_PREFLIGHT_TEST_NODE").expect("owned Node path required");
        let script = std::env::var("EXHIBITOS_PREFLIGHT_TEST_SCRIPT").expect("owned preflight script required");
        let root = std::env::var("EXHIBITOS_PREFLIGHT_TEST_ROOT").expect("owned preflight root required");
        let fixture = std::env::var("EXHIBITOS_PREFLIGHT_TEST_FIXTURE").expect("owned synthetic fixture required");
        let bytes = fs::read(fixture).expect("owned synthetic fixture");
        assert!(!bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024);
        let path = std::env::temp_dir().join(format!("native-oex-preflight-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap(); fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = File::open(&path).unwrap(); let stage_id = Uuid::new_v4().to_string();
        let stage = super::super::oex_read_fd::Stage::create(&directory, &stage_id).unwrap();
        let mut written = 0usize;
        while written < bytes.len() {
            let n = stage.materializer_write().write_at(&bytes[written..], written as u64).unwrap();
            assert!(n > 0); written += n;
        }
        let cancelled = AtomicBool::new(false);
        let observed = super::super::oex_read_fd::observe_stage(stage, bytes.len() as u64, &digest(&bytes), &cancelled, &mut || Ok(())).unwrap();
        let next = || Uuid::new_v4().to_string();
        let mut control = Control { version: 1, kind: "oex-preflight-v1".into(), attempt_id: next(), runtime_generation: next(),
            stage_id, reservation_id: next(), reservation_generation: next(), reservation_digest: "a".repeat(64),
            archive_bytes: observed.bytes, archive_sha256: observed.sha256.clone(), input_device: observed.device.to_string(), input_inode: observed.inode.to_string() };
        let prepared = run_owned(Path::new(&node), Path::new(&script), Path::new(&root), &observed, &control, None, &cancelled, &mut || Ok(())).unwrap();
        assert!(prepared.matches(&control));
        assert!(prepared.inventory.expanded_bytes >= prepared.inventory.packed_bytes);
        control.input_inode = observed.inode.checked_add(1).unwrap_or(observed.inode - 1).to_string();
        assert!(!prepared.matches(&control));
        let error = match run_owned(Path::new(&node), Path::new(&script), Path::new(&root), &observed, &control, None, &cancelled, &mut || Ok(())) {
            Ok(_) => panic!("mismatched input inode must fail"), Err(error) => error,
        };
        assert_eq!(error.code, "NATIVE_OEX_WORKER_UNCERTAIN");
        drop(prepared); drop(observed); drop(directory); fs::remove_dir_all(path).unwrap();
    }
}
