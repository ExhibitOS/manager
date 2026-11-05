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
/// V3 save correlation is separate from the selected v2 settlement protocol.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct SaveFrameV3 {
    pub version:u32,pub kind:String,pub correlation_id:String,pub operation_correlation_id:String,
    pub attempt_id:String,pub runtime_generation:String,pub stage_id:String,pub report_sha256:String,
    pub exhibition_id:String,pub etag:String,pub request_id:String,
}
#[allow(dead_code)]
impl SaveFrameV3 {
    pub(super) fn parse_ready(bytes:&[u8],bound:&super::oex_preflight_frames::Bound)->Result<Self> {
        if bytes.is_empty()||bytes.len()>LIMIT{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let v:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        let identity=bound.operation_identity();
        if v.version!=1||v.kind!="save-ready-v3"||!valid_id(&v.correlation_id)||v.correlation_id==identity.0
            || (v.operation_correlation_id.as_str(),v.attempt_id.as_str(),v.runtime_generation.as_str(),v.stage_id.as_str(),v.report_sha256.as_str())!=identity
            || !valid_id(&v.exhibition_id)||!valid_id(&v.request_id)||!studio_etag(&v.etag)
            || serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        Ok(v)
    }
    pub(super) fn observed(&self,etag:&str)->Result<Vec<u8>> {
        if !studio_etag(etag)||etag==self.etag{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let mut v=self.clone();v.kind="save-observed-v3".into();v.etag=etag.into();
        serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))
    }
    pub(super) fn check_verified(&self,etag:&str,bytes:&[u8])->Result<()> {
        let mut v=self.clone();self.observed(etag)?;v.kind="save-verified-v3".into();v.etag=etag.into();
        if serde_json::to_vec(&v).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(())
    }
    pub(super) fn check_completed(&self,etag:&str,bytes:&[u8])->Result<()> {
        #[derive(Serialize)] #[serde(rename_all="camelCase")]
        struct Completed<'a>{version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,exhibition_id:&'a str,etag:&'a str,request_id:&'a str}
        self.observed(etag)?;
        let expected=Completed{version:1,kind:"operation-complete-v3",correlation_id:&self.correlation_id,attempt_id:&self.attempt_id,exhibition_id:&self.exhibition_id,etag,request_id:&self.request_id};
        if serde_json::to_vec(&expected).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}Ok(())
    }
}
/// Wire correlation only: the caller must first establish actual save,
/// no-further-writes, child drain and safe temporary-byte retirement.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct SettlementWireV3 {
    version:u32,kind:String,correlation_id:String,operation_correlation_id:String,attempt_id:String,runtime_generation:String,generation:String,stage_id:String,report_sha256:String,
    input_reservation_id:String,input_reservation_generation:String,input_receipt_digest:String,
    output_reservation_id:Option<String>,output_reservation_generation:Option<String>,output_receipt_digest:Option<String>,
    exhibition_id:String,etag:String,request_id:String,
}
pub(super) struct SettlementV3(SettlementWireV3);
#[allow(dead_code)]
impl SettlementV3 {
    pub(super) fn parse_check(bytes:&[u8],c:&ClaimChallenge,bound:&super::oex_preflight_frames::Bound,save:&SaveFrameV3,etag:&str)->Result<Self> {
        if bytes.is_empty()||bytes.len()>LIMIT||c.kind!="claim-challenge-v3" {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        SaveFrameV3::parse_ready(&serde_json::to_vec(save).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?,bound)?;
        let s:SettlementWireV3=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        let identity=bound.operation_identity();let (_,_,output)=bound.output_plan();
        let actual_output=match (&s.output_reservation_id,&s.output_reservation_generation,&s.output_receipt_digest) {
            (None,None,None)=>None,(Some(id),Some(generation),Some(digest))=>Some((id.as_str(),generation.as_str(),digest.as_str())),_=>return Err(err("NATIVE_OEX_ATTEMPT_INVALID")),
        };
        if (c.correlation_id.as_str(),c.attempt_id.as_str(),c.runtime_generation.as_str(),c.stage_id.as_str())!=(identity.0,identity.1,identity.2,identity.3)
            || (c.generation.as_str(),c.reservation_id.as_str(),c.reservation_generation.as_str(),c.receipt_digest.as_str())!=bound.input_plan()
            || (save.operation_correlation_id.as_str(),save.attempt_id.as_str(),save.runtime_generation.as_str(),save.stage_id.as_str(),save.report_sha256.as_str())!=identity
            || s.version!=1||s.kind!="settle-check-v3"||!valid_id(&s.correlation_id)||s.correlation_id==identity.0||s.correlation_id==save.correlation_id
            || (s.operation_correlation_id.as_str(),s.attempt_id.as_str(),s.runtime_generation.as_str(),s.stage_id.as_str(),s.report_sha256.as_str())!=identity
            || s.generation!=c.generation||s.input_reservation_id!=c.reservation_id||s.input_reservation_generation!=c.reservation_generation
            || s.input_receipt_digest!=c.receipt_digest||actual_output!=output||s.exhibition_id!=save.exhibition_id||s.request_id!=save.request_id
            || s.etag!=etag||!studio_etag(etag)||etag==save.etag
            || serde_json::to_vec(&s).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?.as_slice()!=bytes {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        Ok(Self(s))
    }
    fn encoded_as(&self,kind:&str)->Result<Vec<u8>> {
        let mut result=self.0.clone();result.kind=kind.into();serde_json::to_vec(&result).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))
    }
    pub(super) fn ready(&self)->Result<Vec<u8>> {self.encoded_as("settle-ready-v3")}
    pub(super) fn receipt_ack(&self,receipt:&[u8])->Result<Vec<u8>> {
        if self.encoded_as("settle-receipt-v3")?.as_slice()!=receipt{return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        self.encoded_as("settle-receipt-ack-v3")
    }
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
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub archive_format:Option<String>,
}
impl UploadSelection {
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        if self.version!=1||self.kind!="upload-selection"||[&self.correlation_id,&self.runtime_generation,&self.workspace_id,&self.tenant_id,&self.principal_id,&self.request_id].iter().any(|v|!valid_id(v))
            || self.archive_format.as_deref().is_some_and(|f| !matches!(f,"1.0.0-draft.9"|"1.0.0-draft.10"))
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
        if self.kind!="claim-challenge" || !matches!(kind,"settle-check"|"settle-ready"|"settle-receipt")
            || [&self.correlation_id,&self.attempt_id,&self.runtime_generation,&self.generation,
                &self.reservation_id,&self.reservation_generation].iter().any(|v|!valid_id(v))
            || !hash_valid(&self.receipt_digest) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        serde_json::to_vec(&Settlement{version:1,kind,correlation_id:&self.correlation_id,attempt_id:&self.attempt_id,
            runtime_generation:&self.runtime_generation,generation:&self.generation,reservation_id:&self.reservation_id,
            reservation_generation:&self.reservation_generation,receipt_digest:&self.receipt_digest})
            .map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))
    }
    pub(super) fn parse(bytes:&[u8],open:&ClaimOpen,principal:&str)->Result<Self> {
        Self::parse_for(bytes,open,principal,"claim-open","claim-challenge")
    }
    pub(super) fn parse_v3(bytes:&[u8],open:&ClaimOpen,principal:&str)->Result<Self> {
        Self::parse_for(bytes,open,principal,"claim-open-v3","claim-challenge-v3")
    }
    fn parse_for(bytes:&[u8],open:&ClaimOpen,principal:&str,open_kind:&str,challenge_kind:&str)->Result<Self> {
        if open.encode_for(open_kind).is_err() || !valid_id(principal) || bytes.is_empty() || bytes.len()>LIMIT {
            return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
        }
        let c:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if c.version!=1 || c.kind!=challenge_kind || c.correlation_id!=open.correlation_id
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
        Self::parse_for(bytes,c,mac,expected_request,expected_bytes,expected_sha256,("claim-challenge","claim-ready"))
    }
    pub(super) fn parse_v3(bytes:&[u8],c:&ClaimChallenge,mac:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str)->Result<Self> {
        Self::parse_for(bytes,c,mac,expected_request,expected_bytes,expected_sha256,("claim-challenge-v3","claim-ready-v3"))
    }
    pub(super) fn parse_v4(bytes:&[u8],c:&ClaimChallenge,mac:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str,format:&str)->Result<Self> {
        Self::parse_for_format(bytes,c,mac,expected_request,expected_bytes,expected_sha256,("claim-challenge-v3","claim-ready-v3"),format)
    }
    fn parse_for(bytes:&[u8],c:&ClaimChallenge,mac:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str,kinds:(&str,&str))->Result<Self> {
        Self::parse_for_format(bytes,c,mac,expected_request,expected_bytes,expected_sha256,kinds,"1.0.0-draft.9")
    }
    fn parse_for_format(bytes:&[u8],c:&ClaimChallenge,mac:&str,expected_request:&str,expected_bytes:u64,expected_sha256:&str,kinds:(&str,&str),format:&str)->Result<Self> {
        if !matches!(format,"1.0.0-draft.9"|"1.0.0-draft.10") {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let (challenge_kind,ready_kind)=kinds;
        if bytes.is_empty()||bytes.len()>LIMIT||!hash_valid(mac)||!hash_valid(expected_sha256)
            || !valid_id(expected_request) || !(1..=512*1024*1024).contains(&expected_bytes) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let r:Self=serde_json::from_slice(bytes).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if c.kind!=challenge_kind || r.version!=1 || r.kind!=ready_kind || r.correlation_id!=c.correlation_id || r.runtime_generation!=c.runtime_generation
            || r.workspace_id!=c.workspace_id || r.tenant_id!=c.tenant_id || r.job_id!=c.job_id || r.user_id!=c.principal_id
            || r.attempt_id!=c.attempt_id || r.generation!=c.generation || r.stage_id!=c.stage_id
            || r.reservation_id!=c.reservation_id || r.reservation_generation!=c.reservation_generation
            || r.request_id!=expected_request || !valid_id(&r.lease_id) || r.archive_bytes!=expected_bytes
            || r.archive_sha256!=expected_sha256 || r.source_digest!=r.archive_sha256 || r.reservation_digest!=c.receipt_digest
            || r.manager_binding_digest!=mac || r.profile!="bounded-file-v1" || r.format!=format
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
        self.binding_for(mac,"claim-challenge","claim-binding")
    }
    pub(super) fn binding_v3(&self,mac:&str)->Result<Vec<u8>> {
        self.binding_for(mac,"claim-challenge-v3","claim-binding-v3")
    }
    fn binding_for(&self,mac:&str,challenge_kind:&str,binding_kind:&'static str)->Result<Vec<u8>> {
        if self.kind!=challenge_kind || !hash_valid(mac) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let reply=ClaimBinding{version:1,kind:binding_kind,correlation_id:&self.correlation_id,
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
        self.encode_for("claim-open")
    }
    pub(super) fn encode_v3(&self)->Result<Vec<u8>> {
        self.encode_for("claim-open-v3")
    }
    fn encode_for(&self,kind:&str)->Result<Vec<u8>> {
        if self.version!=1 || self.kind!=kind || [&self.correlation_id,&self.runtime_generation,
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
    fn valid_for(&self,kind:&str)->bool {
        self.version==1 && self.kind==kind
            && [&self.correlation_id,&self.runtime_generation,&self.workspace_id,&self.tenant_id,&self.job_id,
                &self.attempt_id,&self.generation,&self.stage_id,&self.reservation_id,&self.reservation_generation]
                .iter().all(|s|valid_id(s))
            && decimal(&self.stage_device,false) && decimal(&self.stage_inode,true)
    }
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        self.encode_for("stage-bound")
    }
    pub(super) fn encode_v3(&self)->Result<Vec<u8>> {
        self.encode_for("stage-bound-v3")
    }
    fn encode_for(&self,kind:&str)->Result<Vec<u8>> {
        if !self.valid_for(kind) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        let bytes=serde_json::to_vec(self).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
        if bytes.is_empty() || bytes.len()>LIMIT {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
        Ok(bytes)
    }
    /// Only an exact echo on the retained authenticated connection may advance
    /// correlation. It does not prove the API observed this file descriptor.
    pub(super) fn check_accepted(&self,sent:&[u8],reply:&[u8])->Result<()> {
        self.check_accepted_for(sent,reply,"stage-bound","stage-accepted")
    }
    pub(super) fn check_accepted_v3(&self,sent:&[u8],reply:&[u8])->Result<()> {
        self.check_accepted_for(sent,reply,"stage-bound-v3","stage-accepted-v3")
    }
    fn check_accepted_for(&self,sent:&[u8],reply:&[u8],bound_kind:&str,accepted_kind:&str)->Result<()> {
        if self.encode_for(bound_kind)?.as_slice()!=sent || reply.is_empty() || reply.len()>LIMIT {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let mut exact=self.clone();exact.kind=accepted_kind.into();
        if serde_json::to_vec(&exact).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?.as_slice()!=reply {
            return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
        }
        let mut accepted:StageBound=serde_json::from_slice(reply).map_err(|_|err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
        if accepted.kind!=accepted_kind {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        accepted.kind=bound_kind.into();
        if &accepted!=self {return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));}
        Ok(())
    }
}

// Frame validation proves correlation only. The owning Session and API keep
// independent live provider, process, file and two-receipt settlement guards.

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
    fn v3_claim_frames_reject_cross_version_and_changed_bindings() {
        let id=||Uuid::new_v4().to_string();let principal=id();
        let open=ClaimOpen{version:1,kind:"claim-open-v3".into(),correlation_id:id(),runtime_generation:id(),workspace_id:id(),tenant_id:id(),job_id:id()};
        assert!(open.encode_v3().is_ok());assert!(open.encode().is_err());
        let challenge=ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:open.correlation_id.clone(),
            workspace_id:open.workspace_id.clone(),tenant_id:open.tenant_id.clone(),principal_id:principal.clone(),
            runtime_generation:open.runtime_generation.clone(),job_id:open.job_id.clone(),attempt_id:id(),generation:id(),stage_id:id(),
            reservation_id:id(),reservation_generation:id(),claim_digest:"a".repeat(64),receipt_digest:"b".repeat(64)};
        let bytes=serde_json::to_vec(&challenge).unwrap();
        assert!(ClaimChallenge::parse_v3(&bytes,&open,&principal).is_ok());
        assert!(ClaimChallenge::parse(&bytes,&open,&principal).is_err());
        assert!(ClaimChallenge::parse_v3(&bytes,&open,&id()).is_err());
        let mac="c".repeat(64);let request=id();let archive="d".repeat(64);
        let binding=challenge.binding_v3(&mac).unwrap();
        let value:Value=serde_json::from_slice(&binding).unwrap();assert_eq!(value["kind"],"claim-binding-v3");
        assert!(challenge.binding(&mac).is_err());
        assert!(challenge.settlement("settle-check").is_err());
        let ready=ClaimReady{version:1,kind:"claim-ready-v3".into(),correlation_id:challenge.correlation_id.clone(),
            runtime_generation:challenge.runtime_generation.clone(),workspace_id:challenge.workspace_id.clone(),tenant_id:challenge.tenant_id.clone(),
            job_id:challenge.job_id.clone(),user_id:principal,request_id:request.clone(),lease_id:id(),attempt_id:challenge.attempt_id.clone(),
            generation:challenge.generation.clone(),stage_id:challenge.stage_id.clone(),reservation_id:challenge.reservation_id.clone(),
            reservation_generation:challenge.reservation_generation.clone(),archive_bytes:100,archive_sha256:archive.clone(),source_digest:archive.clone(),
            reservation_digest:challenge.receipt_digest.clone(),manager_binding_digest:mac.clone(),profile:"bounded-file-v1".into(),format:"1.0.0-draft.9".into()};
        let response=serde_json::to_vec(&ready).unwrap();
        assert!(ClaimReady::parse_v3(&response,&challenge,&mac,&request,100,&archive).is_ok());
        assert!(ClaimReady::parse(&response,&challenge,&mac,&request,100,&archive).is_err());
        assert!(ClaimReady::parse_v3(&response,&challenge,&mac,&request,101,&archive).is_err());
        let mut ready=ready;ready.format="1.0.0-draft.10".into();let response=serde_json::to_vec(&ready).unwrap();
        assert!(ClaimReady::parse_v3(&response,&challenge,&mac,&request,100,&archive).is_err());
        assert!(ClaimReady::parse_v4(&response,&challenge,&mac,&request,100,&archive,"1.0.0-draft.10").is_ok());
        assert!(ClaimReady::parse_v4(&response,&challenge,&mac,&request,100,&archive,"1.0.0-draft.9").is_err());
        assert!(ClaimReady::parse_v4(&response,&challenge,&mac,&request,100,&archive,"1.0.0-draft.11").is_err());
        let mut selection=UploadSelection{version:1,kind:"upload-selection".into(),correlation_id:id(),runtime_generation:challenge.runtime_generation,
            workspace_id:challenge.workspace_id,tenant_id:challenge.tenant_id,principal_id:challenge.principal_id,request_id:request,
            archive_bytes:100,archive_sha256:archive,archive_format:None};
        let old=selection.encode().unwrap();let value:Value=serde_json::from_slice(&old).unwrap();assert!(value.get("archiveFormat").is_none());
        selection.archive_format=Some("1.0.0-draft.10".into());let sent=selection.encode().unwrap();
        let mut reply=selection.clone();reply.kind="upload-selected".into();assert!(selection.check_selected(&serde_json::to_vec(&reply).unwrap()).is_ok());
        reply.archive_format=Some("1.0.0-draft.9".into());assert!(selection.check_selected(&serde_json::to_vec(&reply).unwrap()).is_err());
        assert_ne!(old,sent);

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
        let mut v3=stage.clone();v3.kind="stage-bound-v3".into();let sent_v3=v3.encode_v3().unwrap();
        let mut accepted_v3=v3.clone();accepted_v3.kind="stage-accepted-v3".into();let reply_v3=serde_json::to_vec(&accepted_v3).unwrap();
        assert!(v3.check_accepted_v3(&sent_v3,&reply_v3).is_ok());
        assert!(v3.encode().is_err());assert!(v3.check_accepted(&sent_v3,&reply_v3).is_err());
        assert!(v3.check_accepted_v3(&sent_v3,&accepted).is_err());
    }
}
