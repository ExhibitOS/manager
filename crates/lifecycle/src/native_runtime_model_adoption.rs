// SPDX-License-Identifier: Apache-2.0
//! Manager retains the source grant in its owning helper across workspace
//! selection. The operator supplies identities, never an imported grant.

use super::{Session, Value, err};
use crate::{
    Result,
    installations::native_document::DocumentState,
    native_model_adoption_stage::Reservation,
    native_model_adoption_wire::{self as wire, Blob, Grant, Target},
};
use serde::Deserialize;
use serde_json::json;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]

pub enum Input {
    StageSource {
        source_artwork_id: String,
        source_artwork_revision: u32,
        original: Blob,
        display: Blob,
        target: Target,
    },
    PreviewTarget {
        operation_id: String,
        expected_revision: u64,
    },
    ConfirmTarget {
        operation_id: String,
        grant_sha256: String,
        expected_revision: u64,
    },
    StatusTarget {
        operation_id: String,
        grant_sha256: String,
    },
    RetainedOperation,
    ReleasePrecommit {
        operation_id: String,
    },
    SettleTarget {
        operation_id: String,
        grant_sha256: String,
        retire: bool,
    },
}

pub(crate) struct Operation {
    operation_id: Uuid,
    reservation: Option<Reservation>,
    source_owner: (String, String, String),
    target: Target,
    grant: Option<Grant>,
    preview: Option<(String, u64)>,
    commit_started: bool,
    retirement_started: bool,
}

