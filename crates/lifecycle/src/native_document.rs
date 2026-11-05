// SPDX-License-Identifier: Apache-2.0
//! A current native exhibition draft and its exact uncertain save request.
//! Private profile data only; no credentials, source paths or asset bytes.
use super::*;
const LIMIT: u64 = 2_200_000;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pending {
    pub request_id: String,
    pub etag: Option<String>,
    pub draft: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentState {
    pub format: u32,
    pub revision: u64,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub draft: Option<Value>,
    pub etag: Option<String>,
    pub saved_edit_version: Option<u64>,
    pub pending: Option<Pending>,
}
fn valid_etag(value: &str) -> bool {
    value
        .strip_prefix("\"studio-r")
        .and_then(|v| v.strip_suffix('"'))
        .and_then(|v| v.split_once('-'))
        .is_some_and(|(r, h)| {
            r.parse::<u32>().is_ok_and(|v| v > 0 && v <= 2_147_483_647)
                && !r.starts_with('0')
                && hash_valid(h)
        })
}
fn draft_id(value: &Value) -> Option<&str> {
    let candidate = value.get("candidate")?;
    let schema = value.get("schemaVersion")?.as_str()?;
    let candidate_schema = candidate.get("schemaVersion");
    let compatible = match schema {
        "1.0.0-draft.1" => candidate_schema.is_none_or(|version| version == schema),
        "1.0.0-draft.2" => candidate_schema.is_some_and(|version| version == schema)
            && candidate["kind"] == "exhibition-revision",
        _ => false,
    };
    if value.get("kind")?.as_str()? != "exhibition-draft" || !compatible {
        return None;
    }
    let id = value.get("exhibitionId")?.as_str()?;
    if !uuid(id)
        || !uuid(value.get("id")?.as_str()?)
        || !candidate
            .get("id")?
            .as_str()?
            .eq_ignore_ascii_case(id)
        || !value
            .get("editVersion")?
            .as_u64()
            .is_some_and(|v| v > 0 && v <= 2_147_483_647)
    {
        return None;
    }
    Some(id)
}
pub(crate) fn validate(s: &DocumentState) -> Result<()> {
    if s.format != 1
        || !uuid(&s.workspace_id)
        || !uuid(&s.tenant_id)
        || !uuid(&s.principal_id)
        || s.etag.as_ref().is_some_and(|v| !valid_etag(v))
        || s.saved_edit_version
            .is_some_and(|v| v == 0 || v > 2_147_483_647)
        || (s.etag.is_some() != s.saved_edit_version.is_some())
        || (s.draft.is_none() && (s.etag.is_some() || s.pending.is_some()))
        || s.draft.as_ref().is_some_and(|v| draft_id(v).is_none())
    {
        return Err(err("NATIVE_DOCUMENT_INVALID"));
    }
    if let Some(p) = &s.pending
        && (!uuid(&p.request_id)
            || p.origin.as_deref().is_some_and(|v| v != "bounded-placement-v1" && v != "bounded-import-v1")
            || (p.origin.is_some() && p.etag.is_none())
            || p.etag != s.etag
            || p.etag.as_ref().is_some_and(|v| !valid_etag(v))
            || draft_id(&p.draft).is_none()
            || draft_id(&p.draft) != s.draft.as_ref().and_then(draft_id))
    {
        return Err(err("NATIVE_DOCUMENT_INVALID"));
    }
    Ok(())
}
impl InstallationController {
    pub(super) fn document_unlocked(&self, service: &LifecycleService) -> Result<(String, DocumentState)> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let file = format!(
            "native-document-{}.json",
            digest(service.root.as_os_str().as_encoded_bytes())
        );
        let default = DocumentState {
            format: 1,
            revision: 0,
            workspace_id,
            tenant_id,
            principal_id,
            draft: None,
            etag: None,
            saved_edit_version: None,
            pending: None,
        };
        let saved =
            match fs::symlink_metadata(profile.root.join(&file)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => default.clone(),
                Ok(_) => serde_json::from_slice::<DocumentState>(
                    &installation_backup::source_bytes(&profile.root, &file, LIMIT, true)?,
                )
                .map_err(|_| err("NATIVE_DOCUMENT_INVALID"))?,
                Err(_) => return Err(err("STATE_UNAVAILABLE")),
            };
        validate(&saved)?;
        if saved.workspace_id != default.workspace_id
            || saved.tenant_id != default.tenant_id
            || saved.principal_id != default.principal_id
        {
            return Err(err("NATIVE_DOCUMENT_SCOPE_CHANGED"));
        }
        Ok((file, saved))
    }
    pub fn native_document(&self, token: &str) -> Result<DocumentState> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; self.ensure_active_creation_unlocked(service)?;
            Ok(self.document_unlocked(service)?.1)
        })
    }
    pub fn save_native_document(
        &self,
        token: &str,
        mut state: DocumentState,
    ) -> Result<DocumentState> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; self.ensure_active_creation_unlocked(service)?;
            let (file, current) = self.document_unlocked(service)?;
            validate(&state)?;
            if state.workspace_id != current.workspace_id
                || state.tenant_id != current.tenant_id
                || state.principal_id != current.principal_id
            {
                return Err(err("NATIVE_DOCUMENT_SCOPE_CHANGED"));
            }
            if state.revision != current.revision {
                return Err(err("NATIVE_DOCUMENT_STATE_CHANGED"));
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| err("NATIVE_DOCUMENT_INVALID"))?;
            let bytes = serde_json::to_vec(&state).map_err(|_| err("STATE_INVALID"))?;
            if bytes.len() as u64 > LIMIT {
                return Err(err("NATIVE_DOCUMENT_INVALID"));
            }
            write_json(&profile.root, &file, &state)?;
            File::open(&profile.root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
            Ok(state)
        })
    }
}

