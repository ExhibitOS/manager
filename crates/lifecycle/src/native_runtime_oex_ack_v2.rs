// SPDX-License-Identifier: Apache-2.0
//! Inactive v2 fixed-child reply and packed-byte verification. This does not
//! prove a live claim, writer drain, Spec provenance, DB commit or release.
use super::*;
use base64::{engine::general_purpose::STANDARD,Engine as _};
use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt,MetadataExt};

const MAX_ACK:usize=4*1024*1024;
const MAX_DOCUMENT:usize=1024*1024;
const MAX_OUTPUT:u64=512*1024*1024;
const MAX_ENTRY:u64=32*1024*1024;

#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {pub path:String,pub offset:u64,pub bytes:u64,pub sha256:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Ack {
    version:u32,kind:String,status:String,attempt_id:String,runtime_generation:String,
    reservation_id:String,reservation_generation:String,receipt_sha256:String,archive_sha256:String,
    manifest_sha256:String,exhibition_sha256:String,manifest_base64:String,exhibition_base64:String,
    output_device:String,output_inode:String,output_bytes:u64,entries:Vec<Entry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt,PermissionsExt};
    #[test]
    fn correlates_documents_and_actual_packed_bytes() {
        let dir=std::env::temp_dir().join(format!("native-oex-ack-v2-{}",Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();fs::set_permissions(&dir,fs::Permissions::from_mode(0o700)).unwrap();
        let mut output=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(dir.join("packed")).unwrap();
        output.write_all(b"abc").unwrap();output.sync_all().unwrap();let m=output.metadata().unwrap();
        let attempt=Uuid::new_v4().to_string();let runtime=Uuid::new_v4().to_string();let reservation=Uuid::new_v4().to_string();let generation=Uuid::new_v4().to_string();
        let receipt="a".repeat(64);let archive="b".repeat(64);let manifest=b"{}";let exhibition=b"{\"a\":1}";
        let expected=Expected{attempt_id:&attempt,runtime_generation:&runtime,reservation_id:&reservation,reservation_generation:&generation,
            receipt_sha256:&receipt,archive_sha256:&archive,output_device:m.dev(),output_inode:m.ino(),output_bytes:3,capacity:3};
        let ack=Ack{version:2,kind:"prepared".into(),status:"complete".into(),attempt_id:attempt.clone(),runtime_generation:runtime.clone(),
            reservation_id:reservation.clone(),reservation_generation:generation.clone(),receipt_sha256:receipt.clone(),archive_sha256:archive.clone(),
            manifest_sha256:digest(manifest),exhibition_sha256:digest(exhibition),manifest_base64:STANDARD.encode(manifest),
            exhibition_base64:STANDARD.encode(exhibition),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),output_bytes:3,
            entries:vec![Entry{path:"assets/a".into(),offset:0,bytes:3,sha256:digest(b"abc")} ]};
        let data=serde_json::to_vec(&ack).unwrap();let cancel=AtomicBool::new(false);
        assert!(check(&data,&expected,&output,&cancel,&mut ||Ok(())).is_ok());
        let mut changed=ack.clone();changed.manifest_base64.push('=');assert!(check(&serde_json::to_vec(&changed).unwrap(),&expected,&output,&cancel,&mut ||Ok(())).is_err());
        let mut changed=ack.clone();changed.entries[0].path="assets/../bad".into();assert!(check(&serde_json::to_vec(&changed).unwrap(),&expected,&output,&cancel,&mut ||Ok(())).is_err());
        output.write_at(b"x",0).unwrap();assert!(check(&data,&expected,&output,&cancel,&mut ||Ok(())).is_err());
        let empty=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(dir.join("empty")).unwrap();let em=empty.metadata().unwrap();
        let expected_empty=Expected{output_device:em.dev(),output_inode:em.ino(),output_bytes:0,capacity:0,..expected};
        let mut empty_ack=ack.clone();empty_ack.output_device=em.dev().to_string();empty_ack.output_inode=em.ino().to_string();empty_ack.output_bytes=0;empty_ack.entries.clear();
        assert!(check(&serde_json::to_vec(&empty_ack).unwrap(),&expected_empty,&empty,&cancel,&mut ||Ok(())).is_ok());drop(empty);
        drop(output);fs::remove_dir_all(dir).unwrap();
    }
}
#[allow(dead_code)]
pub(super) struct Expected<'a> {
    pub attempt_id:&'a str,pub runtime_generation:&'a str,pub reservation_id:&'a str,pub reservation_generation:&'a str,
    pub receipt_sha256:&'a str,pub archive_sha256:&'a str,pub output_device:u64,pub output_inode:u64,
    pub output_bytes:u64,pub capacity:u64,
}
/// Byte correlation only; keeping this object does not keep Session or fence alive.
#[allow(dead_code)]
pub(super) struct Verified {
    /// Exact bounded worker reply, for separately validated same-Session chunk
    /// transfer. Possession of these bytes is not proof of authority or drain.
    pub raw_ack:Vec<u8>,pub manifest:Vec<u8>,pub exhibition:Vec<u8>,pub entries:Vec<Entry>,
    pub manifest_sha256:String,pub exhibition_sha256:String,
    /// Exact post-hash output state, carried into streaming.
    pub output_stamp:(u64,u64,u64,i64,i64,i64,i64),
}

