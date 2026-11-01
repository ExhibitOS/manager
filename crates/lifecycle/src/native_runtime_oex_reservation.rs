// SPDX-License-Identifier: Apache-2.0
//! Inactive validation of provider receipt-v1 bytes. Digest/decoded fields
//! are correlation only: Node must hold and reverify the real live provider.
use super::*;
use std::collections::HashSet;

const MAGIC:&[u8]=b"EXHIBITOS-DISK-RESERVATION\0v1\0";
const MAX:u64=512*1024*1024;
struct Reader<'a>{bytes:&'a [u8],at:usize}
impl<'a> Reader<'a> {
    fn u32(&mut self)->Result<u32>{let end=self.at.checked_add(4).ok_or_else(bad)?;let b=self.bytes.get(self.at..end).ok_or_else(bad)?;self.at=end;Ok(u32::from_be_bytes(b.try_into().map_err(|_|bad())?))}
    fn text(&mut self)->Result<&'a str>{let n=self.u32()? as usize;if n>32768{return Err(bad());}let end=self.at.checked_add(n).ok_or_else(bad)?;let b=self.bytes.get(self.at..end).ok_or_else(bad)?;self.at=end;
        let s=std::str::from_utf8(b).map_err(|_|bad())?;if s.contains('\0'){return Err(bad());}Ok(s)}
}
fn bad()->LifecycleError {err("NATIVE_OEX_ATTEMPT_INVALID")}
fn uuid(s:&str)->bool {Uuid::parse_str(s).is_ok_and(|v|v.to_string()==s&&v.get_variant()==uuid::Variant::RFC4122&&(1..=8).contains(&v.get_version_num()))}
fn digits(s:&str)->bool {!s.is_empty()&&s.len()<=24&&s.bytes().all(|v|v.is_ascii_digit())}
fn canonical_bytes(s:&str)->Option<u64>{let n=s.parse::<u64>().ok()?;(n.to_string()==s&&(1..=2_147_483_648).contains(&n)).then_some(n)}

/// The stage path/device come from the held Manager pin, never from a path to
/// open supplied by the peer. Other simultaneous bytes must be independently
/// accounted before selecting a capacity; no other plan entry is summed.
#[allow(dead_code)]
pub(super) fn stage_capacity(bytes:&[u8],expected_digest:&str,id:&str,generation:&str,
    stage_path:&Path,stage_device:u64,archive_bytes:u64,other_stage_bytes:u64)->Result<u64>{
    if bytes.len()>32768||!bytes.starts_with(MAGIC)||!hash_valid(expected_digest)||digest(bytes)!=expected_digest
        || !uuid(id)||!uuid(generation)||!(1..=MAX).contains(&archive_bytes){return Err(bad());}
    let path=stage_path.to_str().ok_or_else(bad)?;if !stage_path.is_absolute(){return Err(bad());}
    let mut r=Reader{bytes,at:MAGIC.len()};
    if r.text()?!=id||r.text()?!=generation||r.text()? != "oex-import"{return Err(bad());}
    let system=r.text()?;let database=r.text()?;let store=r.text()?;let store_device=r.text()?;let store_inode=r.text()?;
    if !digits(system)||!digits(database)||!Path::new(store).is_absolute()||!digits(store_device)||!digits(store_inode){return Err(bad());}
    let count=r.u32()?;if !(1..=4).contains(&count){return Err(bad());}
    let mut seen=HashSet::new();let mut matching=None;
    for _ in 0..count {
        let root=r.text()?;let device=r.text()?;let amount=r.text()?;
        if !Path::new(root).is_absolute()||!digits(device)||!seen.insert(root){return Err(bad());}
        if root!=path && (Path::new(root).starts_with(stage_path)||stage_path.starts_with(Path::new(root))){return Err(bad());}
        let size=canonical_bytes(amount).ok_or_else(bad)?;
        if root==path {
            if device.parse::<u64>().ok()!=Some(stage_device)||matching.is_some(){return Err(bad());}
            matching=Some(size);
        }
    }
    if r.at!=bytes.len(){return Err(bad());}
    let occupied=archive_bytes.checked_add(other_stage_bytes).ok_or_else(bad)?;
    let remaining=matching.ok_or_else(bad)?.checked_sub(occupied).ok_or_else(bad)?;
    Ok(remaining.min(MAX))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    fn field(out:&mut Vec<u8>,s:&str){out.extend_from_slice(&(s.len() as u32).to_be_bytes());out.extend_from_slice(s.as_bytes());}
    pub(crate) fn fixture(id:&str,generation:&str,stage:&str,duplicate:bool)->Vec<u8>{
        let mut out=MAGIC.to_vec();
        for s in [id,generation,"oex-import","00123","00456","/store","1","2"]{field(&mut out,s);}
        out.extend_from_slice(&(if duplicate{2u32}else{1u32}).to_be_bytes());
        for _ in 0..if duplicate{2}else{1}{for s in [stage,"1","600000000"]{field(&mut out,s);}}
        out
    }
    #[test]
    fn one_exact_stage_entry_and_checked_capacity() {
        let id=Uuid::new_v4().to_string();let generation=Uuid::new_v4().to_string();let path=Path::new("/private/stage");
        let bytes=fixture(&id,&generation,"/private/stage",false);let sha=digest(&bytes);
        assert_eq!(stage_capacity(&bytes,&sha,&id,&generation,path,1,67_108_865,0).unwrap(),532_891_135);
        assert!(stage_capacity(&bytes,&sha,&id,&generation,path,2,67_108_865,0).is_err());
        assert!(stage_capacity(&bytes,&sha,&id,&generation,path,1,67_108_865,600_000_000).is_err());
        let duplicated=fixture(&id,&generation,"/private/stage",true);
        assert!(stage_capacity(&duplicated,&digest(&duplicated),&id,&generation,path,1,67_108_865,0).is_err());
        let mut trailing=bytes.clone();trailing.push(0);assert!(stage_capacity(&trailing,&digest(&trailing),&id,&generation,path,1,67_108_865,0).is_err());
    }
}
