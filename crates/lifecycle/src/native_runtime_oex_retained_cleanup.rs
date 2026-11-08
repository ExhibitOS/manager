// SPDX-License-Identifier: Apache-2.0
//! The private child exchange for settling a previously saved OEX import.
use super::*;

pub(super) const ENV: &str = "EXHIBITOS_NATIVE_OEX_CLEANUP_SELECTION";
pub(super) const ERROR: &str = "LOCAL_RUNTIME_START_FAILED_OEX_CLEANUP";
const RESULT: &[u8] = b"EXHIBITOS_NATIVE_OEX_CLEANUP_RESULT ";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Selection {
    job_id: String,
    import_runtime_generation: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    version: u32,
    job_id: String,
    import_runtime_generation: String,
    state: String,
    cleanup_pending: bool,
    staging_chunks: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Unresolved {
    version: u32,
    job_id: String,
    import_runtime_generation: String,
    state: String,
    #[serde(rename = "cleanupPending")]
    _cleanup_pending: (),
    error_code: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ResultFrame {
    Complete(Receipt),
    Unresolved(Unresolved),
}

#[derive(Clone)]
pub(super) struct Run {
    selection: Selection,
    evidence: Arc<Mutex<Evidence>>,
}

#[derive(Default)]
enum Evidence {
    #[default]
    Waiting,
    Complete(Receipt),
    Unresolved,
    Invalid,
}

impl Run {
    pub(super) fn new(selection: Selection) -> Self {
        Self { selection, evidence: Arc::new(Mutex::new(Evidence::Waiting)) }
    }

    pub(super) fn environment(&self) -> Result<String> { self.selection.environment() }

    pub(super) fn observe(&self, line: &[u8], overflow: bool, ready_seen: bool) {
        if let Ok(mut evidence) = self.evidence.lock() {
            match self.selection.result_frame(line, overflow) {
                Ok(None) => (),
                Ok(Some(ResultFrame::Complete(receipt))) if !ready_seen && matches!(*evidence, Evidence::Waiting) =>
                    *evidence = Evidence::Complete(receipt),
                Ok(Some(ResultFrame::Unresolved(_))) if !ready_seen && matches!(*evidence, Evidence::Waiting) =>
                    *evidence = Evidence::Unresolved,
                _ => *evidence = Evidence::Invalid,
            }
        }
    }

    pub(super) fn unresolved(&self) -> bool {
        self.evidence.lock().is_ok_and(|evidence| matches!(*evidence, Evidence::Unresolved))
    }

    pub(super) fn receipt(&self) -> Result<Receipt> {
        match &*self.evidence.lock().map_err(|_| err(ERROR))? {
            Evidence::Complete(receipt) => Ok(receipt.clone()),
            _ => Err(err(ERROR)),
        }
    }
}

impl Selection {
    pub(crate) fn new(job_id: &str, import_runtime_generation: &str) -> Result<Self> {
        if !oex_preflight_frames::uuid(job_id) || !oex_preflight_frames::uuid(import_runtime_generation) {
            return Err(err(ERROR));
        }
        Ok(Self { job_id: job_id.into(), import_runtime_generation: import_runtime_generation.into() })
    }

    pub(super) fn environment(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|_| err(ERROR))
    }

    fn result_frame(&self, line: &[u8], overflow: bool) -> Result<Option<ResultFrame>> {
        let Some(json) = line.strip_prefix(RESULT) else { return Ok(None) };
        if overflow { return Err(err(ERROR)); }
        let result: ResultFrame = serde_json::from_slice(json).map_err(|_| err(ERROR))?;
        let matches = |version, job: &str, generation: &str| version == 1
            && job == self.job_id && generation == self.import_runtime_generation;
        let valid = match &result {
            ResultFrame::Complete(receipt) => matches(receipt.version, &receipt.job_id, &receipt.import_runtime_generation)
                && receipt.state == "complete" && !receipt.cleanup_pending
                && (1..=32).contains(&receipt.staging_chunks),
            ResultFrame::Unresolved(receipt) => matches(receipt.version, &receipt.job_id, &receipt.import_runtime_generation)
                && receipt.state == "unresolved" && receipt.error_code == "NATIVE_OEX_CLEANUP_FAILED",
        };
        if !valid { return Err(err(ERROR)); }
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_selector_and_receipt_are_closed_and_correlated() {
        let job = "00000000-0000-4000-8000-000000000001";
        let generation = "00000000-0000-4000-8000-000000000002";
        let selection = Selection::new(job, generation).unwrap();
        for invalid in ["../foreign", "00000000-0000-0000-0000-000000000000",
            "00000000-0000-9000-8000-000000000001", "00000000-0000-4000-c000-000000000001"]
        {
            assert!(Selection::new(invalid, generation).is_err());
            assert!(Selection::new(job, invalid).is_err());
        }
        assert_eq!(serde_json::from_str::<Value>(&selection.environment().unwrap()).unwrap(),
            serde_json::json!({"jobId":job,"importRuntimeGeneration":generation}));
        let receipt = serde_json::json!({"version":1,"jobId":job,"importRuntimeGeneration":generation,
            "state":"complete","cleanupPending":false,"stagingChunks":3});
        let line = |value: &Value| [RESULT, value.to_string().as_bytes()].concat();
        assert!(selection.result_frame(&line(&receipt), false).unwrap().is_some());
        assert!(selection.result_frame(b"ExhibitOS local runtime started.", false).unwrap().is_none());
        for (field, value) in [
            ("jobId", serde_json::json!(generation)),
            ("importRuntimeGeneration", serde_json::json!(job)),
            ("cleanupPending", serde_json::json!(true)),
            ("state", serde_json::json!("processing")),
            ("version", serde_json::json!(2)),
            ("stagingChunks", serde_json::json!(0)),
            ("stagingChunks", serde_json::json!(33)),
            ("path", serde_json::json!("/unexpected")),
        ] {
            let mut wrong = receipt.clone();
            wrong[field] = value;
            assert!(selection.result_frame(&line(&wrong), false).is_err());
        }
        let late = Run::new(selection.clone());
        late.observe(&line(&receipt), false, true);
        assert!(late.receipt().is_err());
        let run = Run::new(selection);
        assert!(run.receipt().is_err());
        run.observe(&line(&receipt), false, false);
        assert!(run.receipt().is_ok());
        run.observe(&line(&receipt), false, false);
        assert!(run.receipt().is_err());
    }

    #[test]
    fn only_a_correlated_unresolved_result_can_request_owned_drain() {
        let job = "00000000-0000-4000-8000-000000000001";
        let generation = "00000000-0000-4000-8000-000000000002";
        let selection = Selection::new(job, generation).unwrap();
        let unresolved = serde_json::json!({"version":1,"jobId":job,"importRuntimeGeneration":generation,
            "state":"unresolved","cleanupPending":null,"errorCode":"NATIVE_OEX_CLEANUP_FAILED"});
        let line = |value: &Value| [RESULT, value.to_string().as_bytes()].concat();
        let run = Run::new(selection.clone());
        run.observe(ERROR.as_bytes(), false, false);
        assert!(!run.unresolved());
        run.observe(&line(&unresolved), false, false);
        assert!(run.unresolved());
        assert!(run.receipt().is_err());
        run.observe(&line(&unresolved), false, false);
        assert!(!run.unresolved());
        for (field, value) in [
            ("jobId", serde_json::json!(generation)),
            ("importRuntimeGeneration", serde_json::json!(job)),
            ("cleanupPending", serde_json::json!(false)),
            ("errorCode", serde_json::json!("other")),
        ] {
            let mut wrong = unresolved.clone();
            wrong[field] = value;
            let run = Run::new(selection.clone());
            run.observe(&line(&wrong), false, false);
            assert!(!run.unresolved());
        }
        let late = Run::new(selection);
        late.observe(&line(&unresolved), false, true);
        assert!(!late.unresolved());
    }

    #[test]
    fn a_truncated_result_cannot_authorize_ready_or_drain() {
        let job = "00000000-0000-4000-8000-000000000001";
        let generation = "00000000-0000-4000-8000-000000000002";
        let selection = Selection::new(job, generation).unwrap();
        for frame in [
            serde_json::json!({"version":1,"jobId":job,"importRuntimeGeneration":generation,
                "state":"complete","cleanupPending":false,"stagingChunks":1}),
            serde_json::json!({"version":1,"jobId":job,"importRuntimeGeneration":generation,
                "state":"unresolved","cleanupPending":null,"errorCode":"NATIVE_OEX_CLEANUP_FAILED"}),
        ] {
            let mut prefix = [RESULT, frame.to_string().as_bytes()].concat();
            prefix.resize(1024, b' ');
            let run = Run::new(selection.clone());
            run.observe(&prefix, true, false);
            assert!(!run.unresolved());
            assert!(run.receipt().is_err());
        }
    }
}
