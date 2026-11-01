// SPDX-License-Identifier: Apache-2.0
//! Inactive OEX attempt binding to an existing native Session. This provides
//! neither materializer retirement nor worker/descendant drain authority.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
    version: u32,
    kind: String,
    tenant_id: String,
    resource_id: String,
    job_id: String,
    user_id: String,
    request_id: String,
    lease_id: String,
    attempt_id: String,
    runtime_generation: String,
    stage_id: String,
    reservation_id: String,
    reservation_generation: String,
    profile: String,
    format: String,
    archive_bytes: u64,
    archive_sha256: String,
    manifest_sha256: String,
    source_digest: String,
    worker_digest: String,
    reservation_digest: String,
    capacity: u64,
    input_device: String,
    input_inode: String,
    output_device: String,
    output_inode: String,
}
fn decimal(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}
fn parse_receipt(bytes: &[u8], expected_sha256: &str) -> Result<Receipt> {
    if bytes.is_empty() || bytes.len() > 8192 || !hash_valid(expected_sha256) || digest(bytes) != expected_sha256 {
        return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
    }
    let receipt: Receipt = serde_json::from_slice(bytes).map_err(|_| err("NATIVE_OEX_ATTEMPT_INVALID"))?;
    if !receipt.valid() { return Err(err("NATIVE_OEX_ATTEMPT_INVALID")); }
    Ok(receipt)
}
impl Receipt {
    fn valid(&self) -> bool {
        self.version == 1 && self.kind == "oex-packed-v1"
            && [&self.tenant_id, &self.resource_id, &self.job_id, &self.user_id, &self.request_id,
                &self.lease_id, &self.attempt_id, &self.runtime_generation,
                &self.stage_id, &self.reservation_id, &self.reservation_generation]
                .iter().all(|v| Uuid::parse_str(v).is_ok_and(|id| id.to_string() == **v && id.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num())))
            && self.profile == "bounded-file-v1" && self.format == "1.0.0-draft.9"
            && self.source_digest == self.archive_sha256
            && (1..=536_870_912).contains(&self.archive_bytes)
            && (1..=536_870_912).contains(&self.capacity)
            && [&self.archive_sha256, &self.manifest_sha256, &self.source_digest, &self.worker_digest, &self.reservation_digest].iter().all(|v| hash_valid(v))
            && decimal(&self.input_device).is_some() && decimal(&self.output_device).is_some()
            && decimal(&self.input_inode).is_some_and(|v| v != 0) && decimal(&self.output_inode).is_some_and(|v| v != 0)
            && (&self.input_device, &self.input_inode) != (&self.output_device, &self.output_inode)
    }
}

