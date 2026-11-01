// SPDX-License-Identifier: Apache-2.0
//! Private, unconnected storage primitive for a selected imported save.
//! A record does not grant authority: a future caller must hold the profile
//! lock and revalidate the live session, selected offer and transfer together.
use super::*;

const LIMIT: usize = 2_200_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingImportedSave {
    pub format: u32,
    pub runtime_generation: String,
    pub job_id: String,
    pub attempt_id: String,
    pub import_request_id: String,
    pub transfer_revision: u64,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    pub exhibition_id: String,
    pub document: native_document::DocumentState,
}

fn validate(record: &PendingImportedSave, transfer: &native_oex_import::Transfer) -> Result<()> {
    native_oex_import::validate(transfer)?;
    native_document::validate(&record.document)?;
    let intent = transfer
        .intent
        .as_ref()
        .ok_or_else(|| err("NATIVE_OEX_IMPORT_INVALID"))?;
    let pending = record
        .document
        .pending
        .as_ref()
        .ok_or_else(|| err("NATIVE_OEX_IMPORT_INVALID"))?;
    let draft = record
        .document
        .draft
        .as_ref()
        .ok_or_else(|| err("NATIVE_OEX_IMPORT_INVALID"))?;
    if record.format != 1
        || !uuid(&record.runtime_generation)
        || !uuid(&record.job_id)
        || !uuid(&record.attempt_id)
        || !uuid(&record.import_request_id)
        || !uuid(&record.exhibition_id)
        || transfer.job_id.as_deref() != Some(record.job_id.as_str())
        || intent.request_id != record.import_request_id
        || transfer.revision != record.transfer_revision
        || intent.bytes != record.archive_bytes
        || intent.sha256 != record.archive_sha256
        || intent.profile.as_deref() != Some("bounded-file-v1")
        || intent.oex_format.as_deref() != Some("1.0.0-draft.9")
        || record.document.workspace_id != transfer.workspace_id
        || record.document.tenant_id != transfer.tenant_id
        || record.document.principal_id != transfer.principal_id
        || record.document.revision != 0
        || record.document.etag.is_none()
        || pending.draft != *draft
        || draft.get("exhibitionId").and_then(Value::as_str) != Some(record.exhibition_id.as_str())
        || draft.get("editVersion").and_then(Value::as_u64) != record.document.saved_edit_version
    {
        return Err(err("NATIVE_OEX_IMPORT_INVALID"));
    }
    Ok(())
}

