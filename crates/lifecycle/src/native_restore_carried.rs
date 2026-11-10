// SPDX-License-Identifier: Apache-2.0
//! Exact carried proof for the private catalog 045 restore bootstrap.
use super::*;

pub(super) const HANDOFF_SHA256: &str = "60ed3615a4072856a1cb1449fa532acd307d8c237a09095b356566db4ed90b88";
const ARCHIVE_CIPHER_SHA256: &str = "d176fb664d8f370d98dc7ef449b5fae7c4b85272402756187ecba5a02ccc5d5b";
const ARCHIVE_MANIFEST_SHA256: &str = "7a65e63f34d4bb036a346a611c8c9eacda4f02411caca320263e9f4b5ecb3e55";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub handoff: PathBuf,
    pub source_payload: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Observation {
    version: u32,
    source_handoff_sha256: String,
    prior_restore_proof: RestoreProof,
    archive_source_database_system_id: String,
    archive_source_database_oid: String,
    target_database_system_id: String,
    target_database_oid: String,
    provenance_row_count: u64,
    provenance_table_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Request<'a> {
    version: u32,
    source_handoff_sha256: &'static str,
    archive_backup_id: &'a str,
    archive_manifest_sha256: &'a str,
    prior_restore_proof: &'a RestoreProof,
}

impl Observation {
    pub(crate) fn matches(&self, archive_manifest: &str, target_system: &str) -> bool {
        let oid = self.target_database_oid.parse::<u32>().ok();
        self.version == 1 && self.source_handoff_sha256 == HANDOFF_SHA256
            && archive_manifest == ARCHIVE_MANIFEST_SHA256
            && self.target_database_system_id == target_system
            && target_system != self.archive_source_database_system_id
            && target_system != self.prior_restore_proof.source_database_system_id
            && oid.is_some_and(|oid| oid != 0 && oid.to_string() == self.target_database_oid)
            && self.archive_source_database_system_id == self.prior_restore_proof.target_database_system_id
            && self.archive_source_database_oid == self.prior_restore_proof.target_database_oid
            && self.provenance_row_count > 0
            && self.provenance_row_count == self.prior_restore_proof.provenance_row_count
            && self.provenance_table_sha256 == self.prior_restore_proof.provenance_table_sha256
    }

    pub(super) fn matches_manifest(&self, manifest: &Value, archive_id: &str) -> Result<()> {
        check_manifest(manifest, archive_id, &self.prior_restore_proof)
    }
}

fn check_manifest(manifest: &Value, archive_id: &str, proof: &RestoreProof) -> Result<()> {
    let source = &manifest["sourceIdentity"];
    let mut entries = manifest["inventory"]["tables"].as_array().ok_or_else(bad)?
        .iter().filter(|entry| entry["name"] == "native_oex_exhibition_provenance");
    let table = entries.next().ok_or_else(bad)?;
    if manifest["id"].as_str() != Some(archive_id)
        || source["systemIdentifier"].as_str() != Some(&proof.target_database_system_id)
        || source["databaseOid"].as_str() != Some(&proof.target_database_oid)
        || table["rowCount"].as_u64() != Some(proof.provenance_row_count)
        || table["sha256"].as_str() != Some(&proof.provenance_table_sha256)
        || entries.next().is_some() { return Err(bad()); }
    Ok(())
}

pub(super) struct Handoff {
    pub source_payload: Payload,
    original_plan: Plan,
    prior_restore_proof: RestoreProof,
    archive_backup_id: String,
    owner: Value,
}