fn document(value:&str,expected:&str)->Result<Vec<u8>> {
    let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
    if !hash_valid(expected)||value.is_empty()||value.len()>4*((MAX_DOCUMENT+2)/3){return Err(bad());}
    let bytes=STANDARD.decode(value).map_err(|_|bad())?;
    if bytes.is_empty()||bytes.len()>MAX_DOCUMENT||STANDARD.encode(&bytes)!=value||digest(&bytes)!=expected{return Err(bad());}
    Ok(bytes)
}
/// Mirror Spec's bounded ZIP path profile without interpreting any path as a
/// filesystem location; the consumer still independently checks its manifest.
fn entry_path(path:&str)->bool {
    if path.is_empty()||path.len()>240||!path.is_ascii(){return false;}
    path.split('/').all(|part| {
        let mut chars=part.bytes();
        if !chars.next().is_some_and(|c|c.is_ascii_alphanumeric()||c==b'_'||c==b'-') || part.ends_with('.')
            || !chars.all(|c|c.is_ascii_alphanumeric()||b"_.-".contains(&c)){return false;}
        let stem=part.split('.').next().unwrap_or("").to_ascii_lowercase();
        !matches!(stem.as_str(),"con"|"prn"|"aux"|"nul"|"com1"|"com2"|"com3"|"com4"|"com5"|"com6"|"com7"|"com8"|"com9"|"lpt1"|"lpt2"|"lpt3"|"lpt4"|"lpt5"|"lpt6"|"lpt7"|"lpt8"|"lpt9")
    })
}

