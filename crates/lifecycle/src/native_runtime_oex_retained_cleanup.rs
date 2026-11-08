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
    Invalid,
}

impl Run {
    pub(super) fn new(selection: Selection) -> Self {
        Self { selection, evidence: Arc::new(Mutex::new(Evidence::Waiting)) }
    }

    pub(super) fn environment(&self) -> Result<String> { self.selection.environment() }

    pub(super) fn observe(&self, line: &[u8], ready_seen: bool) {
        if let Ok(mut evidence) = self.evidence.lock() {
            match self.selection.receipt(line) {
                Ok(None) => (),
                Ok(Some(receipt)) if !ready_seen && matches!(*evidence, Evidence::Waiting) =>
                    *evidence = Evidence::Complete(receipt),
                _ => *evidence = Evidence::Invalid,
            }
        }
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
        if !installations::uuid(job_id) || !installations::uuid(import_runtime_generation) {
            return Err(err(ERROR));
        }
        Ok(Self { job_id: job_id.into(), import_runtime_generation: import_runtime_generation.into() })
    }

    pub(super) fn environment(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|_| err(ERROR))
    }

    pub(super) fn receipt(&self, line: &[u8]) -> Result<Option<Receipt>> {
        let Some(json) = line.strip_prefix(RESULT) else { return Ok(None) };
        let receipt: Receipt = serde_json::from_slice(json).map_err(|_| err(ERROR))?;
        if receipt.version != 1 || receipt.job_id != self.job_id
            || receipt.import_runtime_generation != self.import_runtime_generation
            || receipt.state != "complete" || receipt.cleanup_pending
        {
            return Err(err(ERROR));
        }
        Ok(Some(receipt))
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
        assert!(Selection::new("../foreign", generation).is_err());
        assert_eq!(serde_json::from_str::<Value>(&selection.environment().unwrap()).unwrap(),
            serde_json::json!({"jobId":job,"importRuntimeGeneration":generation}));
        let receipt = serde_json::json!({"version":1,"jobId":job,"importRuntimeGeneration":generation,
            "state":"complete","cleanupPending":false,"stagingChunks":3});
        let line = |value: &Value| [RESULT, value.to_string().as_bytes()].concat();
        assert!(selection.receipt(&line(&receipt)).unwrap().is_some());
        assert!(selection.receipt(b"ExhibitOS local runtime started.").unwrap().is_none());
        for (field, value) in [
            ("jobId", serde_json::json!(generation)),
            ("importRuntimeGeneration", serde_json::json!(job)),
            ("cleanupPending", serde_json::json!(true)),
            ("state", serde_json::json!("processing")),
            ("version", serde_json::json!(2)),
            ("path", serde_json::json!("/unexpected")),
        ] {
            let mut wrong = receipt.clone();
            wrong[field] = value;
            assert!(selection.receipt(&line(&wrong)).is_err());
        }
        let late = Run::new(selection.clone());
        late.observe(&line(&receipt), true);
        assert!(late.receipt().is_err());
        let run = Run::new(selection);
        assert!(run.receipt().is_err());
        run.observe(&line(&receipt), false);
        assert!(run.receipt().is_ok());
        run.observe(&line(&receipt), false);
        assert!(run.receipt().is_err());
    }
}
