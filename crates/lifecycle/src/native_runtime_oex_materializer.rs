// SPDX-License-Identifier: Apache-2.0
//! Inactive direct-child materializer transport. Caller must independently
//! hold the claim, provider reservation, Session and common writer fence until
//! a separately verified outcome; this module never releases any of them.
use super::*;
use std::io::{Read, Write};
use std::os::unix::{net::UnixStream, process::CommandExt};

pub(super) struct OwnedChild { child: std::process::Child, reaped: bool, succeeded: bool }
/// Minted only after this exact owned Child has reported an exit through wait.
pub(super) struct ReapedChild(());
#[cfg(test)]
impl ReapedChild { pub(super) fn test_only()->Self {Self(())} }
impl OwnedChild {
    pub(super) fn new(child:std::process::Child)->Self {Self{child,reaped:false,succeeded:false}}
    pub(super) fn poll(&mut self)->std::io::Result<Option<std::process::ExitStatus>> {
        let status=self.child.try_wait()?;if let Some(exit)=status {self.reaped=true;self.succeeded=exit.success();}Ok(status)
    }
    pub(super) fn reaped_token(&self)->Result<ReapedChild> {
        if !self.reaped || !self.succeeded {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        Ok(ReapedChild(()))
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {return;}
        // Never infer a drain from kill or a PID. Wait on the actual retained
        // Child, including on errors; an uninterruptible child retains scope.
        let _=self.child.kill();
        loop { match self.child.wait() {
            Ok(_)=>break,
            Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>continue,
            Err(_)=>thread::sleep(Duration::from_millis(100)),
        }}
    }
}

/// This result records only a successful direct-child wait and correlated
/// response. It is not a live claim, a descendant drain, or a stage seal.
#[allow(dead_code)]
pub(super) struct ReapedMaterializer {
    _child:ReapedChild,
    attempt_id:String, runtime_generation:String, stage_id:String,
    reservation_id:String, reservation_generation:String,
    stage_device:String, stage_inode:String, receipt_sha256:String,
}
#[allow(dead_code)]
impl ReapedMaterializer {
    /// Correlation only. This object may outlive the Session and cannot
    /// establish current liveness, exclusive ownership, or a sealed stage.
    pub(super) fn matches(&self,request:&super::oex_materializer_protocol::Request,encoded:&[u8])->bool {
        request.encode().is_ok_and(|bytes|bytes.as_slice()==encoded)
            && self.attempt_id==request.attempt_id && self.runtime_generation==request.runtime_generation
            && self.stage_id==request.stage_id && self.reservation_id==request.reservation_id
            && self.reservation_generation==request.reservation_generation && self.stage_device==request.stage_device
            && self.stage_inode==request.stage_inode && self.receipt_sha256==digest(encoded)
    }
}

/// The caller must already have authorized and pinned all inputs, closed
/// competing writer admission, and hold the live common fence and reservation.
/// Nothing here implements those prerequisites or starts a production route.
#[allow(dead_code)]
pub(super) fn run_direct(
    payload:&Payload, stage:&super::oex_read_fd::Stage, chunks:&[super::oex_chunks::Chunk],
    request:&super::oex_materializer_protocol::Request, cancelled:&AtomicBool,
    check_owner:&mut dyn FnMut()->Result<()>,
) -> Result<ReapedMaterializer> {
    let uncertain=||err("NATIVE_OEX_WORKER_UNCERTAIN");
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
    if request.worker_digest != payload.context().manifest_sha256 {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
    let (node,script,root)=payload.oex_materializer()?;
    let (mut control,parent_control_child)=UnixStream::pair().map_err(|_|uncertain())?;
    let (mut ack,parent_ack_child)=UnixStream::pair().map_err(|_|uncertain())?;
    control.set_nonblocking(true).map_err(|_|uncertain())?;
    ack.set_nonblocking(true).map_err(|_|uncertain())?;
    parent_control_child.set_nonblocking(true).map_err(|_|uncertain())?;
    parent_ack_child.set_nonblocking(true).map_err(|_|uncertain())?;
    let fds=super::oex_materializer_fds::MaterializerFds::new(parent_control_child,parent_ack_child,stage,chunks)?;
    let mut command=Command::new(node);
    command.arg("--permission").arg(format!("--allow-fs-read={}",root.display())).arg("--no-addons").arg(script)
        .env_clear().env("PATH","/usr/bin:/bin").env("LANG","C")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let file_limit=request.archive_bytes;
    unsafe {command.pre_exec(move || {
        let limit=libc::rlimit {rlim_cur:file_limit as libc::rlim_t,rlim_max:file_limit as libc::rlim_t};
        if libc::setrlimit(libc::RLIMIT_FSIZE,&limit)!=0 {return Err(std::io::Error::last_os_error());}
        Ok(())
    });}
    let (child,encoded)=fds.spawn(command,request)?;
    let mut child=OwnedChild::new(child);
    let mut frame=Vec::with_capacity(encoded.len()+4);
    frame.extend_from_slice(&(encoded.len() as u32).to_be_bytes());frame.extend_from_slice(&encoded);
    let deadline=Instant::now()+Duration::from_secs(120);
    let mut sent=0usize;let mut closed=false;let mut reply=Vec::new();let mut eof=false;
    let mut status=None;
    loop {
        check_owner()?;
        if cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
        if Instant::now()>=deadline {return Err(uncertain());}
        if !closed {
            match control.write(&frame[sent..]) {
                Ok(0)=>return Err(uncertain()),
                Ok(n)=>{sent+=n;if sent==frame.len(){control.shutdown(std::net::Shutdown::Write).map_err(|_|uncertain())?;closed=true;}},
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted || e.kind()==std::io::ErrorKind::WouldBlock=>{},
                Err(_)=>return Err(uncertain()),
            }
        }
        if !eof {
            let mut buffer=[0u8;2048];
            match ack.read(&mut buffer) {
                Ok(0)=>eof=true,
                Ok(n)=>{if reply.len()+n>8192{return Err(uncertain());}reply.extend_from_slice(&buffer[..n]);},
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted || e.kind()==std::io::ErrorKind::WouldBlock=>{},
                Err(_)=>return Err(uncertain()),
            }
        }
        if status.is_none() {
            status=child.poll().map_err(|_|uncertain())?;
        }
        if let Some(exit)=status {
            if !exit.success() {return Err(uncertain());}
            if eof && closed {break;}
        }
        thread::sleep(Duration::from_millis(10));
    }
    request.check_ack(&encoded,&reply)?;
    check_owner()?;
    if cancelled.load(Ordering::SeqCst) {return Err(err("NATIVE_CANCELLED"));}
    Ok(ReapedMaterializer {_child:child.reaped_token()?,attempt_id:request.attempt_id.clone(),runtime_generation:request.runtime_generation.clone(),
        stage_id:request.stage_id.clone(),reservation_id:request.reservation_id.clone(),reservation_generation:request.reservation_generation.clone(),
        stage_device:request.stage_device.clone(),stage_inode:request.stage_inode.clone(),receipt_sha256:digest(&encoded)})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_token_requires_actual_successful_exit() {
        let child=Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut child=OwnedChild::new(child);
        assert!(child.reaped_token().is_err());
        drop(child.child.stdin.take());
        let deadline=Instant::now()+Duration::from_secs(5);
        while child.poll().unwrap().is_none() {
            assert!(Instant::now()<deadline);thread::sleep(Duration::from_millis(10));
        }
        assert!(child.reaped_token().is_ok());
        let child=Command::new("/usr/bin/false").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut child=OwnedChild::new(child);
        while child.poll().unwrap().is_none() {
            assert!(Instant::now()<deadline);thread::sleep(Duration::from_millis(10));
        }
        assert!(child.reaped_token().is_err());
    }
    #[test]
    fn reaped_result_matches_complete_original_request_only() {
        let mut request=super::super::oex_materializer_protocol::tests::request();
        let bytes=request.encode().unwrap();
        let reaped=ReapedMaterializer {_child:ReapedChild::test_only(),attempt_id:request.attempt_id.clone(),runtime_generation:request.runtime_generation.clone(),
            stage_id:request.stage_id.clone(),reservation_id:request.reservation_id.clone(),reservation_generation:request.reservation_generation.clone(),
            stage_device:request.stage_device.clone(),stage_inode:request.stage_inode.clone(),receipt_sha256:digest(&bytes)};
        assert!(reaped.matches(&request,&bytes));
        request.job_id=Uuid::new_v4().to_string();
        assert!(!reaped.matches(&request,&bytes));
    }
}
