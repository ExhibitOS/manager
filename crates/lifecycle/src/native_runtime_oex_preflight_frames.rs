// SPDX-License-Identifier: Apache-2.0
//! Inactive same-channel v3 preflight correlation. Receipt bytes are not a
//! live provider lease; caller must hold/recheck the owner, session and FDs.
use super::*;
use super::oex_reservation::V3PlanScope;

fn invalid()->LifecycleError {err("NATIVE_OEX_PREFLIGHT_INVALID")}
fn uuid(s:&str)->bool {Uuid::parse_str(s).is_ok_and(|v|v.to_string()==s&&v.get_variant()==uuid::Variant::RFC4122&&(1..=8).contains(&v.get_version_num()))}

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct BoundWire {
    version:u32,kind:String,correlation_id:String,attempt_id:String,runtime_generation:String,stage_id:String,
    report_sha256:String,input_receipt_digest:String,output_reservation_id:Option<String>,output_reservation_generation:Option<String>,output_receipt_digest:Option<String>,
    archive_bytes:u64,archive_sha256:String,input_device:String,input_inode:String,expanded_bytes:u64,packed_bytes:u64,manifest_sha256:String,entry_count:usize,
}
#[derive(Clone)]
pub(super) struct InputScope { stage:PathBuf,stage_device:u64,store:PathBuf,store_device:u64,store_inode:u64,system_id:String,database_id:String,
    reservation_id:String,reservation_generation:String,receipt_digest:String,control:Vec<u8> }
#[allow(dead_code)]
impl InputScope {
    /// Compare the A-only receipt with independently held roots/owner. This is
    /// still not verification of a live provider lease or session.
    pub(super) fn from_receipt(receipt:&[u8],request:&super::oex_preflight::Control,scope:V3PlanScope<'_>)->Result<Self> {
        let control=request.encode()?;
        if scope.stage_device.to_string()!=request.input_device{return Err(invalid());}
        super::oex_reservation::check_v3_plan(receipt,&request.reservation_digest,&request.reservation_id,&request.reservation_generation,
            scope,request.archive_bytes,0)?;
        Ok(Self{stage:scope.stage_path.to_path_buf(),stage_device:scope.stage_device,store:scope.store_path.to_path_buf(),store_device:scope.store_device,store_inode:scope.store_inode,
            system_id:scope.system_id.into(),database_id:scope.database_id.into(),reservation_id:request.reservation_id.clone(),
            reservation_generation:request.reservation_generation.clone(),receipt_digest:request.reservation_digest.clone(),control})
    }
}
pub(super) struct Bound { wire: BoundWire, scope: InputScope, generation:String }