impl Operation {
    pub(crate) fn execute(
        held: &mut Option<Self>,
        session: &mut Session,
        input: Input,
        document: &DocumentState,
    ) -> Result<Value> {
        let owner = session.adoption_owner_scope()?;
        if owner
            != (
                document.workspace_id.clone(),
                document.tenant_id.clone(),
                document.principal_id.clone(),
            )
        {
            return Err(wire::unavailable());
        }
        match input {
            Input::StageSource {
                source_artwork_id,
                source_artwork_revision,
                original,
                display,
                target,
            } => {
                if held.is_some()
                    || !target.valid()
                    || owner.0 == target.workspace_id
                    || !wire::valid_id(&source_artwork_id)
                    || !(1..=i32::MAX as u32).contains(&source_artwork_revision)
                    || !original.valid()
                    || !display.valid()
                {
                    return Err(wire::unavailable());
                }
                let root = session.adoption_root()?;
                let id = Uuid::new_v4();
                let operation_id = id.to_string();
                let expires_at = wire::now_ms()?
                    .checked_add(wire::MAX_AGE_MS)
                    .ok_or_else(wire::unavailable)?;
                // Preserve the exact reservation if either source write or the
                // response becomes uncertain.
                *held = Some(Self {
                    operation_id: id,
                    reservation: None,
                    source_owner: owner.clone(),
                    target: target.clone(),
                    grant: None,
                    preview: None,
                    commit_started: false,
                    retirement_started: false,
                });
                let reservation = Reservation::create(root, id)?;
                held.as_mut().ok_or_else(wire::unavailable)?.reservation = Some(reservation);
                let response: wire::SourceStaged = session.adoption_exchange(
                    &operation_id,
                    wire::StageSource {
                        source_artwork_id: &source_artwork_id,
                        source_artwork_revision,
                        target: &target,
                        expires_at,
                    },
                )?;
                let grant = response.grant;
                grant.check(wire::now_ms()?)?;
                if grant.operation_id != operation_id
                    || grant.expires_at != expires_at
                    || grant.target != target
                    || grant.original != original
                    || grant.display != display
                    || (
                        &grant.source.workspace_id,
                        &grant.source.tenant_id,
                        &grant.source.principal_id,
                    ) != (&owner.0, &owner.1, &owner.2)
                    || grant.source.artwork_id != source_artwork_id
                    || grant.source.artwork_revision != source_artwork_revision
                {
                    return Err(wire::unavailable());
                }
                let current = held.as_mut().ok_or_else(wire::unavailable)?;
                current
                    .stage()?
                    .verify(grant.original.identity(), grant.display.identity())?;
                let digest = grant.digest()?;
                let result = json!({
                    "operationId": operation_id, "grantSha256": digest, "expiresAt": expires_at,
                    "target": grant.target, "original": grant.original, "display": grant.display,
                });
                current.grant = Some(grant);
                Ok(result)
            }
            Input::PreviewTarget {
                operation_id,
                expected_revision,
            } => {
                let current = held.as_mut().ok_or_else(wire::unavailable)?;
                if current.commit_started {
                    return Err(wire::unavailable());
                }
                let grant = current.grant.as_ref().ok_or_else(wire::unavailable)?;
                check_target(document, expected_revision, &operation_id, grant)?;
                let root = session.adoption_root()?;
                if !current.stage()?.matches_root(&root)? {
                    return Err(wire::unavailable());
                }
                current
                    .stage()?
                    .verify(grant.original.identity(), grant.display.identity())?;
                let digest = grant.digest()?;
                let response: wire::TargetPreview =
                    session.adoption_exchange(&operation_id, wire::PreviewTarget { grant })?;
                if response.grant_sha256 != digest || !response.preview.matches(grant) {
                    return Err(wire::unavailable());
                }
                let generation = session.adoption_generation()?;
                current.preview = Some((generation, expected_revision));
                Ok(
                    json!({"operationId": operation_id, "grantSha256": digest, "preview": response.preview}),
                )
            }
            Input::ConfirmTarget {
                operation_id,
                grant_sha256,
                expected_revision,
            } => {
                let current = held.as_mut().ok_or_else(wire::unavailable)?;
                let grant = current.grant.as_ref().ok_or_else(wire::unavailable)?;
                let generation = session.adoption_generation()?;
                if current.commit_started
                    || current.preview.as_ref() != Some(&(generation, expected_revision))
                    || grant_sha256 != grant.digest()?
                {
                    return Err(wire::unavailable());
                }
                check_target(document, expected_revision, &operation_id, grant)?;
                let root = session.adoption_root()?;
                if !current.stage()?.matches_root(&root)? {
                    return Err(wire::unavailable());
                }
                current
                    .stage()?
                    .verify(grant.original.identity(), grant.display.identity())?;
                // Neither a negative reply nor a disconnect proves retirement
                // is safe after commit was sent.
                current.commit_started = true;
                let result: wire::TargetCommitted = session.adoption_exchange(
                    &operation_id,
                    wire::CommitTarget {
                        grant_sha256: &grant_sha256,
                        grant,
                    },
                )?;
                if result.grant_sha256 != grant_sha256
                    || wire::etag(&result.etag) != Some(result.revision)
                    || wire::etag(&grant.target.etag).and_then(|revision| revision.checked_add(1))
                        != Some(result.revision)
                {
                    return Err(wire::unavailable());
                }
                Ok(
                    json!({"operationId": operation_id, "grantSha256": grant_sha256, "outcome": "committed",
                          "revision": result.revision, "etag": result.etag}),
                )
            }
            Input::StatusTarget {
                operation_id,
                grant_sha256,
            } => {
                let result = target_status(session, &operation_id, &grant_sha256)?;
                let outcome = match result.outcome {
                    wire::Outcome::Committed => "committed",
                    wire::Outcome::Unresolved => "unresolved",
                };
                Ok(
                    json!({"operationId": operation_id, "grantSha256": grant_sha256, "outcome": outcome,
                          "revision": result.revision, "etag": result.etag}),
                )
            }
            Input::RetainedOperation => {
                let Some(current) = held.as_ref() else {
                    return Ok(Value::Null);
                };
                if !current.belongs_to(&owner) {
                    return Err(wire::unavailable());
                }
                let digest = current.grant.as_ref().map(Grant::digest).transpose()?;
                Ok(
                    json!({"operationId": current.operation_id.to_string(), "grantSha256": digest,
                          "reservationReady": current.reservation.is_some(),
                          "commitAttempted": current.commit_started, "retirementAttempted": current.retirement_started}),
                )
            }
            Input::ReleasePrecommit { operation_id } => {
                let current = held.as_ref().ok_or_else(wire::unavailable)?;
                current.check_precommit_release(&owner, &operation_id)?;
                // An unavailable source response can leave owned work draining.
                // This only releases the helper slot. The old UUID stays reserved.
                *held = None;
                Ok(json!({"operationId": operation_id, "outcome": "precommit-retained"}))
            }
            Input::SettleTarget {
                operation_id,
                grant_sha256,
                retire,
            } => {
                let current = held.as_mut().ok_or_else(wire::unavailable)?;
                let grant = current.grant.as_ref().ok_or_else(wire::unavailable)?;
                if !current.commit_started
                    || !current.is_target(&owner)
                    || operation_id != grant.operation_id
                    || grant_sha256 != grant.digest()?
                    || (retire && current.retirement_started)
                {
                    return Err(wire::unavailable());
                }
                let result = target_status(session, &operation_id, &grant_sha256)?;
                if result.outcome != wire::Outcome::Committed
                    || wire::etag(&grant.target.etag).and_then(|revision| revision.checked_add(1))
                        != result.revision
                {
                    return Err(wire::unavailable());
                }
                if retire {
                    let root = session.adoption_root()?;
                    if !current.stage()?.matches_root(&root)? {
                        return Err(wire::unavailable());
                    }
                    current.retirement_started = true;
                    current
                        .stage()?
                        .retire(grant.original.identity(), grant.display.identity())?;
                }
                *held = None;
                Ok(
                    json!({"operationId": operation_id, "grantSha256": grant_sha256, "outcome": "committed",
                          "revision": result.revision, "etag": result.etag, "stageRetired": retire}),
                )
            }
        }
    }

