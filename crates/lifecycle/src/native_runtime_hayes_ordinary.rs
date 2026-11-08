// SPDX-License-Identifier: Apache-2.0
//! Ordinary owner-scoped orchestration of one queued Hayes original import.

use super::{
    hayes::{
        operation::{ExecuteBinding, VerifiedRelease},
        provision,
    },
    *,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Input {
    binding: ExecuteBinding,
    worker_runtime: PathBuf,
}

impl Input {
    pub(crate) fn matches(&self, saved: &crate::installations::native_upload::Transfer) -> bool {
        saved.job_id.as_deref() == Some(self.binding.job_id.to_string().as_str())
            && saved.intent.as_ref().is_some_and(|intent| {
                intent.artwork_id == self.binding.artwork_id.to_string()
                    && intent.hayes_original.as_ref().is_some_and(|hayes|
                        hayes.artwork_revision == self.binding.artwork_revision)
            })
    }
}

impl Session {
    pub(crate) fn ordinary_hayes_available(&self) -> bool {
        self.state == "ready" && self.payload.ordinary_hayes_release()
            && matches!(self.hayes_bootstrap, HayesBootstrap::Issued(_))
            && self.active_furnishing_binding().is_some_and(|binding| binding.catalog == 44)
    }

    pub(crate) fn execute_ordinary_hayes(&mut self, input: Input) -> Result<Value> {
        if !self.status()?.readiness.ready || !self.ordinary_hayes_available()
            || input.binding.job_id.is_nil()
            || input.binding.artwork_id.is_nil()
            || !(1..=i32::MAX as u32).contains(&input.binding.artwork_revision)
        {
            return Err(unavailable());
        }
        self.check_native_observation_fence()?;
        let worker = Payload::open(&input.worker_runtime)?;
        // Fail before the API can claim the import if the immutable worker is absent.
        worker.hayes_read_only_generate()?.check()?;
        let lease = provision::acquire_user_lease()?;
        let deadline = Instant::now() + Duration::from_secs(175);
        let cancellation = self.cancellation.clone();
        let mut request = ExecuteRequest::start(self, input.binding)?;
        let mut exchange = self.begin_owned_hayes_open_for(
            lease, deadline.min(Instant::now() + Duration::from_secs(30)), input.binding,
        )?;
        exchange.receive_source(deadline)?;
        exchange.revalidate()?;
        exchange.observe_revalidated_workers(&worker, deadline)?;
        exchange.emit_output_ready(deadline)?;
        exchange.receive_output_reservation(deadline)?;
        exchange.transfer_reserved_output(deadline)?;
        exchange.observe_committed_disposition(deadline)?;
        let verified = exchange.return_committed_custody(deadline)?;
        let response = request.finish(deadline, &cancellation)?;
        if response != verified {
            return Err(unavailable());
        }
        serde_json::to_value(verified).map_err(|_| unavailable())
    }
}

fn unavailable() -> LifecycleError {
    err("HAYES_OPERATION_UNAVAILABLE")
}

struct ExecuteRequest(Owned);

impl ExecuteRequest {
    fn start(session: &Session, binding: ExecuteBinding) -> Result<Self> {
        let auth = session.local_authority.as_ref().ok_or_else(unavailable)?;
        let mut input = serde_json::to_vec(&serde_json::json!({
            "origin": format!("http://127.0.0.1:{}", session.ports.web),
            "tenant": auth.tenant_id,
            "token": auth.session_token,
            "csrf": auth.csrf_token,
            "jobId": binding.job_id,
            "artworkId": binding.artwork_id,
            "artworkRevision": binding.artwork_revision,
        })).map_err(|_| unavailable())?;
        // The bundled Node child only emits a bounded closed receipt. Local
        // session credentials are written to its stdin and never enter argv.
        let script = r#"
const { readFileSync } = require('node:fs');
(async () => {
  const {origin, tenant, token, csrf, jobId, artworkId, artworkRevision} = JSON.parse(readFileSync(0, 'utf8'));
  const response = await fetch(origin + '/api/v1/tenants/' + tenant + '/imports/native-hayes-original/' + jobId + '/execute', {
    method: 'POST', redirect: 'error', signal: AbortSignal.timeout(180000),
    headers: {origin, 'content-type':'application/json', accept:'application/json', cookie:'exhibitos_local_session=' + token, 'x-csrf-token':csrf},
    body: JSON.stringify({artworkId, artworkRevision}),
  });
  if (!response.ok) return false;
  const chunks = [];
  let length = 0;
  for await (const chunk of response.body) {
    length += chunk.length;
    if (length > 4096) return false;
    chunks.push(chunk);
  }
  const receipt = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  const {receiptId, receiptSha256, completionFrameSha256} = receipt;
  if (Object.keys(receipt).length !== 4 || receipt.jobId !== jobId
      || typeof receiptId !== 'string' || !/^[0-9a-f-]{36}$/.test(receiptId)
      || [receiptSha256, completionFrameSha256].some(v => typeof v !== 'string' || !/^[0-9a-f]{64}$/.test(v))) return false;
  process.stdout.write(JSON.stringify({jobId, receiptId, receiptSha256, completionFrameSha256}));
  return true;
})().then(ok => { if (!ok) process.exitCode = 1; }, () => { process.exitCode = 1; });
"#;
        let child = session.spawn(
            &session.payload.node(), &["-e".into(), script.into()], &[], Some(&input), true, false, None,
        );
        input.fill(0);
        Ok(Self(child?))
    }

    fn finish(&mut self, deadline: Instant, cancellation: &AtomicBool) -> Result<VerifiedRelease> {
        while Instant::now() < deadline && !cancelled() && !cancellation.load(Ordering::SeqCst) {
            if let Some(code) = self.0.poll()? {
                if code != 0 {
                    return Err(unavailable());
                }
                let bytes = self.0.capture.as_ref().ok_or_else(unavailable)?
                    .lock().map_err(|_| unavailable())?;
                return serde_json::from_slice(&bytes).map_err(|_| unavailable());
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(unavailable())
    }
}

impl Drop for ExecuteRequest {
    fn drop(&mut self) {
        // This direct child is request-local. Reap it before returning even
        // if the native exchange aborted while the HTTP request was pending.
        let _ = self.0.child.kill();
        let _ = self.0.child.wait();
        let _ = self.0.poll();
    }
}
