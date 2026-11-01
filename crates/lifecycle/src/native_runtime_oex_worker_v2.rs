// SPDX-License-Identifier: Apache-2.0
//! Inactive direct-child packed-v2 executor. Its result is not a durable
//! drain, provider claim, output seal, authorization or publication commit.
use super::*;
use std::io::{Read,Write};
use std::os::fd::AsRawFd;
use std::os::unix::{fs::MetadataExt,net::UnixStream,process::CommandExt};

const ACK_LIMIT:usize=4*1024*1024;

/// Both are exact bytes from the owned execution; neither is standalone
/// authority or a substitute for the retained Session/claim/provider.
#[allow(dead_code)]
pub(super) struct Prepared {pub control:Vec<u8>,pub verified:super::oex_ack_v2::Verified,
    _child:super::oex_materializer::ReapedChild}
#[cfg(test)]
impl Prepared {
    pub(super) fn test_only(control:Vec<u8>,verified:super::oex_ack_v2::Verified)->Self {
        Self{control,verified,_child:super::oex_materializer::ReapedChild::test_only()}
    }
}

/// Caller retains Session, verified provider reservation and exclusive stage
/// through this operation. Input is Manager's observed read-only FD; output is
/// a fresh pinned private FD. No caller-selected executable or path is used.
#[allow(dead_code)]
pub(super) fn run_direct(payload:&Payload,journal:&File,input:&super::oex_read_fd::ObservedStage,output:&File,
    request:&super::oex_control_v2::Control,cancelled:&AtomicBool,check_owner:&mut dyn FnMut()->Result<()>)
    -> Result<Prepared> {
    let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
    let encoded=request.encode()?;check_owner()?;
    if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    let src=input.file();let a=src.metadata().map_err(|_|bad())?;let b=output.metadata().map_err(|_|bad())?;
    let af=unsafe{libc::fcntl(src.as_raw_fd(),libc::F_GETFL)};let bf=unsafe{libc::fcntl(output.as_raw_fd(),libc::F_GETFL)};
    if !a.is_file()||!b.is_file()||a.uid()!=unsafe{libc::geteuid()}||b.uid()!=a.uid()
        || a.mode()&0o777!=0o600||b.mode()&0o777!=0o600||a.nlink()!=1||b.nlink()!=1
        || (a.dev(),a.ino(),a.len())!=(input.device,input.inode,input.bytes)||b.len()!=0
        || request.input_device!=a.dev().to_string()||request.input_inode!=a.ino().to_string()
        || request.output_device!=b.dev().to_string()||request.output_inode!=b.ino().to_string()
        || request.archive_bytes!=input.bytes||request.archive_sha256!=input.sha256
        || request.worker_digest!=payload.context().manifest_sha256
        || af<0||bf<0||af&libc::O_ACCMODE!=libc::O_RDONLY||bf&libc::O_ACCMODE!=libc::O_RDWR||bf&libc::O_APPEND!=0
        || (a.dev(),a.ino())==(b.dev(),b.ino()){return Err(bad());}
    let (node,script,root)=payload.oex_packed_worker_v2()?;
    let (mut control,control_child)=UnixStream::pair().map_err(|_|bad())?;
    let (mut ack,ack_child)=UnixStream::pair().map_err(|_|bad())?;
    for socket in [&control,&control_child,&ack,&ack_child] {socket.set_nonblocking(true).map_err(|_|bad())?;}
    let copies=vec![super::oex_materializer_fds::duplicate(control_child.as_raw_fd())?,
        super::oex_materializer_fds::duplicate(ack_child.as_raw_fd())?,super::oex_materializer_fds::duplicate(src.as_raw_fd())?,
        super::oex_materializer_fds::duplicate(output.as_raw_fd())?];
    drop(control_child);drop(ack_child);
    let fds:Vec<_>=copies.iter().map(AsRawFd::as_raw_fd).collect();
    let list=fds.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
    let file_limit=request.capacity;
    let mut command=Command::new(node);
    command.arg("--permission").arg(format!("--allow-fs-read={}",root.display())).arg("--no-addons").arg(script)
        .env_clear().env("PATH","/usr/bin:/bin").env("LANG","C").env("EXHIBITOS_OEX_PACKED_V2_FDS",list)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {command.pre_exec(move || {
        let limit=libc::rlimit{rlim_cur:file_limit as libc::rlim_t,rlim_max:file_limit as libc::rlim_t};
        if libc::setrlimit(libc::RLIMIT_FSIZE,&limit)!=0{return Err(std::io::Error::last_os_error());}
        for fd in &fds {let flags=libc::fcntl(*fd,libc::F_GETFD);if flags<0||libc::fcntl(*fd,libc::F_SETFD,flags&!libc::FD_CLOEXEC)<0{return Err(std::io::Error::last_os_error());}}
        Ok(())
    });}
    check_owner()?;if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    let pre=src.metadata().map_err(|_|bad())?;let pre_out=output.metadata().map_err(|_|bad())?;
    let pre_af=unsafe{libc::fcntl(src.as_raw_fd(),libc::F_GETFL)};let pre_bf=unsafe{libc::fcntl(output.as_raw_fd(),libc::F_GETFL)};
    if (pre.dev(),pre.ino(),pre.len(),pre.mtime(),pre.mtime_nsec(),pre.ctime(),pre.ctime_nsec())
        !=(a.dev(),a.ino(),a.len(),a.mtime(),a.mtime_nsec(),a.ctime(),a.ctime_nsec())
        || !pre.is_file()||!pre_out.is_file()||pre.uid()!=a.uid()||pre_out.uid()!=b.uid()
        || pre.mode()&0o777!=0o600||pre_out.mode()&0o777!=0o600||pre.nlink()!=1||pre_out.nlink()!=1
        || pre_af<0||pre_bf<0||pre_af&libc::O_ACCMODE!=libc::O_RDONLY||pre_bf&libc::O_ACCMODE!=libc::O_RDWR||pre_bf&libc::O_APPEND!=0
        || (pre_out.dev(),pre_out.ino(),pre_out.len())!=(b.dev(),b.ino(),0) {return Err(bad());}
    let identity=super::oex_worker::OexWorkerIdentity{attempt_id:request.attempt_id.clone(),runtime_generation:request.runtime_generation.clone(),
        reservation_generation:request.reservation_generation.clone(),receipt_sha256:digest(&encoded),archive_sha256:request.archive_sha256.clone(),archive_bytes:request.archive_bytes};
    super::oex_worker::reserve(journal,&identity)?;check_owner()?;
    if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    let child=command.spawn().map_err(|_|err("NATIVE_PROCESS_START_FAILED"))?;drop(copies);
    let mut child=super::oex_materializer::OwnedChild::new(child);
    let mut frame=Vec::with_capacity(encoded.len()+4);frame.extend_from_slice(&(encoded.len() as u32).to_be_bytes());frame.extend_from_slice(&encoded);
    let deadline=Instant::now()+Duration::from_secs(120);let mut sent=0;let mut closed=false;let mut reply=Vec::new();let mut eof=false;let mut status=None;
    loop {
        check_owner()?;if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
        if Instant::now()>=deadline{return Err(bad());}
        let mut progressed=false;
        if !closed {match control.write(&frame[sent..]) {
            Ok(0)=>return Err(bad()),Ok(n)=>{progressed=true;sent+=n;if sent==frame.len(){control.shutdown(std::net::Shutdown::Write).map_err(|_|bad())?;closed=true;}},
            Err(e) if e.kind()==std::io::ErrorKind::Interrupted||e.kind()==std::io::ErrorKind::WouldBlock=>{},Err(_)=>return Err(bad()),
        }}
        if !eof {let mut block=[0u8;65536];match ack.read(&mut block){
            Ok(0)=>eof=true,Ok(n)=>{progressed=true;if reply.len()+n>ACK_LIMIT{return Err(bad());}reply.extend_from_slice(&block[..n]);},
            Err(e) if e.kind()==std::io::ErrorKind::Interrupted||e.kind()==std::io::ErrorKind::WouldBlock=>{},Err(_)=>return Err(bad()),
        }}
        if status.is_none(){status=child.poll().map_err(|_|bad())?;}
        if let Some(exit)=status {if !exit.success(){return Err(bad());}if eof&&closed{break;}}
        if !progressed {thread::sleep(Duration::from_millis(5));}
    }
    output.sync_all().map_err(|_|bad())?;check_owner()?;
    let after=src.metadata().map_err(|_|bad())?;
    if !after.is_file()||after.uid()!=a.uid()||after.mode()&0o777!=0o600||after.nlink()!=1
        || (after.dev(),after.ino(),after.len(),after.mtime(),after.mtime_nsec(),after.ctime(),after.ctime_nsec())
            !=(a.dev(),a.ino(),a.len(),a.mtime(),a.mtime_nsec(),a.ctime(),a.ctime_nsec()){return Err(bad());}
    let final_size=output.metadata().map_err(|_|bad())?.len();let receipt_digest=digest(&encoded);
    let expected=super::oex_ack_v2::Expected{attempt_id:&request.attempt_id,runtime_generation:&request.runtime_generation,
        reservation_id:&request.reservation_id,reservation_generation:&request.reservation_generation,receipt_sha256:&receipt_digest,
        archive_sha256:&request.archive_sha256,output_device:b.dev(),output_inode:b.ino(),output_bytes:final_size,capacity:request.capacity};
    let verified=super::oex_ack_v2::check(&reply,&expected,output,cancelled,check_owner)?;
    Ok(Prepared{control:encoded,verified,_child:child.reaped_token()?})
}