impl Handoff {
    pub(super) fn open(input: &Input, profile: &Path, target: &Payload, cipher_sha256: &str) -> Result<Self> {
        if !crate::native_payload::catalog45_carried_restore_payload(target)
            || cipher_sha256 != ARCHIVE_CIPHER_SHA256 || !input.handoff.is_absolute()
            || input.handoff.starts_with(profile) || input.source_payload.starts_with(profile)
            || fs::canonicalize(&input.handoff).ok().as_deref() != Some(input.handoff.as_path())
            || fs::canonicalize(&input.source_payload).ok().as_deref() != Some(input.source_payload.as_path())
        { return Err(bad()); }
        let dir = input.handoff.parent().ok_or_else(bad)?;
        private_directory(dir)?;
        if input.handoff.file_name().and_then(|n| n.to_str()) != Some("handoff.json") { return Err(bad()); }
        let bytes = installation_backup::source_bytes(dir, "handoff.json", 4096, true)?;
        if digest(&bytes) != HANDOFF_SHA256 { return Err(bad()); }
        let sidecar: Value = serde_json::from_slice(&bytes).map_err(|_| bad())?;
        let files = sidecar["files"].as_object().ok_or_else(bad)?;
        let read = |name: &str, limit: u64| -> Result<Vec<u8>> {
            let bytes = installation_backup::source_bytes(dir, name, limit, true)?;
            if files.get(name).and_then(Value::as_str) != Some(digest(&bytes).as_str()) { return Err(bad()); }
            Ok(bytes)
        };
        let plan: Plan = serde_json::from_slice(&read("stage.json", 65536)?).map_err(|_| bad())?;
        let receipt: Receipt = serde_json::from_slice(&read("activated.json", 4096)?).map_err(|_| bad())?;
        let archived_bytes = read("restored-manifest.json", 16 * 1024 * 1024)?;
        let manifest: Value = serde_json::from_slice(&archived_bytes).map_err(|_| bad())?;
        let current: Value = serde_json::from_slice(&read("native-current-run.json", 8192)?).map_err(|_| bad())?;
        let cold: Value = serde_json::from_slice(&read("r62-cold-return-release.json", 4096)?).map_err(|_| bad())?;
        let writer: Value = serde_json::from_slice(&read("writer-state.json", 4096)?).map_err(|_| bad())?;
        let _: Value = serde_json::from_slice(&read("diagnostic-state.json", 4096)?).map_err(|_| bad())?;
        let proof = plan.restore_proof.clone().ok_or_else(bad)?;
        let binding = &current["furnishing"];
        if files.len() != 7 || sidecar["sourceCold"] != true || sidecar["sourcesRechecked"] != true
            || receipt.state != "activated" || receipt.id != plan.id || receipt.backup_id != plan.backup_id
            || plan.kind != "personal" || plan.manifest_sha256 != digest(&archived_bytes)
            || sidecar["restoreId"] != plan.id || sidecar["priorBackupId"] != plan.backup_id
            || sidecar["authenticatedOriginalManifestSha256"] != plan.manifest_sha256
            || sidecar["targetDatabaseSystemIdentifier"] != plan.system_identifier
            || sidecar["targetDatabaseOid"] != proof.target_database_oid
            || !proof.matches(&plan.id, &plan.backup_id, &plan.manifest_sha256, &plan.system_identifier)
            || proof.provenance_row_count == 0
            || binding["phase"] != "clean" || binding["owner"] != sidecar["owner"]
            || binding["physical"] != sidecar["physical"] || binding["generation"] != sidecar["writerGeneration"]
            || binding["restore"]["provenance"] != serde_json::to_value(&proof).map_err(|_| bad())?
            || current["id"] != sidecar["writerGeneration"] || writer["jobId"] != sidecar["writerGeneration"]
            || writer["state"] != "stopped"
            || writer["furnishingCompletion"]["bindingSha256"] != sidecar["writerBindingSha256"]
            || !writer["furnishingCompletion"]["consumedBy"].is_null()
            || cold["backupId"] != sidecar["r62BackupId"] || cold["writerBindingSha256"] != sidecar["writerBindingSha256"]
            || cold["encryptedManifestSha256"] != ARCHIVE_CIPHER_SHA256
            || cold["verifiedPlaintextManifestSha256"] != ARCHIVE_MANIFEST_SHA256 || cold["released"] != true
        { return Err(bad()); }
        proof.matches_manifest(&manifest)?;
        let source_payload = Payload::open(&input.source_payload)?;
        if !crate::native_payload::catalog45_carried_restore_source(&source_payload)
            || plan.payload_sha256 != source_payload.context().manifest_sha256
        { return Err(bad()); }
        let archive_backup_id = sidecar["r62BackupId"].as_str().filter(|id| uuid(id)).ok_or_else(bad)?.to_owned();
        Ok(Self { source_payload, original_plan: plan, prior_restore_proof: proof,
            archive_backup_id, owner: sidecar["owner"].clone() })
    }

    pub(super) fn check_archive(&self, source: &SourceMap, manifest: &Value, digest: &str, fields: &Fields) -> Result<()> {
        valid_plan(&self.original_plan, &source.source_root)?;
        check_manifest(manifest, &self.archive_backup_id, &self.prior_restore_proof)?;
        if digest != ARCHIVE_MANIFEST_SHA256 || source.manifest_sha256 != self.source_payload.context().manifest_sha256 {
            return Err(bad());
        }
        for (key, field) in [("workspaceId", "WORKSPACE_ID"), ("tenantId", "TENANT_ID"), ("principalId", "PRINCIPAL_ID")] {
            if self.owner[key].as_str() != fields.0.get(field).map(String::as_str) { return Err(bad()); }
        }
        let uid = self.owner["ownerUid"].as_u64().ok_or_else(bad)?.to_string();
        if fields.0.get("OWNER_UID") != Some(&uid) { return Err(bad()); }
        Ok(())
    }

    pub(super) fn request<'a>(&'a self, plan: &'a Plan) -> Request<'a> {
        Request { version: 1, source_handoff_sha256: HANDOFF_SHA256, archive_backup_id: &plan.backup_id,
            archive_manifest_sha256: &plan.manifest_sha256, prior_restore_proof: &self.prior_restore_proof }
    }

    pub(super) fn observation(&self, plan: &Plan, manifest: &Value, result: &Value) -> Result<Observation> {
        if result["operation"] != "restored-inventory-matched" || result["backupId"] != plan.backup_id
            || result["authenticatedManifestSha256"] != plan.manifest_sha256 || result["currentInventoryVerified"] != true
            || result.get("restoredProvenanceInventory").is_some() { return Err(bad()); }
        let proof: Observation = serde_json::from_value(result["carriedRestoreProvenanceInventory"].clone()).map_err(|_| bad())?;
        if proof.prior_restore_proof != self.prior_restore_proof || !proof.matches(&plan.manifest_sha256, &plan.system_identifier) {
            return Err(bad());
        }
        proof.matches_manifest(manifest, &plan.backup_id)?;
        Ok(proof)
    }
}
