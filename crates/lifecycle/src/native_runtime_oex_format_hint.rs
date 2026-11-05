// SPDX-License-Identifier: Apache-2.0
//! Read-only provisional format inspection of a held Swift snapshot. No profile,
//! installation, HTTP request, reservation or import authority is opened here.
use super::*;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{fs::MetadataExt, process::CommandExt};

#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Hint {
    version:u32, kind:String, archive_format:String, archive_bytes:u64,
    manifest_sha256:String, input_device:String, input_inode:String,
}
fn invalid()->LifecycleError {err("NATIVE_OEX_FORMAT_INVALID")}
fn stamp(m:&fs::Metadata)->(u64,u64,u64,i64,i64,i64,i64) {
    (m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())
}
fn check_hint(bytes:&[u8],before:&fs::Metadata)->Result<()> {
    let h:Hint=serde_json::from_slice(bytes).map_err(|_|invalid())?;
    if h.version!=1||h.kind!="oex-format-hint"||!matches!(h.archive_format.as_str(),"1.0.0-draft.9"|"1.0.0-draft.10")
        || h.archive_bytes!=before.len()||h.input_device!=before.dev().to_string()||h.input_inode!=before.ino().to_string()
        || !hash_valid(&h.manifest_sha256)||serde_json::to_vec(&h).map_err(|_|invalid())?!=bytes {return Err(invalid());} Ok(())
}
pub(super) fn run(resources:&Path, input_fd:i32, cancelled:&AtomicBool)->Result<Vec<u8>> {
    let payload=Payload::open(resources)?;
    let (node,script,root)=payload.oex_format_hint()?;
    let fd=unsafe{libc::fcntl(input_fd,libc::F_DUPFD_CLOEXEC,64)};
    if fd<0{return Err(invalid());}
    let source=unsafe{File::from_raw_fd(fd)};
    run_owned(&node,&script,&root,source,cancelled)
}
fn run_owned(node:&Path,script:&Path,root:&Path,source:File,cancelled:&AtomicBool)->Result<Vec<u8>> {
    let before=source.metadata().map_err(|_|invalid())?;
    let flags=unsafe{libc::fcntl(source.as_raw_fd(),libc::F_GETFL)};
    if !before.is_file()||before.uid()!=unsafe{libc::geteuid()}||before.mode()&0o777!=0o600||before.nlink()!=1
        || !(1..=512*1024*1024).contains(&before.len())||flags<0||flags&libc::O_ACCMODE!=libc::O_RDONLY {return Err(invalid());}
    let copy=super::oex_materializer_fds::duplicate(source.as_raw_fd())?;
    let fd=copy.as_raw_fd();
    let mut command=Command::new(node);
    command.arg("--permission").arg(format!("--allow-fs-read={}",root.display())).arg("--no-addons").arg(script)
        .env_clear().env("PATH","/usr/bin:/bin").env("LANG","C").env("EXHIBITOS_OEX_FORMAT_FD",fd.to_string())
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    unsafe{command.pre_exec(move||{
        let limit=libc::rlimit{rlim_cur:0,rlim_max:0};
        if libc::setrlimit(libc::RLIMIT_FSIZE,&limit)!=0{return Err(std::io::Error::last_os_error());}
        let flags=libc::fcntl(fd,libc::F_GETFD);
        if flags<0||libc::fcntl(fd,libc::F_SETFD,flags & !libc::FD_CLOEXEC)<0{return Err(std::io::Error::last_os_error());} Ok(())
    });}
    if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    let mut child=command.spawn().map_err(|_|err("NATIVE_PROCESS_START_FAILED"))?;
    let mut output=child.stdout.take().ok_or_else(invalid)?;
    let mut child=super::oex_materializer::OwnedChild::new(child);
    drop(copy);
    let fd=output.as_raw_fd();
    let flags=unsafe{libc::fcntl(fd,libc::F_GETFL)};
    if flags<0||unsafe{libc::fcntl(fd,libc::F_SETFL,flags|libc::O_NONBLOCK)}<0{return Err(invalid());}
    let deadline=Instant::now()+Duration::from_secs(30);
    let mut reply=Vec::new();let mut eof=false;let mut exit=None;
    loop {
        if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
        if Instant::now()>=deadline{return Err(invalid());}
        let mut block=[0u8;1024];
        if !eof {match output.read(&mut block){
            Ok(0)=>eof=true,
            Ok(n)=>{if reply.len()+n>1024{return Err(invalid());}reply.extend_from_slice(&block[..n]);},
            Err(e) if matches!(e.kind(),std::io::ErrorKind::Interrupted|std::io::ErrorKind::WouldBlock)=>{},
            Err(_)=>return Err(invalid()),
        }}
        if exit.is_none(){exit=child.poll().map_err(|_|invalid())?;}
        if let Some(status)=exit {if !status.success(){return Err(invalid());}if eof{break;}}
        thread::sleep(Duration::from_millis(5));
    }
    let _reaped=child.reaped_token()?;
    let after=source.metadata().map_err(|_|invalid())?;
    if stamp(&before)!=stamp(&after)||after.mode()&0o777!=0o600||after.uid()!=before.uid()||after.nlink()!=1{return Err(invalid());}
    if reply.pop()!=Some(b'\n'){return Err(invalid());}
    check_hint(&reply,&before)?;
    if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));} Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;
    #[test]
    fn hint_is_provisional_exact_and_binds_actual_fd_identity() {
        let path=std::env::temp_dir().join(format!("oex-hint-{}",Uuid::new_v4()));
        let file=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).unwrap();file.set_len(100).unwrap();
        let before=file.metadata().unwrap();
        let mut hint=Hint{version:1,kind:"oex-format-hint".into(),archive_format:"1.0.0-draft.10".into(),archive_bytes:100,
            manifest_sha256:"a".repeat(64),input_device:before.dev().to_string(),input_inode:before.ino().to_string()};
        for format in ["1.0.0-draft.9","1.0.0-draft.10"]{hint.archive_format=format.into();assert!(check_hint(&serde_json::to_vec(&hint).unwrap(),&before).is_ok());}
        hint.archive_format="1.0.0-draft.11".into();assert!(check_hint(&serde_json::to_vec(&hint).unwrap(),&before).is_err());
        hint.archive_format="1.0.0-draft.10".into();hint.input_inode=format!("0{}",before.ino());assert!(check_hint(&serde_json::to_vec(&hint).unwrap(),&before).is_err());
        hint.input_inode=before.ino().to_string();hint.archive_bytes=101;assert!(check_hint(&serde_json::to_vec(&hint).unwrap(),&before).is_err());
        hint.archive_bytes=100;let mut bytes=serde_json::to_vec(&hint).unwrap();bytes.push(b' ');assert!(check_hint(&bytes,&before).is_err());
        drop(file);fs::remove_file(path).unwrap();
    }
}