/// The borrow keeps the Session and its operation guard alive. The open
/// directory is held through reservation and is checked against the named root.
/// This is intentionally not a grant to start a worker or release storage.
#[allow(dead_code)]
pub(super) struct Bound<'a> {
    session: &'a mut Session,
    root: File,
    source: File,
    output: File,
    receipt: Receipt,
    receipt_bytes: Vec<u8>,
    disk_receipt: Vec<u8>,
}
fn check_handles(receipt: &Receipt, source: &File, output: &File) -> Result<()> {
    let a = source.metadata().map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
    let b = output.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
    let flags = |f: &File| unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
    let af = flags(source); let bf = flags(output);
    if !receipt.valid() || !a.is_file() || !b.is_file() || a.nlink() != 1 || b.nlink() != 1
        || a.uid() != unsafe { libc::geteuid() } || b.uid() != unsafe { libc::geteuid() }
        || a.mode() & 0o077 != 0 || b.mode() & 0o077 != 0
        || a.len() != receipt.archive_bytes || b.len() != 0
        || (Some(a.dev()), Some(a.ino())) != (decimal(&receipt.input_device), decimal(&receipt.input_inode))
        || (Some(b.dev()), Some(b.ino())) != (decimal(&receipt.output_device), decimal(&receipt.output_inode))
        || af < 0 || bf < 0 || af & libc::O_ACCMODE != libc::O_RDONLY || bf & libc::O_ACCMODE != libc::O_RDWR
        || bf & libc::O_APPEND != 0 {
        return Err(err("NATIVE_OEX_INPUT_INVALID"));
    }
    Ok(())
}
/// Constrain this inactive path to the selected personal workspace and the
/// live Session's local authority. This does not authenticate an API job,
/// prove a provider reservation, or acquire the common operation fence.
fn check_personal(session: &Session, receipt: &Receipt) -> Result<()> {
    let (workspace, tenant, principal) = session.service.personal_identity()?;
    let authority = session.local_authority.as_ref().ok_or_else(|| err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
    if authority.owner_uid != unsafe { libc::geteuid() }
        || (authority.workspace_id.as_str(), authority.tenant_id.as_str(), authority.principal_id.as_str())
            != (workspace.as_str(), tenant.as_str(), principal.as_str())
        || (receipt.resource_id.as_str(), receipt.tenant_id.as_str(), receipt.user_id.as_str())
            != (workspace.as_str(), tenant.as_str(), principal.as_str()) {
        return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
    }
    Ok(())
}
#[allow(dead_code)]
impl<'a> Bound<'a> {
    /// Call only after the future materializer owner has separately sealed its
    /// writes. The checks below do not establish that prerequisite.
    pub(super) fn new(session: &'a mut Session, bytes: &[u8], expected_sha256: &str, trusted_manifest_sha256: &str, disk_bytes: &[u8], source: &File, output: &File) -> Result<Self> {
        let receipt = parse_receipt(bytes, expected_sha256)?;
        check_disk_byte_identity(disk_bytes, &receipt.reservation_digest)?;
        session.check_native_observation_fence()?;
        check_personal(session, &receipt)?;
        if session.job_id.as_deref() != Some(receipt.runtime_generation.as_str())
            || session.payload.context().manifest_sha256 != receipt.worker_digest
            || !hash_valid(trusted_manifest_sha256) || receipt.manifest_sha256 != trusted_manifest_sha256 {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        check_handles(&receipt, source, output)?;
        let root = session.observation_root.try_clone_file()?;
        let bound = Self { session, root, source: source.try_clone().map_err(|_| err("STATE_UNAVAILABLE"))?,
            output: output.try_clone().map_err(|_| err("STATE_UNAVAILABLE"))?, receipt, receipt_bytes: bytes.to_vec(), disk_receipt: disk_bytes.to_vec() };
        bound.check()?; Ok(bound)
    }
    fn check(&self) -> Result<()> {
        self.check_root()?;
        check_disk_byte_identity(&self.disk_receipt, &self.receipt.reservation_digest)?;
        check_handles(&self.receipt, &self.source, &self.output)
    }
    fn check_root(&self) -> Result<()> {
        self.session.check_native_observation_fence()?;
        check_personal(&*self.session, &self.receipt)?;
        let held = self.root.metadata().map_err(|_| err("STATE_UNAVAILABLE"))?;
        let named = fs::symlink_metadata(&self.session.service.root).map_err(|_| err("STATE_UNAVAILABLE"))?;
        if named.is_symlink() || !held.is_dir() || held.dev() != named.dev() || held.ino() != named.ino() {
            return Err(err("STATE_UNAVAILABLE"));
        }
        Ok(())
    }
    /// Exclusive path-free intent in the fixed fenced native root. Existing or
    /// partial intent is retained and rejected, never overwritten or adopted.
    fn reserve(&self) -> Result<String> {
        self.check()?;
        write_intent(&self.root, &self.receipt.attempt_id, &self.receipt_bytes)?;
        self.check()?;
        Ok(digest(&self.receipt_bytes))
    }
    /// Inactive direct-child path. The caller must independently hold the
    /// materializer, live reservation and trusted Spec manifest through this
    /// call. No result here grants publication, cleanup or drain authority.
    pub(super) fn run_direct(&mut self, trusted_output_bytes: u64, cancelled: &AtomicBool) -> Result<super::oex_worker::OwnedWorkerSuccess> {
        self.check()?;
        if self.session.state != "ready" || trusted_output_bytes > self.receipt.capacity {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let receipt_sha256 = self.reserve()?;
        let identity = super::oex_worker::OexWorkerIdentity {
            attempt_id: self.receipt.attempt_id.clone(), runtime_generation: self.receipt.runtime_generation.clone(),
            reservation_generation: self.receipt.reservation_generation.clone(), receipt_sha256: receipt_sha256.clone(),
            archive_sha256: self.receipt.archive_sha256.clone(), archive_bytes: self.receipt.archive_bytes,
        };
        let expected = super::oex_ack::Expected {
            attempt_id: &self.receipt.attempt_id, runtime_generation: &self.receipt.runtime_generation,
            reservation_id: &self.receipt.reservation_id, reservation_generation: &self.receipt.reservation_generation,
            receipt_sha256: &receipt_sha256, manifest_sha256: &self.receipt.manifest_sha256,
            output_device: decimal(&self.receipt.output_device).ok_or_else(|| err("NATIVE_OEX_ATTEMPT_INVALID"))?,
            output_inode: decimal(&self.receipt.output_inode).ok_or_else(|| err("NATIVE_OEX_ATTEMPT_INVALID"))?,
            actual_output_bytes: trusted_output_bytes, capacity: self.receipt.capacity,
        };
        let payload = self.session.payload.clone();
        let session = &mut *self.session;
        let mut check_owned_services = || {
            session.poll()?;
            if session.state != "ready" || session.node.is_none() || session.postgres.is_none() {
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            Ok(())
        };
        let result = super::oex_worker::run_direct(&payload, &self.root, &identity,
            &self.source, &self.output, cancelled, &expected, &self.receipt_bytes, &mut check_owned_services);
        self.check_root()?;
        result
    }
}
/// Byte identity only. The provider must verify its locked live reservation;
/// this does not parse or authorize the encoded reservation contents.
fn check_disk_byte_identity(bytes: &[u8], expected: &str) -> Result<()> {
    const MAGIC: &[u8] = b"EXHIBITOS-DISK-RESERVATION\0v1\0";
    if bytes.len() < MAGIC.len() || bytes.len() > 65_536 || !bytes.starts_with(MAGIC)
        || !hash_valid(expected) || digest(bytes) != expected {
        return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
    }
    Ok(())
}
fn write_intent(root: &File, attempt: &str, bytes: &[u8]) -> Result<()> {
    if Uuid::parse_str(attempt).ok().is_none_or(|id| id.to_string() != attempt) || bytes.is_empty() || bytes.len() > 8192 {
        return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
    }
    let name = std::ffi::CString::new(format!("native-oex-intent-{attempt}.json")).map_err(|_| err("STATE_UNAVAILABLE"))?;
    let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(err("STATE_UNAVAILABLE")); }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
    root.sync_all().map_err(|_| err("STATE_UNAVAILABLE"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    #[test]
    fn disk_receipt_matches_provider_synthetic_bytes() {
        // Public provider vector: d62be137, version 1. This checks exact bytes,
        // not live reservation validity or semantic decoding.
        let hex = "455848494249544f532d4449534b2d5245534552564154494f4e007631000000002431303030303030302d303030302d343030302d383030302d3030303030303030303030310000002431303030303030302d303030302d343030302d383030302d3030303030303030303030320000000b7075626c69636174696f6e0000001331323334353637383930313233343536373839000000053136333834000000092f73746f72652fc3a90000000131000000013200000002000000062f7374616765000000013300000009353336383730393132000000092f73746f72652fc3a900000001310000000a31303733373431383234";
        let mut bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect::<Vec<_>>();
        let expected = "944d92bf65fb0656fc049543fe8fdc48005f982490f99e4f45374fb2f247907d";
        assert_eq!(bytes.len(), 244); assert!(check_disk_byte_identity(&bytes, expected).is_ok());
        assert!(check_disk_byte_identity(b"", expected).is_err());
        let mut bad_magic = bytes.clone(); bad_magic[0] ^= 1; assert!(check_disk_byte_identity(&bad_magic, &digest(&bad_magic)).is_err());
        let last = bytes.len()-1; bytes[last] ^= 1; assert!(check_disk_byte_identity(&bytes, expected).is_err());
    }
    #[test]
    fn provider_leading_zero_bytes_are_preserved() {
        // Provider's synthetic publication vector 8ad029b; this checks only
        // opaque-byte identity and cannot authorize or decode a reservation.
        let hex = "455848494249544f532d4449534b2d5245534552564154494f4e007631000000002431303030303030302d303030302d343030302d383030302d3030303030303030303030310000002431303030303030302d303030302d343030302d383030302d3030303030303030303030320000000b7075626c69636174696f6e000000053030313233000000053030343536000000092f73746f72652fc3a90000000131000000013200000002000000062f7374616765000000013300000009353336383730393132000000092f73746f72652fc3a900000001310000000a31303733373431383234";
        let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect::<Vec<_>>();
        assert_eq!(bytes.len(),230);
        assert!(bytes.windows(5).any(|w| w == b"00123")); assert!(bytes.windows(5).any(|w| w == b"00456"));
        assert!(check_disk_byte_identity(&bytes, "9d8e0fe01cd9597c260c476e8be942b06e0b3d9dcb74b4470d7f1443aec358cc").is_ok());
    }
    #[test]
    #[ignore = "requires fresh isolated actual native profile and explicit heavy/PG ownership"]
    fn fresh_real_session_bound_to_actual_worker_with_synthetic_authority() {
        use std::os::unix::fs::FileExt;
        let root=PathBuf::from(std::env::var_os("EXHIBITOS_TEST_SESSION_ROOT").expect("fresh isolated root required"));
        assert!(root.is_absolute() && !root.exists());
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
        let root=fs::canonicalize(root).unwrap();
        let payload=Arc::new(Payload::open(Path::new(&std::env::var_os("EXHIBITOS_TEST_PAYLOAD").expect("payload required"))).unwrap());
        let fixture_bytes=fs::read(std::env::var_os("EXHIBITOS_TEST_FIXTURE").expect("fixture receipt required")).unwrap();
        let fixture:Value=serde_json::from_slice(&fixture_bytes).unwrap();
        let free=||std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a=free();let b=free();let c=free();
        let ports=Ports{web:a.local_addr().unwrap().port(),api:b.local_addr().unwrap().port(),database:c.local_addr().unwrap().port()};drop((a,b,c));
        let service=LifecycleService::new(root.clone()).unwrap();
        service.prepare_native_personal(&payload,ports).unwrap();
        let (workspace,tenant,principal)=service.personal_identity().unwrap();
        let mut session=Session::new(&service,payload.clone(),ports).unwrap();
        let started=session.start().unwrap();assert_eq!(started.state,"ready");
        fs::copy(std::env::var_os("EXHIBITOS_TEST_ARCHIVE").expect("archive required"),root.join("synthetic.oex")).unwrap();
        fs::set_permissions(root.join("synthetic.oex"),fs::Permissions::from_mode(0o600)).unwrap();
        let source=File::open(root.join("synthetic.oex")).unwrap();
        let output=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(root.join("packed")).unwrap();
        let a=source.metadata().unwrap();let b=output.metadata().unwrap();let id=||Uuid::new_v4().to_string();
        let manifest=fixture["manifestSha256"].as_str().unwrap().to_string();let total=fixture["totalAssetBytes"].as_u64().unwrap();
        let synthetic_disk=b"EXHIBITOS-DISK-RESERVATION\0v1\0synthetic-not-provider-authority";
        let receipt=Receipt{version:1,kind:"oex-packed-v1".into(),tenant_id:tenant,resource_id:workspace,job_id:id(),user_id:principal,request_id:id(),lease_id:id(),attempt_id:id(),runtime_generation:session.job_id.clone().unwrap(),stage_id:id(),reservation_id:id(),reservation_generation:id(),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into(),archive_bytes:fixture["archiveBytes"].as_u64().unwrap(),archive_sha256:fixture["archiveSha256"].as_str().unwrap().into(),manifest_sha256:manifest.clone(),source_digest:fixture["archiveSha256"].as_str().unwrap().into(),worker_digest:payload.context().manifest_sha256,reservation_digest:digest(synthetic_disk),capacity:total,input_device:a.dev().to_string(),input_inode:a.ino().to_string(),output_device:b.dev().to_string(),output_inode:b.ino().to_string()};
        for field in ["workspace", "tenant", "principal"] {
            let mut wrong=receipt.clone();
            match field { "workspace"=>wrong.resource_id=id(), "tenant"=>wrong.tenant_id=id(), _=>wrong.user_id=id() }
            let wrong_bytes=serde_json::to_vec(&wrong).unwrap();
            assert!(Bound::new(&mut session,&wrong_bytes,&digest(&wrong_bytes),&manifest,synthetic_disk,&source,&output).is_err());
        }
        let bytes=serde_json::to_vec(&receipt).unwrap();
        let mut bound=Bound::new(&mut session,&bytes,&digest(&bytes),&manifest,synthetic_disk,&source,&output).unwrap();
        let reply=bound.run_direct(total,&AtomicBool::new(false)).unwrap();
        let ack:Value=serde_json::from_slice(reply.ack()).unwrap();let entries=ack["entries"].as_array().unwrap();let refs=fixture["assets"].as_array().unwrap();assert_eq!(entries.len(),refs.len());
        for entry in entries {let reference=refs.iter().find(|v|v["path"]==entry["path"]).unwrap();let len=entry["bytes"].as_u64().unwrap() as usize;assert_eq!(reference["bytes"].as_u64().unwrap() as usize,len);assert_eq!(entry["sha256"],reference["sha256"]);let mut data=vec![0;len];let mut read=0;while read<len {let n=output.read_at(&mut data[read..],entry["offset"].as_u64().unwrap()+read as u64).unwrap();assert!(n>0);read+=n;}assert_eq!(digest(&data),reference["sha256"].as_str().unwrap());}
        assert!(root.join(format!("native-oex-intent-{}.json",receipt.attempt_id)).exists());assert!(root.join(format!("oex-attempt-{}.json",receipt.attempt_id)).exists());
        drop(bound);drop(source);drop(output);
        let stopped=session.stop().unwrap();assert_eq!(stopped.state,"stopped");assert!(stopped.node_pid.is_none()&&stopped.postgres_pid.is_none());
        assert!(!root.join("native-database/postmaster.pid").exists());
        println!("isolated_session_root={} payload_manifest_sha256={} archive_sha256={} fixture_sha256={} output_bytes={} node_exit={:?} postgres_exit={:?} authority=SYNTHETIC_NOT_PROVIDER",root.display(),payload.context().manifest_sha256,fixture["archiveSha256"].as_str().unwrap(),digest(&fixture_bytes),total,stopped.exits.get("node"),stopped.exits.get("postgres"));
    }
    #[test]
    fn receipt_binds_exact_private_handles_and_intent_is_exclusive() {
        let path = std::env::temp_dir().join(format!("native-oex-binding-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap(); fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let path = fs::canonicalize(path).unwrap();
        write_private_new(&path, "source", b"abc", 3).unwrap();
        let source = File::open(path.join("source")).unwrap();
        let output = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path.join("output")).unwrap();
        let wrong = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path.join("wrong")).unwrap();
        let a = source.metadata().unwrap(); let b = output.metadata().unwrap();
        let id = || Uuid::new_v4().to_string();
        let mut receipt = Receipt { version: 1, kind: "oex-packed-v1".into(), tenant_id: id(), resource_id: id(), job_id: id(), user_id: id(), request_id: id(), lease_id: id(), attempt_id: id(), runtime_generation: id(), stage_id: id(), reservation_id: id(), reservation_generation: id(), profile: "bounded-file-v1".into(), format: "1.0.0-draft.9".into(), archive_bytes: 3, archive_sha256: digest(b"abc"), manifest_sha256: "d".repeat(64), source_digest: digest(b"abc"), worker_digest: "b".repeat(64), reservation_digest: "c".repeat(64), capacity: 20, input_device: a.dev().to_string(), input_inode: a.ino().to_string(), output_device: b.dev().to_string(), output_inode: b.ino().to_string() };
        assert!(check_handles(&receipt, &source, &output).is_ok());
        receipt.source_digest="a".repeat(64);assert!(check_handles(&receipt,&source,&output).is_err());receipt.source_digest=receipt.archive_sha256.clone();
        assert!(check_handles(&receipt, &source, &wrong).is_err());
        let append = OpenOptions::new().read(true).append(true).create_new(true).mode(0o600).open(path.join("append")).unwrap();
        let append_meta = append.metadata().unwrap();
        let output_device = receipt.output_device.clone(); let output_inode = receipt.output_inode.clone();
        receipt.output_device = append_meta.dev().to_string(); receipt.output_inode = append_meta.ino().to_string();
        assert!(check_handles(&receipt, &source, &append).is_err());
        receipt.output_device = output_device; receipt.output_inode = output_inode;
        let saved_inode = receipt.input_inode.clone(); receipt.input_inode = format!("0{saved_inode}"); assert!(check_handles(&receipt, &source, &output).is_err()); receipt.input_inode = saved_inode;
        receipt.reservation_generation = "not-a-uuid".into(); assert!(check_handles(&receipt, &source, &output).is_err());
        receipt.reservation_generation = id();
        let root = File::open(&path).unwrap(); let bytes = serde_json::to_vec(&receipt).unwrap();
        assert!(parse_receipt(&bytes, &digest(&bytes)).is_ok());
        assert!(parse_receipt(&bytes, &"0".repeat(64)).is_err());
        write_intent(&root, &receipt.attempt_id, &bytes).unwrap();
        assert!(write_intent(&root, &receipt.attempt_id, &bytes).is_err());
        assert_eq!(fs::read(path.join(format!("native-oex-intent-{}.json", receipt.attempt_id))).unwrap(), bytes);
        drop(root); drop(source); drop(output); drop(wrong); drop(append); fs::remove_dir_all(path).unwrap();
    }
}
