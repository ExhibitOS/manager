// SPDX-License-Identifier: Apache-2.0
//! Explicit diagnostic-only PG custody. No Session, journal or ordinary admission.
use super::*;
use super::owner_inspection_sql::{Owner, ClusterObservation};
use std::os::{fd::AsRawFd, unix::{fs::{MetadataExt,OpenOptionsExt},process::CommandExt}};
use std::net::SocketAddr;

const ERROR:&str="NATIVE_OWNER_INSPECTION_TRANSPORT_INVALID";
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Input {
    identity_mode:String,
    owner:Owner,
    pointer_sha256:String,
    failed_state_sha256:String,
    predecessor_state_sha256:String,
    predecessor:Binding,
}
struct Locked { file:File,path:PathBuf }
impl Locked {
    fn open(path:PathBuf)->Result<Self>{
        let file=OpenOptions::new().read(true).write(true)
            .custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK|libc::O_CLOEXEC).open(&path).map_err(|_|err(ERROR))?;
        let lock=Self{file,path};lock.check()?;
        lock.file.try_lock_exclusive().map_err(|_|err("NATIVE_OWNER_INSPECTION_BUSY"))?;lock.check()?;Ok(lock)
    }
    fn check(&self)->Result<()>{
        let a=self.file.metadata().map_err(|_|err(ERROR))?;let b=fs::symlink_metadata(&self.path).map_err(|_|err(ERROR))?;
        if !a.is_file()||b.is_symlink()||a.uid()!=unsafe{libc::geteuid()}||a.mode()&0o777!=0o600||a.nlink()!=1
            ||(a.dev(),a.ino())!=(b.dev(),b.ino()){return Err(err(ERROR));}Ok(())
    }
}
struct Snapshot {root:PathBuf,name:String,hash:String,identity:(u64,u64)}
impl Snapshot {
    fn open(root:&Path,name:&str,expected:Option<&str>)->Result<Self>{
        let bytes=installation_backup::source_bytes(root,name,16384,true)?;
        let hash=digest(&bytes);if expected.is_some_and(|e|e!=hash){return Err(err(ERROR));}
        let m=fs::symlink_metadata(root.join(name)).map_err(|_|err(ERROR))?;
        Ok(Self{root:root.into(),name:name.into(),hash,identity:(m.dev(),m.ino())})
    }
    fn empty_log(root:&Path,name:&str)->Result<Self>{
        let file=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(root.join(name)).map_err(|_|err(ERROR))?;
        let a=file.metadata().map_err(|_|err(ERROR))?;let b=fs::symlink_metadata(root.join(name)).map_err(|_|err(ERROR))?;
        if !a.is_file()||b.is_symlink()||a.len()!=0||a.uid()!=unsafe{libc::geteuid()}||a.mode()&0o7777!=0o600||a.nlink()!=1||(a.dev(),a.ino())!=(b.dev(),b.ino()){return Err(err(ERROR));}
        Ok(Self{root:root.into(),name:name.into(),hash:digest(b""),identity:(a.dev(),a.ino())})
    }
    fn check(&self)->Result<()>{let next=if self.name.ends_with("/process.log"){Self::empty_log(&self.root,&self.name)?}else{Self::open(&self.root,&self.name,Some(&self.hash))?};
        if next.identity!=self.identity{return Err(err(ERROR));}Ok(())}
}
struct Lease {
    directories:Vec<crate::maintenance_source::DirectoryPin>,locks:Vec<Locked>,
    writer:furnishing_lifetime::WriterLifetime,snapshots:Vec<Snapshot>,root:PathBuf,ports:Ports,
}
impl Lease {
    fn check(&self)->Result<()>{for d in &self.directories{d.check()?;}for l in &self.locks{l.check()?;}
        self.writer.identity()?;
        for marker in ["native-restore-current.json","native-target-preparation-current.json","native-legacy-migration-current.json",WORKSPACE_FURNISHING_INITIALIZATION]{if exists(&self.root.join(marker))?{return Err(err(ERROR));}}
        for s in &self.snapshots{s.check()?;}Ok(())}
    fn ports_free(&self)->Result<Vec<TcpListener>>{
        [self.ports.web,self.ports.api,self.ports.database].iter().map(|p|TcpListener::bind(SocketAddr::from(([127,0,0,1],*p))).map_err(|_|err("NATIVE_OWNER_INSPECTION_PORT_BUSY"))).collect()
    }
    fn cold(&self)->Result<()>{self.check()?;check_interrupted_root(&self.root)?;
        if exists(&self.root.join("native-database/postmaster.pid"))?{return Err(err(ERROR));}
        drop(self.ports_free()?);Ok(())}
    fn configure(&self,c:&mut Command)->Result<()> {
        self.check()?;let mut fds:Vec<_>=self.locks.iter().map(|l|l.file.as_raw_fd()).collect();
        let writer=self.writer.duplicate_for_writer()?;fds.push(writer.as_raw_fd());
        // Duplicate is retained by this closure until spawn; the original OFD
        // remains held by Lease. All processes inherit the same existing locks.
        unsafe{c.pre_exec(move||{let _keep=&writer;
            if libc::setpgid(0,0)!=0{return Err(std::io::Error::last_os_error());}
            for fd in &fds{let flags=libc::fcntl(*fd,libc::F_GETFD);if flags<0||libc::fcntl(*fd,libc::F_SETFD,flags&!libc::FD_CLOEXEC)<0{return Err(std::io::Error::last_os_error());}}
            Ok(())});}Ok(())
    }
}
struct Process { child:Option<Child>,lease:Arc<Lease>,group:i32,terminal:Option<std::process::ExitStatus> }
impl Process {
    fn spawn(lease:Arc<Lease>,mut c:Command)->Result<Self>{lease.configure(&mut c)?;let child=c.spawn().map_err(|_|err(ERROR))?;
        let group=child.id() as i32;Ok(Self{child:Some(child),lease,group,terminal:None})}
    fn group_absent(&self)->bool{(unsafe{libc::kill(-self.group,0)})==-1&&std::io::Error::last_os_error().raw_os_error()==Some(libc::ESRCH)}
    fn poll(&mut self)->Result<Option<std::process::ExitStatus>>{
        if self.terminal.is_none(){self.terminal=self.child.as_mut().ok_or_else(||err(ERROR))?.try_wait().map_err(|_|err(ERROR))?;}
        Ok(self.terminal)
    }
    fn signal(&mut self)->Result<bool>{
        // An unreaped child owns its PID. Once wait reports terminal status,
        // numeric PID reuse is possible and no signal is allowed.
        if self.poll()?.is_some(){return Ok(false);}
        let result=unsafe{libc::kill(self.child.as_ref().unwrap().id() as i32,libc::SIGTERM)};
        if result!=0&&std::io::Error::last_os_error().raw_os_error()!=Some(libc::ESRCH){return Err(err(ERROR));}Ok(result==0)
    }
    fn stop(&mut self,deadline:Instant)->Result<i32>{
        self.signal()?;
        loop{if let Some(status)=self.poll()?{
            if self.group_absent(){if status.success(){self.child=None;}use std::os::unix::process::ExitStatusExt;return Ok(status.code().unwrap_or(-status.signal().unwrap_or(1)));}
        }if Instant::now()>=deadline{return Err(err("NATIVE_OWNER_INSPECTION_CLEANUP_UNCONFIRMED"));}thread::sleep(Duration::from_millis(20));}
    }
}
impl Drop for Process {
    fn drop(&mut self){if self.child.is_some(){let _=self.signal();}
        if let Some(mut child)=self.child.take(){
        // No forced kill or fence release while cleanup is unproved. On helper
        // loss, inherited OFDs continue to exclude ordinary writers until PG exits.
        let lease=self.lease.clone();let group=self.group;
        let mut terminal=self.terminal;
        thread::spawn(move||{loop{let status=if let Some(status)=terminal{Ok(Some(status))}else{child.try_wait()};match status{Ok(Some(status))=>{terminal=Some(status);
            let absent=(unsafe{libc::kill(-group,0)})==-1&&std::io::Error::last_os_error().raw_os_error()==Some(libc::ESRCH);
            if absent&&status.success()&&lease.cold().is_ok(){return;}
            if absent{std::mem::forget(lease);return;}
        },Err(_)=>{std::mem::forget(lease);return;},_=>{}}thread::sleep(Duration::from_millis(100));}});
    }}
}
fn command(program:&Path,root:&Path)->Command{let mut c=Command::new(program);c.current_dir(root).env_clear().env("PATH","/usr/bin:/bin").env("LANG","C").env("LC_ALL","C").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());c}
fn nonblocking(fd:i32)->Result<()>{let flags=unsafe{libc::fcntl(fd,libc::F_GETFL)};
    if flags<0||unsafe{libc::fcntl(fd,libc::F_SETFL,flags|libc::O_NONBLOCK)}<0{return Err(err(ERROR));}Ok(())}
