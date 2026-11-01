// SPDX-License-Identifier: Apache-2.0
//! Inactive descriptor mapping for one directly spawned OEX materializer.
//! Mapping descriptors is not a live claim, authorization, child drain proof,
//! or an assurance that the files cannot be changed by another writer.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::{fs::MetadataExt, net::UnixStream, process::CommandExt};

/// The copies stay CLOEXEC in Manager and are retained through spawn. Their
/// actual high descriptor numbers are passed to the admitted child; no low-FD
/// remapping is performed.
#[allow(dead_code)]
pub(super) struct MaterializerFds { copies: Vec<File> }

pub(super) fn duplicate(fd: RawFd) -> Result<File> {
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 64) };
    if copy < 0 { return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE")); }
    let file=unsafe { File::from_raw_fd(copy) };
    if copy > 1023 { return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE")); }
    Ok(file)
}

#[allow(dead_code)]
impl MaterializerFds {
    /// `control` and `ack` are the child's ends of separate socket pairs.
    /// The caller independently establishes the live claim, pinned source
    /// directory, exclusive stage, and writer fence before using these FDs.
    pub(super) fn new(control: UnixStream, ack: UnixStream,
        stage: &super::oex_read_fd::Stage, chunks: &[super::oex_chunks::Chunk]) -> Result<Self> {
        if chunks.is_empty() || chunks.len() > 32 || control.as_raw_fd() == ack.as_raw_fd() {
            return Err(err("NATIVE_OEX_INPUT_INVALID"));
        }
        let output = stage.materializer_write();
        let stat = output.metadata().map_err(|_| err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
        if !stat.is_file() || stat.uid() != unsafe { libc::geteuid() } || stat.mode() & 0o777 != 0o600
            || stat.nlink() != 1 || stat.len() != 0 || flags < 0
            || flags & libc::O_ACCMODE != libc::O_RDWR || flags & libc::O_APPEND != 0 {
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        let mut copies = Vec::with_capacity(3 + chunks.len());
        let control_copy=duplicate(control.as_raw_fd())?;
        let ack_copy=duplicate(ack.as_raw_fd())?;
        let a=control_copy.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
        let b=ack_copy.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
        if (a.dev(),a.ino()) == (b.dev(),b.ino()) { return Err(err("NATIVE_OEX_INPUT_INVALID")); }
        copies.push(control_copy);
        copies.push(ack_copy);
        copies.push(duplicate(output.as_raw_fd())?);
        let mut seen=std::collections::HashSet::new();
        for chunk in chunks {
            let meta = chunk.file.metadata().map_err(|_| err("NATIVE_OEX_INPUT_INVALID"))?;
            let flags = unsafe { libc::fcntl(chunk.file.as_raw_fd(), libc::F_GETFL) };
            if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0
                || meta.nlink() != 1 || (meta.dev(), meta.ino(), meta.len()) != (chunk.device, chunk.inode, chunk.bytes)
                || flags < 0 || flags & libc::O_ACCMODE != libc::O_RDONLY
                || (meta.dev(), meta.ino()) == (stat.dev(), stat.ino())
                || !seen.insert((meta.dev(),meta.ino())) {
                return Err(err("NATIVE_OEX_INPUT_INVALID"));
            }
            copies.push(duplicate(chunk.file.as_raw_fd())?);
        }
        Ok(Self { copies })
    }

    /// Own all descriptor copies until spawn returns. Unlike low-FD dup2,
    /// passing the actual descriptors cannot overwrite Command's private
    /// exec-error pipe. Correlation below is not a claim or a source seal;
    /// the caller must retain the live fence and reap the returned Child.
    pub(super) fn spawn(self, mut command: Command, request:&super::oex_materializer_protocol::Request) -> Result<(std::process::Child,Vec<u8>)> {
        let encoded=request.encode()?;
        if self.copies.len()!=request.chunks.len()+3 {return Err(err("NATIVE_OEX_INPUT_INVALID"));}
        let output=self.copies[2].metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let output_flags=unsafe {libc::fcntl(self.copies[2].as_raw_fd(),libc::F_GETFL)};
        if !output.is_file() || output.uid()!=unsafe {libc::geteuid()} || output.mode()&0o777!=0o600 || output.nlink()!=1
            || output_flags<0 || output_flags&libc::O_ACCMODE!=libc::O_RDWR || output_flags&libc::O_APPEND!=0
            || output.dev().to_string()!=request.stage_device || output.ino().to_string()!=request.stage_inode || output.len()!=0 {
            return Err(err("NATIVE_OEX_OUTPUT_INVALID"));
        }
        for (file,expected) in self.copies[3..].iter().zip(&request.chunks) {
            let stat=file.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
            let flags=unsafe {libc::fcntl(file.as_raw_fd(),libc::F_GETFL)};
            if !stat.is_file() || stat.uid()!=unsafe {libc::geteuid()} || stat.mode()&0o077!=0 || stat.nlink()!=1
                || flags<0 || flags&libc::O_ACCMODE!=libc::O_RDONLY
                || stat.dev().to_string()!=expected.device || stat.ino().to_string()!=expected.inode || stat.len()!=expected.bytes {
                return Err(err("NATIVE_OEX_INPUT_INVALID"));
            }
        }
        let sources:Vec<RawFd>=self.copies.iter().map(AsRawFd::as_raw_fd).collect();
        let list=sources.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
        command.env("EXHIBITOS_OEX_MATERIALIZER_FDS",list);
        unsafe { command.pre_exec(move || {
            for fd in &sources {
                let flags=libc::fcntl(*fd,libc::F_GETFD);
                if flags<0 || libc::fcntl(*fd,libc::F_SETFD,flags & !libc::FD_CLOEXEC)<0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        }); }
        let child=command.spawn().map_err(|_|err("NATIVE_PROCESS_START_FAILED"));
        drop(self);
        child.map(|child|(child,encoded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    #[test]
    fn spawn_failure_and_aliased_channel_are_refused() {
        let path=std::env::temp_dir().join(format!("native-oex-materializer-fds-{}",Uuid::new_v4()));
        fs::create_dir(&path).unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o700)).unwrap();
        let dir=File::open(&path).unwrap();let stage=super::super::oex_read_fd::Stage::create(&dir,&Uuid::new_v4().to_string()).unwrap();
        let mut writable=OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join("chunk")).unwrap();
        writable.write_all(b"abc").unwrap();drop(writable);let file=File::open(path.join("chunk")).unwrap();let m=file.metadata().unwrap();
        let chunks=vec![super::super::oex_chunks::Chunk{file,device:m.dev(),inode:m.ino(),bytes:3}];
        let (control,_other_control)=UnixStream::pair().unwrap();let (ack,_other_ack)=UnixStream::pair().unwrap();
        let alias=control.try_clone().unwrap();let duplicate=control.try_clone().unwrap();
        assert!(MaterializerFds::new(duplicate,alias,&stage,&chunks).is_err());
        let fds=MaterializerFds::new(control,ack,&stage,&chunks).unwrap();
        let output=stage.materializer_write().metadata().unwrap();let mut request=super::super::oex_materializer_protocol::tests::request();
        request.stage_device=output.dev().to_string();request.stage_inode=output.ino().to_string();
        request.chunks[0].device=m.dev().to_string();request.chunks[0].inode=m.ino().to_string();
        assert!(fds.spawn(Command::new(path.join("missing-executable")),&request).is_err());
        drop(stage);drop(chunks);drop(dir);fs::remove_dir_all(path).unwrap();
    }
}
