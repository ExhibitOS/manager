// SPDX-License-Identifier: Apache-2.0
//! Inactive closed IPC frame shapes. Matching fields is correlation only: a
//! live Session, authenticated peer, journal claim, provider binding and held
//! workspace fence are separate prerequisites. EOF is terminal, never adopt.
use super::*;

const LIMIT: usize = 8192;

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Ready<'a> {version:u32,kind:&'static str,workspace_id:&'a str,tenant_id:&'a str,principal_id:&'a str,runtime_generation:&'a str}

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ReceiptReady {pub version:u32,pub kind:String,pub correlation_id:String,pub attempt_id:String,pub bytes:usize,pub sha256:String}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ReadReceipt<'a> {pub version:u32,pub kind:&'static str,pub correlation_id:&'a str,pub attempt_id:&'a str,pub offset:usize,pub length:usize}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ReceiptChunk {pub version:u32,pub kind:String,pub correlation_id:String,pub attempt_id:String,pub offset:usize,pub data:String}

/// Exact identity echo, not a substitute for a live Session/fence check.
#[allow(dead_code)]
pub(super) fn check_ready(bytes:&[u8],workspace:&str,tenant:&str,principal:&str,generation:&str)->Result<()> {
    if [workspace,tenant,principal,generation].iter().any(|v|!valid_id(v)) || bytes.is_empty() || bytes.len()>LIMIT {
        return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
    }
    let expected=Ready{version:1,kind:"operation-ready",workspace_id:workspace,tenant_id:tenant,principal_id:principal,runtime_generation:generation};
    if serde_json::to_vec(&expected).map_err(|_|err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"))?.as_slice()!=bytes {
        return Err(err("NATIVE_OWNER_CHANNEL_UNAVAILABLE"));
    }
    Ok(())
}

fn valid_id(value:&str)->bool {
    Uuid::parse_str(value).is_ok_and(|v|v.to_string()==value
        && v.get_variant()==uuid::Variant::RFC4122 && (1..=8).contains(&v.get_version_num()))
}
fn decimal(value:&str,nonzero:bool)->bool {
    value.parse::<u64>().is_ok_and(|v|v.to_string()==value && (!nonzero || v!=0))
}

