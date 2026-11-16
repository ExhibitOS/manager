// SPDX-License-Identifier: Apache-2.0
//! Authenticated, pinned carried restore handoffs.
use super::*;

pub(super) const HANDOFF_SHA256: &str = "60ed3615a4072856a1cb1449fa532acd307d8c237a09095b356566db4ed90b88";
const ARCHIVE_CIPHER_SHA256: &str = "d176fb664d8f370d98dc7ef449b5fae7c4b85272402756187ecba5a02ccc5d5b";
const ARCHIVE_MANIFEST_SHA256: &str = "7a65e63f34d4bb036a346a611c8c9eacda4f02411caca320263e9f4b5ecb3e55";

#[derive(Clone, Copy)]
struct ArchivePin {
    handoff: &'static str,
    ciphertext: &'static str,
    manifest: &'static str,
}

const HISTORICAL: ArchivePin = ArchivePin {
    handoff: HANDOFF_SHA256,
    ciphertext: ARCHIVE_CIPHER_SHA256,
    manifest: ARCHIVE_MANIFEST_SHA256,
};

struct Catalog52SourcePin<'a> {
    archive: ArchivePin,
    backup_id: &'a str,
    prior_issuer: &'a str,
    prior_proof_sha256: &'a str,
}

impl Catalog52SourcePin<'_> {
    fn matches_prior(&self, proof: &RestoreProof) -> bool {
        serde_json::to_vec(proof).is_ok_and(|bytes| digest(&bytes) == self.prior_proof_sha256)
    }
}

// A separate authorization and independent review must supply the actual
// detached owner's archive and prior activation before this route can run.
const CATALOG52_SOURCE: Option<Catalog52SourcePin<'static>> = None;