#[derive(Clone,Serialize)]
#[cfg_attr(test,derive(Deserialize))]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct PackedReady {
    version:u32,kind:String,correlation_id:String,attempt_id:String,runtime_generation:String,stage_id:String,
    report_sha256:String,input_receipt_digest:String,output_reservation_id:Option<String>,output_reservation_generation:Option<String>,output_receipt_digest:Option<String>,
    archive_bytes:u64,archive_sha256:String,input_device:String,input_inode:String,output_device:String,output_inode:String,packed_bytes:u64,manifest_sha256:String,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct DrainWire {
    version:u32,kind:String,correlation_id:String,operation_correlation_id:String,attempt_id:String,runtime_generation:String,stage_id:String,report_sha256:String,
    input_reservation_id:String,input_reservation_generation:String,input_receipt_digest:String,
    output_reservation_id:Option<String>,output_reservation_generation:Option<String>,output_receipt_digest:Option<String>,
}
#[derive(Serialize)]
struct DrainReply {#[serde(flatten)] fields:DrainWire,status:&'static str}
impl PackedReady {
    pub(super) fn encode(&self)->Result<Vec<u8>> {serde_json::to_vec(self).map_err(|_|invalid())}
    pub(super) fn check_accepted(&self,reply:&[u8])->Result<()> {
        let mut expected=self.clone();
        expected.kind="packed-accepted-v3".into();
        if expected.encode()?.as_slice()!=reply{return Err(invalid());}Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Confirm<'a> {version:u32,kind:&'static str,correlation_id:&'a str,attempt_id:&'a str,runtime_generation:&'a str,
    stage_id:&'a str,report_sha256:&'a str,input_receipt_digest:&'a str,output_receipt_digest:Option<&'a str>}

#[allow(dead_code)]
impl Bound {
    pub(super) fn output_receipt_digest(&self)->Option<&str> {self.wire.output_receipt_digest.as_deref()}
    pub(super) fn operation_identity(&self)->(&str,&str,&str,&str,&str) {
        let w=&self.wire;(&w.correlation_id,&w.attempt_id,&w.runtime_generation,&w.stage_id,&w.report_sha256)
    }
    pub(super) fn input_plan(&self)->(&str,&str,&str,&str) {
        (&self.generation,&self.scope.reservation_id,&self.scope.reservation_generation,&self.scope.receipt_digest)
    }
    pub(super) fn output_plan(&self)->(u64,&str,Option<(&str,&str,&str)>) {
        let w=&self.wire;
        (w.packed_bytes,&w.manifest_sha256,match (&w.output_reservation_id,&w.output_reservation_generation,&w.output_receipt_digest) {
            (Some(id),Some(generation),Some(digest))=>Some((id,generation,digest)),_=>None,
        })
    }
    /// Correlates the peer's drain request; callers must independently hold
    /// the actual reaped children, pinned FDs and live Session fence.
    pub(super) fn drain_reply(&self,bytes:&[u8])->Result<Vec<u8>> {
        let mut d:DrainWire=serde_json::from_slice(bytes).map_err(|_|invalid())?;let w=&self.wire;
        if bytes.len()>8192||serde_json::to_vec(&d).map_err(|_|invalid())?.as_slice()!=bytes
            || d.version!=1||d.kind!="drain-check-v3"||!uuid(&d.correlation_id)||d.correlation_id==w.correlation_id
            || d.operation_correlation_id!=w.correlation_id||d.attempt_id!=w.attempt_id||d.runtime_generation!=w.runtime_generation
            || d.stage_id!=w.stage_id||d.report_sha256!=w.report_sha256||d.input_reservation_id!=self.scope.reservation_id
            || d.input_reservation_generation!=self.scope.reservation_generation||d.input_receipt_digest!=self.scope.receipt_digest
            || d.output_reservation_id!=w.output_reservation_id||d.output_reservation_generation!=w.output_reservation_generation
            || d.output_receipt_digest!=w.output_receipt_digest {return Err(invalid());}
        d.kind="drain-result-v3".into();serde_json::to_vec(&DrainReply{fields:d,status:"drained"}).map_err(|_|invalid())
    }
    pub(super) fn packed_ready(&self,output:&File,receipt:Option<&[u8]>)->Result<PackedReady> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;
        match receipt {Some(bytes)=>{self.confirm_output(bytes)?;},None=>{self.confirm_empty()?;}}
        let m=output.metadata().map_err(|_|invalid())?;let flags=unsafe{libc::fcntl(output.as_raw_fd(),libc::F_GETFL)};
        if !m.is_file()||m.uid()!=unsafe{libc::geteuid()}||m.mode()&0o777!=0o600||m.nlink()!=1||m.len()!=0
            || m.dev()!=self.scope.stage_device||m.ino()==0||m.ino().to_string()==self.wire.input_inode
            || flags<0||flags&libc::O_ACCMODE!=libc::O_RDWR||flags&libc::O_APPEND!=0 {return Err(invalid());}
        let w=&self.wire;
        Ok(PackedReady{version:1,kind:"packed-ready-v3".into(),correlation_id:w.correlation_id.clone(),attempt_id:w.attempt_id.clone(),runtime_generation:w.runtime_generation.clone(),stage_id:w.stage_id.clone(),report_sha256:w.report_sha256.clone(),input_receipt_digest:w.input_receipt_digest.clone(),output_reservation_id:w.output_reservation_id.clone(),output_reservation_generation:w.output_reservation_generation.clone(),output_receipt_digest:w.output_receipt_digest.clone(),archive_bytes:w.archive_bytes,archive_sha256:w.archive_sha256.clone(),input_device:w.input_device.clone(),input_inode:w.input_inode.clone(),output_device:m.dev().to_string(),output_inode:m.ino().to_string(),packed_bytes:w.packed_bytes,manifest_sha256:w.manifest_sha256.clone()})
    }
    /// Only correlates the response with the exact reaped-child evidence. It
    /// does not send a frame, check provider liveness or authorize admission.
    pub(super) fn parse(bytes:&[u8],prepared:&super::oex_preflight::Prepared,request:&super::oex_preflight::Control,
        challenge:&super::owner_claim_frames::ClaimChallenge,scope:&InputScope)->Result<Self> {
        if bytes.is_empty()||bytes.len()>8192||scope.stage_device.to_string()!=request.input_device
            || scope.reservation_id!=request.reservation_id||scope.reservation_generation!=request.reservation_generation
            || scope.receipt_digest!=request.reservation_digest||request.encode()?!=scope.control{return Err(invalid());}
        let report=prepared.report(request,challenge)?;
        let value:BoundWire=serde_json::from_slice(bytes).map_err(|_|invalid())?;
        if serde_json::to_vec(&value).map_err(|_|invalid())?.as_slice()!=bytes || value.version!=1 || value.kind!="preflight-bound-v3"
            || value.correlation_id!=challenge.correlation_id || value.attempt_id!=request.attempt_id
            || value.runtime_generation!=request.runtime_generation || value.stage_id!=request.stage_id
            || value.report_sha256!=digest(&report)||value.input_receipt_digest!=request.reservation_digest
            || value.archive_bytes!=request.archive_bytes || value.archive_sha256!=request.archive_sha256
            || value.input_device!=request.input_device || value.input_inode!=request.input_inode
            || value.expanded_bytes!=prepared.inventory().expanded_bytes || value.packed_bytes!=prepared.inventory().packed_bytes
            || value.manifest_sha256!=prepared.inventory().manifest_sha256 || value.entry_count!=prepared.inventory().entry_count {return Err(invalid());}
        match (&value.output_reservation_id,&value.output_reservation_generation,&value.output_receipt_digest,value.packed_bytes) {
            (None,None,None,0)=>{},
            (Some(id),Some(generation),Some(sha),n) if n>0 && uuid(id) && uuid(generation) && hash_valid(sha) && id!=&request.reservation_id=>{},
            _=>return Err(invalid()),
        }
        Ok(Self{wire:value,scope:scope.clone(),generation:challenge.generation.clone()})
    }
    /// For E=0 no output receipt exists. Caller still rechecks the live input
    /// lease and owner before sending the returned confirmation.
    pub(super) fn confirm_empty(&self)->Result<Vec<u8>> {
        if self.wire.packed_bytes!=0 || self.wire.output_receipt_digest.is_some(){return Err(invalid());}
        self.confirm()
    }
    /// Correlate the separate output receipt to independently held root and DB
    /// identity before forming a confirmation; provider liveness remains a
    /// separate authenticated check on both sides.
    pub(super) fn confirm_output(&self,receipt:&[u8])->Result<Vec<u8>> {
        let id=self.wire.output_reservation_id.as_deref().ok_or_else(invalid)?;
        let generation=self.wire.output_reservation_generation.as_deref().ok_or_else(invalid)?;
        let sha=self.wire.output_receipt_digest.as_deref().ok_or_else(invalid)?;
        if self.wire.packed_bytes==0{return Err(invalid());}
        let scope=V3PlanScope{system_id:&self.scope.system_id,database_id:&self.scope.database_id,stage_path:&self.scope.stage,
            stage_device:self.scope.stage_device,store_path:&self.scope.store,store_device:self.scope.store_device,store_inode:self.scope.store_inode};
        super::oex_reservation::check_v3_plan(receipt,sha,id,generation,scope,self.wire.packed_bytes,self.wire.packed_bytes)?;
        self.confirm()
    }
    fn confirm(&self)->Result<Vec<u8>> {
        serde_json::to_vec(&Confirm{version:1,kind:"preflight-confirm-v3",correlation_id:&self.wire.correlation_id,
            attempt_id:&self.wire.attempt_id,runtime_generation:&self.wire.runtime_generation,stage_id:&self.wire.stage_id,
            report_sha256:&self.wire.report_sha256,input_receipt_digest:&self.wire.input_receipt_digest,
            output_receipt_digest:self.wire.output_receipt_digest.as_deref()}).map_err(|_|invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(out:&mut Vec<u8>,s:&str){out.extend_from_slice(&(s.len() as u32).to_be_bytes());out.extend_from_slice(s.as_bytes());}
    #[test]
    fn packed_ready_requires_empty_distinct_private_output() {
        use std::os::unix::fs::{MetadataExt,OpenOptionsExt};
        use std::io::Write;
        let path=std::env::temp_dir().join(format!("oex-v3-packed-{}",Uuid::new_v4()));fs::create_dir(&path).unwrap();
        let input=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path.join("input")).unwrap();
        let mut output=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path.join("output")).unwrap();
        let meta=input.metadata().unwrap();let id=||Uuid::new_v4().to_string();
        let wire=BoundWire{version:1,kind:"preflight-bound-v3".into(),correlation_id:id(),attempt_id:id(),runtime_generation:id(),stage_id:id(),report_sha256:"a".repeat(64),input_receipt_digest:"b".repeat(64),output_reservation_id:None,output_reservation_generation:None,output_receipt_digest:None,archive_bytes:1,archive_sha256:"c".repeat(64),input_device:meta.dev().to_string(),input_inode:meta.ino().to_string(),expanded_bytes:1,packed_bytes:0,manifest_sha256:"d".repeat(64),entry_count:2};
        let scope=InputScope{stage:path.clone(),stage_device:meta.dev(),store:PathBuf::from("/private/store"),store_device:1,store_inode:1,system_id:"1".into(),database_id:"1".into(),reservation_id:id(),reservation_generation:id(),receipt_digest:wire.input_receipt_digest.clone(),control:Vec::new()};
        let bound=Bound{wire,scope,generation:id()};assert!(bound.packed_ready(&output,None).is_ok());
        assert!(bound.packed_ready(&input,None).is_err());
        output.write_all(b"x").unwrap();assert!(bound.packed_ready(&output,None).is_err());
        drop((input,output));fs::remove_dir_all(path).unwrap();
    }
    #[test]
    #[ignore = "requires explicit Viewer synthetic wire fixture"]
    fn viewer_canonical_preflight_frames_match_exactly() {
        let path=PathBuf::from(std::env::var("EXHIBITOS_PREFLIGHT_FRAME_FIXTURES").expect("owned Viewer fixture required"));
        let read=|name:&str|fs::read(path.join(name)).unwrap();
        let request:super::super::oex_preflight::Control=serde_json::from_slice(&read("control.json")).unwrap();
        assert_eq!(request.encode().unwrap(),read("control.json"));
        let wire:BoundWire=serde_json::from_slice(&read("bound.json")).unwrap();
        let next=||Uuid::new_v4().to_string();
        let settlement_fixture:Value=serde_json::from_slice(&read("settle-check.json")).unwrap();
        let challenge=super::super::owner_claim_frames::ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:wire.correlation_id,
            workspace_id:next(),tenant_id:next(),principal_id:next(),runtime_generation:request.runtime_generation.clone(),job_id:next(),
            attempt_id:request.attempt_id.clone(),generation:settlement_fixture["generation"].as_str().unwrap().into(),stage_id:request.stage_id.clone(),reservation_id:request.reservation_id.clone(),
            reservation_generation:request.reservation_generation.clone(),claim_digest:"a".repeat(64),receipt_digest:request.reservation_digest.clone()};
        // Fixture contains no real A/E receipt. This test exercises only
        // canonical cross-language wire bytes, never provider authority.
        let scope=InputScope{stage:PathBuf::from("/private/stage"),stage_device:1,store:PathBuf::from("/private/store"),store_device:2,store_inode:9,
            system_id:"123".into(),database_id:"456".into(),reservation_id:request.reservation_id.clone(),reservation_generation:request.reservation_generation.clone(),
            receipt_digest:request.reservation_digest.clone(),control:request.encode().unwrap()};
        for (suffix,empty) in [("",false),("-zero",true)] {
            let prepared=super::super::oex_preflight::Prepared::test_only_raw(&request,read(&format!("ack{suffix}.json")));
            assert_eq!(prepared.report(&request,&challenge).unwrap(),read(&format!("report{suffix}.json")));
            let bound=Bound::parse(&read(&format!("bound{suffix}.json")),&prepared,&request,&challenge,&scope).unwrap();
            let confirmation=if empty {bound.confirm_empty().unwrap()} else {bound.confirm().unwrap()};
            assert_eq!(confirmation,read(&format!("confirm{suffix}.json")));
            let packed:PackedReady=serde_json::from_slice(&read(&format!("packed-ready{suffix}.json"))).unwrap();
            assert_eq!(packed.encode().unwrap(),read(&format!("packed-ready{suffix}.json")));
            packed.check_accepted(&read(&format!("packed-accepted{suffix}.json"))).unwrap();
            let drain=read(&format!("drain-check{suffix}.json"));
            assert_eq!(bound.drain_reply(&drain).unwrap(),read(&format!("drain-result{suffix}.json")));
            let mut changed:DrainWire=serde_json::from_slice(&drain).unwrap();changed.input_reservation_id=next();
            assert!(bound.drain_reply(&serde_json::to_vec(&changed).unwrap()).is_err());
            let check=read(&format!("settle-check{suffix}.json"));let value:Value=serde_json::from_slice(&check).unwrap();
            let mut save:super::super::owner_claim_frames::SaveFrameV3=serde_json::from_slice(&read("save-ready.json")).unwrap();
            save.report_sha256=bound.operation_identity().4.into();save.etag=format!("\"studio-r2-{}\"","e".repeat(64));
            let settlement=super::super::owner_claim_frames::SettlementV3::parse_check(&check,&challenge,&bound,&save,value["etag"].as_str().unwrap()).unwrap();
            assert_eq!(settlement.ready().unwrap(),read(&format!("settle-ready{suffix}.json")));
            assert_eq!(settlement.receipt_ack(&read(&format!("settle-receipt{suffix}.json"))).unwrap(),read(&format!("settle-receipt-ack{suffix}.json")));
            assert!(settlement.receipt_ack(&check).is_err());
            let mut wrong=challenge.clone();wrong.generation=next();
            assert!(super::super::owner_claim_frames::SettlementV3::parse_check(&check,&wrong,&bound,&save,value["etag"].as_str().unwrap()).is_err());
            wrong=challenge.clone();wrong.reservation_id=next();
            assert!(super::super::owner_claim_frames::SettlementV3::parse_check(&check,&wrong,&bound,&save,value["etag"].as_str().unwrap()).is_err());
            if !empty {
                let save=super::super::owner_claim_frames::SaveFrameV3::parse_ready(&read("save-ready.json"),&bound).unwrap();
                let observed:Value=serde_json::from_slice(&read("save-observed.json")).unwrap();let etag=observed["etag"].as_str().unwrap();
                assert_eq!(save.observed(etag).unwrap(),read("save-observed.json"));
                save.check_verified(etag,&read("save-verified.json")).unwrap();
                save.check_completed(etag,&read("operation-complete.json")).unwrap();
                assert!(save.observed(&save.etag).is_err());
                assert!(save.check_verified(etag,&read("save-observed.json")).is_err());
                assert!(save.check_completed(etag,&read("save-verified.json")).is_err());
            }
        }
    }
    #[test]
    fn bound_requires_same_preflight_and_correct_output_receipt() {
        let next=||Uuid::new_v4().to_string();
        let mut request=super::super::oex_preflight::Control{version:1,kind:"oex-preflight-v1".into(),attempt_id:next(),runtime_generation:next(),stage_id:next(),reservation_id:next(),reservation_generation:next(),reservation_digest:"a".repeat(64),archive_bytes:100,archive_sha256:"b".repeat(64),input_device:"1".into(),input_inode:"2".into()};
        let mut input=b"EXHIBITOS-DISK-RESERVATION\0v1\0".to_vec();
        for s in [request.reservation_id.as_str(),request.reservation_generation.as_str(),"oex-import","123","456","/private/store","2","9"]{field(&mut input,s);}
        input.extend_from_slice(&1u32.to_be_bytes());for s in ["/private/stage","1","100"]{field(&mut input,s);}
        request.reservation_digest=digest(&input);
        let plan=V3PlanScope{system_id:"123",database_id:"456",stage_path:Path::new("/private/stage"),stage_device:1,store_path:Path::new("/private/store"),store_device:2,store_inode:9};
        let scope=InputScope::from_receipt(&input,&request,plan).unwrap();
        assert!(InputScope::from_receipt(&input,&request,V3PlanScope{database_id:"457",..plan}).is_err());
        assert!(InputScope::from_receipt(&input,&request,V3PlanScope{stage_device:2,..plan}).is_err());
        let challenge=super::super::owner_claim_frames::ClaimChallenge{version:1,kind:"claim-challenge-v3".into(),correlation_id:next(),workspace_id:next(),tenant_id:next(),principal_id:next(),runtime_generation:request.runtime_generation.clone(),job_id:next(),attempt_id:request.attempt_id.clone(),generation:next(),stage_id:request.stage_id.clone(),reservation_id:request.reservation_id.clone(),reservation_generation:request.reservation_generation.clone(),claim_digest:"d".repeat(64),receipt_digest:request.reservation_digest.clone()};
        let prepared=super::super::oex_preflight::Prepared::test_only(&request,80,40);let report=prepared.report(&request,&challenge).unwrap();
        let output_id=next();let output_generation=next();let mut receipt=b"EXHIBITOS-DISK-RESERVATION\0v1\0".to_vec();
        for s in [output_id.as_str(),output_generation.as_str(),"oex-import","123","456","/private/store","2","9"]{field(&mut receipt,s);}
        receipt.extend_from_slice(&2u32.to_be_bytes());for plan in [["/private/stage","1","40"],["/private/store","2","40"]] {for s in plan {field(&mut receipt,s);}}
        let mut bound=BoundWire{version:1,kind:"preflight-bound-v3".into(),correlation_id:challenge.correlation_id.clone(),attempt_id:request.attempt_id.clone(),runtime_generation:request.runtime_generation.clone(),stage_id:request.stage_id.clone(),report_sha256:digest(&report),input_receipt_digest:request.reservation_digest.clone(),output_reservation_id:Some(output_id),output_reservation_generation:Some(output_generation),output_receipt_digest:Some(digest(&receipt)),archive_bytes:request.archive_bytes,archive_sha256:request.archive_sha256.clone(),input_device:request.input_device.clone(),input_inode:request.input_inode.clone(),expanded_bytes:80,packed_bytes:40,manifest_sha256:"c".repeat(64),entry_count:2};
        let bytes=serde_json::to_vec(&bound).unwrap();let parsed=Bound::parse(&bytes,&prepared,&request,&challenge,&scope).unwrap();
        let mut changed_request:super::super::oex_preflight::Control=serde_json::from_slice(&request.encode().unwrap()).unwrap();
        changed_request.archive_bytes=101;
        let changed_prepared=super::super::oex_preflight::Prepared::test_only(&changed_request,80,40);
        let mut changed_bound:BoundWire=serde_json::from_slice(&bytes).unwrap();changed_bound.archive_bytes=101;
        changed_bound.report_sha256=digest(&changed_prepared.report(&changed_request,&challenge).unwrap());
        assert!(Bound::parse(&serde_json::to_vec(&changed_bound).unwrap(),&changed_prepared,&changed_request,&challenge,&scope).is_err());
        assert!(parsed.confirm_empty().is_err());
        let confirm=parsed.confirm_output(&receipt).unwrap();
        let value:Value=serde_json::from_slice(&confirm).unwrap();assert_eq!(value["kind"],"preflight-confirm-v3");
        let mut wrong_receipt=receipt.clone();wrong_receipt.push(0);assert!(parsed.confirm_output(&wrong_receipt).is_err());
        bound.input_inode="3".into();assert!(Bound::parse(&serde_json::to_vec(&bound).unwrap(),&prepared,&request,&challenge,&scope).is_err());
        bound.input_inode="2".into();bound.output_reservation_id=None;assert!(Bound::parse(&serde_json::to_vec(&bound).unwrap(),&prepared,&request,&challenge,&scope).is_err());
        let empty=super::super::oex_preflight::Prepared::test_only(&request,80,0);bound.report_sha256=digest(&empty.report(&request,&challenge).unwrap());bound.packed_bytes=0;bound.output_reservation_generation=None;bound.output_receipt_digest=None;
        let parsed=Bound::parse(&serde_json::to_vec(&bound).unwrap(),&empty,&request,&challenge,&scope).unwrap();assert!(parsed.confirm_empty().is_ok());
    }
}
