// SPDX-License-Identifier: Apache-2.0
//! Separate workspace-bound OEX intent. No archive bytes or local path are persisted.
use super::*;
const LIMIT: u64 = 65536;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent { pub request_id: String, pub sha256: String, pub bytes: u64 }
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
        || s.intent.as_ref().is_some_and(|i| !uuid(&i.request_id) || !hash_valid(&i.sha256) || i.bytes == 0 || i.bytes > 64 * 1024 * 1024)
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
        let mut extra=state; extra["sourcePath"]=serde_json::json!("/tmp/private");
        assert!(serde_json::from_value::<Transfer>(extra).is_err());
    }
}
