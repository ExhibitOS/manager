// SPDX-License-Identifier: Apache-2.0
//! One durable publication-copy preparation per personal workspace. No credential,
//! token, filesystem target or asset bytes enter this revision-checked journal.
use super::*;
const LIMIT: u64 = 2_200_000;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pending {
    pub source_exhibition_id: String,
    pub source_etag: String,
    pub snapshot: Value,
    pub save_request_id: String,
    pub snapshot_etag: Option<String>,
    pub publish_request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_binding_sha256: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publication {
    pub publication_id: String,
    pub public_url: String,
    pub revision_sha256: String,
    pub published_at: String,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Completed {
    pub source_exhibition_id: String,
    pub source_etag: String,
    pub snapshot_exhibition_id: String,
    pub publish_request_id: String,
    pub publication: Publication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_binding_sha256: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationState {
    pub format: u32,
    pub revision: u64,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub pending: Option<Pending>,
    pub completed: Option<Completed>,
}
fn valid_etag(value: &str) -> bool {
    value
        .strip_prefix("\"studio-r")
        .and_then(|v| v.strip_suffix('"'))
        .and_then(|v| v.split_once('-'))
        .is_some_and(|(r, h)| {
            r.parse::<u32>().is_ok_and(|n| n > 0 && n <= 2_147_483_647)
                && !r.starts_with('0')
                && hash_valid(h)
        })
}
pub(crate) fn validate(s: &PublicationState) -> Result<()> {
    let bad = || err("NATIVE_PUBLICATION_STATE_INVALID");
    if s.format != 1 || !uuid(&s.workspace_id) || !uuid(&s.tenant_id) || !uuid(&s.principal_id) {
        return Err(bad());
    }
    if let Some(p) = &s.pending {
        let id = p.snapshot["exhibitionId"].as_str().unwrap_or("");
        if !uuid(&p.source_exhibition_id)
            || !valid_etag(&p.source_etag)
            || !uuid(&p.save_request_id)
            || !uuid(&p.publish_request_id)
            || p.plan_binding_sha256.as_ref().is_some_and(|v| !hash_valid(v))
            || p.snapshot_etag.as_ref().is_some_and(|v| !valid_etag(v))
            || !uuid(id)
            || id == p.source_exhibition_id
            || !uuid(p.snapshot["id"].as_str().unwrap_or(""))
            || p.snapshot["kind"] != "exhibition-draft"
            || p.snapshot["schemaVersion"] != "1.0.0-draft.1"
            || p.snapshot["candidate"]["id"] != id
            || p.snapshot["editVersion"] != 1
        {
            return Err(bad());
        }
    }
    if let Some(c) = &s.completed {
        let p = &c.publication;
        if !uuid(&c.source_exhibition_id)
            || !valid_etag(&c.source_etag)
            || !uuid(&c.snapshot_exhibition_id)
            || c.snapshot_exhibition_id == c.source_exhibition_id
            || !uuid(&c.publish_request_id)
            || c.plan_binding_sha256.as_ref().is_some_and(|v| !hash_valid(v))
            || !uuid(&p.publication_id)
            || p.public_url != format!("/p/{}", p.publication_id)
            || !hash_valid(&p.revision_sha256)
            || p.published_at.is_empty()
            || p.published_at.len() > 64
            || p.published_at.chars().any(char::is_control)
            || !matches!(p.status.as_str(), "published" | "unpublished")
        {
            return Err(bad());
        }
    }
    Ok(())
}
impl InstallationController {
    fn publication_unlocked(
        &self,
        service: &LifecycleService,
    ) -> Result<(String, PublicationState)> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let file = format!(
            "native-publication-{}.json",
            digest(service.root.as_os_str().as_encoded_bytes())
        );
        let default = PublicationState {
            format: 1,
            revision: 0,
            workspace_id,
            tenant_id,
            principal_id,
            pending: None,
            completed: None,
        };
        let saved = match fs::symlink_metadata(profile.root.join(&file)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default.clone(),
            Ok(_) => serde_json::from_slice::<PublicationState>(
                &installation_backup::source_bytes(&profile.root, &file, LIMIT, true)?,
            )
            .map_err(|_| err("NATIVE_PUBLICATION_STATE_INVALID"))?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        validate(&saved)?;
        if saved.workspace_id != default.workspace_id
            || saved.tenant_id != default.tenant_id
            || saved.principal_id != default.principal_id
        {
            return Err(err("NATIVE_PUBLICATION_SCOPE_CHANGED"));
        }
        Ok((file, saved))
    }
    pub fn native_publication(&self, token: &str) -> Result<PublicationState> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            Ok(self.publication_unlocked(service)?.1)
        })
    }
    pub fn save_native_publication(
        &self,
        token: &str,
        mut state: PublicationState,
    ) -> Result<PublicationState> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            let (file, current) = self.publication_unlocked(service)?;
            validate(&state)?;
            if state.workspace_id != current.workspace_id
                || state.tenant_id != current.tenant_id
                || state.principal_id != current.principal_id
            {
                return Err(err("NATIVE_PUBLICATION_SCOPE_CHANGED"));
            }
            if let (Some(old), Some(next)) = (&current.pending, &state.pending) {
                let mut a = old.clone();
                let mut b = next.clone();
                a.snapshot_etag = None;
                b.snapshot_etag = None;
                if a != b
                    || (old.snapshot_etag.is_some() && old.snapshot_etag != next.snapshot_etag)
                {
                    return Err(err("NATIVE_PUBLICATION_REQUEST_CHANGED"));
                }
            }
            if state.revision != current.revision {
                return Err(err("NATIVE_PUBLICATION_STATE_CHANGED"));
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| err("NATIVE_PUBLICATION_STATE_INVALID"))?;
            let bytes = serde_json::to_vec(&state).map_err(|_| err("STATE_INVALID"))?;
            if bytes.len() as u64 > LIMIT {
                return Err(err("NATIVE_PUBLICATION_STATE_INVALID"));
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
mod tests {
    use super::*;

    #[test]
    fn publication_pending_float_payload_is_exact_across_wire_and_cold_retry() {
        // Same 6500 K linear RGB sent by Swift as shortest JSON and by
        // Foundation JSONSerialization as expanded decimals. Both must retain
        // the original blue bit; a one-ULP change breaks immutable request IDs.
        for input in [
            "[1,0.9911020971138298,0.9559733532492861]",
            "[1,0.99110209711382979,0.95597335324928612]",
        ] {
            let color: Value = serde_json::from_str(input).unwrap();
            assert_eq!(color[2].as_f64().unwrap().to_bits(), 0x3fee97556e01b351);
            let pending = Pending {
                source_exhibition_id: Uuid::new_v4().to_string(),
                source_etag: format!("\"studio-r7-{}\"", "a".repeat(64)),
                snapshot: serde_json::json!({"candidate":{"lights":[{"color":color}],"opaque":{"keep":0.12345678901234568}}}),
                save_request_id: Uuid::new_v4().to_string(),
                snapshot_etag: None,
                publish_request_id: Uuid::new_v4().to_string(),
                plan_binding_sha256: Some("b".repeat(64)),
            };
            let request = |p: &Pending| serde_json::to_vec(&serde_json::json!({
                "method":"POST", "input":{"draft":p.snapshot,"requestId":p.save_request_id}
            })).unwrap();
            let original = request(&pending);
            let mut reopened = pending.clone();
            for _ in 0..4 {
                reopened = serde_json::from_slice(&serde_json::to_vec(&reopened).unwrap()).unwrap();
                assert!(pending == reopened);
                assert_eq!(request(&reopened), original);
            }
            let mut altered = reopened.clone();
            altered.snapshot["candidate"]["lights"][0]["color"][2] = serde_json::json!(0.955973353249286_f64);
            assert!(pending != altered, "The existing immutable-pending comparison still rejects a real bit change");
        }
    }

    #[test]
    fn publication_plan_binding_cold_legacy_and_bound_journals() {
        let id = || Uuid::new_v4().to_string();
        let source = id();
        let snapshot = id();
        let legacy = serde_json::json!({
            "format":1,"revision":7,"workspaceId":id(),"tenantId":id(),"principalId":id(),
            "pending":{
                "sourceExhibitionId":source,"sourceEtag":format!("\"studio-r2-{}\"", "a".repeat(64)),
                "snapshot":{"id":id(),"exhibitionId":snapshot,"kind":"exhibition-draft", "schemaVersion":"1.0.0-draft.1","editVersion":1,"candidate":{"id":snapshot}},
                "saveRequestId":id(),"snapshotEtag":null,"publishRequestId":id()
            },
            "completed":null
        });
        let mut state: PublicationState = serde_json::from_value(legacy).unwrap();
        validate(&state).unwrap();
        assert!(state.pending.as_ref().unwrap().plan_binding_sha256.is_none());
        assert!(serde_json::to_value(&state).unwrap()["pending"].get("planBindingSha256").is_none());
        let old = state.pending.as_ref().unwrap().clone();
        state.pending.as_mut().unwrap().plan_binding_sha256 = Some("b".repeat(64));
        validate(&state).unwrap();
        let reopened: PublicationState = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(reopened.pending == state.pending);
        // The existing durable pending comparison includes the binding, so
        // changing it cannot silently retain an earlier request identity.
        assert!(reopened.pending.as_ref().unwrap() != &old);
        state.pending.as_mut().unwrap().plan_binding_sha256 = Some("BAD".into());
        assert!(validate(&state).is_err());
        state.pending = None;
        state.completed = Some(Completed {
            source_exhibition_id: source, source_etag: format!("\"studio-r2-{}\"", "a".repeat(64)),
            snapshot_exhibition_id: snapshot, publish_request_id:id(),plan_binding_sha256:Some("b".repeat(64)),
            publication:Publication{publication_id:id(),public_url:String::new(),revision_sha256:"c".repeat(64),published_at:"2026-10-28T00:00:00Z".into(),status:"published".into()}
        });
        let completed = state.completed.as_mut().unwrap();
        completed.publication.public_url = format!("/p/{}",completed.publication.publication_id);
        validate(&state).unwrap();
        state.completed.as_mut().unwrap().plan_binding_sha256 = Some("BAD".into());
        assert!(validate(&state).is_err());
    }
}