    fn stage(&self) -> Result<&Reservation> {
        self.reservation.as_ref().ok_or_else(wire::unavailable)
    }

    fn is_target(&self, owner: &(String, String, String)) -> bool {
        (&owner.0, &owner.1, &owner.2)
            == (
                &self.target.workspace_id,
                &self.target.tenant_id,
                &self.target.principal_id,
            )
    }

    fn belongs_to(&self, owner: &(String, String, String)) -> bool {
        &self.source_owner == owner || self.is_target(owner)
    }

    fn check_precommit_release(
        &self,
        owner: &(String, String, String),
        operation_id: &str,
    ) -> Result<()> {
        if !self.belongs_to(owner)
            || self.commit_started
            || self.operation_id.to_string() != operation_id
        {
            return Err(wire::unavailable());
        }
        Ok(())
    }
}

fn target_status(
    session: &mut Session,
    operation_id: &str,
    grant_sha256: &str,
) -> Result<wire::TargetStatus> {
    if !crate::hash_valid(grant_sha256) {
        return Err(wire::unavailable());
    }
    let result: wire::TargetStatus =
        session.adoption_exchange(operation_id, wire::StatusTarget { grant_sha256 })?;
    if result.grant_sha256 != grant_sha256 {
        return Err(wire::unavailable());
    }
    match (result.outcome, result.revision, result.etag.as_deref()) {
        (wire::Outcome::Committed, Some(revision), Some(etag))
            if wire::etag(etag) == Some(revision) =>
        {
            ()
        }
        (wire::Outcome::Unresolved, None, None) => (),
        _ => return Err(wire::unavailable()),
    }
    Ok(result)
}

