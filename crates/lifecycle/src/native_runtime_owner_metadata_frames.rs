// SPDX-License-Identifier: Apache-2.0
//! Inactive bounded, path-free transfer of the already checked worker ACK.
//! Correlation and byte transfer do not establish claim, drain or commit.
use super::*;
use base64::{engine::general_purpose::STANDARD,Engine as _};

fn uuid(value:&str)->bool {Uuid::parse_str(value).is_ok_and(|id|id.to_string()==value
    && id.get_variant()==uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num()))}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Announcement<'a> {version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,bytes:usize,sha256:String}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Read {version:u32,kind:String,correlation_id:String,attempt_id:String,offset:usize,length:usize}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Chunk<'a> {version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,offset:usize,data:String}

#[allow(dead_code)]
pub(super) struct Transfer<'a> {bytes:&'a [u8],attempt:&'a str,next:usize}
#[allow(dead_code)]
impl<'a> Transfer<'a> {
    pub(super) fn new(bytes:&'a [u8],attempt:&'a str,correlation:&str)->Result<(Self,Vec<u8>)> {
        let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
        if bytes.is_empty()||bytes.len()>4*1024*1024||!uuid(attempt)||!uuid(correlation){return Err(bad());}
        let msg=Announcement{version:1,kind:"metadata-ready",correlation_id:correlation,attempt_id:attempt,bytes:bytes.len(),sha256:digest(bytes)};
        let encoded=serde_json::to_vec(&msg).map_err(|_|bad())?;
        Ok((Self{bytes,attempt,next:0},encoded))
    }
    pub(super) fn complete(&self)->bool {self.next==self.bytes.len()}
    /// Strict sequential access only; no caller-provided path or seek.
    pub(super) fn reply(&mut self,request:&[u8])->Result<Vec<u8>> {
        let bad=||err("NATIVE_OEX_WORKER_UNCERTAIN");
        if self.complete()||request.is_empty()||request.len()>8192{return Err(bad());}
        let value:Read=serde_json::from_slice(request).map_err(|_|bad())?;
        let expected=4096.min(self.bytes.len()-self.next);
        if serde_json::to_vec(&value).map_err(|_|bad())?.as_slice()!=request
            || value.version!=1||value.kind!="read-metadata"||!uuid(&value.correlation_id)||value.attempt_id!=self.attempt
            || value.offset!=self.next||value.length!=expected{return Err(bad());}
        let end=self.next.checked_add(expected).ok_or_else(bad)?;
        let msg=Chunk{version:1,kind:"metadata-chunk",correlation_id:&value.correlation_id,attempt_id:self.attempt,
            offset:self.next,data:STANDARD.encode(&self.bytes[self.next..end])};
        let encoded=serde_json::to_vec(&msg).map_err(|_|bad())?;
        if encoded.len()>8192{return Err(bad());}self.next=end;Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_order_and_lengths_prevent_seek_and_replay() {
        let attempt=Uuid::new_v4().to_string();let correlation=Uuid::new_v4().to_string();let data=vec![7u8;4097];
        let (mut transfer,ready)=Transfer::new(&data,&attempt,&correlation).unwrap();assert!(!ready.is_empty());
        let request=Read{version:1,kind:"read-metadata".into(),correlation_id:Uuid::new_v4().to_string(),attempt_id:attempt.clone(),offset:0,length:4096};
        let first=serde_json::to_vec(&request).unwrap();assert!(transfer.reply(&first).is_ok());assert!(transfer.reply(&first).is_err());
        let last=Read{correlation_id:Uuid::new_v4().to_string(),offset:4096,length:1,..request};
        assert!(transfer.reply(&serde_json::to_vec(&last).unwrap()).is_ok());assert!(transfer.complete());
    }
}
