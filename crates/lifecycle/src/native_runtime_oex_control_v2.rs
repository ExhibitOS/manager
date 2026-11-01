// SPDX-License-Identifier: Apache-2.0
//! Inactive fixed-worker request. Canonical fields correlate exact handles and
//! selected operation; they are not proof of Session, reservation or authority.
use super::*;

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct Control {
    pub version:u32,pub kind:String,pub tenant_id:String,pub resource_id:String,pub job_id:String,pub user_id:String,
    pub request_id:String,pub lease_id:String,pub attempt_id:String,pub runtime_generation:String,pub stage_id:String,
    pub reservation_id:String,pub reservation_generation:String,pub archive_bytes:u64,pub archive_sha256:String,pub capacity:u64,
    pub input_device:String,pub input_inode:String,pub output_device:String,pub output_inode:String,
    pub reservation_digest:String,pub source_digest:String,pub worker_digest:String,pub profile:String,pub format:String,
}
fn decimal(s:&str,nonzero:bool)->bool {s.parse::<u64>().is_ok_and(|n|n.to_string()==s&&(!nonzero||n!=0))}
#[allow(dead_code)]
impl Control {
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        let bad=||err("NATIVE_OEX_ATTEMPT_INVALID");
        if self.version!=2||self.kind!="oex-packed-v2"||self.profile!="bounded-file-v1"||self.format!="1.0.0-draft.9"
            || !(1..=512*1024*1024).contains(&self.archive_bytes)||self.capacity>512*1024*1024
            || self.source_digest!=self.archive_sha256
            || [&self.tenant_id,&self.resource_id,&self.job_id,&self.user_id,&self.request_id,&self.lease_id,&self.attempt_id,
                &self.runtime_generation,&self.stage_id,&self.reservation_id,&self.reservation_generation].iter().any(|v|
                    Uuid::parse_str(v).ok().is_none_or(|id|id.to_string()!=v.as_str()||id.get_variant()!=uuid::Variant::RFC4122||!(1..=8).contains(&id.get_version_num())))
            || [&self.archive_sha256,&self.reservation_digest,&self.source_digest,&self.worker_digest].iter().any(|v|!hash_valid(v))
            || !decimal(&self.input_device,false)||!decimal(&self.output_device,false)
            || !decimal(&self.input_inode,true)||!decimal(&self.output_inode,true)
            || (&self.input_device,&self.input_inode)==(&self.output_device,&self.output_inode) {return Err(bad());}
        let bytes=serde_json::to_vec(self).map_err(|_|bad())?;if bytes.is_empty()||bytes.len()>8192{return Err(bad());}Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_mismatched_source_and_alias_handles() {
        let id=||Uuid::new_v4().to_string();let sha="a".repeat(64);
        let mut value=Control{version:2,kind:"oex-packed-v2".into(),tenant_id:id(),resource_id:id(),job_id:id(),user_id:id(),
            request_id:id(),lease_id:id(),attempt_id:id(),runtime_generation:id(),stage_id:id(),reservation_id:id(),reservation_generation:id(),
            archive_bytes:67_108_865,archive_sha256:sha.clone(),capacity:100,input_device:"1".into(),input_inode:"2".into(),
            output_device:"1".into(),output_inode:"3".into(),reservation_digest:"b".repeat(64),source_digest:sha.clone(),
            worker_digest:"c".repeat(64),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
        let bytes=value.encode().unwrap();let parsed:Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(parsed["version"],2);
        value.source_digest="d".repeat(64);assert!(value.encode().is_err());value.source_digest=sha;
        value.output_inode="2".into();assert!(value.encode().is_err());
    }
}