fn check_target(
    document: &DocumentState,
    revision: u64,
    operation: &str,
    grant: &Grant,
) -> Result<()> {
    grant.check(wire::now_ms()?)?;
    let target = &grant.target;
    if operation != grant.operation_id
        || document.revision != revision
        || document.pending.is_some()
        || document.etag.as_deref() != Some(&target.etag)
        || (
            &document.workspace_id,
            &document.tenant_id,
            &document.principal_id,
        ) != (
            &target.workspace_id,
            &target.tenant_id,
            &target.principal_id,
        )
    {
        return Err(err("NATIVE_DOCUMENT_STATE_CHANGED"));
    }
    let draft = document.draft.as_ref().ok_or_else(wire::unavailable)?;
    let candidate = &draft["candidate"];
    if draft["exhibitionId"].as_str() != Some(&target.exhibition_id)
        || document.saved_edit_version.is_none()
        || draft["editVersion"].as_u64() != document.saved_edit_version
    {
        return Err(err("NATIVE_DOCUMENT_STATE_CHANGED"));
    }
    let artworks = candidate["artworks"]
        .as_array()
        .ok_or_else(wire::unavailable)?;
    let artwork = artworks
        .iter()
        .find(|row| {
            row["id"].as_str() == Some(&target.artwork_id)
                && row["revision"].as_u64() == Some(u64::from(target.artwork_revision))
        })
        .ok_or_else(wire::unavailable)?;
    let artwork_revision = artwork["revisionId"]
        .as_str()
        .ok_or_else(wire::unavailable)?;
    let primary_asset = artwork["primaryAssetId"]
        .as_str()
        .ok_or_else(wire::unavailable)?;
    let rows = candidate["placements"]
        .as_array()
        .ok_or_else(wire::unavailable)?;
    let mut placements = rows
        .iter()
        .filter(|row| row["artworkRevisionId"].as_str() == Some(artwork_revision));
    if !placements.next().is_some_and(|row| {
        row["id"].as_str() == Some(&target.placement_id)
            && row["assetId"].as_str() == Some(primary_asset)
    }) || placements.next().is_some()
    {
        return Err(wire::unavailable());
    }
    Ok(())
}

impl Session {
    fn adoption_generation(&self) -> Result<String> {
        self.job_id.clone().ok_or_else(wire::unavailable)
    }

    fn adoption_owner_scope(&mut self) -> Result<(String, String, String)> {
        self.check_adoption_live()?;
        let authority = self
            .local_authority
            .as_ref()
            .ok_or_else(wire::unavailable)?;
        let scope = self.service.personal_identity()?;
        if scope
            != (
                authority.workspace_id.clone(),
                authority.tenant_id.clone(),
                authority.principal_id.clone(),
            )
            || authority.owner_uid != unsafe { libc::geteuid() }
        {
            return Err(wire::unavailable());
        }
        Ok(scope)
    }

    fn check_adoption_live(&mut self) -> Result<()> {
        self.poll()?;
        self.check_native_observation_fence()?;
        if self.state != "ready"
            || self.node.is_none()
            || self.postgres.is_none()
            || self.auxiliary.is_some()
            || self.cancelled()
        {
            return Err(wire::unavailable());
        }
        Ok(())
    }

    fn adoption_root(&self) -> Result<std::fs::File> {
        self.adoption_owner
            .as_ref()
            .ok_or_else(wire::unavailable)?
            .root()
    }

