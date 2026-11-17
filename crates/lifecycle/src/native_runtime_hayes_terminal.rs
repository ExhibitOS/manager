// SPDX-License-Identifier: Apache-2.0
//! Terminalize only the exact retained pre-OPEN claim on an active successor.

use super::{hayes_terminal_wire::{Proof, Receipt}, oex_owner::Owner, owner_channel::AuthenticatedHayes, *};
use crate::installations::{native_document::DocumentState, native_maintenance::RetainedHayesPreopen, native_upload::Transfer};

fn unconfirmed() -> crate::LifecycleError {
    err("HAYES_PREOPEN_TERMINAL_UNCONFIRMED")
}

impl Session {
    pub(crate) fn terminalize_hayes_preopen(&mut self, retained: &RetainedHayesPreopen, saved: &Transfer, draft: &DocumentState) -> Result<Value> {
        if !self.payload.hayes_preopen_terminal() || !self.status()?.readiness.ready
            || self.transition_fence.is_some() || self.retained_hayes_trial.is_some()
            || self.retained_hayes.is_some()
            || !self.active_furnishing_binding().is_some_and(|binding| {
                binding.catalog == 52 && binding.phase == Phase::Active
                    && binding.admission_kind == Admission::CleanReopen && binding.restore.is_none()
            })
        {
            return Err(unconfirmed());
        }
        self.check_native_observation_fence()?;
        self.payload.recheck()?;
        self.verify_workspace_furnishing_ledger()?;
        let owner = Owner::capture(self)?;
        owner.check_database_identity_result(&retained.database_identity()?)?;
        let proof = Proof::checked(&retained.observation, &retained.proof_sha256)?;
        let expected_custody = retained.observation["custodyAuthoritySha256"].as_str().ok_or_else(unconfirmed)?;
        let custody = self.pin_hayes_preopen_custody(expected_custody)?;
        let receipt = match self.read_preopen_terminal(&proof, &owner)? {
            Some(receipt) => receipt,
            None => {
                // The signed digest is not admission by itself. Confirm the
                // full processing observation before sending the one-use proof.
                self.verify_retained_hayes_preopen_with_lease(retained, saved, draft, &custody)?;
                let ack = self.record_preopen_terminal(&proof, &owner)?;
                let receipt = self.read_preopen_terminal(&proof, &owner)?.ok_or_else(unconfirmed)?;
                if ack != receipt { return Err(unconfirmed()); }
                receipt
            }
        };
        owner.recheck_database_identity(self)?;
        owner.check(self)?;
        custody.check_preopen_custody(expected_custody)?;
        serde_json::to_value(receipt).map_err(|_| unconfirmed())
    }

    fn record_preopen_terminal(&mut self, proof: &Proof<'_>, owner: &Owner) -> Result<Receipt> {
        let AuthenticatedHayes { mut channel, pin } = self.hayes_bootstrap.consume()?;
        channel.claim_hayes()?;
        let manifest = self.payload.context().manifest_sha256;
        if pin.host_manifest_sha256() != manifest
            || self.job_id.as_deref() != Some(pin.runtime_generation().to_string().as_str())
        {
            return Err(unconfirmed());
        }
        let mut request = TerminalRequest::start(self, proof, true)?;
        let deadline = Instant::now() + Duration::from_secs(31);
        let cancellation = self.cancellation.clone();
        let mut live = || owner.check(self);
        let challenge = channel.read_frame(deadline, &cancellation, &mut live)?;
        let (nonce, frame) = proof.answer(&challenge, pin.native_owner_id(), pin.runtime_generation(), &manifest)?;
        channel.send_frame(&frame, deadline, &cancellation, &mut live)?;
        let ack = proof.recorded(&channel.read_frame(deadline, &cancellation, &mut live)?, nonce)?;
        if request.finish(proof, deadline, &cancellation)?.as_ref() != Some(&ack) {
            return Err(unconfirmed());
        }
        Ok(ack)
    }

    fn read_preopen_terminal(&mut self, proof: &Proof<'_>, owner: &Owner) -> Result<Option<Receipt>> {
        owner.check(self)?;
        let mut request = TerminalRequest::start(self, proof, false)?;
        let response = request.finish(proof, Instant::now() + Duration::from_secs(16), &self.cancellation)?;
        owner.check(self)?;
        Ok(response)
    }
}

struct TerminalRequest(Owned);

impl TerminalRequest {
    fn start(session: &Session, proof: &Proof<'_>, write: bool) -> Result<Self> {
        let auth = session.local_authority.as_ref().ok_or_else(unconfirmed)?;
        let mut input = serde_json::to_vec(&serde_json::json!({
            "origin": format!("http://127.0.0.1:{}", session.ports.web), "tenant": auth.tenant_id, "token": auth.session_token,
            "csrf": auth.csrf_token, "jobId": proof.job_id, "proofSha256": proof.sha256, "write": write,
        })).map_err(|_| unconfirmed())?;
        // The issued local credentials enter the child only through stdin.
        // The GET stdout can carry either a strict receipt or an empty 404.
        let script = r#"
const { readFileSync } = require('node:fs');
(async () => {
 const {origin,tenant,token,csrf,jobId,proofSha256,write} = JSON.parse(readFileSync(0,'utf8'));
 const path=origin+'/api/v1/tenants/'+tenant+'/imports/native-hayes-original/'+jobId+'/preopen-terminal';
 const options={redirect:'error',signal:AbortSignal.timeout(write?30000:15000),
  headers:{origin,accept:'application/json',cookie:'exhibitos_local_session='+token,'x-csrf-token':csrf}};
 if(write){options.method='POST';options.headers['content-type']='application/json';options.body=JSON.stringify({proofSha256});}
 const response=await fetch(write?path:path+'?proofSha256='+proofSha256,options);
 if(!write&&response.status===404)return;
 if(!response.ok||!response.body)throw Error('RECEIPT_UNAVAILABLE');
 const chunks=[];let length=0;
 for await(const chunk of response.body){length+=chunk.length;if(length>1024)throw Error('RECEIPT_UNAVAILABLE');chunks.push(chunk);}
 if(length===0)throw Error('RECEIPT_UNAVAILABLE');
 process.stdout.write(Buffer.concat(chunks));
})().catch(()=>{process.exitCode=1;});
"#;
        let child = session.spawn(&session.payload.node(), &["-e".into(), script.into()], &[], Some(&input), true, false, None);
        input.fill(0);
        Ok(Self(child?))
    }

    fn finish(&mut self, proof: &Proof<'_>, deadline: Instant, cancellation: &AtomicBool) -> Result<Option<Receipt>> {
        while Instant::now() < deadline && !cancelled() && !cancellation.load(Ordering::SeqCst) {
            if let Some(code) = self.0.poll()? {
                if code != 0 { return Err(unconfirmed()); }
                let capture = self.0.capture.as_ref().ok_or_else(unconfirmed)?.lock().map_err(|_| unconfirmed())?;
                return if capture.is_empty() { Ok(None) } else { proof.readback(&capture).map(Some) };
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(unconfirmed())
    }
}

impl Drop for TerminalRequest {
    fn drop(&mut self) {
        let _ = self.0.child.kill();
        let _ = self.0.child.wait();
        let _ = self.0.poll();
    }
}