#[cfg(test)]
mod pending_origin_tests {
    use super::*;

    #[derive(Deserialize)]
    #[allow(dead_code)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct LegacyPending { request_id: String, etag: Option<String>, draft: Value }

    #[test]
    fn qualified_draft_requires_matching_candidate_schema() {
        let exhibition = Uuid::new_v4().to_string();
        let mut draft = serde_json::json!({"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.2",
            "id":Uuid::new_v4().to_string(),"exhibitionId":exhibition,"editVersion":2,
            "candidate":{"kind":"exhibition-revision","schemaVersion":"1.0.0-draft.2","id":exhibition}});
        assert_eq!(draft_id(&draft), Some(exhibition.as_str()));
        draft["candidate"]["schemaVersion"] = serde_json::json!("1.0.0-draft.1");
        assert!(draft_id(&draft).is_none());
        draft["schemaVersion"] = serde_json::json!("1.0.0-draft.1");
        assert_eq!(draft_id(&draft), Some(exhibition.as_str()));
        draft["candidate"]["schemaVersion"] = serde_json::json!("1.0.0-draft.2");
        assert!(draft_id(&draft).is_none());
    }

    #[test]
    fn tagged_pending_round_trips_and_unknown_origins_fail_closed() {
        let workspace = Uuid::new_v4().to_string();
        let exhibition = Uuid::new_v4().to_string();
        let draft = serde_json::json!({"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.1",
            "id":Uuid::new_v4().to_string(),"exhibitionId":exhibition,"editVersion":1,
            "candidate":{"id":exhibition}});
        let etag = format!("\"studio-r1-{}\"", "a".repeat(64));
        let mut state = DocumentState { format: 1, revision: 0, workspace_id: workspace,
            tenant_id: Uuid::new_v4().to_string(), principal_id: Uuid::new_v4().to_string(),
            draft: Some(draft.clone()), etag: Some(etag.clone()), saved_edit_version: Some(1),
            pending: Some(Pending { request_id: Uuid::new_v4().to_string(), etag: Some(etag), draft, origin: None }) };
        let legacy = serde_json::to_value(&state).unwrap();
        assert!(legacy["pending"].get("origin").is_none());
        assert!(serde_json::from_value::<LegacyPending>(legacy["pending"].clone()).is_ok());
        assert!(validate(&serde_json::from_value(legacy).unwrap()).is_ok());
        state.pending.as_mut().unwrap().origin = Some("bounded-placement-v1".into());
        let tagged = serde_json::to_value(&state).unwrap();
        assert_eq!(tagged["pending"]["origin"], "bounded-placement-v1");
        assert!(serde_json::from_value::<LegacyPending>(tagged["pending"].clone()).is_err());
        assert!(validate(&serde_json::from_value(tagged).unwrap()).is_ok());
        state.pending.as_mut().unwrap().origin = Some("bounded-import-v1".into());
        assert!(validate(&state).is_ok());
        state.pending.as_mut().unwrap().origin = Some("other".into());
        assert!(validate(&state).is_err());
    }
}