/// Publish immutable bytes without replacement. The caller is responsible for
/// profile locking and checking live offer/owner before and after this call.
/// No request, retry, acknowledgement, journal clear or editor switch occurs.
pub(crate) fn create_pending(
    root: &Path,
    root_identity: &str,
    transfer: &native_oex_import::Transfer,
    record: &PendingImportedSave,
) -> Result<()> {
    validate(record, transfer)?;
    private_directory(root)?;
    if !hash_valid(root_identity) {
        return Err(err("NATIVE_OEX_IMPORT_INVALID"));
    }
    let bytes = serde_json::to_vec(record).map_err(|_| err("NATIVE_OEX_IMPORT_INVALID"))?;
    if bytes.len() > LIMIT {
        return Err(err("NATIVE_OEX_IMPORT_INVALID"));
    }
    let name = format!(
        "native-oex-save-{root_identity}-{}.json",
        record.exhibition_id
    );
    let destination = root.join(&name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            let existing = installation_backup::source_bytes(root, &name, LIMIT as u64, true)?;
            if existing != bytes {
                return Err(err("NATIVE_OEX_PENDING_CONFLICT"));
            }
            File::open(root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(err("STATE_UNAVAILABLE")),
    }
    let temp = root.join(format!(".native-oex-save-{}.tmp", Uuid::new_v4()));
    let mut file = private_options()
        .open(&temp)
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    // hard_link fails if the destination exists; no other record is replaced.
    fs::hard_link(&temp, &destination).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            err("NATIVE_OEX_PENDING_CONFLICT")
        } else {
            err("STATE_UNAVAILABLE")
        }
    })?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    fs::remove_file(&temp).map_err(|_| err("STATE_UNAVAILABLE"))?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("STATE_UNAVAILABLE"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (native_oex_import::Transfer, PendingImportedSave) {
        let workspace = Uuid::new_v4().to_string();
        let tenant = Uuid::new_v4().to_string();
        let principal = Uuid::new_v4().to_string();
        let job = Uuid::new_v4().to_string();
        let import_request = Uuid::new_v4().to_string();
        let save_request = Uuid::new_v4().to_string();
        let exhibition = Uuid::new_v4().to_string();
        let draft = serde_json::json!({"kind":"exhibition-draft", "schemaVersion":"1.0.0-draft.1",
            "exhibitionId":exhibition, "id":Uuid::new_v4().to_string(), "candidate":{"id":exhibition}, "editVersion":1});
        let etag = format!("\"studio-r1-{}\"", "a".repeat(64));
        let transfer = serde_json::from_value(
            serde_json::json!({"format":1,"revision":1,"workspaceId":workspace,
            "tenantId":tenant,"principalId":principal,"jobId":job,"fileName":"synthetic.oex",
            "intent":{"requestId":import_request,"sha256":"b".repeat(64),"bytes":67_108_865,
                      "profile":"bounded-file-v1","oexFormat":"1.0.0-draft.9"}}),
        )
        .unwrap();
        let document = serde_json::from_value(serde_json::json!({"format":1,"revision":0,"workspaceId":workspace,
            "tenantId":tenant,"principalId":principal,"draft":draft,"etag":etag,"savedEditVersion":1,
            "pending":{"requestId":save_request,"etag":etag,"draft":draft}})).unwrap();
        let record = PendingImportedSave {
            format: 1,
            runtime_generation: Uuid::new_v4().to_string(),
            job_id: job,
            attempt_id: Uuid::new_v4().to_string(),
            import_request_id: import_request,
            transfer_revision: 1,
            archive_bytes: 67_108_865,
            archive_sha256: "b".repeat(64),
            exhibition_id: exhibition,
            document,
        };
        (transfer, record)
    }

    #[test]
    fn selected_save_is_durable_idempotent_and_never_replaces_a_conflict() {
        let root = std::env::temp_dir().join(format!("exhibitos-oex-pending-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let (transfer, record) = fixture();
        let root_identity = "c".repeat(64);
        create_pending(&root, &root_identity, &transfer, &record).unwrap();
        let path = root.join(format!(
            "native-oex-save-{root_identity}-{}.json",
            record.exhibition_id
        ));
        let original = fs::read(&path).unwrap();
        create_pending(&root, &root_identity, &transfer, &record).unwrap();
        let mut conflict = record.clone();
        conflict.attempt_id = Uuid::new_v4().to_string();
        assert!(create_pending(&root, &root_identity, &transfer, &conflict).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        let mut wrong = record.clone();
        wrong.import_request_id = Uuid::new_v4().to_string();
        assert!(create_pending(&root, &root_identity, &transfer, &wrong).is_err());
        let mut wrong_digest = record.clone();
        wrong_digest.archive_sha256 = "d".repeat(64);
        assert!(create_pending(&root, &root_identity, &transfer, &wrong_digest).is_err());
        let mut wrong_draft = record.clone();
        wrong_draft.document.pending.as_mut().unwrap().draft["editVersion"] = serde_json::json!(2);
        assert!(create_pending(&root, &root_identity, &transfer, &wrong_draft).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        // A crash after publication but before removal can leave two names.
        // The primitive must retain that uncertainty, not adopt or overwrite it.
        fs::hard_link(&path, root.join("retained-link")).unwrap();
        assert!(create_pending(&root, &root_identity, &transfer, &record).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}
