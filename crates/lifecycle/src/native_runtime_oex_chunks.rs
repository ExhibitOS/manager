// SPDX-License-Identifier: Apache-2.0
//! Local FileBlobStore OEX input handles only. This does not authenticate DB
//! ownership, prevent concurrent writers, or grant materializer/drain authority.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;

const CHUNK: u64 = 16 * 1024 * 1024;
const MAX: u64 = 512 * 1024 * 1024;
#[allow(dead_code)]
pub(super) struct Chunk { pub(super) file: File, pub(super) device: u64, pub(super) inode: u64, pub(super) bytes: u64 }
fn canonical_uuid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok_and(|v| v.to_string() == s && v.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&v.get_version_num()))
}
fn open_relative(parent: &File, name: &str, directory: bool) -> Result<File> {
    let name=std::ffi::CString::new(name).map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
    let flags=libc::O_RDONLY|libc::O_NOFOLLOW|libc::O_NONBLOCK|libc::O_CLOEXEC|if directory {libc::O_DIRECTORY} else {0};
    let fd=unsafe{libc::openat(parent.as_raw_fd(),name.as_ptr(),flags)};
    if fd<0{return Err(err("NATIVE_OEX_INPUT_INVALID"));}
    let file=unsafe{File::from_raw_fd(fd)};let m=file.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
    if m.uid()!=unsafe{libc::geteuid()}||m.mode()&0o077!=0||if directory{!m.is_dir()}else{!m.is_file()||m.nlink()!=1}{return Err(err("NATIVE_OEX_INPUT_INVALID"));}
    Ok(file)
}
/// Open only the deterministic local chunk keys for an already-authorized job.
/// The supplied root must be the held Session root; the caller separately
/// proves the live job, reservation, source stability, and process ownership.
#[allow(dead_code)]
pub(super) fn open_local(root:&File,tenant:&str,job:&str,total:u64)->Result<Vec<Chunk>>{
    let m=root.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
    if !m.is_dir()||m.uid()!=unsafe{libc::geteuid()}||m.mode()&0o777!=0o700||!canonical_uuid(tenant)||!canonical_uuid(job)||!(1..=MAX).contains(&total){return Err(err("NATIVE_OEX_INPUT_INVALID"));}
    let blobs=open_relative(root,"blobs",true)?;let tenant_dir=open_relative(&blobs,tenant,true)?;
    let staging=open_relative(&tenant_dir,"oex-staging",true)?;let job_dir=open_relative(&staging,job,true)?;
    let count=total.div_ceil(CHUNK);let mut result=Vec::with_capacity(count as usize);
    for index in 0..count {
        let file=open_relative(&job_dir,&format!("chunk-{index}"),false)?;
        let stat=file.metadata().map_err(|_|err("NATIVE_OEX_INPUT_INVALID"))?;
        let expected=CHUNK.min(total-index*CHUNK);
        if stat.len()!=expected{return Err(err("NATIVE_OEX_INPUT_INVALID"));}
        result.push(Chunk{file,device:stat.dev(),inode:stat.ino(),bytes:stat.len()});
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt,symlink};
    #[test]
    fn opens_only_exact_private_local_chunk_without_following_links() {
        let root=std::env::temp_dir().join(format!("native-oex-chunks-{}",Uuid::new_v4()));
        fs::create_dir(&root).unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();
        let tenant=Uuid::new_v4().to_string();let job=Uuid::new_v4().to_string();
        let dirs=[root.join("blobs"),root.join("blobs").join(&tenant),root.join("blobs").join(&tenant).join("oex-staging"),root.join("blobs").join(&tenant).join("oex-staging").join(&job)];
        for d in &dirs {fs::create_dir(d).unwrap();fs::set_permissions(d,fs::Permissions::from_mode(0o700)).unwrap();}
        write_private_new(&dirs[3],"chunk-0",b"abc",3).unwrap();let handle=File::open(&root).unwrap();
        let chunks=open_local(&handle,&tenant,&job,3).unwrap();assert_eq!(chunks.len(),1);assert_eq!(chunks[0].bytes,3);
        assert!(open_local(&handle,&tenant,&job,4).is_err());assert!(open_local(&handle,"../bad",&job,3).is_err());
        drop(chunks);fs::rename(dirs[3].join("chunk-0"),dirs[3].join("original")).unwrap();symlink("original",dirs[3].join("chunk-0")).unwrap();
        assert!(open_local(&handle,&tenant,&job,3).is_err());drop(handle);fs::remove_dir_all(root).unwrap();
    }
}