/// Bounded stdin/stdout pumping; neither an inherited pipe nor query output can
/// block cancellation/cleanup. Stderr and credentials are never persisted.
fn collect(mut p:Process,input:&[u8],limit:usize,deadline:Instant,cancel:&AtomicBool)->Result<(i32,Vec<u8>)>{
    let child=p.child.as_mut().ok_or_else(||err(ERROR))?;let mut stdin=child.stdin.take();let mut stdout=child.stdout.take().ok_or_else(||err(ERROR))?;
    if let Some(s)=&stdin{nonblocking(s.as_raw_fd())?;}nonblocking(stdout.as_raw_fd())?;
    let mut offset=0;let mut output=Vec::new();let mut eof=false;
    loop{p.lease.check()?;if cancel.load(Ordering::SeqCst)||Instant::now()>=deadline{return Err(err("NATIVE_OWNER_INSPECTION_CANCELLED_OR_TIMEOUT"));}
        if let Some(s)=&mut stdin{match s.write(&input[offset..]){Ok(n)=>{offset+=n;if offset==input.len(){stdin=None;}},Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>{},_=>return Err(err(ERROR))}}
        let mut block=[0u8;4096];loop{match stdout.read(&mut block){Ok(0)=>{eof=true;break},Ok(n)=>{if output.len()+n>limit{return Err(err("NATIVE_OWNER_INSPECTION_OUTPUT_LIMIT"));}output.extend_from_slice(&block[..n]);},Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>break,_=>return Err(err(ERROR))}}
        if let Some(status)=p.poll()?{if eof&&p.group_absent(){p.child=None;return Ok((status.code().unwrap_or(-1),output));}}
        thread::sleep(Duration::from_millis(20));
    }
}
fn cluster_id(bytes:&[u8])->Result<String>{let s=std::str::from_utf8(bytes).map_err(|_|err(ERROR))?;
    let ids:Vec<_>=s.lines().filter_map(|l|l.strip_prefix("Database system identifier:").map(str::trim)).collect();
    if ids.len()!=1||!ids[0].parse::<u64>().is_ok_and(|n|n>0&&n.to_string()==ids[0]){return Err(err(ERROR));}Ok(ids[0].into())}

