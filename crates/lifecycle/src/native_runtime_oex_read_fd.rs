// SPDX-License-Identifier: Apache-2.0
//! Inactive private stage FD preparation. This is not operation authority,
//! IPC authentication, parent-death recovery, or reader/worker drain proof.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{fs::{FileExt, MetadataExt}, process::CommandExt};

/// A single newly created empty stage. No reuse/truncate or retirement is
/// implemented here; ownership must stay with the eventual runtime fence.
#[allow(dead_code)]
pub(super) struct Stage { write: File, read: File, name: String }

/// A byte observation of the original read-only descriptor. This is not a
/// sealed inode, a live claim, or proof that another writer cannot still run.
#[allow(dead_code)]
pub(super) struct ObservedStage {
    read: File,
    pub(super) device: u64,
    pub(super) inode: u64,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}
#[allow(dead_code)]
impl Stage {
    pub(super) fn matches_stage(&self,stage_id:&str)->bool { self.name==format!(".oex-stage-{stage_id}") }
    pub(super) fn identity(&self)->Result<(u64,u64)> {
        let a=self.write.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let b=self.read.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !a.is_file() || !b.is_file() || a.uid()!=unsafe {libc::geteuid()} || b.uid()!=a.uid()
            || a.mode()&0o777!=0o600 || b.mode()&0o777!=0o600 || a.nlink()!=1 || b.nlink()!=1
            || a.len()!=0 || b.len()!=0 || (a.dev(),a.ino())!=(b.dev(),b.ino()) {
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        Ok((a.dev(),a.ino()))
    }
    /// Only the retained manager-owned launcher may inherit this write handle.
    /// It grants no operation authority and the stage must not be observed
    /// until the actual direct child has exited and writer admission is closed.
    pub(super) fn materializer_write(&self) -> &File { &self.write }
    /// The caller supplies the already pinned, private reservation directory
    /// and a stage ID durably reserved before calling; this function does not
    /// establish that prerequisite or grant ownership.
    /// Only a new empty inode is created; errors never adopt an existing entry
    /// or delete data. The stage is retained for future verified recovery.
    pub(super) fn create(directory: &File, stage_id: &str) -> Result<Self> {
        if Uuid::parse_str(stage_id).ok().is_none_or(|id| id.to_string() != stage_id) { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        let d = directory.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !d.is_dir() || d.uid() != unsafe { libc::geteuid() } || d.mode() & 0o777 != 0o700 {
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        let name_text = format!(".oex-stage-{stage_id}");
        let name = std::ffi::CString::new(name_text.as_str()).map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let w = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
        if w < 0 { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        let write = unsafe { File::from_raw_fd(w) };
        let r = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if r < 0 { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        let read = unsafe { File::from_raw_fd(r) };
        let a = write.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let b = read.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !a.is_file() || !b.is_file() || a.uid() != unsafe { libc::geteuid() } || a.mode() & 0o777 != 0o600
            || a.nlink() != 1 || b.nlink() != 1 || a.len() != 0 || b.len() != 0
            || (a.dev(),a.ino()) != (b.dev(),b.ino()) { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        write.sync_all().and_then(|_| directory.sync_all()).map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let a = write.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let b = read.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !a.is_file() || !b.is_file() || a.uid() != unsafe { libc::geteuid() } || b.uid() != a.uid()
            || a.mode() & 0o777 != 0o600 || b.mode() & 0o777 != 0o600 || a.nlink() != 1 || b.nlink() != 1
            || a.len() != 0 || b.len() != 0 || (a.dev(),a.ino()) != (b.dev(),b.ino()) { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        let mut named = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstatat(directory.as_raw_fd(), name.as_ptr(), named.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        let named = unsafe { named.assume_init() };
        if (named.st_dev as u64, named.st_ino as u64) != (a.dev(), a.ino()) || named.st_mode & libc::S_IFMT != libc::S_IFREG
            || named.st_uid != a.uid() || named.st_mode & 0o777 != 0o600 || named.st_nlink != 1 { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        Ok(Self { write, read, name: name_text })
    }
    /// Keep this object alive through spawn. This only makes one descriptor
    /// inheritable in the forked child, and never changes the parent's flags or
    /// credential stdin. It does not verify descendant descriptor inheritance.
    pub(super) fn prepare_inheritance(&self) -> Result<Inherited> {
        let fd = unsafe { libc::fcntl(self.read.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if fd < 0 { return Err(err("NATIVE_OEX_OUTPUT_INVALID")); }
        Ok(Inherited(unsafe { File::from_raw_fd(fd) }))
    }

    /// Observe a nonempty assembled input archive, not a possibly empty packed
    /// output. The caller must first close writer admission and wait for every owned
    /// writer. This function cannot prove that precondition or exclude an
    /// external writer; it only syncs and observes the same pinned inode.
    /// On any error the named stage is retained for later reconciliation. The
    /// caller must retain the claim and treat the operation as uncertain; this
    /// check does not transition a cached Session state or authorize a retry.
    pub(super) fn observe_after_owned_writer(
        self, session: &mut Session, expected_bytes: u64, expected_sha256: &str,
        cancelled: &AtomicBool,
    ) -> Result<ObservedStage> {
        let mut check_owner = || {
            session.poll()?;
            if session.state != "ready" || session.node.is_none() || session.postgres.is_none() {
                return Err(err("NATIVE_PROCESS_EXITED"));
            }
            session.check_native_observation_fence()
        };
        observe_stage(self, expected_bytes, expected_sha256, cancelled, &mut check_owner)
    }
}
#[allow(dead_code)]
impl ObservedStage {
    pub(super) fn file(&self)->&File {&self.read}
}

pub(super) fn observe_stage(
    stage: Stage, expected_bytes: u64, expected_sha256: &str, cancelled: &AtomicBool,
    check_owner: &mut dyn FnMut() -> Result<()>,
) -> Result<ObservedStage> {
    let invalid = || err("NATIVE_OEX_OUTPUT_INVALID");
    if !(1..=512 * 1024 * 1024).contains(&expected_bytes) || !hash_valid(expected_sha256) {
        return Err(invalid());
    }
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    let Stage { write, read, name: _ } = stage;
    write.sync_all().map_err(|_| invalid())?;
    let a = write.metadata().map_err(|_| invalid())?;
    let b = read.metadata().map_err(|_| invalid())?;
    let flags = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_GETFL) };
    if !a.is_file() || !b.is_file() || a.uid() != unsafe { libc::geteuid() } || b.uid() != a.uid()
        || a.mode() & 0o777 != 0o600 || b.mode() & 0o777 != 0o600 || a.nlink() != 1 || b.nlink() != 1
        || (a.dev(), a.ino(), a.len()) != (b.dev(), b.ino(), expected_bytes)
        || flags < 0 || flags & libc::O_ACCMODE != libc::O_RDONLY {
        return Err(invalid());
    }
    drop(write);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut offset = 0u64;
    while offset < expected_bytes {
        check_owner()?;
        if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
        let amount = ((expected_bytes - offset) as usize).min(buffer.len());
        let count = read.read_at(&mut buffer[..amount], offset).map_err(|_| invalid())?;
        if count == 0 { return Err(invalid()); }
        hasher.update(&buffer[..count]);
        offset += count as u64;
    }
    let digest = format!("{:x}", hasher.finalize());
    let after = read.metadata().map_err(|_| invalid())?;
    if digest != expected_sha256 || !after.is_file() || after.uid() != a.uid() || after.mode() & 0o777 != 0o600
        || after.nlink() != 1 || (after.dev(), after.ino(), after.len()) != (a.dev(), a.ino(), expected_bytes)
        || (after.mtime(), after.mtime_nsec(), after.ctime(), after.ctime_nsec())
            != (a.mtime(), a.mtime_nsec(), a.ctime(), a.ctime_nsec()) {
        return Err(invalid());
    }
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) { return Err(err("NATIVE_CANCELLED")); }
    Ok(ObservedStage { read, device: a.dev(), inode: a.ino(), bytes: expected_bytes, sha256: digest })
}
#[allow(dead_code)]
pub(super) struct Inherited(File);
#[allow(dead_code)]
impl Inherited {
    pub(super) fn spawn(self, mut command: Command) -> std::io::Result<std::process::Child> {
        let fd = self.0.as_raw_fd();
        command.env("EXHIBITOS_NATIVE_PACKED_READ_FD", fd.to_string());
        unsafe { command.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 { return Err(std::io::Error::last_os_error()); }
            Ok(())
        }); }
        command.spawn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{PermissionsExt, FileExt};
    #[test]
    #[ignore = "requires EXHIBITOS_TEST_NODE and a coordinated native test slot"]
    fn actual_node_inherits_read_fd_without_replacing_stdin() {
        use std::io::Write;
        let path=std::env::temp_dir().join(format!("native-oex-fd-{}",Uuid::new_v4()));
        fs::create_dir(&path).unwrap(); fs::set_permissions(&path,fs::Permissions::from_mode(0o700)).unwrap();
        let directory=File::open(&path).unwrap(); let stage_id=Uuid::new_v4().to_string(); let mut stage=Stage::create(&directory,&stage_id).unwrap();
        stage.write.write_all(b"abc").unwrap(); stage.write.sync_all().unwrap();
        let inherited=stage.prepare_inheritance().unwrap();
        let node=std::env::var_os("EXHIBITOS_TEST_NODE").expect("EXHIBITOS_TEST_NODE required");
        let script="const fs=require('node:fs');const fd=Number(process.env.EXHIBITOS_NATIVE_PACKED_READ_FD);const input=fs.readFileSync(0,'utf8');const b=Buffer.alloc(3);fs.readSync(fd,b,0,3,0);if(input!=='test-input'||b.toString()!=='abc')process.exit(2);const id=fs.fstatSync(fd,{bigint:true});const code='try{const s=require(\"node:fs\").fstatSync(Number(process.argv[1]),{bigint:true});process.stdout.write(s.dev.toString()===process.argv[2]&&s.ino.toString()===process.argv[3]?\"LEAKED\":\"REUSED\")}catch(e){process.stdout.write(e.code===\"EBADF\"?\"CLOSED\":String(e.code))}';const sub=require('node:child_process').spawnSync(process.execPath,['-e',code,String(fd),id.dev.toString(),id.ino.toString()],{encoding:'utf8'});if(sub.status!==0||!['CLOSED','REUSED'].includes(sub.stdout)){process.stderr.write('substatus='+sub.status+' substdout='+sub.stdout+' substderr='+sub.stderr);process.exit(3);}process.stdout.write('OK');";
        let mut command=Command::new(node); command.arg("-e").arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child=inherited.spawn(command).unwrap();
        child.stdin.take().unwrap().write_all(b"test-input").unwrap();
        let result=child.wait_with_output().unwrap(); assert!(result.status.success(), "status={:?} stderr={}",result.status,String::from_utf8_lossy(&result.stderr));assert_eq!(result.stdout,b"OK");
        drop(stage);drop(directory);fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn stage_is_private_same_inode_and_reader_is_read_only() {
        let path=std::env::temp_dir().join(format!("native-oex-stage-{}",Uuid::new_v4()));
        fs::create_dir(&path).unwrap(); fs::set_permissions(&path,fs::Permissions::from_mode(0o700)).unwrap();
        let directory=File::open(&path).unwrap(); let stage_id=Uuid::new_v4().to_string(); let mut stage=Stage::create(&directory,&stage_id).unwrap();
        assert_eq!(stage.write.metadata().unwrap().nlink(),1); assert_eq!(stage.read.metadata().unwrap().nlink(),1);
        assert_eq!(stage.write.metadata().unwrap().ino(),stage.read.metadata().unwrap().ino());
        stage.write.write_all(b"abc").unwrap(); stage.write.sync_all().unwrap();
        let mut bytes=[0;3]; assert_eq!(stage.read.read_at(&mut bytes,0).unwrap(),3); assert_eq!(&bytes,b"abc");
        assert!(stage.read.write_at(b"x",0).is_err()); assert_eq!(fs::read_dir(&path).unwrap().count(),1);
        assert!(Stage::create(&directory,&stage_id).is_err());
        drop(stage);drop(directory);fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn observes_same_read_only_stage_and_retains_failed_stage() {
        let path=std::env::temp_dir().join(format!("native-oex-observe-{}",Uuid::new_v4()));
        fs::create_dir(&path).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o700)).unwrap();
        let directory=File::open(&path).unwrap();let id=Uuid::new_v4().to_string();
        let mut stage=Stage::create(&directory,&id).unwrap();stage.write.write_all(b"abc").unwrap();
        let inode=stage.read.metadata().unwrap().ino();let cancelled=AtomicBool::new(false);
        let observed=observe_stage(stage,3,&digest(b"abc"),&cancelled,&mut || Ok(())).unwrap();
        assert_eq!((observed.inode,observed.bytes,observed.sha256.as_str()),(inode,3,digest(b"abc").as_str()));
        assert!(observed.read.write_at(b"x",0).is_err());
        let mut bytes=[0;3];assert_eq!(observed.read.read_at(&mut bytes,0).unwrap(),3);assert_eq!(&bytes,b"abc");
        assert!(Stage::create(&directory,&id).is_err());drop(observed);
        let failed_id=Uuid::new_v4().to_string();let mut failed=Stage::create(&directory,&failed_id).unwrap();
        failed.write.write_all(b"abc").unwrap();
        assert!(observe_stage(failed,3,&digest(b"different"),&cancelled,&mut || Ok(())).is_err());
        assert_eq!(fs::read(path.join(format!(".oex-stage-{failed_id}"))).unwrap(),b"abc");
        assert!(Stage::create(&directory,&failed_id).is_err());
        let stopped_id=Uuid::new_v4().to_string();let mut stopped=Stage::create(&directory,&stopped_id).unwrap();
        stopped.write.write_all(b"abc").unwrap();let calls=std::cell::Cell::new(0);
        let result=observe_stage(stopped,3,&digest(b"abc"),&cancelled,&mut || {
            calls.set(calls.get()+1); if calls.get()==1 { Ok(()) } else { Err(err("NATIVE_PROCESS_EXITED")) }
        });
        assert!(result.is_err());assert_eq!(fs::read(path.join(format!(".oex-stage-{stopped_id}"))).unwrap(),b"abc");
        let cancelled_id=Uuid::new_v4().to_string();let mut cancelled_stage=Stage::create(&directory,&cancelled_id).unwrap();
        cancelled_stage.write.write_all(b"abc").unwrap();let signal=AtomicBool::new(false);let calls=std::cell::Cell::new(0);
        let result=observe_stage(cancelled_stage,3,&digest(b"abc"),&signal,&mut || {
            calls.set(calls.get()+1);if calls.get()==3 {signal.store(true,Ordering::SeqCst);}Ok(())
        });
        assert!(result.is_err());assert_eq!(fs::read(path.join(format!(".oex-stage-{cancelled_id}"))).unwrap(),b"abc");
        drop(directory);fs::remove_dir_all(path).unwrap();
    }
}
