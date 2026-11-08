// SPDX-License-Identifier: Apache-2.0
//! One durable publication-copy preparation per personal workspace. No credential,
//! token, filesystem target or asset bytes enter this revision-checked journal.
use super::*;
use std::collections::BTreeMap;
const LIMIT: u64 = 2_200_000;
const ORIGINAL_ATTEMPT_LIMIT: usize = 256;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pending {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_publication_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
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
pub struct NativeOriginalAttempt {
    pub plan_binding_sha256: String,
    pub export_claimed: bool,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native_original_attempts: BTreeMap<String, NativeOriginalAttempt>,
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
    if s.native_original_attempts.len() > ORIGINAL_ATTEMPT_LIMIT
        || s.native_original_attempts.iter().any(|(id, attempt)| !uuid(id) || !hash_valid(&attempt.plan_binding_sha256))
    {
        return Err(bad());
    }
    if let Some(p) = &s.pending {
        let id = p.snapshot["exhibitionId"].as_str().unwrap_or("");
        let reusable = p.mode.as_deref() == Some("reusable-draft-v1");
        let native_original = p.mode.as_deref() == Some("native-original-v1");
        let direct = reusable || native_original;
        if (p.mode.is_some() && !direct)
            || (reusable && !p.capability_publication_id.as_deref().is_some_and(uuid))
            || (native_original && (p.capability_publication_id.is_some() || p.plan_binding_sha256.is_none() || p.save_request_id != p.publish_request_id))
            || (native_original && !s.native_original_attempts.get(&p.publish_request_id).is_some_and(|attempt| {
                !attempt.export_claimed && Some(&attempt.plan_binding_sha256) == p.plan_binding_sha256.as_ref()
            }))
            || (direct && (id != p.source_exhibition_id || p.snapshot_etag.as_deref() != Some(p.source_etag.as_str()) || !p.snapshot["editVersion"].as_u64().is_some_and(|v| v > 0 && v <= 2_147_483_647)))
            || (!direct && (p.capability_publication_id.is_some() || id == p.source_exhibition_id || p.snapshot["editVersion"] != 1))
            || !uuid(&p.source_exhibition_id)
            || !valid_etag(&p.source_etag)
            || !uuid(&p.save_request_id)
            || !uuid(&p.publish_request_id)
            || p.plan_binding_sha256.as_ref().is_some_and(|v| !hash_valid(v))
            || p.snapshot_etag.as_ref().is_some_and(|v| !valid_etag(v))
            || native_document::draft_id(&p.snapshot) != Some(id)
            || p.snapshot["candidate"]["id"] != id
        {
            return Err(bad());
        }
    }
    if let Some(c) = &s.completed {
        let p = &c.publication;
        let native_original = c.mode.as_deref() == Some("native-original-v1");
        let direct = native_original || c.mode.as_deref() == Some("reusable-draft-v1");
        if (c.mode.is_some() && !direct)
            || (direct != (c.snapshot_exhibition_id == c.source_exhibition_id))
            || (native_original && c.plan_binding_sha256.is_none())
            || (native_original && !s.native_original_attempts.get(&c.publish_request_id).is_some_and(|attempt| {
                Some(&attempt.plan_binding_sha256) == c.plan_binding_sha256.as_ref()
            }))
            || !uuid(&c.source_exhibition_id)
            || !valid_etag(&c.source_etag)
            || !uuid(&c.snapshot_exhibition_id)
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

fn validate_original_attempt_change(current: &PublicationState, next: &PublicationState) -> Result<()> {
    let invalid = || err("NATIVE_PUBLICATION_REQUEST_CHANGED");
    let mut changes = 0;
    for (id, old) in &current.native_original_attempts {
        let attempt = next.native_original_attempts.get(id).ok_or_else(invalid)?;
        if attempt.plan_binding_sha256 != old.plan_binding_sha256 || (old.export_claimed && !attempt.export_claimed) {
            return Err(invalid());
        }
        if old.export_claimed != attempt.export_claimed {
            changes += 1;
            if next.pending.is_some() || !next.completed.as_ref().is_some_and(|completed| {
                completed.mode.as_deref() == Some("native-original-v1") && completed.publish_request_id == *id
                    && completed.plan_binding_sha256.as_ref() == Some(&attempt.plan_binding_sha256)
            }) {
                return Err(invalid());
            }
        }
    }
    for (id, attempt) in &next.native_original_attempts {
        if !current.native_original_attempts.contains_key(id) {
            changes += 1;
            if attempt.export_claimed || !next.pending.as_ref().is_some_and(|pending| {
                pending.mode.as_deref() == Some("native-original-v1") && pending.publish_request_id == *id
                    && pending.plan_binding_sha256.as_ref() == Some(&attempt.plan_binding_sha256)
            }) {
                return Err(invalid());
            }
        }
    }
    if changes > 1 { return Err(invalid()); }
    Ok(())
}
impl InstallationController {
    pub(super) fn publication_unlocked(
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
            native_original_attempts: BTreeMap::new(),
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
            validate_original_attempt_change(&current, &state)?;
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
    fn qualified_publication_copy_retains_matching_versions_in_the_journal() {
        let id = || Uuid::new_v4().to_string();
        let snapshot = id();
        let wire = serde_json::json!({"format":1,"revision":0,"workspaceId":id(),"tenantId":id(),"principalId":id(),
            "pending":{"sourceExhibitionId":id(),"sourceEtag":format!("\"studio-r2-{}\"", "a".repeat(64)),
                "snapshot":{"id":id(),"exhibitionId":snapshot,"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.2","editVersion":1,
                    "candidate":{"id":snapshot,"kind":"exhibition-revision","schemaVersion":"1.0.0-draft.2"}},
                "saveRequestId":id(),"snapshotEtag":null,"publishRequestId":id()},"completed":null});
        let state: PublicationState = serde_json::from_value(wire).unwrap();
        validate(&state).unwrap();
        let mut reopened: PublicationState = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(reopened.pending == state.pending);
        reopened.pending.as_mut().unwrap().snapshot["candidate"]["schemaVersion"] = serde_json::json!("1.0.0-draft.1");
        assert!(validate(&reopened).is_err());
    }

    #[test]
    fn direct_publication_journal_binds_same_draft_and_exact_etag() {
        let id = || Uuid::new_v4().to_string();
        let source = id(); let etag = format!("\"studio-r2-{}\"", "a".repeat(64));
        let wire = serde_json::json!({"format":1,"revision":0,"workspaceId":id(),"tenantId":id(),"principalId":id(),
            "pending":{"mode":"reusable-draft-v1","capabilityPublicationId":id(),"sourceExhibitionId":source,"sourceEtag":etag,
                "snapshot":{"id":id(),"exhibitionId":source,"kind":"exhibition-draft","schemaVersion":"1.0.0-draft.1","editVersion":2,"candidate":{"id":source}},
                "saveRequestId":id(),"snapshotEtag":etag,"publishRequestId":id()},"completed":null});
        let state: PublicationState = serde_json::from_value(wire.clone()).unwrap(); validate(&state).unwrap();
        let round: PublicationState = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(state.pending == round.pending);
        for (key,value) in [("mode",serde_json::json!("unknown")),("capabilityPublicationId",serde_json::json!(null)),("snapshotEtag",serde_json::json!(null)),("sourceEtag",serde_json::json!(format!("\"studio-r3-{}\"","a".repeat(64))))] {
            let mut changed=wire.clone(); changed["pending"][key]=value;
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        }
        let mut changed=wire.clone();changed["pending"]["mode"]=serde_json::Value::Null;
        assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        let mut completed=wire.clone(); completed["pending"]=serde_json::Value::Null;
        let publication=id();completed["completed"]=serde_json::json!({"mode":"reusable-draft-v1","sourceExhibitionId":source,"sourceEtag":etag,"snapshotExhibitionId":source,"publishRequestId":id(),"publication":{"publicationId":publication,"publicUrl":format!("/p/{publication}"),"revisionSha256":"c".repeat(64),"publishedAt":"2026-10-30T00:00:00Z","status":"unpublished"}});
        validate(&serde_json::from_value(completed.clone()).unwrap()).unwrap();
        completed["completed"]["mode"]=serde_json::Value::Null;
        assert!(validate(&serde_json::from_value(completed).unwrap()).is_err());

        let mut original = wire.clone();
        original["pending"]["mode"] = serde_json::json!("native-original-v1");
        assert!(validate(&serde_json::from_value(original.clone()).unwrap()).is_err());
        original["pending"].as_object_mut().unwrap().remove("capabilityPublicationId");
        original["pending"]["planBindingSha256"] = serde_json::json!("d".repeat(64));
        assert!(validate(&serde_json::from_value(original.clone()).unwrap()).is_err());
        original["pending"]["saveRequestId"] = original["pending"]["publishRequestId"].clone();
        let request_id = original["pending"]["publishRequestId"].as_str().unwrap().to_owned();
        let mut empty: PublicationState = serde_json::from_value(wire).unwrap();
        empty.pending = None;
        original["nativeOriginalAttempts"][&request_id] =
            serde_json::json!({"planBindingSha256":"d".repeat(64),"exportClaimed":false});
        let staged: PublicationState = serde_json::from_value(original.clone()).unwrap();
        validate(&staged).unwrap();
        validate_original_attempt_change(&empty, &staged).unwrap();
        let mut discarded = staged.clone();
        discarded.native_original_attempts.clear();
        assert!(validate_original_attempt_change(&staged, &discarded).is_err());
        let mut rebound = staged.clone();
        rebound.native_original_attempts.get_mut(&request_id).unwrap().plan_binding_sha256 = "e".repeat(64);
        assert!(validate_original_attempt_change(&staged, &rebound).is_err());
        let mut claimed_without_completion = staged.clone();
        claimed_without_completion.native_original_attempts.get_mut(&request_id).unwrap().export_claimed = true;
        assert!(validate_original_attempt_change(&staged, &claimed_without_completion).is_err());
        let mut ready = staged.clone();
        ready.pending = None;
        ready.completed = Some(Completed {
            mode: Some("native-original-v1".into()),
            source_exhibition_id: source.clone(),
            source_etag: etag.clone(),
            snapshot_exhibition_id: source,
            publish_request_id: request_id.clone(),
            publication: Publication {
                public_url: format!("/p/{publication}"),
                publication_id: publication,
                revision_sha256: "f".repeat(64),
                published_at: "2026-11-08T00:00:00Z".into(),
                status: "published".into(),
            },
            plan_binding_sha256: Some("d".repeat(64)),
        });
        validate(&ready).unwrap();
        validate_original_attempt_change(&staged, &ready).unwrap();
        let mut claimed = ready.clone();
        claimed.native_original_attempts.get_mut(&request_id).unwrap().export_claimed = true;
        validate(&claimed).unwrap();
        validate_original_attempt_change(&ready, &claimed).unwrap();
        assert!(validate_original_attempt_change(&claimed, &ready).is_err());
        original["pending"]["snapshotEtag"] = serde_json::Value::Null;
        assert!(validate(&serde_json::from_value(original).unwrap()).is_err());
    }

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
                mode: None, capability_publication_id: None,
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
            mode: None,
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
