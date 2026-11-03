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
    /// Retire only the two already pinned, private temporary names after the
    /// caller has proved its owned writers and consumer have drained. A
    /// partial unlink is uncertain; this method never retries or removes the
    /// directory/journal. The cooperative fence does not exclude same-user
    /// outsiders and removal does not prove physical blocks were reclaimed.
    pub(super) fn retire_pair(&self,session:&mut Session,owner:&super::oex_owner::Owner,ready:&super::owner_claim_frames::ClaimReady,input:&File,output:&File,cancelled:&AtomicBool)->Result<()> {
        let (stage,attempt)=(&ready.stage_id,&ready.attempt_id);
        let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
        owner.check(session)?;if cancelled.load(Ordering::SeqCst){return Err(bad());}
        let a=input.metadata().map_err(|_|bad())?;let b=output.metadata().map_err(|_|bad())?;
        let ids=[(a.dev(),a.ino()),(b.dev(),b.ino())];
        self.check_pair(session,stage,ids[0],attempt,ids[1])?;
        let dir=self.pin.try_clone_file()?;
        let names=[format!(".oex-stage-{stage}"),format!(".oex-packed-{attempt}")];
        let mut check=|| {owner.check(session)?;self.check(session)?;if cancelled.load(Ordering::SeqCst){return Err(bad());}Ok(())};
        retire_names(&dir,&self.path,&names,input,output,&mut check)
    }
}

/// Caller already owns the private directory and its cooperative writer fence.
fn retire_names(dir:&File,path:&Path,names:&[String;2],input:&File,output:&File,check:&mut dyn FnMut()->Result<()>)->Result<()> {
        let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
        let a=input.metadata().map_err(|_|bad())?;let b=output.metadata().map_err(|_|bad())?;
        let ids=[(a.dev(),a.ino()),(b.dev(),b.ino())];
        if ids[0]==ids[1]||names[0]==names[1]{return Err(bad());}
        let files=[input,output];let before=[&a,&b];
        let verify=|index:usize|->Result<std::ffi::CString> {
            let name=&names[index];let file=files[index];let expected=ids[index];let before=before[index];
            let c=std::ffi::CString::new(name.as_bytes()).map_err(|_|bad())?;let mut st=std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe{libc::fstatat(dir.as_raw_fd(),c.as_ptr(),st.as_mut_ptr(),libc::AT_SYMLINK_NOFOLLOW)}!=0{return Err(bad());}
            let named=unsafe{st.assume_init()};let held=file.metadata().map_err(|_|bad())?;
            if named.st_mode&libc::S_IFMT!=libc::S_IFREG||named.st_uid!=unsafe{libc::geteuid()}||named.st_mode&0o777!=0o600||named.st_nlink!=1
                || (named.st_dev as u64,named.st_ino)!=expected||(held.dev(),held.ino(),held.nlink())!=(expected.0,expected.1,1)
                || (held.len(),held.mtime(),held.mtime_nsec(),held.ctime(),held.ctime_nsec())!=(before.len(),before.mtime(),before.mtime_nsec(),before.ctime(),before.ctime_nsec()) {return Err(bad());}Ok(c)
        };
        check()?;verify(0)?;verify(1)?;
        for index in 0..2 {
            check()?;let c=verify(index)?;let file=files[index];let expected=ids[index];let before=before[index];
            if unsafe{libc::unlinkat(dir.as_raw_fd(),c.as_ptr(),0)}!=0{return Err(bad());}
            let after=file.metadata().map_err(|_|bad())?;
            if (after.dev(),after.ino(),after.nlink())!=(expected.0,expected.1,0)
                || (after.len(),after.mtime(),after.mtime_nsec())!=(before.len(),before.mtime(),before.mtime_nsec()){return Err(bad());}
        }
        dir.sync_all().map_err(|_|bad())?;check()?;
        if fs::read_dir(path).map_err(|_|bad())?.next().is_some(){return Err(bad());}
        check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;
    #[test]
    fn retire_names_only_owned_pair_and_preserves_partial_failure() {
        for case in ["success","wrong-output","interrupted"] {
            let path=std::env::temp_dir().join(format!("oex-v3-retire-{}",Uuid::new_v4()));fs::create_dir(&path).unwrap();
            let dir=File::open(&path).unwrap();let names=["input".to_string(),"output".to_string()];
            let open=|name:&str|OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path.join(name)).unwrap();
            let input=open(&names[0]);let output=open(&names[1]);
            let result=if case=="wrong-output" {retire_names(&dir,&path,&names,&input,&input,&mut ||Ok(()))}
                else {let calls=std::cell::Cell::new(0);retire_names(&dir,&path,&names,&input,&output,&mut ||{calls.set(calls.get()+1);if case=="interrupted"&&calls.get()==3 {Err(err("NATIVE_CANCELLED"))}else{Ok(())}})};
            match case {
                "success"=>{assert!(result.is_ok());assert_eq!(input.metadata().unwrap().nlink(),0);assert_eq!(output.metadata().unwrap().nlink(),0);},
                "wrong-output"=>{assert!(result.is_err());assert!(path.join("input").exists()&&path.join("output").exists());},
                _=>{assert!(result.is_err());assert!(!path.join("input").exists()&&path.join("output").exists());},
            }
            drop((input,output,dir));fs::remove_dir_all(path).unwrap();
        }
    }
}