    fn adoption_exchange<Q: wire::Request, R: wire::Reply>(
        &mut self,
        operation: &str,
        request: Q,
    ) -> Result<R> {
        self.adoption_owner_scope()?;
        let generation = self.adoption_generation()?;
        let bytes = wire::request(operation, &generation, request)?;
        let mut owner = self.adoption_owner.take().ok_or_else(wire::unavailable)?;
        let cancelled = self.cancellation.clone();
        let deadline = Instant::now() + Duration::from_secs(90);
        let result = owner.exchange(&generation, &bytes, deadline, &cancelled, &mut || {
            self.check_adoption_live()
        });
        let raw = result?;
        if let Ok(reply) = wire::reply::<R>(&raw, operation, &generation) {
            self.adoption_owner = Some(owner);
            return Ok(reply);
        }
        if let Ok(refused) = wire::reply::<wire::Refused>(&raw, operation, &generation) {
            self.adoption_owner = Some(owner);
            return Err(wire::refused(&refused.code));
        }
        Err(wire::unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire::tests::{fixture_grant, fixture_id};

    #[test]
    fn target_confirmation_fences_saved_head_and_placement() {
        let now = wire::now_ms().unwrap();
        let grant = fixture_grant(now, now + 10000);
        let target = &grant.target;
        let asset = fixture_id(30);
        let revision_id = fixture_id(31);
        let mut state: DocumentState = serde_json::from_value(json!({
            "format":1,"revision":9,"workspaceId":target.workspace_id,"tenantId":target.tenant_id,
            "principalId":target.principal_id,"etag":target.etag,"savedEditVersion":3,"pending":null,
            "draft":{"exhibitionId":target.exhibition_id,"editVersion":3,"candidate":{
                "artworks":[{"id":target.artwork_id,"revision":target.artwork_revision,"revisionId":revision_id,"primaryAssetId":asset}],
                "placements":[{"id":target.placement_id,"artworkRevisionId":revision_id,"assetId":asset}]
            }}
        })).unwrap();
        assert!(check_target(&state, 9, &grant.operation_id, &grant).is_ok());
        assert!(check_target(&state, 8, &grant.operation_id, &grant).is_err());
        assert!(check_target(&state, 9, &fixture_id(99), &grant).is_err());
        state.draft.as_mut().unwrap()["editVersion"] = json!(4);
        assert!(check_target(&state, 9, &grant.operation_id, &grant).is_err());
        state.draft.as_mut().unwrap()["editVersion"] = json!(3);
        state.draft.as_mut().unwrap()["candidate"]["placements"][0]["id"] = json!(fixture_id(32));
        assert!(check_target(&state, 9, &grant.operation_id, &grant).is_err());
    }

    #[test]
    fn only_an_exact_precommit_owner_can_release_without_retiring_bytes() {
        use std::fs::{self, DirBuilder, File};
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("adoption-precommit-{}", Uuid::new_v4()));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let operation_id = Uuid::new_v4();
        let target = fixture_grant(1000, 2000).target;
        let owner = (fixture_id(9), fixture_id(10), fixture_id(11));
        let mut operation = Operation {
            operation_id,
            reservation: Some(
                Reservation::create(File::open(&root).unwrap(), operation_id).unwrap(),
            ),
            source_owner: owner.clone(),
            target,
            grant: None,
            preview: None,
            commit_started: false,
            retirement_started: false,
        };
        let id = operation_id.to_string();
        assert!(operation.check_precommit_release(&owner, &id).is_ok());
        assert!(
            operation
                .check_precommit_release(&owner, &fixture_id(99))
                .is_err()
        );
        assert!(
            operation
                .check_precommit_release(&(fixture_id(80), owner.1.clone(), owner.2.clone()), &id)
                .is_err()
        );
        operation.commit_started = true;
        assert!(operation.check_precommit_release(&owner, &id).is_err());
        drop(operation);
        assert!(
            root.join(&id).join("original").exists() && root.join(&id).join("display").exists()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_command_refuses_an_imported_grant_and_missing_confirmation() {
        let operation_id = fixture_id(1);
        let digest = "a".repeat(64);
        for invalid in [
            json!({"action":"preview_target","operationId":operation_id,"expectedRevision":1,"grant":{}}),
            json!({"action":"confirm_target","operationId":operation_id,"expectedRevision":1}),
        ] {
            assert!(serde_json::from_value::<Input>(invalid).is_err());
        }
        assert!(serde_json::from_value::<Input>(json!({
            "action":"confirm_target","operationId":operation_id,"expectedRevision":1,"grantSha256":digest,
        })).is_ok());
    }
}
