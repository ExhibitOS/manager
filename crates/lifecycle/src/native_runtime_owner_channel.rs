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

#[path = "native_runtime_hayes_owner_bootstrap.rs"]
mod hayes_bootstrap;
use hayes_bootstrap::HayesSetup;
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
pub(super) use hayes_bootstrap::AuthenticatedHayes;
pub(super) use hayes_bootstrap::HayesSessionPin;

/// Holds both original endpoints until spawn. Only the duplicated child
/// descriptor is made inheritable in the forked API child.
pub(super) struct Setup {
    parent: UnixStream,
    child: Option<File>,
    secret: String,
    furnishing_writer: Option<super::furnishing_lifetime::ProbedWriter>,
    hayes: Option<HayesSetup>,
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    probe_source: Option<super::hayes_probe_source::ProbeSourceFd>,
}
/// Intended to stay inside the owning Session; this type does not enforce
/// that lifetime. It is not a claim, readiness, fence or authorization; loss
/// of the socket is terminal once observed by the transport.
pub(super) struct Connection { socket: UnixStream, key: [u8;32], valid: bool, ready: bool, furnishing_bound: Option<super::FurnishingStart>, ordinary: Option<super::furnishing_lifecycle::Exchange>, claim_started: bool, metadata_started: bool }
/// Keep the session connection only after the exact completion was checked.
pub(super) fn finish_v3_claim(channel: &mut Option<Connection>, completed: bool) {
    if !completed && let Some(mut connection) = channel.take() { connection.invalidate(); }
}

pub(super) struct PackedServe<'a> {
    pub control: &'a super::oex_control_v2::Control,
    pub prepared: &'a super::oex_worker_v2::Prepared,
    pub output: &'a File,
}

