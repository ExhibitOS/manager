// SPDX-License-Identifier: Apache-2.0
//! One durable, revision-checked import intent per selected personal workspace.
//! No source path, asset bytes, password, cookie or service token is stored here.
use super::*;
const LIMIT: u64 = 128 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent {
    pub artwork_id: String,
    pub idempotency_key: String,
    pub mime: String,
    pub sha256: String,
    pub bytes: u64,
    pub scale_meters: f64,
    pub rights: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transfer {
    pub format: u32,
    pub revision: u64,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub intent: Option<Intent>,
    pub job_id: Option<String>,
    pub file_name: Option<String>,
}
pub(crate) fn validate(s: &Transfer) -> Result<()> {
    if s.format != 1
        || !uuid(&s.workspace_id)
        || !uuid(&s.tenant_id)
        || !uuid(&s.principal_id)
        || s.job_id.as_ref().is_some_and(|id| !uuid(id))
        || s.file_name.as_ref().is_some_and(|name| {
            name.is_empty()
                || name.len() > 1024
                || name.chars().any(char::is_control)
                || name.contains('/')
                || name.contains('\\')
        })
        || (s.intent.is_none() && (s.job_id.is_some() || s.file_name.is_some()))
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    if let Some(i) = &s.intent
        && (!uuid(&i.artwork_id)
            || !uuid(&i.idempotency_key)
            || !matches!(i.mime.as_str(), "model/gltf-binary" | "image/png")
            || !hash_valid(&i.sha256)
            || i.bytes == 0
            || i.bytes > 32 * 1024 * 1024
            || !i.scale_meters.is_finite()
            || i.scale_meters <= 0.0
            || i.scale_meters > 1_000_000.0
            || !i.rights.is_object())
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    Ok(())
}
impl InstallationController {
    fn upload_unlocked(&self, service: &LifecycleService) -> Result<(String, Transfer)> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let file = format!(
            "native-upload-{}.json",
            digest(service.root.as_os_str().as_encoded_bytes())
        );
        let default = Transfer {
            format: 1,
            revision: 0,
            workspace_id,
            tenant_id,
            principal_id,
            intent: None,
            job_id: None,
            file_name: None,
        };
        let saved = match fs::symlink_metadata(profile.root.join(&file)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default.clone(),
            Ok(_) => serde_json::from_slice::<Transfer>(&installation_backup::source_bytes(
                &profile.root,
                &file,
                LIMIT,
                true,
            )?)
            .map_err(|_| err("NATIVE_UPLOAD_STATE_INVALID"))?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        validate(&saved)?;
        if saved.workspace_id != default.workspace_id
            || saved.tenant_id != default.tenant_id
            || saved.principal_id != default.principal_id
        {
            return Err(err("NATIVE_UPLOAD_SCOPE_CHANGED"));
        }
        Ok((file, saved))
    }
    pub fn native_upload(&self, token: &str) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            Ok(self.upload_unlocked(service)?.1)
        })
    }
    pub fn save_native_upload(&self, token: &str, mut state: Transfer) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            let (file, current) = self.upload_unlocked(service)?;
            validate(&state)?;
            if state.workspace_id != current.workspace_id
                || state.tenant_id != current.tenant_id
                || state.principal_id != current.principal_id
            {
                return Err(err("NATIVE_UPLOAD_SCOPE_CHANGED"));
            }
            if state.revision != current.revision {
                return Err(err("NATIVE_UPLOAD_STATE_CHANGED"));
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| err("NATIVE_UPLOAD_STATE_INVALID"))?;
            let bytes = serde_json::to_vec(&state).map_err(|_| err("STATE_INVALID"))?;
            if bytes.len() as u64 > LIMIT {
                return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
            }
            write_json(&profile.root, &file, &state)?;
            File::open(&profile.root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
            Ok(state)
        })
    }
}
