// SPDX-License-Identifier: Apache-2.0
//! Private, opt-in handshake over a socket inherited at API child launch.
//! This is not an operation claim, a worker drain proof, or a release grant.
use super::*;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{net::UnixStream, process::CommandExt};
use base64::{engine::general_purpose::STANDARD,Engine as _};

const LIMIT: usize = 2048;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerBootstrap<'a> { workspace: &'a LocalAuthority, owner_channel: OwnerBinding<'a> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerBinding<'a> { version: u32, runtime_generation: &'a str, secret: &'a str }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreshOwnerBootstrap<'a> { workspace: &'a LocalAuthority, owner_channel: FreshOwnerBinding<'a> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreshOwnerBinding<'a> { version: u32, runtime_generation: &'a str, secret: &'a str, mode: &'static str }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hello<'a> { version: u32, kind: &'static str, workspace_id: &'a str, tenant_id: &'a str, principal_id: &'a str, runtime_generation: &'a str, secret: &'a str }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack { version: u32, kind: String, workspace_id: String, tenant_id: String, principal_id: String, runtime_generation: String }

/// Holds both original endpoints until spawn. Only the duplicated child
/// descriptor is made inheritable in the forked API child.
pub(super) struct Setup { parent: UnixStream, child: Option<File>, secret: String }
/// Intended to stay inside the owning Session; this type does not enforce
/// that lifetime. It is not a claim, readiness, fence or authorization; loss
/// of the socket is terminal once observed by the transport.
pub(super) struct Connection { socket: UnixStream, key: [u8;32], valid: bool, ready: bool, claim_started: bool, metadata_started: bool }
impl Drop for Connection {
    fn drop(&mut self) { self.key.fill(0); }
}
impl Drop for Setup {
    fn drop(&mut self) { let mut b=std::mem::take(&mut self.secret).into_bytes(); b.fill(0); }
}
impl Setup {
    pub(super) fn new() -> Result<Self> {
        let (parent, child) = UnixStream::pair().map_err(|_| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        for stream in [&parent, &child] {
            let flags=unsafe { libc::fcntl(stream.as_raw_fd(),libc::F_GETFD) };
            if flags<0 || unsafe { libc::fcntl(stream.as_raw_fd(),libc::F_SETFD,flags|libc::FD_CLOEXEC) }<0 {
                return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
            }
        }
        let fd=unsafe { libc::fcntl(child.as_raw_fd(),libc::F_DUPFD_CLOEXEC,10) };
        if fd<0 { return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        let child=unsafe { File::from_raw_fd(fd) };
        if fd>1023 { return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        let mut bytes=[0u8;32];
        if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(),bytes.len()) }!=0 { return Err(err("NATIVE_RANDOM_UNAVAILABLE")); }
        let secret=bytes.iter().map(|v|format!("{v:02x}")).collect::<String>();bytes.fill(0);
        parent.set_nonblocking(true).map_err(|_| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        Ok(Self {parent,child:Some(child),secret})
    }
    pub(super) fn bootstrap(&self, auth:&LocalAuthority, generation:&str) -> Result<Vec<u8>> {
        let value=OwnerBootstrap { workspace:auth, owner_channel:OwnerBinding {version:1,runtime_generation:generation,secret:&self.secret} };
        let mut bytes=serde_json::to_vec(&value).map_err(|_| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.len()>4096 { bytes.fill(0);return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        Ok(bytes)
    }
    /// Separate closed ABI for the fresh-only test Session. No public request
    /// or environment value can select this mode.
    pub(super) fn fresh_bootstrap(&self, auth:&LocalAuthority, generation:&str) -> Result<Vec<u8>> {
        let value=FreshOwnerBootstrap {workspace:auth,owner_channel:FreshOwnerBinding {version:2,runtime_generation:generation,secret:&self.secret,mode:"fresh-oex-test-v1"}};
        let mut bytes=serde_json::to_vec(&value).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.len()>4096 {bytes.fill(0);return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        Ok(bytes)
    }
    /// Unconnected normal-mode envelope. Payload admission and a live owned
    /// Session must be established by the caller; this grants no authority.
    pub(super) fn normal_bootstrap(&self, auth:&LocalAuthority, generation:&str) -> Result<Vec<u8>> {
        let value=FreshOwnerBootstrap {workspace:auth,owner_channel:FreshOwnerBinding {version:2,runtime_generation:generation,secret:&self.secret,mode:"normal-oex-v1"}};
        let mut bytes=serde_json::to_vec(&value).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.len()>4096 {bytes.fill(0);return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        Ok(bytes)
    }
    pub(super) fn configure(&self, command:&mut Command) -> Result<()> {
        let fd=self.child.as_ref().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.as_raw_fd();
        command.env("EXHIBITOS_NATIVE_OWNER_FD",fd.to_string());
        unsafe { command.pre_exec(move || {
            let flags=libc::fcntl(fd,libc::F_GETFD);
            if flags<0 || libc::fcntl(fd,libc::F_SETFD,flags & !libc::FD_CLOEXEC)<0 { return Err(std::io::Error::last_os_error()); }
            Ok(())
        }); }
        Ok(())
    }
    pub(super) fn close_child(&mut self) { self.child.take(); }
    /// A successful hello proves only possession of this inherited socket and
    /// agreement on the launch identity. The operation still needs live claim,
    /// workspace, process and fence verification; EOF/timeout is unavailable.
    pub(super) fn handshake(mut self, auth:&LocalAuthority, generation:&str, cancellation:&AtomicBool) -> Result<Connection> {
        let deadline=Instant::now()+Duration::from_secs(30);
        let hello=Hello {version:1,kind:"hello",workspace_id:&auth.workspace_id,tenant_id:&auth.tenant_id,principal_id:&auth.principal_id,runtime_generation:generation,secret:&self.secret};
        let mut bytes=serde_json::to_vec(&hello).map_err(|_| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.is_empty()||bytes.len()>LIMIT { bytes.fill(0);return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        let sent=write_bounded(&mut self.parent,&(bytes.len() as u32).to_be_bytes(),deadline,cancellation,&mut ||Ok(())).and_then(|_|write_bounded(&mut self.parent,&bytes,deadline,cancellation,&mut ||Ok(())));bytes.fill(0);
        sent?;
        let mut len=[0u8;4];read_bounded(&mut self.parent,&mut len,deadline,cancellation,&mut ||Ok(()))?;
        let n=u32::from_be_bytes(len) as usize;if n==0||n>LIMIT{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let mut reply=vec![0u8;n];read_bounded(&mut self.parent,&mut reply,deadline,cancellation,&mut ||Ok(()))?;
        let ack:Ack=serde_json::from_slice(&reply).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if ack.version!=1||ack.kind!="hello-ack"||ack.workspace_id!=auth.workspace_id||ack.tenant_id!=auth.tenant_id||ack.principal_id!=auth.principal_id||ack.runtime_generation!=generation {return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let socket=self.parent.try_clone().map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut key=[0u8;32];
        for (i,pair) in self.secret.as_bytes().chunks_exact(2).enumerate() {
            let digit=|v:u8|if v<=b'9'{v-b'0'}else{v-b'a'+10};
            key[i]=(digit(pair[0])<<4)|digit(pair[1]);
        }
        Ok(Connection {socket,key,valid:true,ready:false,claim_started:false,metadata_started:false})
    }
}

#[allow(dead_code)]
impl Connection {
    // No MAC accessor yet: cached `valid` alone cannot prove the owned peer or
    // fence is live. A typed, single-use Session-bound challenge is required.
    fn fail(&mut self) {self.valid=false;self.key.fill(0);let _=self.socket.shutdown(std::net::Shutdown::Both);}
    fn read_frame(&mut self, deadline:Instant, cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)->Result<Vec<u8>> {
        let mut len=[0u8;4];read_bounded(&mut self.socket,&mut len,deadline,cancellation,check)?;
        let n=u32::from_be_bytes(len) as usize;if n==0||n>8192{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let mut bytes=vec![0u8;n];read_bounded(&mut self.socket,&mut bytes,deadline,cancellation,check)?;Ok(bytes)
    }
    /// Accept exactly one canonical readiness frame tied to the supplied live
    /// personal identity. Any mismatch or transport error closes permanently.
    pub(super) fn receive_ready(&mut self,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>,
        workspace:&str,tenant:&str,principal:&str,generation:&str)->Result<()> {
        if !self.valid||self.ready {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let result=self.read_frame(Instant::now()+Duration::from_secs(30),cancellation,check)
            .and_then(|bytes|super::owner_claim_frames::check_ready(&bytes,workspace,tenant,principal,generation))
            .and_then(|_|check());
        match result {Ok(())=>{self.ready=true;Ok(())},Err(error)=>{self.fail();Err(error)}}
    }
    /// One request/one reply on the same owned connection. The caller must
    /// invalidate on semantic mismatch; this transport grants no claim.
    pub(super) fn exchange(&mut self,request:&[u8],cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)->Result<Vec<u8>> {
        if !self.valid||!self.ready||request.is_empty()||request.len()>8192 {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let deadline=Instant::now()+Duration::from_secs(30);
        let result=write_bounded(&mut self.socket,&(request.len() as u32).to_be_bytes(),deadline,cancellation,check)
            .and_then(|_|write_bounded(&mut self.socket,request,deadline,cancellation,check))
            .and_then(|_|self.read_frame(deadline,cancellation,check));
        if result.is_err(){self.fail();}result
    }
    pub(super) fn invalidate(&mut self) {self.fail();}
    fn send_frame(&mut self,bytes:&[u8],deadline:Instant,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)->Result<()> {
        if bytes.is_empty()||bytes.len()>8192{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        write_bounded(&mut self.socket,&(bytes.len() as u32).to_be_bytes(),deadline,cancellation,check)?;
        write_bounded(&mut self.socket,bytes,deadline,cancellation,check)
    }
    /// The ACK must already have passed the fixed child/FD/identity checks;
    /// caller retains the same claim, Session and output throughout. No path,
    /// arbitrary offset, second transfer or recovery adoption is accepted.
    pub(super) fn serve_metadata(&mut self,bytes:&[u8],attempt:&str,correlation:&str,cancellation:&AtomicBool,
        check:&mut dyn FnMut()->Result<()>)->Result<()> {
        if !self.valid||!self.ready||!self.claim_started||self.metadata_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        self.metadata_started=true;
        let result=(|| {
            let (mut transfer,announcement)=super::owner_metadata_frames::Transfer::new(bytes,attempt,correlation)?;
            self.send_frame(&announcement,Instant::now()+Duration::from_secs(30),cancellation,check)?;
            while !transfer.complete() {
                let deadline=Instant::now()+Duration::from_secs(30);
                let request=self.read_frame(deadline,cancellation,check)?;
                let reply=transfer.reply(&request)?;
                self.send_frame(&reply,deadline,cancellation,check)?;
            }
            check()?;if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}Ok(())
        })();
        if result.is_err(){self.fail();}result
    }
    /// Single sequential transfer, then one corroborating drain response. This
    /// never commits, releases the reservation or certifies Node descendants.
    pub(super) fn serve_packed(&mut self,control:&super::oex_control_v2::Control,prepared:&super::oex_worker_v2::Prepared,
        output:&File,attempt:&str,correlation:&str,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
        use std::os::unix::fs::{FileExt,MetadataExt};
        #[derive(Serialize)] #[serde(rename_all="camelCase")]
        struct Notice<'a>{version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,control:&'a super::oex_control_v2::Control}
        #[derive(Serialize,Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
        struct Read {version:u32,kind:String,correlation_id:String,attempt_id:String,index:usize,offset:u64,length:usize}
        #[derive(Serialize)] #[serde(rename_all="camelCase")]
        struct Data<'a>{version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,index:usize,offset:u64,data:String}
        #[derive(Serialize,Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
        struct Drain {version:u32,kind:String,correlation_id:String,attempt_id:String}
        #[derive(Serialize)] #[serde(rename_all="camelCase")]
        struct Drained<'a>{version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,status:&'static str}
        let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
        let result=(|| {
            if !self.valid||!self.ready||!self.claim_started||self.metadata_started {return Err(bad());}
            if control.encode()?!=prepared.control || control.attempt_id!=attempt || Uuid::parse_str(correlation).ok().is_none_or(|v|v.to_string()!=correlation){return Err(bad());}
            let before=output.metadata().map_err(|_|bad())?;
            let stamp=|m:&std::fs::Metadata|(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec());
            if stamp(&before)!=prepared.verified.output_stamp || !before.is_file()||before.uid()!=unsafe{libc::geteuid()}||before.mode()&0o777!=0o600||before.nlink()!=1
                || before.dev().to_string()!=control.output_device||before.ino().to_string()!=control.output_inode {return Err(bad());}
            check()?;
            let notice=Notice{version:1,kind:"worker-control",correlation_id:correlation,attempt_id:attempt,control};
            let bytes=serde_json::to_vec(&notice).map_err(|_|bad())?;
            self.send_frame(&bytes,Instant::now()+Duration::from_secs(30),cancellation,check)?;
            self.serve_metadata(&prepared.verified.raw_ack,attempt,correlation,cancellation,check)?;
            for (index,entry) in prepared.verified.entries.iter().enumerate() {
                let mut offset=0u64;
                while offset<entry.bytes {
                    let deadline=Instant::now()+Duration::from_secs(300);
                    let raw=self.read_frame(deadline,cancellation,check)?;
                    let req:Read=serde_json::from_slice(&raw).map_err(|_|bad())?;
                    let length=(entry.bytes-offset).min(4096) as usize;
                    if serde_json::to_vec(&req).map_err(|_|bad())?!=raw || req.version!=1||req.kind!="read-chunk"||req.attempt_id!=attempt
                        || Uuid::parse_str(&req.correlation_id).ok().is_none_or(|v|v.to_string()!=req.correlation_id)
                        || req.index!=index||req.offset!=offset||req.length!=length{return Err(bad());}
                    let after=output.metadata().map_err(|_|bad())?;if stamp(&after)!=stamp(&before){return Err(bad());}
                    let pos=entry.offset.checked_add(offset).ok_or_else(bad)?;let mut data=vec![0u8;length];let mut filled=0;
                    while filled<length {let n=output.read_at(&mut data[filled..],pos+filled as u64).map_err(|_|bad())?;if n==0{return Err(bad());}filled+=n;}
                    let after=output.metadata().map_err(|_|bad())?;if stamp(&after)!=stamp(&before){return Err(bad());}
                    let response=Data{version:1,kind:"chunk-data",correlation_id:&req.correlation_id,attempt_id:attempt,index,offset,data:STANDARD.encode(&data)};
                    self.send_frame(&serde_json::to_vec(&response).map_err(|_|bad())?,deadline,cancellation,check)?;offset+=length as u64;
                }
            }
            let deadline=Instant::now()+Duration::from_secs(300);let raw=self.read_frame(deadline,cancellation,check)?;
            let request:Drain=serde_json::from_slice(&raw).map_err(|_|bad())?;
            if serde_json::to_vec(&request).map_err(|_|bad())?!=raw||request.version!=1||request.kind!="drain-check"||request.attempt_id!=attempt
                || Uuid::parse_str(&request.correlation_id).ok().is_none_or(|v|v.to_string()!=request.correlation_id) {return Err(bad());}
            check()?;let after=output.metadata().map_err(|_|bad())?;if stamp(&after)!=stamp(&before){return Err(bad());}
            let response=Drained{version:1,kind:"drain-result",correlation_id:&request.correlation_id,attempt_id:attempt,status:"drained"};
            self.send_frame(&serde_json::to_vec(&response).map_err(|_|bad())?,deadline,cancellation,check)?;check()
        })();
        if result.is_err(){self.fail();}result
    }
    /// Wait for Node's post-COMMIT one-shot offer. The caller must retain the
    /// live claim and check its local persisted request/draft before any PUT.
    pub(super) fn receive_save_ready(&mut self,attempt:&str,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<super::owner_claim_frames::SaveFrame> {
        if !self.valid||!self.ready||!self.claim_started||!self.metadata_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let result=self.read_frame(Instant::now()+Duration::from_secs(120),cancellation,check)
            .and_then(|bytes|super::owner_claim_frames::SaveFrame::parse_ready(&bytes,attempt)).and_then(|ready|{check()?;Ok(ready)});
        if result.is_err(){self.fail();}result
    }
    /// Only a previously checked actual save/reopen may be reported. A reply
    /// is correlation to Node's verification, never retirement authority.
    pub(super) fn confirm_save(&mut self,ready:&super::owner_claim_frames::SaveFrame,challenge:&super::owner_claim_frames::ClaimChallenge,etag:&str,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>,trace:&mut dyn FnMut(&'static [u8])) -> Result<()> {
        let result=(||{let bytes=ready.observed(etag)?;let deadline=Instant::now()+Duration::from_secs(120);
            self.send_frame(&bytes,deadline,cancellation,check)?;let reply=self.read_frame(deadline,cancellation,check)?;
            trace(b"NATIVE_OEX_NATIVE save-verified-read\n");ready.check_verified(etag,&reply)?;trace(b"NATIVE_OEX_NATIVE save-verified-valid\n");check()?;
            let settlement=self.read_frame(deadline,cancellation,check)?;
            if settlement!=challenge.settlement("settle-check")? {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
            check()?;self.send_frame(&challenge.settlement("settle-ready")?,deadline,cancellation,check)?;
            let receipt=self.read_frame(deadline,cancellation,check)?;
            if receipt!=challenge.settlement("settle-receipt")? {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
            check()?;trace(b"NATIVE_OEX_NATIVE settlement-valid\n");
            let completed=self.read_frame(deadline,cancellation,check)?;trace(b"NATIVE_OEX_NATIVE operation-complete-read\n");ready.check_completed(etag,&completed)?;trace(b"NATIVE_OEX_NATIVE operation-complete-valid\n");check()})();
        if result.is_err(){self.fail();}result
    }
    /// While the owned child runs, the protocol permits no unsolicited
    /// inbound bytes. EOF, error, or unexpected data makes the operation
    /// uncertain; EAGAIN is only a momentary observation, not authority.
    pub(super) fn check_idle(&mut self)->Result<()> {
        if !self.valid||!self.ready {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let mut byte=0u8;
        for _ in 0..4 {
            let n=unsafe {libc::recv(self.socket.as_raw_fd(),(&mut byte as *mut u8).cast(),1,libc::MSG_PEEK|libc::MSG_DONTWAIT)};
            if n>=0 {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
            let error=std::io::Error::last_os_error();
            if error.kind()==std::io::ErrorKind::Interrupted {continue;}
            if error.kind()==std::io::ErrorKind::WouldBlock {return Ok(());}
            self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }
        self.fail();Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))
    }

    /// One same-socket attempt only. Even a correlated reply grants no
    /// authority or permission to release a claim; API keeps its live fence.
    pub(super) fn begin_claim(&mut self,open:&super::owner_claim_frames::ClaimOpen,principal:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str,
        stage_path:&Path,stage_device:u64,other_stage_bytes:u64,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)
        -> Result<(super::owner_claim_frames::ClaimChallenge,super::owner_claim_frames::ClaimReady,u64)> {
        if !self.valid||!self.ready||self.claim_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        self.claim_started=true;
        let result=(|| {
            let request=open.encode()?;
            let bytes=self.exchange(&request,cancellation,check)?;
            let challenge=super::owner_claim_frames::ClaimChallenge::parse(&bytes,open,principal)?;
            // Provider has issued an actual receipt but has not bound it yet.
            let ready_bytes=self.read_frame(Instant::now()+Duration::from_secs(30),cancellation,check)?;
            let notice:super::owner_claim_frames::ReceiptReady=serde_json::from_slice(&ready_bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
            if serde_json::to_vec(&notice).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=ready_bytes
                || notice.version!=1||notice.kind!="receipt-ready"||notice.correlation_id!=challenge.correlation_id
                || notice.attempt_id!=challenge.attempt_id||notice.sha256!=challenge.receipt_digest||!(1..=32768).contains(&notice.bytes){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
            let mut receipt=Vec::with_capacity(notice.bytes);
            while receipt.len()<notice.bytes {
                let offset=receipt.len();let length=4096.min(notice.bytes-offset);let correlation=Uuid::new_v4().to_string();
                let req=super::owner_claim_frames::ReadReceipt{version:1,kind:"read-receipt",correlation_id:&correlation,
                    attempt_id:&challenge.attempt_id,offset,length};
                let response=self.exchange(&serde_json::to_vec(&req).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?,cancellation,check)?;
                let chunk:super::owner_claim_frames::ReceiptChunk=serde_json::from_slice(&response).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
                if serde_json::to_vec(&chunk).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=response
                    || chunk.version!=1||chunk.kind!="receipt-chunk"||chunk.correlation_id!=correlation||chunk.attempt_id!=challenge.attempt_id||chunk.offset!=offset {
                    return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
                }
                let decoded=STANDARD.decode(&chunk.data).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
                if decoded.len()!=length||STANDARD.encode(&decoded)!=chunk.data{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}receipt.extend_from_slice(&decoded);
            }
            let capacity=super::oex_reservation::stage_capacity(&receipt,&challenge.receipt_digest,&challenge.reservation_id,
                &challenge.reservation_generation,stage_path,stage_device,expected_bytes,other_stage_bytes)?;
            check()?;if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            let fields=super::owner_binding_encoding::Fields {workspace:&challenge.workspace_id,tenant:&challenge.tenant_id,
                principal:&challenge.principal_id,runtime_generation:&challenge.runtime_generation,job:&challenge.job_id,
                attempt:&challenge.attempt_id,generation:&challenge.generation,stage:&challenge.stage_id,
                reservation:&challenge.reservation_id,reservation_generation:&challenge.reservation_generation,
                claim_digest:&challenge.claim_digest,receipt_digest:&challenge.receipt_digest};
            let mac=super::owner_binding_encoding::correlation_mac(&self.key,&fields)?;
            check()?;
            let encoded=mac.iter().map(|v|format!("{v:02x}")).collect::<String>();
            let binding=challenge.binding(&encoded)?;
            let reply=self.exchange(&binding,cancellation,check)?;
            let ready=super::owner_claim_frames::ClaimReady::parse(&reply,&challenge,&encoded,expected_request,expected_bytes,expected_sha256)?;
            check()?;
            if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            Ok((challenge,ready,capacity))
        })();
        if result.is_err(){self.fail();}result
    }
}

fn progress(deadline:Instant, cancellation:&AtomicBool) -> Result<()> {
    if cancellation.load(Ordering::SeqCst) || super::cancelled() { return Err(err("NATIVE_CANCELLED")); }
    if Instant::now()>=deadline { return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
    Ok(())
}
fn write_bounded(stream:&mut UnixStream, bytes:&[u8], deadline:Instant, cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
    let mut offset=0;
    while offset<bytes.len() {
        progress(deadline,cancellation)?;check()?;
        match stream.write(&bytes[offset..]) {
            Ok(0)=>return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")),
            Ok(n)=>offset+=n,
            Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>{},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(10)),
            Err(_)=>return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")),
        }
    }
    progress(deadline,cancellation)?;check()
}
fn read_bounded(stream:&mut UnixStream, bytes:&mut [u8], deadline:Instant, cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
    let mut offset=0;
    while offset<bytes.len() {
        progress(deadline,cancellation)?;check()?;
        match stream.read(&mut bytes[offset..]) {
            Ok(0)=>return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")),
            Ok(n)=>offset+=n,
            Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>{},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(10)),
            Err(_)=>return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")),
        }
    }
    progress(deadline,cancellation)?;check()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_requires_same_attempt_receipt_before_completion() {
        use super::super::owner_claim_frames::{ClaimChallenge,SaveFrame};
        fn read_frame(s:&mut UnixStream)->Vec<u8>{let mut h=[0;4];s.read_exact(&mut h).unwrap();let mut b=vec![0;u32::from_be_bytes(h) as usize];s.read_exact(&mut b).unwrap();b}
        fn send(s:&mut UnixStream,b:&[u8]){s.write_all(&(b.len() as u32).to_be_bytes()).unwrap();s.write_all(b).unwrap();}
        for wrong in [false,true] {
            let id=||Uuid::new_v4().to_string();let attempt=id();
            let ready=SaveFrame{version:1,kind:"save-ready".into(),correlation_id:id(),attempt_id:attempt.clone(),exhibition_id:id(),etag:format!("\"studio-r1-{}\"","a".repeat(64)),request_id:id()};
            let challenge=ClaimChallenge{version:1,kind:"claim-challenge".into(),correlation_id:id(),workspace_id:id(),tenant_id:id(),principal_id:id(),runtime_generation:id(),job_id:id(),attempt_id:attempt,generation:id(),stage_id:id(),reservation_id:id(),reservation_generation:id(),claim_digest:"c".repeat(64),receipt_digest:"d".repeat(64)};
            let next=format!("\"studio-r2-{}\"","b".repeat(64));let peer_ready=ready.clone();let peer_challenge=challenge.clone();let peer_etag=next.clone();
            let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
            let worker=std::thread::spawn(move || {
                assert_eq!(read_frame(&mut peer),peer_ready.observed(&peer_etag).unwrap());
                let mut verified=peer_ready.clone();verified.kind="save-verified".into();verified.etag=peer_etag.clone();send(&mut peer,&serde_json::to_vec(&verified).unwrap());
                send(&mut peer,&peer_challenge.settlement("settle-check").unwrap());
                assert_eq!(read_frame(&mut peer),peer_challenge.settlement("settle-ready").unwrap());
                let mut response=peer_challenge.clone();if wrong {response.reservation_generation=Uuid::new_v4().to_string();}
                send(&mut peer,&response.settlement("settle-receipt").unwrap());
                if !wrong {verified.kind="operation-complete".into();send(&mut peer,&serde_json::to_vec(&verified).unwrap());}
            });
            let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,claim_started:true,metadata_started:true};
            let result=channel.confirm_save(&ready,&challenge,&next,&AtomicBool::new(false),&mut ||Ok(()),&mut |_|{});
            assert_eq!(result.is_ok(),!wrong);assert_eq!(channel.valid,!wrong);worker.join().unwrap();
        }
    }
    fn authority() -> LocalAuthority {
        LocalAuthority {format:1,workspace_id:Uuid::new_v4().to_string(),tenant_id:Uuid::new_v4().to_string(),principal_id:Uuid::new_v4().to_string(),owner_uid:unsafe {libc::geteuid()},session_token:"synthetic-session".into(),csrf_token:"synthetic-csrf".into()}
    }
    fn peer(setup:&Setup, wrong:bool) -> std::thread::JoinHandle<()> {
        let fd=unsafe {libc::fcntl(setup.child.as_ref().unwrap().as_raw_fd(),libc::F_DUPFD_CLOEXEC,10)};assert!(fd>=0);
        std::thread::spawn(move || {
            let mut socket=unsafe {UnixStream::from_raw_fd(fd)};let mut len=[0u8;4];socket.read_exact(&mut len).unwrap();
            let n=u32::from_be_bytes(len) as usize;assert!((1..=LIMIT).contains(&n));let mut data=vec![0u8;n];socket.read_exact(&mut data).unwrap();
            let hello:Value=serde_json::from_slice(&data).unwrap();data.fill(0);
            let ack=serde_json::json!({"version":1,"kind":"hello-ack","workspaceId":hello["workspaceId"],"tenantId":hello["tenantId"],"principalId":hello["principalId"],"runtimeGeneration":if wrong {Uuid::new_v4().to_string()} else {hello["runtimeGeneration"].as_str().unwrap().to_string()}});
            let encoded=serde_json::to_vec(&ack).unwrap();socket.write_all(&(encoded.len() as u32).to_be_bytes()).unwrap();socket.write_all(&encoded).unwrap();
        })
    }
    #[test]
    fn fresh_bootstrap_is_closed_v2_and_normal_bootstrap_stays_v1() {
        let auth=authority();let generation=Uuid::new_v4().to_string();let setup=Setup::new().unwrap();
        let mut fresh=setup.fresh_bootstrap(&auth,&generation).unwrap();
        let parsed:Value=serde_json::from_slice(&fresh).unwrap();fresh.fill(0);
        let envelope=parsed.as_object().unwrap();assert_eq!(envelope.len(),2);
        let binding=parsed["ownerChannel"].as_object().unwrap();assert_eq!(binding.len(),4);
        assert_eq!(binding["version"],2);assert_eq!(binding["runtimeGeneration"],generation);
        assert_eq!(binding["mode"],"fresh-oex-test-v1");assert_eq!(binding["secret"].as_str().unwrap().len(),64);
        let mut normal=setup.bootstrap(&auth,&generation).unwrap();let parsed:Value=serde_json::from_slice(&normal).unwrap();normal.fill(0);
        let binding=parsed["ownerChannel"].as_object().unwrap();assert_eq!(binding.len(),3);
        assert_eq!(binding["version"],1);assert!(!binding.contains_key("mode"));
    }
    #[test]
    fn selected_normal_bootstrap_has_exact_v2_identity_and_mode() {
        let auth=authority();let generation=Uuid::new_v4().to_string();let setup=Setup::new().unwrap();
        let mut encoded=setup.normal_bootstrap(&auth,&generation).unwrap();
        let parsed:Value=serde_json::from_slice(&encoded).unwrap();encoded.fill(0);
        let envelope=parsed.as_object().unwrap();assert_eq!(envelope.len(),2);
        assert_eq!(parsed["workspace"]["workspaceId"],auth.workspace_id);
        assert_eq!(parsed["workspace"]["tenantId"],auth.tenant_id);
        assert_eq!(parsed["workspace"]["principalId"],auth.principal_id);
        let binding=parsed["ownerChannel"].as_object().unwrap();assert_eq!(binding.len(),4);
        assert_eq!(binding["version"],2);assert_eq!(binding["runtimeGeneration"],generation);
        assert_eq!(binding["mode"],"normal-oex-v1");assert_eq!(binding["secret"].as_str().unwrap().len(),64);
    }
    #[test]
    fn inherited_socket_ack_requires_exact_session_generation() {
        let auth=authority();let generation=Uuid::new_v4().to_string();let mut setup=Setup::new().unwrap();
        let mut bootstrap=setup.bootstrap(&auth,&generation).unwrap();let value:Value=serde_json::from_slice(&bootstrap).unwrap();bootstrap.fill(0);
        assert_eq!(value["workspace"]["workspaceId"],auth.workspace_id);assert_eq!(value["ownerChannel"]["runtimeGeneration"],generation);
        let cancellation=AtomicBool::new(false);
        let worker=peer(&setup,false);setup.close_child();let stream=setup.handshake(&auth,&generation,&cancellation).unwrap();worker.join().unwrap();drop(stream);
        let mut setup=Setup::new().unwrap();let worker=peer(&setup,true);setup.close_child();assert!(setup.handshake(&auth,&generation,&cancellation).is_err());worker.join().unwrap();
        let mut setup=Setup::new().unwrap();setup.close_child();assert!(setup.handshake(&auth,&generation,&cancellation).is_err());
        let mut setup=Setup::new().unwrap();setup.close_child();let cancelled=AtomicBool::new(true);assert!(setup.handshake(&auth,&generation,&cancelled).is_err());
    }
    #[test]
    fn packed_stream_sends_drain_after_sequential_chunk() {
        use std::os::unix::fs::{MetadataExt,OpenOptionsExt,PermissionsExt};
        fn read_frame(s:&mut UnixStream)->Vec<u8>{let mut h=[0;4];s.read_exact(&mut h).unwrap();let mut b=vec![0;u32::from_be_bytes(h) as usize];s.read_exact(&mut b).unwrap();b}
        let dir=std::env::temp_dir().join(format!("oex-packed-stream-{}",Uuid::new_v4()));fs::create_dir(&dir).unwrap();fs::set_permissions(&dir,fs::Permissions::from_mode(0o700)).unwrap();
        let mut output=std::fs::OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(dir.join("out")).unwrap();output.write_all(b"abc").unwrap();output.sync_all().unwrap();let m=output.metadata().unwrap();
        let id=||Uuid::new_v4().to_string();let attempt=id();let operation=id();let sha=digest(b"abc");
        let control=super::super::oex_control_v2::Control{version:2,kind:"oex-packed-v2".into(),tenant_id:id(),resource_id:id(),job_id:id(),user_id:id(),request_id:id(),lease_id:id(),attempt_id:attempt.clone(),runtime_generation:id(),stage_id:id(),reservation_id:id(),reservation_generation:id(),archive_bytes:3,archive_sha256:sha.clone(),capacity:3,input_device:m.dev().to_string(),input_inode:(m.ino()+1).to_string(),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),reservation_digest:sha.clone(),source_digest:sha.clone(),worker_digest:sha.clone(),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
        let verified=super::super::oex_ack_v2::Verified{raw_ack:b"{}".to_vec(),manifest:vec![],exhibition:vec![],entries:vec![super::super::oex_ack_v2::Entry{path:"assets/a".into(),offset:0,bytes:3,sha256:sha.clone()}],manifest_sha256:sha.clone(),exhibition_sha256:sha,output_stamp:(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())};
        let prepared=super::super::oex_worker_v2::Prepared::test_only(control.encode().unwrap(),verified);let peer_attempt=attempt.clone();let peer_operation=operation.clone();
        let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        let worker=std::thread::spawn(move|| {
            let notice:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();assert_eq!(notice["kind"],"worker-control");assert_eq!(notice["correlationId"],peer_operation);
            let ready:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();assert_eq!(ready["kind"],"metadata-ready");
            let correlation=Uuid::new_v4().to_string();let request=format!("{{\"version\":1,\"kind\":\"read-metadata\",\"correlationId\":\"{}\",\"attemptId\":\"{}\",\"offset\":0,\"length\":2}}",correlation,peer_attempt);
            peer.write_all(&(request.len() as u32).to_be_bytes()).unwrap();peer.write_all(request.as_bytes()).unwrap();
            let _:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();
            let correlation=Uuid::new_v4().to_string();
            // Build ordered frame explicitly: serde_json::Value maps may sort keys.
            let request=format!("{{\"version\":1,\"kind\":\"read-chunk\",\"correlationId\":\"{}\",\"attemptId\":\"{}\",\"index\":0,\"offset\":0,\"length\":3}}",correlation,peer_attempt);
            peer.write_all(&(request.len() as u32).to_be_bytes()).unwrap();peer.write_all(request.as_bytes()).unwrap();
            let data:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();assert_eq!(data["data"],"YWJj");
            let correlation=Uuid::new_v4().to_string();let request=format!("{{\"version\":1,\"kind\":\"drain-check\",\"correlationId\":\"{}\",\"attemptId\":\"{}\"}}",correlation,peer_attempt);
            peer.write_all(&(request.len() as u32).to_be_bytes()).unwrap();peer.write_all(request.as_bytes()).unwrap();
            let done:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();assert_eq!(done["status"],"drained");
        });
        let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,claim_started:true,metadata_started:false};
        channel.serve_packed(&control,&prepared,&output,&attempt,&operation,&AtomicBool::new(false),&mut ||Ok(())).unwrap();worker.join().unwrap();
        drop(channel);drop(output);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn single_connection_frames_and_eof_are_terminal() {
        let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        let auth=authority();let generation=Uuid::new_v4().to_string();
        // Construct the canonical wire order explicitly.
        let ready=format!("{{\"version\":1,\"kind\":\"operation-ready\",\"workspaceId\":\"{}\",\"tenantId\":\"{}\",\"principalId\":\"{}\",\"runtimeGeneration\":\"{}\"}}",auth.workspace_id,auth.tenant_id,auth.principal_id,generation).into_bytes();
        let worker=std::thread::spawn(move || {
            peer.write_all(&(ready.len() as u32).to_be_bytes()).unwrap();peer.write_all(&ready).unwrap();
            let mut header=[0u8;4];peer.read_exact(&mut header).unwrap();let n=u32::from_be_bytes(header) as usize;
            let mut request=vec![0u8;n];peer.read_exact(&mut request).unwrap();assert_eq!(request,b"abc");
            peer.write_all(&2u32.to_be_bytes()).unwrap();peer.write_all(b"ok").unwrap();
        });
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:false,claim_started:false,metadata_started:false};let cancel=AtomicBool::new(false);
        connection.receive_ready(&cancel,&mut ||Ok(()),&auth.workspace_id,&auth.tenant_id,&auth.principal_id,&generation).unwrap();
        assert_eq!(connection.exchange(b"abc",&cancel,&mut ||Ok(())).unwrap(),b"ok");worker.join().unwrap();
        assert!(connection.exchange(b"abc",&cancel,&mut ||Ok(())).is_err());assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
        assert!(connection.exchange(b"abc",&cancel,&mut ||Ok(())).is_err());
    }
    #[test]
    fn mismatched_ready_invalidates_and_clears_key() {
        let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        peer.write_all(&2u32.to_be_bytes()).unwrap();peer.write_all(b"{}").unwrap();
        let auth=authority();let generation=Uuid::new_v4().to_string();
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:false,claim_started:false,metadata_started:false};
        assert!(connection.receive_ready(&AtomicBool::new(false),&mut ||Ok(()),&auth.workspace_id,&auth.tenant_id,&auth.principal_id,&generation).is_err());
        assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
    }
    #[test]
    fn idle_socket_detects_eof_while_peer_handle_still_exists() {
        let (parent,peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:true,claim_started:true,metadata_started:false};
        assert!(connection.check_idle().is_ok());
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(connection.check_idle().is_err());assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
        drop(peer);
    }
    #[cfg(target_os="macos")]
    #[test]
    fn claim_is_one_shot_and_bound_to_expected_request() {
        use super::super::owner_claim_frames::{ClaimChallenge,ClaimOpen,ClaimReady,ReceiptReady,ReceiptChunk};
        fn id(n:usize)->String {format!("10000000-0000-4000-8000-{n:012}")}
        fn read_frame(stream:&mut UnixStream)->Vec<u8> {let mut h=[0;4];stream.read_exact(&mut h).unwrap();let mut b=vec![0;u32::from_be_bytes(h) as usize];stream.read_exact(&mut b).unwrap();b}
        fn send(stream:&mut UnixStream,b:&[u8]) {stream.write_all(&(b.len() as u32).to_be_bytes()).unwrap();stream.write_all(b).unwrap();}
        let open=ClaimOpen{version:1,kind:"claim-open".into(),correlation_id:Uuid::new_v4().to_string(),
            workspace_id:id(1),tenant_id:id(2),runtime_generation:id(4),job_id:id(5)};
        let receipt=super::super::oex_reservation::tests::fixture(&id(9),&id(10),"/private/stage",false);
        let challenge=ClaimChallenge{version:1,kind:"claim-challenge".into(),correlation_id:open.correlation_id.clone(),
            workspace_id:id(1),tenant_id:id(2),principal_id:id(3),runtime_generation:id(4),job_id:id(5),attempt_id:id(6),
            generation:id(7),stage_id:id(8),reservation_id:id(9),reservation_generation:id(10),
            claim_digest:"202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f".into(),
            receipt_digest:digest(&receipt)};
        let request=Uuid::new_v4().to_string();let request_for_peer=request.clone();let archive="d".repeat(64);
        let archive_for_peer=archive.clone();let peer_open=open.clone();
        let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        let worker=std::thread::spawn(move || {
            assert_eq!(read_frame(&mut peer),peer_open.encode().unwrap());
            send(&mut peer,&serde_json::to_vec(&challenge).unwrap());
            let ready=ReceiptReady{version:1,kind:"receipt-ready".into(),correlation_id:challenge.correlation_id.clone(),attempt_id:id(6),bytes:receipt.len(),sha256:challenge.receipt_digest.clone()};
            send(&mut peer,&serde_json::to_vec(&ready).unwrap());
            let read:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();
            assert_eq!(read["offset"],0);assert_eq!(read["length"],receipt.len());
            let chunk=ReceiptChunk{version:1,kind:"receipt-chunk".into(),correlation_id:read["correlationId"].as_str().unwrap().into(),attempt_id:id(6),offset:0,data:base64::Engine::encode(&base64::engine::general_purpose::STANDARD,&receipt)};
            send(&mut peer,&serde_json::to_vec(&chunk).unwrap());
            let binding:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();
            let fields=super::super::owner_binding_encoding::Fields{workspace:&challenge.workspace_id,tenant:&challenge.tenant_id,principal:&challenge.principal_id,runtime_generation:&challenge.runtime_generation,job:&challenge.job_id,attempt:&challenge.attempt_id,generation:&challenge.generation,stage:&challenge.stage_id,reservation:&challenge.reservation_id,reservation_generation:&challenge.reservation_generation,claim_digest:&challenge.claim_digest,receipt_digest:&challenge.receipt_digest};
            let key=std::array::from_fn(|i|i as u8);
            let mac=super::super::owner_binding_encoding::correlation_mac(&key,&fields).unwrap().iter().map(|v|format!("{v:02x}")).collect::<String>();
            assert_eq!(binding["managerBindingDigest"],mac);
            let ready=ClaimReady{version:1,kind:"claim-ready".into(),correlation_id:challenge.correlation_id.clone(),
                runtime_generation:id(4),workspace_id:id(1),tenant_id:id(2),job_id:id(5),user_id:id(3),request_id:request_for_peer,
                lease_id:Uuid::new_v4().to_string(),attempt_id:id(6),generation:id(7),stage_id:id(8),reservation_id:id(9),reservation_generation:id(10),
                archive_bytes:67_108_865,archive_sha256:archive_for_peer.clone(),source_digest:archive_for_peer,
                reservation_digest:challenge.receipt_digest.clone(),manager_binding_digest:mac,profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
            send(&mut peer,&serde_json::to_vec(&ready).unwrap());
        });
        let mut connection=Connection{socket:parent,key:std::array::from_fn(|i|i as u8),valid:true,ready:true,claim_started:false,metadata_started:false};
        let cancel=AtomicBool::new(false);
        let (_,ready,capacity)=connection.begin_claim(&open,&id(3),&request,67_108_865,&archive,Path::new("/private/stage"),1,0,&cancel,&mut ||Ok(())).unwrap();
        assert_eq!(capacity,532_891_135);
        assert_eq!(ready.request_id,request);worker.join().unwrap();
        assert!(connection.begin_claim(&open,&id(3),&request,67_108_865,&archive,Path::new("/private/stage"),1,0,&cancel,&mut ||Ok(())).is_err());
        assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
    }
}