#[derive(Clone, Copy)]
pub(super) struct SelectedArchive<'a> {
    pub request_id: &'a str,
    pub bytes: u64,
    pub sha256: &'a str,
    pub format: Option<&'a str>,
}
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
        Ok(Self {parent,child:Some(child),secret,furnishing_writer:None,
            hayes: None,
            #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
            probe_source: None,
        })
    }
    pub(super) fn with_furnishing_writer(mut self, file: super::furnishing_lifetime::ProbedWriter) -> Self {
        self.furnishing_writer = Some(file);
        self
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
    pub(super) fn normal_027_bootstrap(&self, auth:&LocalAuthority, generation:&str) -> Result<Vec<u8>> {
        let value=FreshOwnerBootstrap {workspace:auth,owner_channel:FreshOwnerBinding {version:3,runtime_generation:generation,secret:&self.secret,mode:"normal-oex-027-v3"}};
        let mut bytes=serde_json::to_vec(&value).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.len()>4096 { bytes.fill(0);return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
        Ok(bytes)
    }
    pub(super) fn normal_028_bootstrap(&self, auth:&LocalAuthority, generation:&str) -> Result<Vec<u8>> {
        self.normal_selected_v3_bootstrap(auth, generation, "normal-oex-028-v4")
    }
    pub(super) fn workspace_v5_bootstrap(&self, auth: &LocalAuthority, generation: &str) -> Result<Vec<u8>> {
        self.normal_selected_v3_bootstrap(auth, generation, "normal-oex-workspace-v5")
    }
    pub(super) fn workspace_import_bootstrap(&self, auth: &LocalAuthority, generation: &str, version: crate::native_payload::WorkspaceImport) -> Result<Vec<u8>> {
        self.normal_selected_v3_bootstrap(auth, generation, super::oex_catalog_frames::Protocol::from(version).owner_mode())
    }
    pub(super) fn workspace_furnishing_bootstrap(&self, auth: &LocalAuthority, generation: &str, start: super::FurnishingStart) -> Result<Vec<u8>> {
        if let Some(hayes) = &self.hayes {
            return hayes.bootstrap(auth, generation, start, &self.secret);
        }
        if matches!(start, super::FurnishingStart::Upgrade040To044 | super::FurnishingStart::Upgrade044To045) {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        self.normal_selected_v3_bootstrap(auth, generation, start.owner_mode())
    }
    fn normal_selected_v3_bootstrap(&self, auth: &LocalAuthority, generation: &str, mode: &'static str) -> Result<Vec<u8>> {
        let value=FreshOwnerBootstrap {workspace:auth,owner_channel:FreshOwnerBinding {version:3,runtime_generation:generation,secret:&self.secret,mode}};
        let bytes=serde_json::to_vec(&value).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        if bytes.len()>4096 {return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));} Ok(bytes)
    }
    pub(super) fn with_hayes(mut self, pin: HayesSessionPin) -> Result<Self> {
        if self.hayes.is_some() || self.furnishing_writer.is_none() {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        self.hayes = Some(HayesSetup::new(pin)?);
        Ok(self)
    }

    pub(super) fn take_hayes(&mut self) -> Option<HayesSetup> {
        self.hayes.take()
    }

    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(super) fn with_hayes_probe_source(mut self, source: super::hayes_probe_source::ProbeSourceFd) -> Result<Self> {
        if self.hayes.is_none() || self.furnishing_writer.is_none() || self.probe_source.is_some() {
            return Err(err("HAYES_PROBE_SOURCE_INVALID"));
        }
        self.probe_source = Some(source);
        Ok(self)
    }

    pub(super) fn configure(&self, command:&mut Command) -> Result<()> {
        let fd=self.child.as_ref().ok_or_else(||err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.as_raw_fd();
        command.env("EXHIBITOS_NATIVE_OWNER_FD",fd.to_string());
        let writer = self.furnishing_writer.as_ref().map(AsRawFd::as_raw_fd);
        if let Some(writer) = writer {
            use super::furnishing_lifetime::{FD_MIN, FD_MAX};
            if writer == fd || !(FD_MIN..=FD_MAX).contains(&writer) {
                return Err(err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"));
            }
            command.env("EXHIBITOS_NATIVE_FURNISHING_WRITER_FD", writer.to_string());
        }
        let hayes_fd = self.hayes.as_ref().map(HayesSetup::child_fd).transpose()?;
        if let Some(hayes_fd) = hayes_fd {
            if hayes_fd == fd || writer == Some(hayes_fd) {
                return Err(err("HAYES_OPERATION_UNAVAILABLE"));
            }
            command.env("EXHIBITOS_NATIVE_HAYES_OWNER_FD", hayes_fd.to_string());
        }
        #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let probe_source = self.probe_source.as_ref().map(super::hayes_probe_source::ProbeSourceFd::child_fd).transpose()?;
        #[cfg(not(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let probe_source: Option<i32> = None;
        if let Some(source) = probe_source {
            if hayes_fd.is_none() || source == fd || writer == Some(source) || hayes_fd == Some(source) {
                return Err(err("HAYES_PROBE_SOURCE_INVALID"));
            }
            command.env("EXHIBITOS_TEST_HAYES_SOURCE_FD", source.to_string());
        }
        unsafe { command.pre_exec(move || {
            for descriptor in std::iter::once(fd).chain(writer).chain(hayes_fd).chain(probe_source) {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags < 0 || libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        }); }
        Ok(())
    }
    pub(super) fn close_child(&mut self) {
        self.child.take();
        self.furnishing_writer.take();
        #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        self.probe_source.take();
        if let Some(hayes) = self.hayes.as_mut() {
            hayes.close_child();
        }
    }
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
        self.authenticated_connection()
    }

    fn authenticated_connection(&self) -> Result<Connection> {
        let socket=self.parent.try_clone().map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        let mut key=[0u8;32];
        for (i,pair) in self.secret.as_bytes().chunks_exact(2).enumerate() {
            let digit=|v:u8|if v<=b'9'{v-b'0'}else{v-b'a'+10};
            key[i]=(digit(pair[0])<<4)|digit(pair[1]);
        }
        Ok(Connection {socket,key,valid:true,ready:false,furnishing_bound: None, ordinary: None,claim_started:false,metadata_started:false})
    }
}

#[allow(dead_code)]
impl Connection {
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(super) fn hayes_fixture(socket: UnixStream) -> Result<Self> {
        socket.set_nonblocking(true).map_err(|_| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
        Ok(Self {
            socket, key: [0; 32], valid: true, ready: true, furnishing_bound: None,
            ordinary: None, claim_started: false, metadata_started: false,
        })
    }

    /// One dedicated Hayes exchange on the inherited, authenticated Session
    /// channel. No legacy or furnishing claim can share this exchange.
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(super) fn claim_hayes(&mut self) -> Result<()> {
        if !self.valid || self.claim_started || self.metadata_started
            || self.furnishing_bound.is_some() || self.ordinary.is_some()
        {
            self.fail();
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        self.claim_started = true;
        Ok(())
    }

    /// During private worker execution the API sends no data. EOF or any
    /// premature input cancels the operation without consuming a frame.
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(super) fn check_hayes_idle(&self) -> Result<()> {
        if !self.valid || !self.claim_started || self.metadata_started
            || self.furnishing_bound.is_some() || self.ordinary.is_some()
        {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        self.check_hayes_peer_idle()
    }

    pub(super) fn check_hayes_peer_idle(&self) -> Result<()> {
        if !self.valid { return Err(err("HAYES_OPERATION_UNAVAILABLE")); }
        let mut byte = 0u8;
        let received = unsafe {
            libc::recv(self.socket.as_raw_fd(), (&mut byte as *mut u8).cast(), 1,
                libc::MSG_DONTWAIT | libc::MSG_PEEK)
        };
        if received < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
            return Ok(());
        }
        Err(err("HAYES_OPERATION_UNAVAILABLE"))
    }

    // No MAC accessor yet: cached `valid` alone cannot prove the owned peer or
    // fence is live. A typed, single-use Session-bound challenge is required.
    fn fail(&mut self) {self.valid=false;self.key.fill(0);let _=self.socket.shutdown(std::net::Shutdown::Both);}
    pub(super) fn read_frame(&mut self, deadline:Instant, cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)->Result<Vec<u8>> {
        self.read_frame_limited(8192, deadline, cancellation, check)
    }
    fn read_frame_limited(&mut self, limit: usize, deadline: Instant, cancellation: &AtomicBool,
                          check: &mut dyn FnMut() -> Result<()>) -> Result<Vec<u8>> {
        let mut len=[0u8;4];read_bounded(&mut self.socket,&mut len,deadline,cancellation,check)?;
        let n=u32::from_be_bytes(len) as usize;if n==0||n>limit{return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let mut bytes=vec![0u8;n];read_bounded(&mut self.socket,&mut bytes,deadline,cancellation,check)?;Ok(bytes)
    }
    pub(super) fn bind_furnishing_lease(&mut self, lease: &super::furnishing_lease::Lease,
        cancellation: &AtomicBool, check: &mut dyn FnMut() -> Result<()>) -> Result<()> {
        use super::furnishing_lease::{Kind, LIMIT};
        if !self.valid || self.ready || self.furnishing_bound.is_some() {
            self.fail();
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = (|| {
            let wire = lease.signed(&self.key, Kind::Lease)?;
            self.send_frame(&wire, deadline, cancellation, check)?;
            let reply = self.read_frame_limited(LIMIT, deadline, cancellation, check)?;
            lease.check_ack(&self.key, &reply)?;
            check()?;
            self.check_authenticated_idle()
        })();
        if result.is_ok() { self.furnishing_bound = Some(lease.start); } else { self.fail(); }
        result
    }

    pub(super) fn bind_ordinary_furnishing(&mut self, lease: &super::furnishing_lease::Lease,
        binding: &super::furnishing_lifecycle::Binding, cancellation: &AtomicBool,
        check: &mut dyn FnMut() -> Result<()>) -> Result<()> {
        use super::furnishing_lifecycle::{Exchange, Kind, LIMIT};
        self.bind_furnishing_lease(lease, cancellation, check)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = (|| {
            let exchange = Exchange::new(binding, &lease.signed(&self.key, super::furnishing_lease::Kind::Lease)?)?;
            self.send_frame(&exchange.signed(&self.key, Kind::Admission)?, deadline, cancellation, check)?;
            let reply = self.read_frame_limited(LIMIT, deadline, cancellation, check)?;
            exchange.check_frame(&self.key, Kind::AdmissionAck, &reply)?;
            check()?;
            // The selected peer sends its catalog proof after admission.
            if !lease.start.is_selected_import() {
                self.check_authenticated_idle()?;
            }
            Ok(exchange)
        })();
        match result {
            Ok(exchange) => { self.ordinary = Some(exchange); Ok(()) }
            Err(error) => { self.fail(); Err(error) }
        }
    }

    pub(super) fn drain_ordinary_furnishing(&mut self, deadline: Instant, cancellation: &AtomicBool,
        check: &mut dyn FnMut() -> Result<()>) -> Result<()> {
        use super::furnishing_lifecycle::{Kind, LIMIT};
        let result = (|| {
            let exchange = self.ordinary.take().ok_or_else(|| err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?;
            self.check_furnishing_idle()?;
            self.send_frame(&exchange.signed(&self.key, Kind::Stop)?, deadline, cancellation, check)?;
            let reply = self.read_frame_limited(LIMIT, deadline, cancellation, check)?;
            exchange.check_frame(&self.key, Kind::Drained, &reply)?;
            check()
        })();
        if result.is_err() { self.fail(); }
        result
    }

    pub(super) fn check_furnishing_idle(&mut self) -> Result<()> {
        if self.furnishing_bound.is_none() {
            self.fail();
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }
        self.check_authenticated_idle()
    }
    /// Accept exactly one canonical readiness frame tied to the supplied live
    /// personal identity. Any mismatch or transport error closes permanently.
    pub(super) fn receive_ready(&mut self,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>,
        workspace:&str,tenant:&str,principal:&str,generation:&str)->Result<()> {
        let selected_furnishing = self.furnishing_bound.is_some_and(super::FurnishingStart::is_selected_import)
            && self.ordinary.is_some();
        if !self.valid || self.ready || (self.furnishing_bound.is_some() && !selected_furnishing) {
            self.fail();
            return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
        }
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
    pub(super) fn send_frame(&mut self,bytes:&[u8],deadline:Instant,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)->Result<()> {
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
        self.serve_packed_for(PackedServe{control,prepared,output},attempt,correlation,None,cancellation,check)
    }
    #[allow(dead_code)]
    pub(super) fn serve_packed_v3(&mut self,packed:PackedServe<'_>,attempt:&str,correlation:&str,
        bound:&super::oex_preflight_frames::Bound,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
        self.serve_packed_for(packed,attempt,correlation,Some(bound),cancellation,check)
    }
    fn serve_packed_for(&mut self,packed:PackedServe<'_>,attempt:&str,correlation:&str,
        v3:Option<&super::oex_preflight_frames::Bound>,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
        use std::os::unix::fs::{FileExt,MetadataExt};
        let PackedServe{control,prepared,output}=packed;
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
            let bytes = worker_control_notice(control, correlation, attempt, v3.is_some())?;
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
            check()?;let after=output.metadata().map_err(|_|bad())?;if stamp(&after)!=stamp(&before){return Err(bad());}
            let response=if let Some(bound)=v3 {bound.drain_reply(&raw)?} else {
                let request:Drain=serde_json::from_slice(&raw).map_err(|_|bad())?;
                if serde_json::to_vec(&request).map_err(|_|bad())?!=raw||request.version!=1||request.kind!="drain-check"||request.attempt_id!=attempt
                    || Uuid::parse_str(&request.correlation_id).ok().is_none_or(|v|v.to_string()!=request.correlation_id) {return Err(bad());}
                serde_json::to_vec(&Drained{version:1,kind:"drain-result",correlation_id:&request.correlation_id,attempt_id:attempt,status:"drained"}).map_err(|_|bad())?
            };
            self.send_frame(&response,deadline,cancellation,check)?;check()
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
    #[allow(dead_code)]
    pub(super) fn receive_save_ready_v3(&mut self,bound:&super::oex_preflight_frames::Bound,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<super::owner_claim_frames::SaveFrameV3> {
        if !self.valid||!self.ready||!self.claim_started||!self.metadata_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let result=self.read_frame(Instant::now()+Duration::from_secs(120),cancellation,check)
            .and_then(|bytes|super::owner_claim_frames::SaveFrameV3::parse_ready(&bytes,bound)).and_then(|ready|{check()?;Ok(ready)});
        if result.is_err(){self.fail();}result
    }
    #[allow(dead_code)]
    pub(super) fn confirm_save_v3(&mut self,ready:&super::owner_claim_frames::SaveFrameV3,etag:&str,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<()> {
        if !self.valid||!self.ready||!self.claim_started||!self.metadata_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        let result=(|| {let deadline=Instant::now()+Duration::from_secs(120);
            self.send_frame(&ready.observed(etag)?,deadline,cancellation,check)?;
            let reply=self.read_frame(deadline,cancellation,check)?;ready.check_verified(etag,&reply)?;check()
        })();if result.is_err(){self.fail();}result
    }
    /// Only a previously checked actual save/reopen may be reported. A reply
    /// is correlation to Node's verification, never retirement authority.
    pub(super) fn confirm_save(&mut self,ready:&super::owner_claim_frames::SaveFrame,challenge:&super::owner_claim_frames::ClaimChallenge,etag:&str,cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>,trace:&mut dyn FnMut(&'static [u8])) -> Result<()> {
        if challenge.kind!="claim-challenge" {self.fail();return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
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
        if !self.ready {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        self.check_authenticated_idle()
    }
    /// Ordinary furnishing startup has no selected OEX ready frame.
    fn check_authenticated_idle(&mut self) -> Result<()> {
        if !self.valid { self.fail(); return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE")); }
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
            let receipt=self.receive_reservation_receipt(&challenge,&challenge.receipt_digest,cancellation,check)?;
            let capacity=super::oex_reservation::stage_capacity(&receipt,&challenge.receipt_digest,&challenge.reservation_id,
                &challenge.reservation_generation,stage_path,stage_device,expected_bytes,other_stage_bytes)?;
            check()?;if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            let encoded=self.claim_binding_mac(&challenge)?;
            check()?;
            let binding=challenge.binding(&encoded)?;
            let reply=self.exchange(&binding,cancellation,check)?;
            let ready=super::owner_claim_frames::ClaimReady::parse(&reply,&challenge,&encoded,expected_request,expected_bytes,expected_sha256)?;
            check()?;
            if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            Ok((challenge,ready,capacity))
        })();
        if result.is_err(){self.fail();}result
    }
    /// Transfer the exact receipt on this authenticated channel. The receipt
    /// does not itself establish that the provider still holds a live lease.
    pub(super) fn receive_reservation_receipt(&mut self,challenge:&super::owner_claim_frames::ClaimChallenge,expected_digest:&str,
        cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>) -> Result<Vec<u8>> {
            let ready_bytes=self.read_frame(Instant::now()+Duration::from_secs(30),cancellation,check)?;
            let notice:super::owner_claim_frames::ReceiptReady=serde_json::from_slice(&ready_bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
            if serde_json::to_vec(&notice).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=ready_bytes
                || notice.version!=1||notice.kind!="receipt-ready"||notice.correlation_id!=challenge.correlation_id
                || notice.attempt_id!=challenge.attempt_id||notice.sha256!=expected_digest||!hash_valid(expected_digest)||!(1..=32768).contains(&notice.bytes){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
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
            if digest(&receipt)!=expected_digest {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
            Ok(receipt)
    }
    fn claim_binding_mac(&self,challenge:&super::owner_claim_frames::ClaimChallenge)->Result<String> {
            let fields=super::owner_binding_encoding::Fields {workspace:&challenge.workspace_id,tenant:&challenge.tenant_id,
                principal:&challenge.principal_id,runtime_generation:&challenge.runtime_generation,job:&challenge.job_id,
                attempt:&challenge.attempt_id,generation:&challenge.generation,stage:&challenge.stage_id,
                reservation:&challenge.reservation_id,reservation_generation:&challenge.reservation_generation,
                claim_digest:&challenge.claim_digest,receipt_digest:&challenge.receipt_digest};
            let mac=super::owner_binding_encoding::correlation_mac(&self.key,&fields)?;
            Ok(mac.iter().map(|v|format!("{v:02x}")).collect())
    }
    /// One v3 input reservation, correlated to independently held DB/blob
    /// and stage pins. The caller keeps checking the owner and live provider.
    #[allow(dead_code)]
    pub(super) fn begin_claim_v3(&mut self,open:&super::owner_claim_frames::ClaimOpen,principal:&str,
        archive:SelectedArchive<'_>,scope:super::oex_reservation::V3PlanScope<'_>,
        cancellation:&AtomicBool,check:&mut dyn FnMut()->Result<()>)
        ->Result<(super::owner_claim_frames::ClaimChallenge,super::owner_claim_frames::ClaimReady,Vec<u8>)> {
        if !self.valid||!self.ready||self.claim_started {self.fail();return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));}
        self.claim_started=true;
        let result=(|| {
            let bytes=self.exchange(&open.encode_v3()?,cancellation,check)?;
            let challenge=super::owner_claim_frames::ClaimChallenge::parse_v3(&bytes,open,principal)?;
            let receipt=self.receive_reservation_receipt(&challenge,&challenge.receipt_digest,cancellation,check)?;
            super::oex_reservation::check_v3_plan(&receipt,&challenge.receipt_digest,&challenge.reservation_id,
                &challenge.reservation_generation,scope,archive.bytes,0)?;
            check()?;if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            let mac=self.claim_binding_mac(&challenge)?;
            let reply=self.exchange(&challenge.binding_v3(&mac)?,cancellation,check)?;
            let ready=match archive.format {
                Some(format)=>super::owner_claim_frames::ClaimReady::parse_v4(&reply,&challenge,&mac,archive.request_id,archive.bytes,archive.sha256,format)?,
                None=>super::owner_claim_frames::ClaimReady::parse_v3(&reply,&challenge,&mac,archive.request_id,archive.bytes,archive.sha256)?,
            };
            check()?;if cancellation.load(Ordering::SeqCst)||super::cancelled(){return Err(err("NATIVE_CANCELLED"));}
            Ok((challenge,ready,receipt))
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

pub(super) fn worker_control_notice(control: &super::oex_control_v2::Control,
    correlation_id: &str, attempt_id: &str, v3: bool) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Notice<'a> {
        version: u32, kind: &'static str, correlation_id: &'a str, attempt_id: &'a str,
        control: &'a super::oex_control_v2::Control,
    }
    let kind = if v3 { "worker-control-v3" } else { "worker-control" };
    serde_json::to_vec(&Notice { version: 1, kind, correlation_id, attempt_id, control })
        .map_err(|_| err("NATIVE_OEX_WORKER_UNCERTAIN"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn furnishing_channel_requires_exact_ack_and_rejects_eof() {
        use super::super::{FurnishingStart, furnishing_lease::{Kind, Lease}, furnishing_lifetime::WriterIdentity};
        let key = [7; 32];
        let generation = Uuid::new_v4().to_string();
        let lease = Lease::new(&authority(), &generation, FurnishingStart::Reopen, "a".repeat(64),
            WriterIdentity { root_device: 1, root_inode: 2, lock_device: 1, lock_inode: 3 });
        for attempt in 0..4 {
            let (parent, mut peer) = UnixStream::pair().unwrap();
            parent.set_nonblocking(true).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let expected = lease.signed(&key, Kind::Lease).unwrap();
            let mut reply = lease.signed(&key, Kind::Ack).unwrap();
            if attempt == 1 { reply = lease.signed(&[8; 32], Kind::Ack).unwrap(); }
            let (release, wait) = std::sync::mpsc::channel();
            let child = std::thread::spawn(move || {
                let mut header = [0; 4];
                peer.read_exact(&mut header).unwrap();
                let mut body = vec![0; u32::from_be_bytes(header) as usize];
                peer.read_exact(&mut body).unwrap();
                assert_eq!(body, expected);
                if attempt == 2 { return; } // EOF before ACK
                if attempt == 3 { peer.write_all(&2049_u32.to_be_bytes()).unwrap(); return; }
                peer.write_all(&(reply.len() as u32).to_be_bytes()).unwrap();
                peer.write_all(&reply).unwrap();
                if attempt == 0 {
                    let _ = wait.recv_timeout(Duration::from_secs(5));
                    peer.shutdown(std::net::Shutdown::Both).unwrap();
                }
            });
            let mut connection = Connection { socket: parent, key, valid: true, ready: false,
                furnishing_bound: None, ordinary: None, claim_started: false, metadata_started: false };
            let cancellation = AtomicBool::new(false);
            let result = connection.bind_furnishing_lease(&lease, &cancellation, &mut || Ok(()));
            if attempt == 0 {
                assert!(result.is_ok());
                assert!(!connection.ready);
                assert!(connection.check_furnishing_idle().is_ok());
                release.send(()).unwrap();
            } else {
                assert!(result.is_err());
                assert!(!connection.valid);
            }
            child.join().unwrap();
            assert!(connection.check_furnishing_idle().is_err());
        }
    }
    #[test]
    fn furnishing_selected_channel_keeps_next_frame_after_admission_ack() {
        use super::super::{
            FurnishingStart, furnishing_lease, furnishing_lifecycle, furnishing_lifetime::WriterIdentity,
        };
        let key = [7; 32];
        let auth = authority();
        let generation = Uuid::new_v4().to_string();
        let physical = WriterIdentity { root_device: 1, root_inode: 2, lock_device: 1, lock_inode: 3 };
        for (start, catalog) in [
            (FurnishingStart::SelectedImport038, 38),
            (FurnishingStart::SelectedImport039, 39),
            (FurnishingStart::SelectedImport040, 40),
            (FurnishingStart::Initialize, 38),
        ] {
            let lease = furnishing_lease::Lease::new(&auth, &generation, start, "a".repeat(64), physical);
            let active = furnishing_lifecycle::Binding {
                version: 1,
                phase: furnishing_lifecycle::Phase::Active,
                generation: generation.clone(),
                previous_generation: None,
                admission_kind: furnishing_lifecycle::Admission::FreshEmpty,
                owner: furnishing_lifecycle::Owner {
                    workspace_id: auth.workspace_id.clone(),
                    tenant_id: auth.tenant_id.clone(),
                    principal_id: auth.principal_id.clone(),
                    owner_uid: auth.owner_uid,
                },
                source_commit: "1".repeat(40),
                producer_sha256: "2".repeat(64),
                payload_manifest_sha256: "a".repeat(64),
                catalog,
                physical,
                restore: None, interrupted_claim_generations: Vec::new(),
            };
            let lease_body = lease.signed(&key, furnishing_lease::Kind::Lease).unwrap();
            let lease_ack = lease.signed(&key, furnishing_lease::Kind::Ack).unwrap();
            let exchange = furnishing_lifecycle::Exchange::new(&active, &lease_body).unwrap();
            let admission = exchange.signed(&key, furnishing_lifecycle::Kind::Admission).unwrap();
            let admission_ack = exchange.signed(&key, furnishing_lifecycle::Kind::AdmissionAck).unwrap();
            let (parent, mut peer) = UnixStream::pair().unwrap();
            parent.set_nonblocking(true).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let worker = std::thread::spawn(move || {
                let read = |stream: &mut UnixStream| {
                    let mut header = [0; 4];
                    stream.read_exact(&mut header).unwrap();
                    let mut body = vec![0; u32::from_be_bytes(header) as usize];
                    stream.read_exact(&mut body).unwrap();
                    body
                };
                let frame = |body: &[u8]| {
                    let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
                    bytes.extend_from_slice(body);
                    bytes
                };
                assert_eq!(read(&mut peer), lease_body);
                peer.write_all(&frame(&lease_ack)).unwrap();
                assert_eq!(read(&mut peer), admission);
                let mut reply = frame(&admission_ack);
                reply.extend(frame(b"next-v7-catalog-frame"));
                peer.write_all(&reply).unwrap();
            });
            let mut channel = Connection { socket: parent, key, valid: true, ready: false,
                furnishing_bound: None, ordinary: None, claim_started: false, metadata_started: false };
            let cancel = AtomicBool::new(false);
            let result = channel.bind_ordinary_furnishing(&lease, &active, &cancel, &mut || Ok(()));
            if start.is_selected_import() {
                result.unwrap();
                let next = channel.read_frame(Instant::now() + Duration::from_secs(2), &cancel, &mut || Ok(())).unwrap();
                assert_eq!(next, b"next-v7-catalog-frame");
            } else {
                assert!(result.is_err());
                assert!(!channel.valid);
            }
            worker.join().unwrap();
        }
    }

    #[test]
    fn workspace_furnishing_bootstrap_uses_closed_v3_modes() {
        let setup = Setup::new().unwrap();
        let auth = authority();
        let generation = Uuid::new_v4().to_string();
        for (start, mode) in [
            (super::super::FurnishingStart::Initialize, "workspace-furnishing-036-initialize-v1"),
            (super::super::FurnishingStart::Reopen, "workspace-furnishing-036-reopen-v1"),
            (super::super::FurnishingStart::SelectedImport038, "workspace-furnishing-038-selected-import-v7"),
            (super::super::FurnishingStart::SelectedImport039, "workspace-furnishing-039-selected-import-v7"),
            (super::super::FurnishingStart::SelectedImport040, "workspace-furnishing-040-selected-import-v7"),
            (super::super::FurnishingStart::Upgrade039To040, "workspace-furnishing-039-to-040-upgrade-v1"),
        ] {
            let wire = setup.workspace_furnishing_bootstrap(&auth, &generation, start).unwrap();
            let value: Value = serde_json::from_slice(&wire).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 2);
            assert_eq!(value["ownerChannel"].as_object().unwrap().len(), 4);
            assert_eq!(value["ownerChannel"]["version"], 3);
            assert_eq!(value["ownerChannel"]["runtimeGeneration"], generation);
            assert_eq!(value["ownerChannel"]["mode"], mode);
            assert_eq!(value["workspace"]["workspaceId"], auth.workspace_id);
        }
    }
    #[test]
    fn workspace_v7_bootstrap_matches_exact_owner_channel_bytes() {
        use crate::native_payload::WorkspaceImport;
        use sha2::{Digest, Sha256};

        let mut setup = Setup::new().unwrap();
        setup.secret = "b".repeat(64);
        let authority = LocalAuthority {
            format: 1,
            workspace_id: "10000000-0000-4000-8000-000000000001".into(),
            tenant_id: "10000000-0000-4000-8000-000000000002".into(),
            principal_id: "10000000-0000-4000-8000-000000000003".into(),
            owner_uid: 501,
            session_token: "synthetic".into(),
            csrf_token: "synthetic".into(),
        };
        let generation = "10000000-0000-4000-8000-000000000004";
        let wire = setup.workspace_import_bootstrap(&authority, generation, WorkspaceImport::V7).unwrap();
        let wire = std::str::from_utf8(&wire).unwrap();
        let channel = wire.split_once(r#","ownerChannel":"#).unwrap().1.strip_suffix('}').unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(channel.as_bytes())), "c5892688797b237f8c9ec235ce49d1f73843ee16c5377b1c6f6137d04696ab03");
        let legacy = setup.workspace_import_bootstrap(&authority, generation, WorkspaceImport::V6).unwrap();
        assert_eq!(legacy, wire.replace("normal-oex-workspace-import-v7", "normal-oex-workspace-import-v6").into_bytes());
    }
    #[test]
    fn v3_claim_completion_keeps_session_socket_but_abandonment_closes_it() {
        for completed in [true, false] {
            let (parent, mut peer) = UnixStream::pair().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            let mut channel = Some(Connection { socket: parent, key: [1;32], valid: true, ready: true, furnishing_bound: None, ordinary: None, claim_started: true, metadata_started: true });
            finish_v3_claim(&mut channel, completed);
            if completed {
                channel.as_mut().unwrap().socket.write_all(b"x").unwrap();
                let mut byte = [0]; peer.read_exact(&mut byte).unwrap();
                assert_eq!(byte, *b"x");
            } else {
                assert!(channel.is_none());
                let mut byte = [0]; assert_eq!(peer.read(&mut byte).unwrap(), 0);
            }
        }
    }
    #[test]
    fn v3_challenge_cannot_send_v2_save_or_settlement() {
        use super::super::owner_claim_frames::{ClaimChallenge,SaveFrame};
        let id=||Uuid::new_v4().to_string();let attempt=id();
        let ready=SaveFrame{version:1,kind:"save-ready".into(),correlation_id:id(),attempt_id:attempt.clone(),exhibition_id:id(),etag:format!("\"studio-r1-{}\"","a".repeat(64)),request_id:id()};
        let challenge=ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:id(),workspace_id:id(),tenant_id:id(),principal_id:id(),runtime_generation:id(),job_id:id(),attempt_id:attempt,generation:id(),stage_id:id(),reservation_id:id(),reservation_generation:id(),claim_digest:"c".repeat(64),receipt_digest:"d".repeat(64)};
        let (parent,mut peer)=UnixStream::pair().unwrap();
        let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:true,metadata_started:true};
        let result=channel.confirm_save(&ready,&challenge,&format!("\"studio-r2-{}\"","b".repeat(64)),&AtomicBool::new(false),&mut ||Ok(()),&mut |_|{});
        assert_eq!(result.unwrap_err().code,"NATIVE_OEX_ATTEMPT_INVALID");
        let mut byte=[0u8;1];assert_eq!(peer.read(&mut byte).unwrap(),0);
    }
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
            let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:true,metadata_started:true};
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
    fn current_normal_027_bootstrap_declares_private_abi_three() {
        let setup=Setup::new().unwrap();
        let auth=authority();
        let generation=Uuid::new_v4().to_string();
        let value:Value=serde_json::from_slice(&setup.normal_027_bootstrap(&auth,&generation).unwrap()).unwrap();
        assert_eq!(value["ownerChannel"]["version"],3);
        assert_eq!(value["ownerChannel"]["mode"],"normal-oex-027-v3");
        assert_eq!(value["ownerChannel"]["runtimeGeneration"],generation);
        assert_eq!(value["workspace"]["workspaceId"],auth.workspace_id);
        assert_eq!(value["ownerChannel"].as_object().unwrap().len(),4);
    }
    #[test]
    fn workspace_v5_bootstrap_uses_the_private_v3_envelope() {
        let setup = Setup::new().unwrap();
        let auth = authority();
        let generation = Uuid::new_v4().to_string();
        let bytes = setup.workspace_v5_bootstrap(&auth, &generation).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let mut legacy: Value = serde_json::from_slice(&setup.normal_028_bootstrap(&auth, &generation).unwrap()).unwrap();
        assert_eq!(legacy["ownerChannel"]["mode"], "normal-oex-028-v4");
        legacy["ownerChannel"]["mode"] = "normal-oex-workspace-v5".into();
        assert_eq!(value, legacy);
        assert_eq!(value["ownerChannel"]["version"], 3);
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
        let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:true,metadata_started:false};
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
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:false,furnishing_bound: None, ordinary: None,claim_started:false,metadata_started:false};let cancel=AtomicBool::new(false);
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
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:false,furnishing_bound: None, ordinary: None,claim_started:false,metadata_started:false};
        assert!(connection.receive_ready(&AtomicBool::new(false),&mut ||Ok(()),&auth.workspace_id,&auth.tenant_id,&auth.principal_id,&generation).is_err());
        assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
    }
    #[test]
    fn idle_socket_detects_eof_while_peer_handle_still_exists() {
        let (parent,peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();
        let mut connection=Connection{socket:parent,key:[9;32],valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:true,metadata_started:false};
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
        let mut connection=Connection{socket:parent,key:std::array::from_fn(|i|i as u8),valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:false,metadata_started:false};
        let cancel=AtomicBool::new(false);
        let (_,ready,capacity)=connection.begin_claim(&open,&id(3),&request,67_108_865,&archive,Path::new("/private/stage"),1,0,&cancel,&mut ||Ok(())).unwrap();
        assert_eq!(capacity,532_891_135);
        assert_eq!(ready.request_id,request);worker.join().unwrap();
        assert!(connection.begin_claim(&open,&id(3),&request,67_108_865,&archive,Path::new("/private/stage"),1,0,&cancel,&mut ||Ok(())).is_err());
        assert!(!connection.valid);assert_eq!(connection.key,[0;32]);
    }
    #[cfg(target_os="macos")]
    #[test]
    fn v3_claim_checks_independent_owner_before_binding() {
        use super::super::owner_claim_frames::{ClaimChallenge,ClaimOpen,ClaimReady,ReceiptReady,ReceiptChunk};
        use std::os::unix::fs::MetadataExt;
        fn id(n:usize)->String {format!("10000000-0000-4000-8000-{n:012}")}
        fn read_frame(s:&mut UnixStream)->Vec<u8> {let mut h=[0;4];s.read_exact(&mut h).unwrap();let mut b=vec![0;u32::from_be_bytes(h) as usize];s.read_exact(&mut b).unwrap();b}
        fn send(s:&mut UnixStream,b:&[u8]) {s.write_all(&(b.len() as u32).to_be_bytes()).unwrap();s.write_all(b).unwrap();}
        fn field(out:&mut Vec<u8>,s:&str){out.extend_from_slice(&(s.len() as u32).to_be_bytes());out.extend_from_slice(s.as_bytes());}
        let path=std::env::temp_dir().join(format!("oex-owner-{}",Uuid::new_v4()));fs::create_dir(&path).unwrap();
        let path=fs::canonicalize(path).unwrap();let stat=fs::metadata(&path).unwrap();
        let owner=super::super::oex_owner::Owner::test_only(&path);
        for valid in [true,false] {
            let mut receipt=b"EXHIBITOS-DISK-RESERVATION\0v1\0".to_vec();
            let fields:[&str;8]=[&id(9),&id(10),"oex-import",if valid{"123"}else{"124"},"456",path.to_str().unwrap(),&stat.dev().to_string(),&stat.ino().to_string()];
            for value in fields {field(&mut receipt,value);}
            receipt.extend_from_slice(&1u32.to_be_bytes());for value in ["/private/stage","1","100"] {field(&mut receipt,value);}
            let open=ClaimOpen{version:1,kind:"claim-open-v3".into(),correlation_id:id(11),workspace_id:id(1),tenant_id:id(2),runtime_generation:id(4),job_id:id(5)};
            let challenge=ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:id(11),workspace_id:id(1),tenant_id:id(2),principal_id:id(3),runtime_generation:id(4),job_id:id(5),attempt_id:id(6),generation:id(7),stage_id:id(8),reservation_id:id(9),reservation_generation:id(10),claim_digest:"c".repeat(64),receipt_digest:digest(&receipt)};
            let request=id(12);let archive="d".repeat(64);let peer_open=open.clone();let peer_request=request.clone();let peer_archive=archive.clone();
            let (parent,mut peer)=UnixStream::pair().unwrap();parent.set_nonblocking(true).unwrap();peer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let worker=std::thread::spawn(move || {
                assert_eq!(read_frame(&mut peer),peer_open.encode_v3().unwrap());send(&mut peer,&serde_json::to_vec(&challenge).unwrap());
                send(&mut peer,&serde_json::to_vec(&ReceiptReady{version:1,kind:"receipt-ready".into(),correlation_id:id(11),attempt_id:id(6),bytes:receipt.len(),sha256:challenge.receipt_digest.clone()}).unwrap());
                let read:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();
                send(&mut peer,&serde_json::to_vec(&ReceiptChunk{version:1,kind:"receipt-chunk".into(),correlation_id:read["correlationId"].as_str().unwrap().into(),attempt_id:id(6),offset:0,data:STANDARD.encode(&receipt)}).unwrap());
                if !valid {let mut byte=[0u8;1];assert_eq!(peer.read(&mut byte).unwrap(),0);return;}
                let binding:Value=serde_json::from_slice(&read_frame(&mut peer)).unwrap();assert_eq!(binding["kind"],"claim-binding-v3");
                let mac=binding["managerBindingDigest"].as_str().unwrap().to_string();
                let ready=ClaimReady{version:1,kind:"claim-ready-v3".into(),correlation_id:id(11),runtime_generation:id(4),workspace_id:id(1),tenant_id:id(2),job_id:id(5),user_id:id(3),request_id:peer_request,lease_id:id(13),attempt_id:id(6),generation:id(7),stage_id:id(8),reservation_id:id(9),reservation_generation:id(10),archive_bytes:100,archive_sha256:peer_archive.clone(),source_digest:peer_archive,reservation_digest:challenge.receipt_digest,manager_binding_digest:mac,profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
                send(&mut peer,&serde_json::to_vec(&ready).unwrap());
            });
            let mut channel=Connection{socket:parent,key:[1;32],valid:true,ready:true,furnishing_bound: None, ordinary: None,claim_started:false,metadata_started:false};
            let selected=SelectedArchive{request_id:&request,bytes:100,sha256:&archive,format:None};
            let scope=super::super::oex_reservation::V3PlanScope::from_owner(&owner,Path::new("/private/stage"),1);
            let result=channel.begin_claim_v3(&open,&id(3),selected,scope,&AtomicBool::new(false),&mut ||Ok(()));
            assert_eq!(result.is_ok(),valid);if !valid {assert!(!channel.valid);assert_eq!(channel.key,[0;32]);}
            worker.join().unwrap();
            assert!(channel.begin_claim_v3(&open,&id(3),selected,scope,&AtomicBool::new(false),&mut ||Ok(())).is_err());
        }
        drop(owner);fs::remove_dir(path).unwrap();
    }
}
