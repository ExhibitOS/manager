// SPDX-License-Identifier: Apache-2.0
//! Separate workspace-bound OEX intent. No archive bytes or local path are persisted.
use super::*;
const LIMIT: u64 = 65536;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent {
    pub request_id: String, pub sha256: String, pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oex_format: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transfer {
    pub format: u32, pub revision: u64, pub workspace_id: String, pub tenant_id: String,
    pub principal_id: String, pub intent: Option<Intent>, pub job_id: Option<String>, pub file_name: Option<String>,
}
pub(crate) fn validate(s: &Transfer) -> Result<()> {
    if s.format != 1 || !uuid(&s.workspace_id) || !uuid(&s.tenant_id) || !uuid(&s.principal_id)
        || s.job_id.as_ref().is_some_and(|v| !uuid(v))
        || s.file_name.as_ref().is_some_and(|v| v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control) || v.contains('/') || v.contains('\\'))
        || (s.intent.is_none() && (s.job_id.is_some() || s.file_name.is_some()))
        || s.intent.as_ref().is_some_and(|i| {
            let limit = match (i.profile.as_deref(), i.oex_format.as_deref()) {
                (None, None) => 64 * 1024 * 1024,
                (Some("bounded-file-v1"), Some("1.0.0-draft.9")) => 512 * 1024 * 1024,
                _ => return true,
            };
            !uuid(&i.request_id) || !hash_valid(&i.sha256) || i.bytes == 0 || i.bytes > limit
        })
    { return Err(err("NATIVE_OEX_IMPORT_INVALID")); }
    Ok(())
}
impl InstallationController {
    fn oex_import_unlocked(&self, service: &LifecycleService) -> Result<(String, Transfer)> {
        let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let file = format!("native-oex-import-{}.json", digest(service.root.as_os_str().as_encoded_bytes()));
        let default = Transfer { format: 1, revision: 0, workspace_id, tenant_id, principal_id, intent: None, job_id: None, file_name: None };
        let saved = match fs::symlink_metadata(profile.root.join(&file)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default.clone(),
            Ok(_) => serde_json::from_slice::<Transfer>(&installation_backup::source_bytes(&profile.root, &file, LIMIT, true)?)
                .map_err(|_| err("NATIVE_OEX_IMPORT_INVALID"))?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        validate(&saved)?;
        if saved.workspace_id != default.workspace_id || saved.tenant_id != default.tenant_id || saved.principal_id != default.principal_id {
            return Err(err("NATIVE_OEX_IMPORT_SCOPE_CHANGED"));
        }
        Ok((file, saved))
    }
    pub fn native_oex_import(&self, token: &str) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; Ok(self.oex_import_unlocked(service)?.1)
        })
    }
    /// Host preflight runs before taking or stopping the owned runtime. The
    /// host serializes requests and marks backup busy before accepting more
    /// journal mutations; the backup core checks again under its snapshot lock.
    pub fn ensure_no_pending_oex_import(&self, token: &str) -> Result<()> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            let file = format!("native-oex-import-{}.json", digest(service.root.as_os_str().as_encoded_bytes()));
            match fs::symlink_metadata(profile.root.join(&file)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(err("STATE_UNAVAILABLE")),
                Ok(_) => {
                    let state = self.oex_import_unlocked(service)?.1;
                    if state.intent.is_some() { Err(err("NATIVE_OEX_IMPORT_PENDING")) } else { Ok(()) }
                }
            }
        })
    }
    pub fn save_native_oex_import(&self, token: &str, mut state: Transfer) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; let (file, current) = self.oex_import_unlocked(service)?;
            validate(&state)?;
            if state.workspace_id != current.workspace_id || state.tenant_id != current.tenant_id || state.principal_id != current.principal_id { return Err(err("NATIVE_OEX_IMPORT_SCOPE_CHANGED")); }
            if state.revision != current.revision { return Err(err("NATIVE_OEX_IMPORT_STATE_CHANGED")); }
            state.revision = state.revision.checked_add(1).ok_or_else(|| err("NATIVE_OEX_IMPORT_INVALID"))?;
            if serde_json::to_vec(&state).map_err(|_| err("STATE_INVALID"))?.len() as u64 > LIMIT { return Err(err("NATIVE_OEX_IMPORT_INVALID")); }
            write_json(&profile.root, &file, &state)?;
            File::open(&profile.root).and_then(|f| f.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
            Ok(state)
        })
    }
    /// Retain terminal proof before changing either pending document or fence.
    /// An exact same-session retry recognizes completed intermediate steps;
    /// a partial or conflicting receipt remains preserved and requires review.
    pub(crate) fn finish_selected_oex(
        &self, token: &str, expected: &Transfer, offer: &crate::native_host::OexSaveOffer,
        saved_etag: &str, proposed: &super::native_document::DocumentState,
    ) -> Result<super::native_document::DocumentState> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            let (file, current) = self.oex_import_unlocked(service)?;
            if expected.intent.is_none() || expected.job_id.is_none()
                || !uuid(&offer.attempt_id) || !uuid(&offer.request_id) || !uuid(&offer.exhibition_id) {
                return Err(err("NATIVE_OEX_IMPORT_SCOPE_CHANGED"));
            }
            let mut cleared = expected.clone();
            cleared.revision = cleared.revision.checked_add(1).ok_or_else(|| err("NATIVE_OEX_IMPORT_INVALID"))?;
            cleared.intent = None; cleared.job_id = None; cleared.file_name = None;
            let current_bytes = serde_json::to_vec(&current).map_err(|_| err("STATE_INVALID"))?;
            let expected_bytes = serde_json::to_vec(expected).map_err(|_| err("STATE_INVALID"))?;
            let cleared_bytes = serde_json::to_vec(&cleared).map_err(|_| err("STATE_INVALID"))?;
            if current_bytes != expected_bytes && current_bytes != cleared_bytes { return Err(err("NATIVE_OEX_IMPORT_SCOPE_CHANGED")); }
            super::native_document::validate(proposed)?;
            let saved_draft = proposed.draft.as_ref().ok_or_else(|| err("NATIVE_OEX_WORKER_UNCERTAIN"))?;
            if proposed.pending.is_some() || proposed.etag.as_deref() != Some(saved_etag)
                || proposed.workspace_id != expected.workspace_id || proposed.tenant_id != expected.tenant_id
                || proposed.principal_id != expected.principal_id
                || saved_draft.get("editVersion").and_then(Value::as_u64) != proposed.saved_edit_version
                || saved_draft.get("exhibitionId").and_then(Value::as_str) != Some(offer.exhibition_id.as_str()) {
                return Err(err("NATIVE_OEX_WORKER_UNCERTAIN"));
            }
            let (document_file, document) = self.document_unlocked(service)?;
            let mut saved = proposed.clone();
            saved.revision = saved.revision.checked_add(1).ok_or_else(|| err("NATIVE_DOCUMENT_INVALID"))?;
            let document_bytes = serde_json::to_vec(&document).map_err(|_| err("STATE_INVALID"))?;
            let saved_bytes = serde_json::to_vec(&saved).map_err(|_| err("STATE_INVALID"))?;
            let pending = document.pending.as_ref();
            let before = document.revision == proposed.revision && pending.is_some_and(|p|
                p.request_id == offer.request_id && p.etag.as_deref() == Some(offer.etag.as_str())
                && &p.draft == saved_draft && document.draft.as_ref() == Some(saved_draft))
                && document.workspace_id == proposed.workspace_id && document.tenant_id == proposed.tenant_id
                && document.principal_id == proposed.principal_id;
            if !before && document_bytes != saved_bytes { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
            if current_bytes == cleared_bytes && document_bytes != saved_bytes { return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")); }
            let receipt = serde_json::json!({"format":1,"workspaceId":expected.workspace_id,
                "jobId":expected.job_id,"attemptId":offer.attempt_id,"importRequestId":expected.intent.as_ref().map(|i| &i.request_id),
                "saveRequestId":offer.request_id,"exhibitionId":offer.exhibition_id,"oldETag":offer.etag,
                "savedETag":saved_etag,"draftSHA256":digest(&serde_json::to_vec(saved_draft).map_err(|_| err("STATE_INVALID"))?)});
            let receipt_bytes = serde_json::to_vec(&receipt).map_err(|_| err("STATE_INVALID"))?;
            let receipt_name = format!("native-oex-complete-{}-{}.json", digest(service.root.as_os_str().as_encoded_bytes()), offer.attempt_id);
            match fs::symlink_metadata(profile.root.join(&receipt_name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_private_new(&profile.root, &receipt_name, &receipt_bytes, 65536)?,
                Ok(_) if installation_backup::source_bytes(&profile.root, &receipt_name, 65536, true)? == receipt_bytes => {},
                _ => return Err(err("NATIVE_OEX_WORKER_UNCERTAIN")),
            }
            File::open(&profile.root).and_then(|f| f.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
            if before { write_json(&profile.root, &document_file, &saved)?;
                File::open(&profile.root).and_then(|f| f.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?; }
            if current_bytes == expected_bytes { write_json(&profile.root, &file, &cleared)?;
                File::open(&profile.root).and_then(|f| f.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?; }
            Ok(saved)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oex_journal_binds_request_and_refuses_paths_and_invalid_sizes() {
        let state = serde_json::json!({"format":1,"revision":0,"workspaceId":Uuid::new_v4().to_string(),"tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),"intent":{"requestId":Uuid::new_v4().to_string(),"sha256":"a".repeat(64),"bytes":100},"jobId":null,"fileName":"test.oex"});
        validate(&serde_json::from_value(state.clone()).unwrap()).unwrap();
        for (pointer, value) in [("/intent/bytes", serde_json::json!(67_108_865u64)), ("/intent/requestId", serde_json::json!("bad")), ("/fileName", serde_json::json!("../private.oex"))] {
            let mut bad=state.clone(); *bad.pointer_mut(pointer).unwrap()=value;
            assert!(validate(&serde_json::from_value(bad).unwrap()).is_err());
        }
        // Large journal metadata is valid only as the exact paired profile;
        // old records keep their original cap and serialization shape.
        let legacy: Transfer = serde_json::from_value(state.clone()).unwrap();
        assert_eq!(serde_json::to_value(&legacy).unwrap(), state);
        let mut large = state.clone();
        large["intent"]["bytes"] = serde_json::json!(64 * 1024 * 1024 + 1);
        large["intent"]["profile"] = serde_json::json!("bounded-file-v1");
        large["intent"]["oexFormat"] = serde_json::json!("1.0.0-draft.9");
        validate(&serde_json::from_value(large.clone()).unwrap()).unwrap();
        for (key, value) in [("profile", serde_json::json!(null)), ("oexFormat", serde_json::json!("1.0.0-draft.8")), ("bytes", serde_json::json!(536_870_913u64))] {
            let mut bad=large.clone(); bad["intent"][key]=value;
            assert!(validate(&serde_json::from_value(bad).unwrap()).is_err());
        }
        let mut extra=state; extra["sourcePath"]=serde_json::json!("/tmp/private");
        assert!(serde_json::from_value::<Transfer>(extra).is_err());
    }
}
