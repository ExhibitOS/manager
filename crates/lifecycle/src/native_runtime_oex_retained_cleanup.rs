// SPDX-License-Identifier: Apache-2.0
//! The private child exchange for settling a previously saved OEX import.
use super::*;

pub(super) const ENV: &str = "EXHIBITOS_NATIVE_OEX_CLEANUP_SELECTION";
pub(super) const HAYES_COMPLETION_ENV: &str = "EXHIBITOS_NATIVE_045_HAYES_COMPLETION";
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

pub(crate) struct Selected045 {
    pub(super) oex: Selection,
    hayes_job_id: String,
    completion_sha256: String,
    retirement: super::oex_recovery::SelectedRetirement,
    absent_historical_chunks: BTreeMap<usize, u64>,
    settled_only: bool,
    staging_bytes: Option<usize>,
}

const CHUNK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selected045Staging {
    bytes: usize,
    cleanup_pending: bool,
}

impl Selected045Staging {
    fn admits(&self, absent: &BTreeMap<usize, u64>, settled_only: bool) -> bool {
        (1..=32 * CHUNK_BYTES).contains(&self.bytes)
            && (!self.cleanup_pending || (!settled_only && absent.is_empty()))
            && absent.iter().all(|(&i, &size)|
                i < self.bytes.div_ceil(CHUNK_BYTES)
                    && size == (self.bytes - i * CHUNK_BYTES).min(CHUNK_BYTES) as u64)
    }
}

impl Selected045 {
    pub(crate) fn new(
        oex: Selection, hayes_job_id: &str, completion_sha256: &str,
        retirement: super::oex_recovery::SelectedRetirement,
        absent_historical_chunks: BTreeMap<usize, u64>, settled_only: bool,
    ) -> Result<Self> {
        if !oex_preflight_frames::uuid(hayes_job_id) || !hash_valid(completion_sha256) {
            return Err(err(ERROR));
        }
        Ok(Self { oex, hayes_job_id: hayes_job_id.into(), completion_sha256: completion_sha256.into(),
            retirement, absent_historical_chunks, settled_only, staging_bytes: None })
    }

    pub(super) fn database_query(&self, owner: &FurnishingOwner) -> Result<String> {
        self.retirement.recheck()?;
        let (attempt, request, source, claim, system, oid) = self.retirement.database_binding();
        if ![owner.workspace_id.as_str(), &owner.tenant_id, &owner.principal_id, &self.hayes_job_id,
            &self.oex.job_id, attempt, request].into_iter().all(oex_preflight_frames::uuid)
            || ![self.completion_sha256.as_str(), source, claim].into_iter().all(hash_valid)
            || system.parse::<u64>().ok().filter(|&id| id > 0).map(|id| id.to_string()).as_deref() != Some(system)
            || oid.parse::<u32>().ok().filter(|&id| id > 0).map(|id| id.to_string()).as_deref() != Some(oid)
        {
            return Err(err(ERROR));
        }
        let mut sql = include_str!("native_runtime_oex_selected_045.sql").to_owned();
        for (index, value) in [owner.workspace_id.as_str(), &owner.tenant_id, &owner.principal_id,
            &self.hayes_job_id, &self.completion_sha256, &self.oex.job_id, attempt, request, source,
            claim, system, oid].into_iter().enumerate()
        {
            sql = sql.replace(&format!("@{}@", index + 1), &format!("'{value}'"));
        }
        Ok(sql)
    }

    pub(super) fn hayes_environment(&self) -> String {
        serde_json::json!({"jobId": self.hayes_job_id, "completionFrameSha256": self.completion_sha256}).to_string()
    }

    pub(super) fn admit_staging(&mut self, value: &str) -> Result<()> {
        let refusal = || err("FURNISHING_RECOVERY_UNAVAILABLE");
        let staging: Selected045Staging = serde_json::from_str(value).map_err(|_| refusal())?;
        if !staging.admits(&self.absent_historical_chunks, self.settled_only) {
            return Err(refusal());
        }
        self.staging_bytes = Some(staging.bytes);
        Ok(())
    }

    pub(super) fn completed_staging_bytes(&self, receipt: Receipt) -> Result<usize> {
        self.retirement.recheck()?;
        self.staging_bytes.filter(|bytes| bytes.div_ceil(CHUNK_BYTES) == receipt.staging_chunks as usize)
            .ok_or_else(|| err(ERROR))
    }
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
    fn missing_historical_chunks_require_settled_exact_db_staging_shape() {
        let absent = BTreeMap::from([(0, CHUNK_BYTES as u64), (1, 7)]);
        assert!(Selected045Staging { bytes: CHUNK_BYTES + 7, cleanup_pending: false }.admits(&absent, false));
        assert!(!Selected045Staging { bytes: CHUNK_BYTES + 7, cleanup_pending: true }.admits(&absent, false));
        assert!(!Selected045Staging { bytes: CHUNK_BYTES + 8, cleanup_pending: false }.admits(&absent, false));
        assert!(!Selected045Staging { bytes: CHUNK_BYTES, cleanup_pending: false }.admits(&absent, false));
        assert!(Selected045Staging { bytes: CHUNK_BYTES, cleanup_pending: true }.admits(&BTreeMap::new(), false));
        assert!(!Selected045Staging { bytes: CHUNK_BYTES, cleanup_pending: true }.admits(&BTreeMap::new(), true));
        assert!(Selected045Staging { bytes: CHUNK_BYTES, cleanup_pending: false }.admits(&BTreeMap::new(), true));
    }

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