pub(super) fn run(profile:&Path,resources:&Path,bytes:&[u8],cancel:&AtomicBool)->Result<Value>{
    if bytes.len()>16384||cancel.load(Ordering::SeqCst){return Err(err(ERROR));}
    let input:Input=serde_json::from_slice(bytes).map_err(|_|err(ERROR))?;
    if input.identity_mode!="cluster-owner-observation"||[&input.pointer_sha256,&input.failed_state_sha256,&input.predecessor_state_sha256].iter().any(|h|!hash_valid(h)){return Err(err(ERROR));}
    if !installations::uuid(&input.predecessor.generation){return Err(err(ERROR));}
    let root=profile.join("local-runtime");
    let mut directories=profile.ancestors().chain([root.as_path(),root.join("native-database").as_path()]).map(crate::maintenance_source::DirectoryPin::open).collect::<Result<Vec<_>>>()?;
    installations::private_directory(profile)?;installations::private_directory(&root)?;installations::private_directory(&root.join("native-database"))?;
    let locks=[profile.join("profile-session.lock"),profile.join("operation.lock"),root.join("operation.lock")].into_iter().map(Locked::open).collect::<Result<Vec<_>>>()?;
    let writer=furnishing_lifetime::WriterLifetime::acquire_existing(&root)?;
    let service=LifecycleService::bound_existing(root.clone())?;
    let prepared=service.personal_prepared()?;let ports=prepared.ports;ports.validate()?;
    let mut snapshots=vec![Snapshot::open(&root,"native-current-run.json",Some(&input.pointer_sha256))?,Snapshot::open(&root,"native-personal.json",None)?,Snapshot::open(&root,"runtime.env",None)?,Snapshot::open(&root,"native-database/PG_VERSION",None)?];
    let current=current_run(&root)?.ok_or_else(||err(ERROR))?;
    for path in [root.join("native-runs"),root.join("native-runs").join(&current.id),root.join("native-runs").join(&input.predecessor.generation)]{directories.push(crate::maintenance_source::DirectoryPin::open(&path)?);}
    let log_name=format!("native-runs/{}/process.log",current.id);
    let log=Snapshot::empty_log(&root,&log_name)?;
    if log.hash!=digest(b""){return Err(err(ERROR));}snapshots.push(log);
    snapshots.push(Snapshot::open(&root,&format!("native-runs/{}/state.json",current.id),Some(&input.failed_state_sha256))?);
    snapshots.push(Snapshot::open(&root,&format!("native-runs/{}/state.json",input.predecessor.generation),Some(&input.predecessor_state_sha256))?);
    let (failed,failed_completion)=read_run(&root,&current.id)?;let(stopped,completion)=read_run(&root,&input.predecessor.generation)?;
    let owner=service.personal_identity()?;
    if (owner.0,owner.1,owner.2)!=(input.owner.workspace_id.clone(),input.owner.tenant_id.clone(),input.owner.principal_id.clone()){return Err(err(ERROR));}
    owner_inspection_source::check_shape(&current,&failed,failed_completion.as_ref(),&input.predecessor,&stopped,&completion.ok_or_else(||err(ERROR))?,&input.owner,writer.identity()?,unsafe{libc::geteuid()})?;
    let lease=Arc::new(Lease{directories,locks,writer,snapshots,root:root.clone(),ports});lease.cold()?;
    let payload=Payload::open(resources)?;let context=payload.context();let binding=current.furnishing.as_ref().ok_or_else(||err(ERROR))?;
    if (&context.source_commit,payload.producer_sha256(),&context.manifest_sha256)!=(&binding.source_commit,binding.producer_sha256.as_str(),&binding.payload_manifest_sha256)||prepared.manifest_sha256!=binding.payload_manifest_sha256{return Err(err(ERROR));}
    let ledger=payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045)?;
    let env=Environment(service.personal_environment(&payload,ports)?);
    let tools=payload.postgres().parent().ok_or_else(||err(ERROR))?.to_path_buf();
    if installation_backup::source_bytes(&root,"native-database/PG_VERSION",32,true)?!=b"18\n"{return Err(err(ERROR));}
    let mut control=command(&tools.join("pg_controldata"),&root);control.arg(root.join("native-database")).stdout(Stdio::piped());
    let(code,out)=collect(Process::spawn(lease.clone(),control)?,&[],4096,Instant::now()+Duration::from_secs(5),cancel)?;
    if code!=0{return Err(err(ERROR));}let system=cluster_id(&out)?;lease.cold()?;
    let reservations=lease.ports_free()?;let nonce=Uuid::new_v4().to_string();
    let mut pg=command(&payload.postgres(),&root);pg.args(["-D",root.join("native-database").to_str().ok_or_else(||err(ERROR))?,"-h","127.0.0.1","-p",&ports.database.to_string(),"-k","","-c","jit=off","-c",&format!("exhibitos.instance={nonce}")]);
    drop(reservations);let mut pg=Process::spawn(lease.clone(),pg)?;
    let attempt=(||{
        let ready_deadline=Instant::now()+Duration::from_secs(30);
        loop{lease.check()?;if pg.poll()?.is_some(){return Err(err(ERROR));}
            let mut ready=command(&tools.join("pg_isready"),&root);ready.args(["-h","127.0.0.1","-p",&ports.database.to_string(),"-U","exhibitos","-t","1"]).stdout(Stdio::piped());
            let(code,_)=collect(Process::spawn(lease.clone(),ready)?,&[],4096,(Instant::now()+Duration::from_secs(2)).min(ready_deadline),cancel)?;
            if code==0{break;}if Instant::now()>=ready_deadline{return Err(err(ERROR));}thread::sleep(Duration::from_millis(100));
        }
        let sql=owner_inspection_sql::observed_transaction(&input.owner,&system,&nonce,&ledger)?;
        let mut query=command(&tools.join("psql"),&root);query.args(["-h","127.0.0.1","-p",&ports.database.to_string(),"-U","exhibitos","-d","exhibitos","--no-password","--no-psqlrc","--quiet","--tuples-only","--no-align","--set=ON_ERROR_STOP=1","--file=-"]).env("PGPASSWORD",env.0.get("POSTGRES_PASSWORD").ok_or_else(||err(ERROR))?).env("PGCONNECT_TIMEOUT","3").stdin(Stdio::piped()).stdout(Stdio::piped());
        let(code,bytes)=collect(Process::spawn(lease.clone(),query)?,sql.as_bytes(),16384,Instant::now()+Duration::from_secs(30),cancel)?;
        if code!=0{return Err(err(ERROR));}ClusterObservation::parse(&bytes,&input.owner,&system)
    })();
    let cleanup=pg.stop(Instant::now()+Duration::from_secs(15)).and_then(|code|if code==0{lease.cold()}else{Err(err("NATIVE_OWNER_INSPECTION_CLEANUP_UNCONFIRMED"))});
    if let Err(error)=cleanup{std::mem::forget(lease.clone());return Err(error);}
    let observation=attempt?;
    let mut result=serde_json::to_value(observation).map_err(|_|err(ERROR))?;
    result["diagnosticComplete"]=true.into();result["normalOwnedCleanupComplete"]=true.into();
    result["journalAndBindingUnchanged"]=true.into();Ok(result)
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn owner_inspection_transport_cluster_id_is_canonical_and_unambiguous(){assert_eq!(cluster_id(b"Database system identifier: 123\n").unwrap(),"123");for b in [b"Database system identifier: 0123\n".as_slice(),b"Database system identifier: 123\nDatabase system identifier: 456\n",b"wrong: 123\n"]{assert!(cluster_id(b).is_err());}}
    #[test]fn owner_inspection_transport_existing_lock_never_creates_or_accepts_links(){use std::os::unix::fs::{PermissionsExt,symlink};let root=std::env::temp_dir().join(format!("inspection-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let path=root.join("operation.lock");assert!(Locked::open(path.clone()).is_err());assert!(!path.exists());fs::write(&path,[]).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();let first=Locked::open(path.clone()).unwrap();assert!(Locked::open(path.clone()).is_err());fs::rename(&path,root.join("old")).unwrap();symlink(root.join("old"),&path).unwrap();assert!(first.check().is_err());assert!(Locked::open(path).is_err());drop(first);fs::remove_dir_all(root).unwrap();}
    fn fixture()->(PathBuf,Arc<Lease>){use std::os::unix::fs::DirBuilderExt;
        let path=std::env::temp_dir().join(format!("inspection-process-{}",Uuid::new_v4()));fs::DirBuilder::new().mode(0o700).create(&path).unwrap();let root=fs::canonicalize(path).unwrap();
        let writer=furnishing_lifetime::WriterLifetime::acquire(&root).unwrap();
        let ports:Vec<_>=(0..3).map(|_|TcpListener::bind("127.0.0.1:0").unwrap()).collect();
        let values:Vec<_>=ports.iter().map(|p|p.local_addr().unwrap().port()).collect();
        let lease=Arc::new(Lease{directories:vec![crate::maintenance_source::DirectoryPin::open(&root).unwrap()],locks:vec![],writer,snapshots:vec![],root:root.clone(),ports:Ports{web:values[0],api:values[1],database:values[2]}});drop(ports);(root,lease)
    }
    #[test]fn owner_inspection_transport_bounded_pipe_and_normal_process_return(){
        let(root,lease)=fixture();let mut c=command(Path::new("/bin/cat"),&root);c.stdin(Stdio::piped()).stdout(Stdio::piped());
        let(code,out)=collect(Process::spawn(lease.clone(),c).unwrap(),b"synthetic-only",32,Instant::now()+Duration::from_secs(3),&AtomicBool::new(false)).unwrap();
        assert_eq!(code,0);assert_eq!(out,b"synthetic-only");lease.cold().unwrap();drop(lease);fs::remove_dir_all(root).unwrap();
    }
    #[test]fn owner_inspection_transport_owned_signal_and_group_reap(){
        let(root,lease)=fixture();let mut c=command(Path::new("/bin/sh"),&root);c.args(["-c","trap 'exit 0' TERM; while :; do sleep 0.05; done"]);
        let mut p=Process::spawn(lease.clone(),c).unwrap();thread::sleep(Duration::from_millis(150));
        assert_eq!(p.stop(Instant::now()+Duration::from_secs(3)).unwrap(),0);assert!(p.group_absent());
        assert!(p.child.is_none());lease.cold().unwrap();drop((p,lease));fs::remove_dir_all(root).unwrap();
    }

    #[test]fn owner_inspection_transport_never_signals_reaped_nonzero_child(){
        let(root,lease)=fixture();let mut c=command(Path::new("/bin/sh"),&root);c.args(["-c","exit 7"]);
        let mut p=Process::spawn(lease.clone(),c).unwrap();let deadline=Instant::now()+Duration::from_secs(3);
        loop{if p.poll().unwrap().is_some(){break;}assert!(Instant::now()<deadline);thread::sleep(Duration::from_millis(10));}
        assert_eq!(p.terminal.unwrap().code(),Some(7));assert!(!p.signal().unwrap());
        assert_eq!(p.stop(deadline).unwrap(),7);assert!(!p.signal().unwrap());
        // Drop retains uncertain/nonzero custody but must never signal the reaped PID.
        drop(p);drop(lease);fs::remove_dir_all(root).unwrap();
    }

    #[test]fn owner_inspection_transport_empty_log_custody_rejects_content_and_replacement(){use std::os::unix::fs::PermissionsExt;
        let(root,lease)=fixture();fs::create_dir(root.join("run")).unwrap();let path=root.join("run/process.log");fs::write(&path,[]).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();
        let pin=Snapshot::empty_log(&root,"run/process.log").unwrap();pin.check().unwrap();fs::write(&path,b"unexpected Node readiness").unwrap();assert!(pin.check().is_err());assert!(Snapshot::empty_log(&root,"run/process.log").is_err());
        fs::write(&path,[]).unwrap();fs::rename(&path,root.join("old-log")).unwrap();fs::write(&path,[]).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();assert!(pin.check().is_err());drop(lease);fs::remove_dir_all(root).unwrap();
    }

}