/// Exact private save correlation. This is not a DB commit or permission to
/// release any claim; Manager also requires the real authenticated GET/PUT/GET.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct SaveFrame {
    pub version:u32,pub kind:String,pub correlation_id:String,pub attempt_id:String,
    pub exhibition_id:String,pub etag:String,pub request_id:String,
}
fn studio_etag(value:&str)->bool {
    let Some(body)=value.strip_prefix("\"studio-r").and_then(|s|s.strip_suffix('"')) else{return false;};
    let Some((revision,hash))=body.split_once('-') else{return false;};
    revision.parse::<u32>().is_ok_and(|n|n>0&&n<=2_147_483_647&&n.to_string()==revision)&&hash_valid(hash)
}
impl SaveFrame {
    pub(super) fn parse_ready(bytes:&[u8],attempt:&str)->Result<Self> {
        if bytes.is_empty()||bytes.len()>LIMIT{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let value:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if value.version!=1||value.kind!="save-ready"||value.attempt_id!=attempt
            || [&value.correlation_id,&value.attempt_id,&value.exhibition_id,&value.request_id].iter().any(|v|!valid_id(v))
            || !studio_etag(&value.etag)||serde_json::to_vec(&value).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?!=bytes {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(value)
    }
    pub(super) fn observed(&self,etag:&str)->Result<Vec<u8>> {
        if !studio_etag(etag)||etag==self.etag{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let mut v=self.clone();v.kind="save-observed".into();v.etag=etag.into();
        serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))
    }
    pub(super) fn check_verified(&self,etag:&str,reply:&[u8])->Result<()> {
        let observed=self.observed(etag)?;let mut v:Self=serde_json::from_slice(&observed).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;v.kind="save-verified".into();
        if serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?!=reply{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(())
    }
    pub(super) fn check_completed(&self,etag:&str,reply:&[u8])->Result<()> {
        let observed=self.observed(etag)?;let mut v:Self=serde_json::from_slice(&observed).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;v.kind="operation-complete".into();
        if serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?!=reply{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(())
    }
}

/// Private one-shot selection correlation; no token, file path or client
/// authority crosses this frame. The caller validates the retained intent.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct UploadSelection {
    pub version:u32,pub kind:String,pub correlation_id:String,pub runtime_generation:String,
    pub workspace_id:String,pub tenant_id:String,pub principal_id:String,pub request_id:String,
    pub archive_bytes:u64,pub archive_sha256:String,
}
impl UploadSelection {
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        if self.version!=1||self.kind!="upload-selection"||[&self.correlation_id,&self.runtime_generation,&self.workspace_id,&self.tenant_id,&self.principal_id,&self.request_id].iter().any(|v|!valid_id(v))
            || !(1..=512*1024*1024).contains(&self.archive_bytes)||!hash_valid(&self.archive_sha256){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let bytes=serde_json::to_vec(self).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.len()>LIMIT{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(bytes)
    }
    pub(super) fn check_selected(&self,reply:&[u8])->Result<()> {
        let mut expected=self.clone();expected.kind="upload-selected".into();
        if self.encode().is_err()||reply!=serde_json::to_vec(&expected).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice(){return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(())
    }
}

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ClaimOpen {
    pub version:u32, pub kind:String, pub correlation_id:String, pub runtime_generation:String,
    pub workspace_id:String, pub tenant_id:String, pub job_id:String,
}

/// Syntactic and Session-identity correlation only. Neither digest is
/// independently authenticated here and this type grants no MAC or claim.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ClaimChallenge {
    pub version:u32,pub kind:String,pub correlation_id:String,pub workspace_id:String,pub tenant_id:String,
    pub principal_id:String,pub runtime_generation:String,pub job_id:String,pub attempt_id:String,
    pub generation:String,pub stage_id:String,pub reservation_id:String,pub reservation_generation:String,
    pub claim_digest:String,pub receipt_digest:String,
}
/// Selected fresh-test settlement correlation. It is meaningful only while
/// the original live Session, claim, worker and file fences are held.
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Settlement<'a> {
    version:u32, kind:&'static str, correlation_id:&'a str, attempt_id:&'a str,
    runtime_generation:&'a str, generation:&'a str, reservation_id:&'a str,
    reservation_generation:&'a str, receipt_digest:&'a str,
}
#[allow(dead_code)]
impl ClaimChallenge {
    pub(super) fn settlement(&self,kind:&'static str)->Result<Vec<u8>> {
        if !matches!(kind,"settle-check"|"settle-ready"|"settle-receipt")
            || [&self.correlation_id,&self.attempt_id,&self.runtime_generation,&self.generation,
                &self.reservation_id,&self.reservation_generation].iter().any(|v|!valid_id(v))
            || !hash_valid(&self.receipt_digest) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        serde_json::to_vec(&Settlement{version:1,kind,correlation_id:&self.correlation_id,attempt_id:&self.attempt_id,
            runtime_generation:&self.runtime_generation,generation:&self.generation,reservation_id:&self.reservation_id,
            reservation_generation:&self.reservation_generation,receipt_digest:&self.receipt_digest})
            .map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))
    }
    pub(super) fn parse(bytes:&[u8],open:&ClaimOpen,principal:&str)->Result<Self> {
        if open.encode().is_err() || !valid_id(principal) || bytes.is_empty() || bytes.len()>LIMIT {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let c:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if c.version!=1 || c.kind!="claim-challenge" || c.correlation_id!=open.correlation_id
            || c.workspace_id!=open.workspace_id || c.tenant_id!=open.tenant_id
            || c.principal_id!=principal || c.runtime_generation!=open.runtime_generation || c.job_id!=open.job_id
            || [&c.attempt_id,&c.generation,&c.stage_id,&c.reservation_id,&c.reservation_generation].iter().any(|v|!valid_id(v))
            || !hash_valid(&c.claim_digest) || !hash_valid(&c.receipt_digest)
            || serde_json::to_vec(&c).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        Ok(c)
    }
}

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ClaimReady {
    pub version:u32,pub kind:String,pub correlation_id:String,pub runtime_generation:String,pub workspace_id:String,
    pub tenant_id:String,pub job_id:String,pub user_id:String,pub request_id:String,pub lease_id:String,
    pub attempt_id:String,pub generation:String,pub stage_id:String,pub reservation_id:String,pub reservation_generation:String,
    pub archive_bytes:u64,pub archive_sha256:String,pub source_digest:String,pub reservation_digest:String,
    pub manager_binding_digest:String,pub profile:String,pub format:String,
}
#[allow(dead_code)]
impl ClaimReady {
    /// Correlates the response with the exact one-shot challenge. The API's
    /// independently held claim/provider/fence remain indispensable.
    pub(super) fn parse(bytes:&[u8],c:&ClaimChallenge,mac:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str)->Result<Self> {
        if bytes.is_empty()||bytes.len()>LIMIT||!hash_valid(mac)||!hash_valid(expected_sha256)
            || !valid_id(expected_request) || !(1..=512*1024*1024).contains(&expected_bytes) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let r:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if r.version!=1 || r.kind!="claim-ready" || r.correlation_id!=c.correlation_id || r.runtime_generation!=c.runtime_generation
            || r.workspace_id!=c.workspace_id || r.tenant_id!=c.tenant_id || r.job_id!=c.job_id || r.user_id!=c.principal_id
            || r.attempt_id!=c.attempt_id || r.generation!=c.generation || r.stage_id!=c.stage_id
            || r.reservation_id!=c.reservation_id || r.reservation_generation!=c.reservation_generation
            || r.request_id!=expected_request || !valid_id(&r.lease_id) || r.archive_bytes!=expected_bytes
            || r.archive_sha256!=expected_sha256 || r.source_digest!=r.archive_sha256 || r.reservation_digest!=c.receipt_digest
            || r.manager_binding_digest!=mac || r.profile!="bounded-file-v1" || r.format!="1.0.0-draft.9"
            || serde_json::to_vec(&r).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        Ok(r)
    }
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ClaimBinding<'a> {
    pub version:u32,pub kind:&'static str,pub correlation_id:&'a str,pub runtime_generation:&'a str,
    pub attempt_id:&'a str,pub generation:&'a str,pub stage_id:&'a str,pub reservation_id:&'a str,
    pub reservation_generation:&'a str,pub claim_digest:&'a str,pub receipt_digest:&'a str,pub manager_binding_digest:&'a str,
}
#[allow(dead_code)]
impl ClaimChallenge {
    /// The encoded MAC is correlation only; the API must independently
    /// verify its held claim, actual provider receipt, and common fence.
    pub(super) fn binding(&self,mac:&str)->Result<Vec<u8>> {
        if !hash_valid(mac) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let reply=ClaimBinding{version:1,kind:"claim-binding",correlation_id:&self.correlation_id,
            runtime_generation:&self.runtime_generation,attempt_id:&self.attempt_id,generation:&self.generation,
            stage_id:&self.stage_id,reservation_id:&self.reservation_id,reservation_generation:&self.reservation_generation,
            claim_digest:&self.claim_digest,receipt_digest:&self.receipt_digest,manager_binding_digest:mac};
        let bytes=serde_json::to_vec(&reply).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.is_empty()||bytes.len()>LIMIT{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(bytes)
    }
}
#[allow(dead_code)]
impl ClaimOpen {
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        if self.version!=1 || self.kind!="claim-open" || [&self.correlation_id,&self.runtime_generation,
            &self.workspace_id,&self.tenant_id,&self.job_id].iter().any(|s|!valid_id(s)) {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let bytes=serde_json::to_vec(self).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.is_empty() || bytes.len()>LIMIT {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        Ok(bytes)
    }
}

#[derive(Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct StageBound {
    pub version:u32, pub kind:String, pub correlation_id:String, pub runtime_generation:String,
    pub workspace_id:String, pub tenant_id:String, pub job_id:String, pub attempt_id:String,
    pub generation:String, pub stage_id:String, pub reservation_id:String, pub reservation_generation:String,
    pub stage_device:String, pub stage_inode:String,
}
#[allow(dead_code)]
impl StageBound {
    fn valid(&self)->bool {
        self.version==1 && self.kind=="stage-bound"
            && [&self.correlation_id,&self.runtime_generation,&self.workspace_id,&self.tenant_id,&self.job_id,
                &self.attempt_id,&self.generation,&self.stage_id,&self.reservation_id,&self.reservation_generation]
                .iter().all(|s|valid_id(s))
            && decimal(&self.stage_device,false) && decimal(&self.stage_inode,true)
    }
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        if !self.valid() {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let bytes=serde_json::to_vec(self).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.is_empty() || bytes.len()>LIMIT {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        Ok(bytes)
    }
    /// Only an exact echo on the retained authenticated connection may advance
    /// correlation. It does not prove the API observed this file descriptor.
    pub(super) fn check_accepted(&self,sent:&[u8],reply:&[u8])->Result<()> {
        if self.encode()?.as_slice()!=sent || reply.is_empty() || reply.len()>LIMIT {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let mut exact=self.clone();exact.kind="stage-accepted".into();
        if serde_json::to_vec(&exact).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?.as_slice()!=reply {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let mut accepted:StageBound=serde_json::from_slice(reply).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        if accepted.kind!="stage-accepted" {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        accepted.kind="stage-bound".into();
        if &accepted!=self {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        Ok(())
    }
}

// There is deliberately no claim-ready parser, bind action, or production
// caller: managerBindingDigest has no agreed producer/encoding or meaning yet.

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_verification_correlates_exact_attempt_and_new_etag() {
        let id=||Uuid::new_v4().to_string();let attempt=id();let old=format!("\"studio-r1-{}\"","a".repeat(64));let next=format!("\"studio-r2-{}\"","b".repeat(64));
        let ready=SaveFrame{version:1,kind:"save-ready".into(),correlation_id:id(),attempt_id:attempt.clone(),exhibition_id:id(),etag:old,request_id:id()};
        let bytes=serde_json::to_vec(&ready).unwrap();let parsed=SaveFrame::parse_ready(&bytes,&attempt).unwrap();
        assert!(SaveFrame::parse_ready(&bytes,&id()).is_err());assert!(parsed.observed(&parsed.etag).is_err());
        let observed=parsed.observed(&next).unwrap();let mut verified:SaveFrame=serde_json::from_slice(&observed).unwrap();verified.kind="save-verified".into();
        let bytes=serde_json::to_vec(&verified).unwrap();assert!(parsed.check_verified(&next,&bytes).is_ok());verified.request_id=id();
        assert!(parsed.check_verified(&next,&serde_json::to_vec(&verified).unwrap()).is_err());
    }
    #[test]
    fn challenge_only_correlates_exact_request_and_principal() {
        let id=||Uuid::new_v4().to_string();let principal=id();
        let open=ClaimOpen{version:1,kind:"claim-open".into(),correlation_id:id(),runtime_generation:id(),workspace_id:id(),tenant_id:id(),job_id:id()};
        let challenge=ClaimChallenge{version:1,kind:"claim-challenge".into(),correlation_id:open.correlation_id.clone(),
            workspace_id:open.workspace_id.clone(),tenant_id:open.tenant_id.clone(),principal_id:principal.clone(),
            runtime_generation:open.runtime_generation.clone(),job_id:open.job_id.clone(),attempt_id:id(),generation:id(),stage_id:id(),
            reservation_id:id(),reservation_generation:id(),claim_digest:"a".repeat(64),receipt_digest:"b".repeat(64)};
        let bytes=serde_json::to_vec(&challenge).unwrap();assert!(ClaimChallenge::parse(&bytes,&open,&principal).is_ok());
        assert!(ClaimChallenge::parse(&bytes,&open,&id()).is_err());
        let mut extra=bytes.clone();extra.pop();extra.extend_from_slice(b",\"extra\":true}");assert!(ClaimChallenge::parse(&extra,&open,&principal).is_err());
        let mac="c".repeat(64);let request=id();let archive="d".repeat(64);
        let ready=ClaimReady{version:1,kind:"claim-ready".into(),correlation_id:challenge.correlation_id.clone(),
            runtime_generation:challenge.runtime_generation.clone(),workspace_id:challenge.workspace_id.clone(),tenant_id:challenge.tenant_id.clone(),
            job_id:challenge.job_id.clone(),user_id:principal.clone(),request_id:request.clone(),lease_id:id(),attempt_id:challenge.attempt_id.clone(),
            generation:challenge.generation.clone(),stage_id:challenge.stage_id.clone(),reservation_id:challenge.reservation_id.clone(),
            reservation_generation:challenge.reservation_generation.clone(),archive_bytes:67_108_865,archive_sha256:archive.clone(),source_digest:archive.clone(),
            reservation_digest:challenge.receipt_digest.clone(),manager_binding_digest:mac.clone(),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
        let response=serde_json::to_vec(&ready).unwrap();
        assert!(ClaimReady::parse(&response,&challenge,&mac,&request,ready.archive_bytes,&archive).is_ok());
        assert!(ClaimReady::parse(&response,&challenge,&mac,&id(),ready.archive_bytes,&archive).is_err());
        assert!(ClaimReady::parse(&response,&challenge,&mac,&request,ready.archive_bytes+1,&archive).is_err());
    }
    #[test]
    fn ready_requires_exact_personal_session_and_runtime() {
        let w=Uuid::new_v4().to_string();let t=Uuid::new_v4().to_string();let p=Uuid::new_v4().to_string();let g=Uuid::new_v4().to_string();
        let bytes=serde_json::to_vec(&Ready{version:1,kind:"operation-ready",workspace_id:&w,tenant_id:&t,principal_id:&p,runtime_generation:&g}).unwrap();
        assert!(check_ready(&bytes,&w,&t,&p,&g).is_ok());
        assert!(check_ready(&bytes,&w,&t,&p,&Uuid::new_v4().to_string()).is_err());
        let mut extra=bytes.clone();extra.pop();extra.extend_from_slice(b",\"extra\":true}");assert!(check_ready(&extra,&w,&t,&p,&g).is_err());
    }
    #[test]
    fn stage_reply_requires_every_field_and_exact_sent_bytes() {
        let id=||Uuid::new_v4().to_string();
        let stage=StageBound {version:1,kind:"stage-bound".into(),correlation_id:id(),runtime_generation:id(),
            workspace_id:id(),tenant_id:id(),job_id:id(),attempt_id:id(),generation:id(),stage_id:id(),
            reservation_id:id(),reservation_generation:id(),stage_device:"1".into(),stage_inode:"2".into()};
        let sent=stage.encode().unwrap();let mut exact=stage.clone();exact.kind="stage-accepted".into();
        let accepted=serde_json::to_vec(&exact).unwrap();assert!(stage.check_accepted(&sent,&accepted).is_ok());
        let mut changed=exact.clone();changed.job_id=id();assert!(stage.check_accepted(&sent,&serde_json::to_vec(&changed).unwrap()).is_err());
        let mut changed=exact.clone();changed.stage_inode="02".into();assert!(stage.check_accepted(&sent,&serde_json::to_vec(&changed).unwrap()).is_err());
        let mut changed=exact.clone();changed.job_id="BAD".into();assert!(stage.check_accepted(&sent,&serde_json::to_vec(&changed).unwrap()).is_err());
        let mut extra=accepted.clone();extra.pop();extra.extend_from_slice(b",\"unknown\":true}");assert!(stage.check_accepted(&sent,&extra).is_err());
        let mut whitespace=accepted.clone();whitespace.insert(1,b' ');assert!(stage.check_accepted(&sent,&whitespace).is_err());
        assert!(stage.check_accepted(b"{}",&accepted).is_err());
    }
}
