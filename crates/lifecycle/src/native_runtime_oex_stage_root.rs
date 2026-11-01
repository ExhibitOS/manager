// SPDX-License-Identifier: Apache-2.0
//! Inactive per-Session private stage directory pin. This is not a provider
//! reservation, claim, available-space guarantee, or permission to release.
use super::*;
use std::os::fd::{AsRawFd,FromRawFd};
use std::os::unix::fs::MetadataExt;

#[allow(dead_code)]
pub(super) struct StageRoot {pin:crate::maintenance_source::DirectoryPin,path:PathBuf,device:u64,inode:u64,runtime:String}

#[allow(dead_code)]
impl StageRoot {
    /// Create once under the pinned personal root before Node's preclaim. An
    /// existing/partial entry is never adopted, removed or overwritten.
    pub(super) fn create(session:&mut Session)->Result<Self> {
        session.poll()?;session.check_native_observation_fence()?;
        let current=session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=session.service.personal_identity()?;
        if session.state!="ready"||session.node.is_none()||session.postgres.is_none()||current.owner_uid!=unsafe{libc::geteuid()}
            || (current.workspace_id.as_str(),current.tenant_id.as_str(),current.principal_id.as_str())
                !=(identity.0.as_str(),identity.1.as_str(),identity.2.as_str()){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let runtime=session.job_id.clone().ok_or_else(||err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if Uuid::parse_str(&runtime).ok().is_none_or(|id|id.to_string()!=runtime){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let parent=session.observation_root.try_clone_file()?;
        let parent_stat=parent.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !parent_stat.is_dir()||parent_stat.uid()!=unsafe{libc::geteuid()}||parent_stat.mode()&0o777!=0o700{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let name=format!("native-oex-stage-{runtime}");let c=std::ffi::CString::new(name.as_str()).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if unsafe{libc::mkdirat(parent.as_raw_fd(),c.as_ptr(),0o700)}!=0{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        parent.sync_all().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let path=session.service.root.join(name);let pin=crate::maintenance_source::DirectoryPin::open(&path)?;
        let file=pin.try_clone_file()?;let stat=file.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !stat.is_dir()||stat.uid()!=unsafe{libc::geteuid()}||stat.mode()&0o777!=0o700{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let result=Self{pin,path,device:stat.dev(),inode:stat.ino(),runtime};result.check(session)?;Ok(result)
    }
    pub(super) fn path(&self)->&Path {&self.path}
    pub(super) fn device(&self)->u64 {self.device}
    pub(super) fn check_empty(&self,session:&mut Session)->Result<()> {
        self.check(session)?;
        if fs::read_dir(&self.path).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?.next().is_some(){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        self.check(session)
    }
    pub(super) fn check(&self,session:&mut Session)->Result<()> {
        session.poll()?;session.check_native_observation_fence()?;self.pin.check()?;
        let parent=session.observation_root.try_clone_file()?;let parent_stat=parent.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let file=self.pin.try_clone_file()?;let stat=file.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let auth=session.local_authority.as_ref().ok_or_else(||err("NATIVE_PERSONAL_SESSION_REQUIRED"))?;
        let identity=session.service.personal_identity()?;
        if session.state!="ready"||session.node.is_none()||session.postgres.is_none()||session.job_id.as_deref()!=Some(self.runtime.as_str())
            || auth.owner_uid!=unsafe{libc::geteuid()} || (auth.workspace_id.as_str(),auth.tenant_id.as_str(),auth.principal_id.as_str())
                !=(identity.0.as_str(),identity.1.as_str(),identity.2.as_str())
            || !parent_stat.is_dir()||parent_stat.uid()!=unsafe{libc::geteuid()}||parent_stat.mode()&0o777!=0o700
            || !stat.is_dir()||stat.uid()!=unsafe{libc::geteuid()}||stat.mode()&0o777!=0o700||(stat.dev(),stat.ino())!=(self.device,self.inode){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        Ok(())
    }
    /// Correlate the retained descriptor with the current name under the pinned
    /// stage directory. This does not exclude unrelated same-user writers.
    pub(super) fn check_input(&self,session:&mut Session,stage:&str,device:u64,inode:u64)->Result<()> {
        self.check(session)?;
        if Uuid::parse_str(stage).ok().is_none_or(|id|id.to_string()!=stage) || device!=self.device {return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let dir=self.pin.try_clone_file()?;
        let expected=format!(".oex-stage-{stage}");
        let mut entries=fs::read_dir(&self.path).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let entry=entries.next().ok_or_else(||err("NATIVE_OEX_OUTPUT_INVALID"))?.map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if entry.file_name()!=std::ffi::OsStr::new(&expected) || entries.next().is_some(){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let name=std::ffi::CString::new(expected).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let mut named=std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe{libc::fstatat(dir.as_raw_fd(),name.as_ptr(),named.as_mut_ptr(),libc::AT_SYMLINK_NOFOLLOW)}!=0{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let named=unsafe{named.assume_init()};
        if named.st_mode&libc::S_IFMT!=libc::S_IFREG || named.st_uid!=unsafe{libc::geteuid()} || named.st_mode&0o777!=0o600 || named.st_nlink!=1
            || (named.st_dev as u64,named.st_ino as u64)!=(device,inode){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        self.check(session)
    }
    /// Both exact files must remain named under this same pinned directory.
    /// This check does not exclude an uncooperative same-user writer.
    pub(super) fn check_pair(&self,session:&mut Session,stage:&str,input:(u64,u64),attempt:&str,output:(u64,u64))->Result<()> {
        self.check(session)?;
        for id in [stage,attempt] {if Uuid::parse_str(id).ok().is_none_or(|v|v.to_string()!=id){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}}
        if input.0!=self.device||output.0!=self.device||input==output{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let names=[format!(".oex-stage-{stage}"),format!(".oex-packed-{attempt}")];
        let mut actual=Vec::new();for entry in fs::read_dir(&self.path).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))? {actual.push(entry.map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?.file_name());}
        if actual.len()!=2||names.iter().any(|name|!actual.iter().any(|v|v==std::ffi::OsStr::new(name))){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let dir=self.pin.try_clone_file()?;
        for (name,expected) in names.iter().zip([input,output]) {
            let c=std::ffi::CString::new(name.as_str()).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;let mut st=std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe{libc::fstatat(dir.as_raw_fd(),c.as_ptr(),st.as_mut_ptr(),libc::AT_SYMLINK_NOFOLLOW)}!=0{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
            let st=unsafe{st.assume_init()};if st.st_mode&libc::S_IFMT!=libc::S_IFREG||st.st_uid!=unsafe{libc::geteuid()}||st.st_mode&0o777!=0o600||st.st_nlink!=1||(st.st_dev as u64,st.st_ino as u64)!=expected{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        }
        self.check(session)
    }
    pub(super) fn create_input(&self,session:&mut Session,stage:&str)->Result<super::oex_read_fd::Stage> {
        self.check(session)?;let file=self.pin.try_clone_file()?;
        let result=super::oex_read_fd::Stage::create(&file,stage)?;self.check(session)?;Ok(result)
    }
    pub(super) fn create_output(&self,session:&mut Session,attempt:&str)->Result<File> {
        self.check(session)?;
        if Uuid::parse_str(attempt).ok().is_none_or(|id|id.to_string()!=attempt){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let dir=self.pin.try_clone_file()?;let name=std::ffi::CString::new(format!(".oex-packed-{attempt}")).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let fd=unsafe{libc::openat(dir.as_raw_fd(),name.as_ptr(),libc::O_RDWR|libc::O_CREAT|libc::O_EXCL|libc::O_NOFOLLOW|libc::O_CLOEXEC,0o600)};
        if fd<0{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}let file=unsafe{File::from_raw_fd(fd)};
        let stat=file.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if !stat.is_file()||stat.uid()!=unsafe{libc::geteuid()}||stat.mode()&0o777!=0o600||stat.nlink()!=1||stat.len()!=0||stat.dev()!=self.device{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        file.sync_all().and_then(|_|dir.sync_all()).map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        let mut named=std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe{libc::fstatat(dir.as_raw_fd(),name.as_ptr(),named.as_mut_ptr(),libc::AT_SYMLINK_NOFOLLOW)}!=0{return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        let named=unsafe{named.assume_init()};let after=file.metadata().map_err(|_|err("NATIVE_OEX_OUTPUT_INVALID"))?;
        if named.st_mode&libc::S_IFMT!=libc::S_IFREG || named.st_uid!=stat.uid() || named.st_mode&0o777!=0o600 || named.st_nlink!=1
            || (named.st_dev as u64,named.st_ino as u64)!=(stat.dev(),stat.ino())
            || (after.dev(),after.ino(),after.len(),after.nlink())!=(stat.dev(),stat.ino(),0,1){return Err(err("NATIVE_OEX_OUTPUT_INVALID"));}
        self.check(session)?;Ok(file)
    }
}