pub(crate) fn catalog52_source_pinned() -> bool {
    CATALOG52_SOURCE.is_some()
}

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
        self.matches_pin(HISTORICAL, archive_manifest, target_system)
    }

    fn matches_pin(&self, pin: ArchivePin, archive_manifest: &str, target_system: &str) -> bool {
        let oid = self.target_database_oid.parse::<u32>().ok();
        self.version == 1 && self.source_handoff_sha256 == pin.handoff
            && archive_manifest == pin.manifest
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

    pub(crate) fn matches_catalog52(
        &self, manifest: &str, target_system: &str, restore_id: &str, backup_id: &str,
    ) -> bool {
        CATALOG52_SOURCE.as_ref().is_some_and(|source|
            self.matches_catalog52_pin(source, manifest, target_system, restore_id, backup_id))
    }

    fn matches_catalog52_pin(
        &self, source: &Catalog52SourcePin<'_>, manifest: &str, target_system: &str, restore_id: &str, backup_id: &str,
    ) -> bool {
        let prior = &self.prior_restore_proof;
        self.matches_pin(source.archive, manifest, target_system)
            && source.matches_prior(prior)
            && uuid(restore_id) && uuid(backup_id) && backup_id == source.backup_id
            && prior.restore_id != restore_id && prior.backup_id != backup_id
            && prior.authenticated_manifest_sha256 != manifest
            && prior.matches(&prior.restore_id, &prior.backup_id, &prior.authenticated_manifest_sha256,
                &self.archive_source_database_system_id)
            && target_system.parse::<u64>().is_ok_and(|id| id != 0 && id.to_string() == target_system)
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

// A current clean Catalog52 binding retains the issued carried activation.
// The ordered rows still identify their original DB, while their custodian
// (now the archive source) is the carried activation's observed target.
fn catalog52_prior_proof(
    plan: &Plan, manifest: &Value, binding: &Value, completion_sha256: &str,
) -> Result<RestoreProof> {
    if crate::native_runtime::catalog52_carried_checkpoint_sha256(binding)? != completion_sha256 {
        return Err(bad());
    }
    let observation = &plan.carried_restore.as_ref().ok_or_else(bad)?.proof;
    let restore = &binding["restore"];
    if plan.restore_proof.is_some() || plan.private_codec_manifest_sha256.is_some()
        || !restore["provenance"].is_null() || !restore["privateCodecManifestSha256"].is_null()
        || restore["id"] != plan.id || restore["backupId"] != plan.backup_id
        || restore["archiveManifestSha256"] != plan.manifest_sha256
        || restore["targetSystemIdentifier"] != plan.system_identifier
        || restore["carriedProvenance"] != serde_json::to_value(observation).map_err(|_| bad())?
        || !observation.matches(&plan.manifest_sha256, &plan.system_identifier)
    {
        return Err(bad());
    }
    observation.matches_manifest(manifest, &plan.backup_id)?;
    let original = &observation.prior_restore_proof;
    if !original.matches(&original.restore_id, &original.backup_id, &original.authenticated_manifest_sha256,
        &observation.archive_source_database_system_id)
        || original.restore_id == plan.id || original.backup_id == plan.backup_id
        || original.authenticated_manifest_sha256 == plan.manifest_sha256
    {
        return Err(bad());
    }
    Ok(RestoreProof {
        restore_id: plan.id.clone(),
        backup_id: plan.backup_id.clone(),
        authenticated_manifest_sha256: plan.manifest_sha256.clone(),
        source_database_system_id: original.source_database_system_id.clone(),
        source_database_oid: original.source_database_oid.clone(),
        target_database_system_id: observation.target_database_system_id.clone(),
        target_database_oid: observation.target_database_oid.clone(),
        provenance_row_count: observation.provenance_row_count,
        provenance_table_sha256: observation.provenance_table_sha256.clone(),
    })
}

fn check_cold_runs(sidecar: &Value, current: &Value, writer: &Value, diagnostic: &Value, cold: &Value) -> Result<()> {
    let writer_id = sidecar["writerGeneration"].as_str().filter(|id| uuid(id)).ok_or_else(bad)?;
    let diagnostic_id = sidecar["diagnosticJobId"].as_str().filter(|id| uuid(id)).ok_or_else(bad)?;
    let binding_hash = sidecar["writerBindingSha256"].as_str().filter(|sha| hash_valid(sha)).ok_or_else(bad)?;
    for run in [writer, diagnostic] {
        if run["format"] != 1 || run["state"] != "stopped" || run["readiness"]["ready"] != false
            || run["exits"]["postgres"] != 0 || run["exits"]["auxiliary"] != 0
            || ["postgresPid", "nodePid", "auxiliaryPid", "errorCode", "failureCode"]
                .iter().any(|field| run.get(field).is_none_or(|value| !value.is_null()))
        {
            return Err(bad());
        }
    }
    if writer_id == diagnostic_id || current["format"] != 1 || current["id"] != diagnostic_id
        || current["furnishing"]["generation"] != writer_id || writer["jobId"] != writer_id
        || diagnostic["jobId"] != diagnostic_id || diagnostic["ports"] != writer["ports"]
        || diagnostic["exits"].get("node").is_some() || diagnostic.get("furnishingCompletion").is_some()
        || writer["exits"]["node"] != 0 || writer["furnishingCompletion"]["bindingSha256"] != binding_hash
        || writer["furnishingCompletion"].get("consumedBy").is_none_or(|value| !value.is_null())
        || cold["diagnosticJobId"] != diagnostic_id || cold["writerGeneration"] != writer_id
        || cold["writerBindingSha256"] != binding_hash || cold["portsClosed"] != true
        || cold["released"] != true || cold["execution_ended"] != true || cold["helperExit"] != 0
    {
        return Err(bad());
    }
    Ok(())
}

pub(super) struct Handoff {
    pub source_payload: Payload,
    original_plan: Plan,
    prior_restore_proof: RestoreProof,
    archive_backup_id: String,
    owner: Value,
    pin: ArchivePin,
    catalog52_source: Option<(PathBuf, FurnishingWriterIdentity)>,
}

impl Handoff {
    pub(super) fn open(input: &Input, profile: &Path, target: &Payload, cipher_sha256: &str) -> Result<Self> {
        let catalog52 = if crate::native_payload::catalog45_carried_restore_payload(target) {
            None
        } else {
            let context = target.context();
            let tuple = (context.source_commit.as_str(), target.producer_sha256(), context.manifest_sha256.as_str());
            if !target.private_backup_codec_manifest()
                .is_some_and(|codec| crate::native_payload::private_exb_restore_host_matches(tuple, codec))
            {
                return Err(bad());
            }
            Some(CATALOG52_SOURCE.as_ref().ok_or_else(bad)?)
        };
        let pin = catalog52.map_or(HISTORICAL, |current| current.archive);
        if cipher_sha256 != pin.ciphertext || !input.handoff.is_absolute()
            || input.handoff.starts_with(profile) || input.source_payload.starts_with(profile)
            || fs::canonicalize(&input.handoff).ok().as_deref() != Some(input.handoff.as_path())
            || fs::canonicalize(&input.source_payload).ok().as_deref() != Some(input.source_payload.as_path())
        { return Err(bad()); }
        let dir = input.handoff.parent().ok_or_else(bad)?;
        private_directory(dir)?;
        if input.handoff.file_name().and_then(|n| n.to_str()) != Some("handoff.json") { return Err(bad()); }
        let bytes = installation_backup::source_bytes(dir, "handoff.json", 4096, true)?;
        if digest(&bytes) != pin.handoff { return Err(bad()); }
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
        let cold_name = if catalog52.is_some() { "cold-return-release.json" } else { "r62-cold-return-release.json" };
        let cold: Value = serde_json::from_slice(&read(cold_name, 4096)?).map_err(|_| bad())?;
        let writer: Value = serde_json::from_slice(&read("writer-state.json", 4096)?).map_err(|_| bad())?;
        let diagnostic: Value = serde_json::from_slice(&read("diagnostic-state.json", 4096)?).map_err(|_| bad())?;
        check_cold_runs(&sidecar, &current, &writer, &diagnostic, &cold)?;
        let binding = &current["furnishing"];
        let proof = if catalog52.is_some() {
            let completion_sha256 = sidecar["writerBindingSha256"].as_str().ok_or_else(bad)?;
            catalog52_prior_proof(&plan, &manifest, binding, completion_sha256)?
        } else {
            let proof = plan.restore_proof.clone().ok_or_else(bad)?;
            if binding["restore"]["provenance"] != serde_json::to_value(&proof).map_err(|_| bad())? {
                return Err(bad());
            }
            proof.matches_manifest(&manifest)?;
            proof
        };
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
            || binding["physical"] != sidecar["physical"]
            || cold["backupId"] != sidecar[if catalog52.is_some() { "backupId" } else { "r62BackupId" }]
            || cold["encryptedManifestSha256"] != pin.ciphertext
            || cold["verifiedPlaintextManifestSha256"] != pin.manifest
        { return Err(bad()); }
        let source_payload = Payload::open(&input.source_payload)?;
        let catalog52_source = if let Some(current_pin) = catalog52 {
            let context = source_payload.context();
            let tuple = (context.source_commit.as_str(), source_payload.producer_sha256(), context.manifest_sha256.as_str());
            let source_root = sidecar["sourceRoot"].as_str().map(PathBuf::from).ok_or_else(bad)?;
            if !target.private_backup_codec_manifest()
                .is_some_and(|codec| crate::native_payload::private_exb_restore_host_matches(tuple, codec))
                || plan.payload_sha256 != current_pin.prior_issuer
                || crate::native_payload::combined_thumbnail_carried_receipt_payload(tuple) != Some(current_pin.prior_issuer)
                || !current_pin.matches_prior(&proof)
                || binding["sourceCommit"] != tuple.0 || binding["producerSha256"] != tuple.1
                || binding["payloadManifestSha256"] != tuple.2
                || !source_root.is_absolute() || source_root.starts_with(profile)
                || source_root.components().any(|part| matches!(part, Component::ParentDir))
            {
                return Err(bad());
            }
            let physical = serde_json::from_value(binding["physical"].clone()).map_err(|_| bad())?;
            Some((source_root, physical))
        } else {
            if !crate::native_payload::catalog45_carried_restore_source(&source_payload)
                || plan.payload_sha256 != source_payload.context().manifest_sha256
            {
                return Err(bad());
            }
            None
        };
        let backup_field = if catalog52.is_some() { "backupId" } else { "r62BackupId" };
        let archive_backup_id = sidecar[backup_field].as_str().filter(|id| uuid(id)).ok_or_else(bad)?.to_owned();
        if catalog52.is_some_and(|current| archive_backup_id != current.backup_id
            || archive_backup_id == proof.backup_id || pin.manifest == proof.authenticated_manifest_sha256)
        {
            return Err(bad());
        }
        Ok(Self { source_payload, original_plan: plan, prior_restore_proof: proof,
            archive_backup_id, owner: sidecar["owner"].clone(), pin, catalog52_source })
    }

    pub(super) fn check_archive(&self, source: &SourceMap, manifest: &Value, digest: &str, fields: &Fields) -> Result<()> {
        valid_plan(&self.original_plan, &source.source_root)?;
        check_manifest(manifest, &self.archive_backup_id, &self.prior_restore_proof)?;
        if digest != self.pin.manifest || source.manifest_sha256 != self.source_payload.context().manifest_sha256
            || self.catalog52_source.as_ref().is_some_and(|(root, _)| root != &source.source_root)
        {
            return Err(bad());
        }
        for (key, field) in [("workspaceId", "WORKSPACE_ID"), ("tenantId", "TENANT_ID"), ("principalId", "PRINCIPAL_ID")] {
            if self.owner[key].as_str() != fields.0.get(field).map(String::as_str) { return Err(bad()); }
        }
        let uid = self.owner["ownerUid"].as_u64().ok_or_else(bad)?.to_string();
        if fields.0.get("OWNER_UID") != Some(&uid) { return Err(bad()); }
        Ok(())
    }

    pub(super) fn check_target(
        &self, target: Option<FurnishingWriterIdentity>, system_identifier: &str,
    ) -> Result<()> {
        if let Some((_, source)) = &self.catalog52_source {
            if !target.is_some_and(|target| target.is_separate_from(source))
                || system_identifier == self.prior_restore_proof.target_database_system_id
                || system_identifier == self.prior_restore_proof.source_database_system_id
            {
                return Err(bad());
            }
        }
        Ok(())
    }

    pub(super) fn check_catalog52_archive(
        &self, source: &SourceMap, manifest: &Value, digest: &str, backup_id: &str,
    ) -> Result<()> {
        if let Some((root, _)) = &self.catalog52_source {
            if root != &source.source_root || digest != self.pin.manifest || backup_id != self.archive_backup_id
                || source.manifest_sha256 != self.source_payload.context().manifest_sha256
            {
                return Err(bad());
            }
            check_manifest(manifest, backup_id, &self.prior_restore_proof)?;
        }
        Ok(())
    }

    pub(super) fn request<'a>(&'a self, plan: &'a Plan) -> Request<'a> {
        Request { version: 1, source_handoff_sha256: self.pin.handoff, archive_backup_id: &plan.backup_id,
            archive_manifest_sha256: &plan.manifest_sha256, prior_restore_proof: &self.prior_restore_proof }
    }

    pub(super) fn observation(&self, plan: &Plan, manifest: &Value, result: &Value) -> Result<Observation> {
        if result["operation"] != "restored-inventory-matched" || result["backupId"] != plan.backup_id
            || result["authenticatedManifestSha256"] != plan.manifest_sha256 || result["currentInventoryVerified"] != true
            || result.get("restoredProvenanceInventory").is_some() { return Err(bad()); }
        let proof: Observation = serde_json::from_value(result["carriedRestoreProvenanceInventory"].clone()).map_err(|_| bad())?;
        let matches = if self.catalog52_source.is_some() {
            proof.matches_catalog52(&plan.manifest_sha256, &plan.system_identifier, &plan.id, &plan.backup_id)
        } else {
            proof.matches(&plan.manifest_sha256, &plan.system_identifier)
        };
        if proof.prior_restore_proof != self.prior_restore_proof || !matches {
            return Err(bad());
        }
        proof.matches_manifest(manifest, &plan.backup_id)?;
        Ok(proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog52_private_exb_requires_exact_pinned_provenance_and_separate_target() {
        let pin = ArchivePin {
            handoff: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ciphertext: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            manifest: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        };
        let prior_restore = "00000000-0000-4000-8000-000000000001";
        let prior_backup = "00000000-0000-4000-8000-000000000002";
        let restore = "00000000-0000-4000-8000-000000000003";
        let backup = "00000000-0000-4000-8000-000000000004";
        let table_sha = "d".repeat(64);
        let value = json!({
            "version": 1, "sourceHandoffSha256": pin.handoff,
            "priorRestoreProof": {
                "restoreId": prior_restore, "backupId": prior_backup, "authenticatedManifestSha256": "e".repeat(64),
                "sourceDatabaseSystemId": "101", "sourceDatabaseOid": "16384",
                "targetDatabaseSystemId": "202", "targetDatabaseOid": "16385",
                "provenanceRowCount": 1, "provenanceTableSha256": table_sha,
            },
            "archiveSourceDatabaseSystemId": "202", "archiveSourceDatabaseOid": "16385",
            "targetDatabaseSystemId": "303", "targetDatabaseOid": "16386",
            "provenanceRowCount": 1, "provenanceTableSha256": table_sha,
        });
        let parse = |value: Value| serde_json::from_value::<Observation>(value).unwrap();
        let proof = parse(value.clone());
        let prior_sha = digest(&serde_json::to_vec(&proof.prior_restore_proof).unwrap());
        let source = Catalog52SourcePin { archive: pin, backup_id: backup, prior_issuer: pin.manifest,
            prior_proof_sha256: &prior_sha };
        assert!(proof.matches_catalog52_pin(&source, pin.manifest, "303", restore, backup));
        assert!(!proof.matches_catalog52(pin.manifest, "303", restore, backup));
        assert!(!proof.matches(pin.manifest, "303"));
        assert!(!proof.matches_catalog52_pin(&source, HISTORICAL.manifest, "303", restore, backup));
        assert!(!proof.matches_catalog52_pin(&source, pin.manifest, "303", prior_restore, backup));
        assert!(!proof.matches_catalog52_pin(&source, pin.manifest, "303", restore, prior_backup));
        for (path, invalid) in [
            ("/sourceHandoffSha256", json!(HISTORICAL.handoff)),
            ("/priorRestoreProof/authenticatedManifestSha256", json!(pin.manifest)),
            ("/priorRestoreProof/sourceDatabaseSystemId", json!("104")),
            ("/archiveSourceDatabaseSystemId", json!("505")),
            ("/archiveSourceDatabaseOid", json!("16390")),
            ("/targetDatabaseOid", json!("016386")),
            ("/provenanceRowCount", json!(0)),
            ("/provenanceTableSha256", json!("f".repeat(64))),
        ] {
            let mut wrong = value.clone();
            *wrong.pointer_mut(path).unwrap() = invalid;
            assert!(!parse(wrong).matches_catalog52_pin(&source, pin.manifest, "303", restore, backup), "{path}");
        }
        for repeated in ["101", "202", "000", "0303"] {
            let mut wrong = value.clone();
            wrong["targetDatabaseSystemId"] = json!(repeated);
            assert!(!parse(wrong).matches_catalog52_pin(&source, pin.manifest, repeated, restore, backup));
        }
        let manifest = json!({"id":backup,"sourceIdentity":{"systemIdentifier":"202","databaseOid":"16385"},
            "inventory":{"tables":[{"name":"native_oex_exhibition_provenance","rowCount":1,"sha256":table_sha}]}});
        assert!(proof.matches_manifest(&manifest, backup).is_ok());
        let mut wrong = manifest.clone();
        wrong["sourceIdentity"]["databaseOid"] = json!("16386");
        assert!(proof.matches_manifest(&wrong, backup).is_err());

        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        {
            let seal = crate::native_payload::FIXED64_PUBLICATION_REPAIR_TARGET.2;
            let codec = "f5267e7123e8562c018feb2f446759beaa0e8878ab0cd36be67d447dfe4c2f0d";
            assert!(crate::native_payload::private_exb_restore_seals_match(seal, codec));
            assert!(!crate::native_payload::private_exb_restore_seals_match(pin.manifest, codec));
            assert!(!crate::native_payload::private_exb_restore_seals_match(seal, pin.manifest));
        }

        let physical = |root, lock| serde_json::from_value::<FurnishingWriterIdentity>(
            json!({"rootDevice":1,"rootInode":root,"lockDevice":1,"lockInode":lock})).unwrap();
        assert!(physical(30, 40).is_separate_from(&physical(10, 20)));
        assert!(!physical(10, 40).is_separate_from(&physical(10, 20)));
        assert!(!physical(30, 20).is_separate_from(&physical(10, 20)));
        assert!(!physical(30, 0).is_separate_from(&physical(10, 20)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog52_handoff_projects_the_product_issued_carried_activation() {
        let tuple = crate::native_payload::FIXED64_PUBLICATION_REPAIR_TARGET;
        let archived_issuer = crate::native_payload::combined_thumbnail_carried_receipt_payload(tuple).unwrap();
        let id = |index| format!("10000000-0000-4000-8000-{index:012}");
        let table_sha = "a".repeat(64);
        let observation: Observation = serde_json::from_value(json!({
            "version":1, "sourceHandoffSha256":HISTORICAL.handoff,
            "priorRestoreProof":{
                "restoreId":id(1),"backupId":id(2),"authenticatedManifestSha256":"b".repeat(64),
                "sourceDatabaseSystemId":"100","sourceDatabaseOid":"16384",
                "targetDatabaseSystemId":"200","targetDatabaseOid":"16385",
                "provenanceRowCount":1,"provenanceTableSha256":table_sha,
            },
            "archiveSourceDatabaseSystemId":"200","archiveSourceDatabaseOid":"16385",
            "targetDatabaseSystemId":"300","targetDatabaseOid":"16386",
            "provenanceRowCount":1,"provenanceTableSha256":table_sha,
        })).unwrap();
        let mut plan = Plan {
            format:1, id:id(3), backup_id:id(4), manifest_sha256:HISTORICAL.manifest.into(),
            payload_sha256:archived_issuer.into(), system_identifier:"300".into(), kind:"personal".into(),
            ports:Ports::default(), files:vec![], attempt:id(5), restore_proof:None,
            carried_restore:Some(CarriedPlan {
                input:Input { handoff:"/offline/handoff.json".into(),source_payload:"/offline/source".into() },
                proof:observation.clone(),
            }),
            private_codec_manifest_sha256:None,
        };
        let binding = json!({
            "version":1,"phase":"clean","generation":id(6),"previousGeneration":id(7),"admissionKind":"clean-reopen",
            "owner":{"workspaceId":id(8),"tenantId":id(9),"principalId":id(10),"ownerUid":501},
            "sourceCommit":tuple.0,"producerSha256":tuple.1,"payloadManifestSha256":tuple.2,"catalog":52,
            "physical":{"rootDevice":1,"rootInode":12,"lockDevice":1,"lockInode":14},
            "restore":{"id":plan.id,"backupId":plan.backup_id,"archiveManifestSha256":plan.manifest_sha256,
                "targetSystemIdentifier":plan.system_identifier,"carriedProvenance":observation},
        });
        let inventory = json!({"tables":[{"name":"native_oex_exhibition_provenance","rowCount":1,"sha256":table_sha}]});
        let manifest = json!({"id":plan.backup_id,"sourceIdentity":{"systemIdentifier":"200","databaseOid":"16385"},
                              "inventory":inventory});
        let completion = crate::native_runtime::catalog52_carried_checkpoint_sha256(&binding).unwrap();
        let proof = catalog52_prior_proof(&plan, &manifest, &binding, &completion).unwrap();
        assert!(catalog52_prior_proof(&plan, &manifest, &binding, &"f".repeat(64)).is_err());
        assert_eq!(proof.restore_id, plan.id);
        assert_eq!(proof.backup_id, plan.backup_id);
        assert_eq!(proof.authenticated_manifest_sha256, HISTORICAL.manifest);
        assert_eq!((&*proof.source_database_system_id, &*proof.source_database_oid), ("100", "16384"));
        assert_eq!((&*proof.target_database_system_id, &*proof.target_database_oid), ("300", "16386"));
        let archive = json!({"id":id(11),"sourceIdentity":{"systemIdentifier":"300","databaseOid":"16386"},
                             "inventory":inventory});
        assert!(check_manifest(&archive, &id(11), &proof).is_ok());
        assert!(check_manifest(&archive, &id(11), &observation.prior_restore_proof).is_err());
        for (path, invalid) in [
            ("/phase", json!("active")),
            ("/restore/targetSystemIdentifier", json!("200")),
            ("/restore/backupId", json!(id(2))),
            ("/restore/carriedProvenance/provenanceRowCount", json!(0)),
        ] {
            let mut wrong = binding.clone();
            *wrong.pointer_mut(path).unwrap() = invalid;
            assert!(catalog52_prior_proof(&plan, &manifest, &wrong, &completion).is_err(), "{path}");
        }
        let mut wrong = archive.clone();
        wrong["sourceIdentity"]["databaseOid"] = json!("16385");
        assert!(check_manifest(&wrong, &id(11), &proof).is_err());
        let mut wrong = manifest.clone();
        wrong["inventory"]["tables"][0]["sha256"] = json!("c".repeat(64));
        assert!(catalog52_prior_proof(&plan, &wrong, &binding, &completion).is_err());
        plan.restore_proof = Some(observation.prior_restore_proof);
        assert!(catalog52_prior_proof(&plan, &manifest, &binding, &completion).is_err());
    }

    #[test]
    fn cold_runs_keep_database_diagnostic_distinct_from_writer_completion() {
        let writer_id = "00000000-0000-4000-8000-000000000001";
        let diagnostic_id = "00000000-0000-4000-8000-000000000002";
        let binding_hash = "a".repeat(64);
        let sidecar = json!({
            "writerGeneration": writer_id, "diagnosticJobId": diagnostic_id,
            "writerBindingSha256": binding_hash,
        });
        let current = json!({"format": 1, "id": diagnostic_id, "furnishing": {"generation": writer_id}});
        let mut diagnostic = json!({
            "format": 1, "jobId": diagnostic_id, "state": "stopped",
            "ports": {"postgres": 57001, "api": 57002, "web": 57003}, "readiness": {"ready": false},
            "postgresPid": null, "nodePid": null, "auxiliaryPid": null, "errorCode": null,
            "failureCode": null, "exits": {"postgres": 0, "auxiliary": 0},
        });
        let mut writer = diagnostic.clone();
        writer["jobId"] = json!(writer_id);
        writer["exits"]["node"] = json!(0);
        writer["furnishingCompletion"] = json!({"bindingSha256": binding_hash, "consumedBy": null});
        let cold = json!({
            "writerGeneration": writer_id, "diagnosticJobId": diagnostic_id,
            "writerBindingSha256": binding_hash, "portsClosed": true, "released": true,
            "execution_ended": true, "helperExit": 0,
        });
        assert!(check_cold_runs(&sidecar, &current, &writer, &diagnostic, &cold).is_ok());

        let mut wrong_current = current.clone();
        wrong_current["id"] = json!(writer_id);
        assert!(check_cold_runs(&sidecar, &wrong_current, &writer, &diagnostic, &cold).is_err());
        diagnostic["exits"]["node"] = json!(0);
        assert!(check_cold_runs(&sidecar, &current, &writer, &diagnostic, &cold).is_err());
    }
}