/// Call only after the directly owned child has actually been reaped. The
/// caller holds the pinned output FD, selected Session and provider fence;
/// Node independently validates the documents, rights and inventory later.
#[allow(dead_code)]
pub(super) fn check(bytes:&[u8],expected:&Expected<'_>,output:&File,cancelled:&AtomicBool,
    check_owner:&mut dyn FnMut()->Result<()>) -> Result<Verified> {
    let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
    if bytes.is_empty()||bytes.len()>MAX_ACK || expected.capacity>MAX_OUTPUT
        || expected.output_bytes>expected.capacity || expected.output_inode==0
        || !hash_valid(expected.receipt_sha256)||!hash_valid(expected.archive_sha256)
        || [expected.attempt_id,expected.runtime_generation,expected.reservation_id,expected.reservation_generation]
            .iter().any(|v|Uuid::parse_str(v).ok().is_none_or(|id|id.to_string()!=*v||id.get_variant()!=uuid::Variant::RFC4122||!(1..=8).contains(&id.get_version_num()))) {
        return Err(bad());
    }
    let ack:Ack=serde_json::from_slice(bytes).map_err(|_|bad())?;
    if serde_json::to_vec(&ack).map_err(|_|bad())?.as_slice()!=bytes || ack.version!=2 || ack.kind!="prepared" || ack.status!="complete"
        || ack.attempt_id!=expected.attempt_id || ack.runtime_generation!=expected.runtime_generation
        || ack.reservation_id!=expected.reservation_id || ack.reservation_generation!=expected.reservation_generation
        || ack.receipt_sha256!=expected.receipt_sha256 || ack.archive_sha256!=expected.archive_sha256
        || ack.output_device!=expected.output_device.to_string() || ack.output_inode!=expected.output_inode.to_string()
        || ack.output_bytes!=expected.output_bytes || ack.entries.len()>254 {return Err(bad());}
    let manifest=document(&ack.manifest_base64,&ack.manifest_sha256)?;
    let exhibition=document(&ack.exhibition_base64,&ack.exhibition_sha256)?;
    check_owner()?;
    if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    let before=output.metadata().map_err(|_|bad())?;
    let flags=unsafe{libc::fcntl(output.as_raw_fd(),libc::F_GETFL)};
    if !before.is_file()||before.uid()!=unsafe{libc::geteuid()}||before.mode()&0o777!=0o600||before.nlink()!=1
        || flags<0 || flags&libc::O_ACCMODE!=libc::O_RDWR || flags&libc::O_APPEND!=0
        || (before.dev(),before.ino(),before.len())!=(expected.output_device,expected.output_inode,expected.output_bytes){return Err(bad());}
    let mut paths=HashSet::new();let mut offset=0u64;let mut buffer=[0u8;65536];
    for entry in &ack.entries {
        let folded=entry.path.to_ascii_lowercase();
        if !entry_path(&entry.path)
            || paths.iter().any(|prior:&String|folded.starts_with(&(prior.clone()+"/"))||prior.starts_with(&(folded.clone()+"/")))
            || !paths.insert(folded)||entry.offset!=offset||entry.bytes==0||entry.bytes>MAX_ENTRY||!hash_valid(&entry.sha256){return Err(bad());}
        let end=offset.checked_add(entry.bytes).ok_or_else(bad)?;if end>expected.output_bytes{return Err(bad());}
        let mut h=Sha256::new();let mut at=offset;
        while at<end {
            check_owner()?;if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
            let length=((end-at) as usize).min(buffer.len());let n=output.read_at(&mut buffer[..length],at).map_err(|_|bad())?;
            if n==0{return Err(bad());}h.update(&buffer[..n]);at+=n as u64;
        }
        if format!("{:x}",h.finalize())!=entry.sha256{return Err(bad());}offset=end;
    }
    let after=output.metadata().map_err(|_|bad())?;
    if offset!=expected.output_bytes || !after.is_file() || after.uid()!=before.uid()||after.mode()&0o777!=0o600||after.nlink()!=1
        || (after.dev(),after.ino(),after.len(),after.mtime(),after.mtime_nsec(),after.ctime(),after.ctime_nsec())
            !=(before.dev(),before.ino(),before.len(),before.mtime(),before.mtime_nsec(),before.ctime(),before.ctime_nsec()){return Err(bad());}
    check_owner()?;if cancelled.load(Ordering::SeqCst){return Err(err("NATIVE_CANCELLED"));}
    Ok(Verified{raw_ack:bytes.to_vec(),manifest,exhibition,entries:ack.entries,manifest_sha256:ack.manifest_sha256,exhibition_sha256:ack.exhibition_sha256,output_stamp:(after.dev(),after.ino(),after.len(),after.mtime(),after.mtime_nsec(),after.ctime(),after.ctime_nsec())})
}
