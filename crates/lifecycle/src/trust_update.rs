// SPDX-License-Identifier: Apache-2.0
//! Trusted executor callbacks and immutable event replay; never an engine executor.
use super::*;
use crate::update::{HealthReceipt, RestoreReceipt, Stage};

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "evidence",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum UpdateEvent {
    Begin(Box<crate::update::Preflight>),
    RenewPrepared(Box<UpdateIntent>),
    ApplicationFinished,
    Health(Box<HealthReceipt>),
    UpdateFailed,
    RequestRollback,
    BeginRestore(String),
    RestoreFinished(Box<RestoreReceipt>),
    ImageOnlyRollback,
    RecoveryFailed,
    Interrupted,
    InterruptedRestoration,
    ResumeRollbackHealth(Box<RestoreReceipt>),
    DiscardPrepared,
    ReleaseCompleted,
}

pub(super) fn in_flight(stage: Stage) -> bool {
    matches!(
        stage,
        Stage::Applying | Stage::AwaitingHealth | Stage::Restoring | Stage::AwaitingRollbackHealth
    )
}

/// Replay is also used by chain validation; only the core may change, never its binding.
pub(super) fn evolve(
    intent: &UpdateIntent,
    event: &UpdateEvent,
) -> Result<Option<UpdateIntent>, Error> {
    let mut next = intent.clone();
    let core = &mut next.update;
    let result = match event {
        UpdateEvent::RenewPrepared(renewed) => {
            validate_prepared_renewal(intent, renewed)?;
            return Ok(Some((**renewed).clone()));
        }
        UpdateEvent::Begin(e) => core.begin_update((**e).clone()),
        UpdateEvent::ApplicationFinished => core.application_finished(),
        UpdateEvent::Health(e) => core.observe_health((**e).clone()),
        UpdateEvent::UpdateFailed => core.update_failed(),
        UpdateEvent::RequestRollback => core.request_rollback(),
        UpdateEvent::BeginRestore(id) => core.begin_restore(id.clone()),
        UpdateEvent::RestoreFinished(e) => core.restore_finished((**e).clone()),
        UpdateEvent::InterruptedRestoration => core.retain_interrupted_restoration(),
        UpdateEvent::ResumeRollbackHealth(e) => core.resume_rollback_health((**e).clone()),
        UpdateEvent::ImageOnlyRollback => core.image_only_rollback(),
        UpdateEvent::RecoveryFailed => core.recovery_failed(),
        UpdateEvent::Interrupted => {
            if !in_flight(core.stage()) {
                return Err(Error::PlanMismatch);
            }
            core.recover_after_restart()
        }
        UpdateEvent::DiscardPrepared => {
            if core.stage() != Stage::Prepared {
                return Err(Error::TrustOperationBusy);
            }
            return Ok(None);
        }
        UpdateEvent::ReleaseCompleted => {
            if !matches!(core.stage(), Stage::Updated | Stage::RolledBack) {
                return Err(Error::TrustOperationBusy);
            }
            return Ok(None);
        }
    };
    result.map_err(|_| Error::PlanMismatch)?;
    Ok(Some(next))
}

pub(super) fn validate_prepared_renewal(
    old: &UpdateIntent,
    next: &UpdateIntent,
) -> Result<(), Error> {
    if old.update.stage() != Stage::Prepared
        || next.update.stage() != Stage::Prepared
        || serde_json::to_vec(&old.update).map_err(|_| invalid())?
            != serde_json::to_vec(&next.update).map_err(|_| invalid())?
    {
        return Err(Error::PlanMismatch);
    }
    old.validate()?;
    next.validate()?;
    let release = |i: &UpdateIntent| -> Result<Release, Error> {
        let e: Envelope = serde_json::from_str(&i.envelope).map_err(|_| invalid())?;
        serde_json::from_str(&e.payload).map_err(|_| invalid())
    };
    let old_release = release(old)?;
    let mut new_release = release(next)?;
    if new_release.sequence <= old_release.sequence
        || new_release.issued_at < old_release.issued_at
        || new_release.expires_at <= old_release.expires_at
    {
        return Err(Error::Replay);
    }
    // Everything except sequence/time/signature must remain byte-equivalent in
    // the closed release schema, including exact artifact and source schemas.
    new_release.sequence = old_release.sequence;
    new_release.issued_at = old_release.issued_at;
    new_release.expires_at = old_release.expires_at;
    if serde_json::to_vec(&new_release).map_err(|_| invalid())?
        != serde_json::to_vec(&old_release).map_err(|_| invalid())?
    {
        return Err(Error::PlanMismatch);
    }
    Ok(())
}

fn new_intent<'a>(old: Option<&Record>, next: &'a Record) -> Option<&'a UpdateIntent> {
    if old.is_some_and(|r| r.intent.is_some()) {
        None
    } else {
        next.intent.as_ref()
    }
}

pub(super) fn validate_new_ids(
    old: Option<&Record>,
    next: &Record,
    operations: &BTreeSet<String>,
    instances: &BTreeSet<String>,
) -> Result<(), Error> {
    if let Some(intent) = new_intent(old, next) {
        let p = intent.update.plan();
        if operations.contains(&p.operation_id) || instances.contains(&p.target_instance) {
            return Err(Error::TrustIdentityReused);
        }
    }
    if let Some(UpdateEvent::BeginRestore(id)) = &next.update_event
        && instances.contains(id)
    {
        return Err(Error::TrustIdentityReused);
    }
    Ok(())
}

/// Called only after validated publication, or while replaying the validated chain.
/// Previous sources/targets/candidates remain reserved after discard/completion/failure.
pub(super) fn remember_ids(
    old: Option<&Record>,
    next: &Record,
    operations: &mut BTreeSet<String>,
    instances: &mut BTreeSet<String>,
) {
    if let Some(intent) = new_intent(old, next) {
        let p = intent.update.plan();
        operations.insert(p.operation_id.clone());
        instances.insert(p.source_instance.clone());
        instances.insert(p.target_instance.clone());
    }
    if let Some(UpdateEvent::BeginRestore(id)) = &next.update_event {
        instances.insert(id.clone());
    }
}

impl Store {
    fn update_event(
        &mut self,
        operation: &str,
        expected_generation: u64,
        event: UpdateEvent,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.check_root()?;
        if expected_generation != self.current.generation {
            return Err(Error::TrustStaleOperation);
        }
        let intent = self.intent().ok_or(Error::PlanMismatch)?;
        if intent.update.plan().operation_id != operation {
            return Err(Error::PlanMismatch);
        }
        // Launch trust must still permit target success. Expired/revoked target
        // metadata must never block recording a failure or an observed rollback.
        if matches!(event, UpdateEvent::Health(_)) && intent.update.stage() == Stage::AwaitingHealth
        {
            self.verify_for_preparation(intent.envelope.as_bytes(), now)?;
        }
        let mut next = self.current.clone();
        next.intent = evolve(intent, &event)?;
        next.update_event = Some(event);
        self.commit(next, now)
    }
    /// Internal native adapter only: historical restoration is not fresh health.
    pub(super) fn resume_restored_runtime(
        &mut self,
        operation: &str,
        generation: u64,
        receipt: RestoreReceipt,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(
            operation,
            generation,
            UpdateEvent::ResumeRollbackHealth(Box::new(receipt)),
            now,
        )
    }
    /// Trusted executor completion is AwaitingHealth, never update success.
    pub fn application_finished(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::ApplicationFinished, now)
    }
    /// Actual ready/image/schema observations must come from a trusted adapter.
    /// Do not expose this or restore callbacks as untrusted boolean/receipt endpoints.
    pub fn observe_health(
        &mut self,
        operation: &str,
        generation: u64,
        receipt: HealthReceipt,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(
            operation,
            generation,
            UpdateEvent::Health(Box::new(receipt)),
            now,
        )
    }
    pub fn update_failed(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::UpdateFailed, now)
    }
    pub fn request_rollback(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::RequestRollback, now)
    }
    /// Must finish durable publication BEFORE starting a separate-candidate restore.
    /// Retained failed candidates are never automatically deleted or reused.
    pub fn begin_restore(
        &mut self,
        operation: &str,
        generation: u64,
        candidate: String,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(
            operation,
            generation,
            UpdateEvent::BeginRestore(candidate),
            now,
        )
    }
    pub fn restore_finished(
        &mut self,
        operation: &str,
        generation: u64,
        receipt: RestoreReceipt,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(
            operation,
            generation,
            UpdateEvent::RestoreFinished(Box::new(receipt)),
            now,
        )
    }
    pub fn image_only_rollback(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::ImageOnlyRollback, now)
    }
    pub fn recovery_failed(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::RecoveryFailed, now)
    }
    /// Clear only an unexecuted intent; original records, IDs, floors and data remain.
    pub fn discard_prepared(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::DiscardPrepared, now)
    }
    /// Caller must reconcile actual runtime/helper ownership before releasing a terminal
    /// operation. This only archives the active pointer; it performs no engine cleanup.
    pub fn release_completed(
        &mut self,
        operation: &str,
        generation: u64,
        now: u64,
    ) -> Result<TrustReceipt, Error> {
        self.update_event(operation, generation, UpdateEvent::ReleaseCompleted, now)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{fixture, observations, plan, prepared_fixture, seal};
    use super::*;
    fn generation(s: &Store) -> u64 {
        s.receipt().generation
    }
    fn now(s: &Store) -> u64 {
        s.receipt().observed_at
    }
    fn health(s: &Store, rollback: Option<&str>) -> HealthReceipt {
        let p = s.intent().unwrap().update.plan();
        HealthReceipt {
            operation_id: p.operation_id.clone(),
            instance_id: rollback.unwrap_or(&p.target_instance).into(),
            image: if rollback.is_some() {
                p.source_image.clone()
            } else {
                p.target_image.clone()
            },
            schema: if rollback.is_some() {
                p.source_schema.clone()
            } else {
                p.target_schema.clone()
            },
            ready: true,
        }
    }
    fn restore(s: &Store, candidate: &str) -> RestoreReceipt {
        let p = s.intent().unwrap().update.plan();
        RestoreReceipt {
            operation_id: p.operation_id.clone(),
            backup_id: p.backup_id.clone(),
            backup_manifest: p.backup_manifest.clone(),
            inventory_digest: p.source_inventory.clone(),
            candidate_id: candidate.into(),
            schema: p.source_schema.clone(),
            inventory_verified: true,
            separate_candidate: true,
        }
    }
    fn begin(s: &mut Store, v: &VerifiedRelease) {
        s.begin_update(observations(), v, 21).unwrap();
    }
    fn finished(s: &mut Store) {
        s.application_finished("update-1", generation(s), now(s))
            .unwrap();
    }
    fn updated(s: &mut Store) {
        finished(s);
        s.observe_health("update-1", generation(s), health(s, None), now(s))
            .unwrap();
    }
    #[test]
    fn completed_update_is_durable_only_after_exact_observed_health() {
        let (p, mut s, v) = prepared_fixture();
        begin(&mut s, &v);
        finished(&mut s);
        let g = generation(&s);
        for field in 0..5 {
            let mut r = health(&s, None);
            match field {
                0 => r.instance_id = "foreign".into(),
                1 => r.image = "0".repeat(64),
                2 => r.schema = "0".repeat(64),
                3 => r.operation_id = "foreign".into(),
                _ => r.ready = false,
            }
            assert_eq!(
                s.observe_health("update-1", g, r, now(&s)).unwrap_err(),
                Error::PlanMismatch
            );
            assert_eq!(generation(&s), g);
        }
        s.observe_health("update-1", g, health(&s, None), now(&s))
            .unwrap();
        assert_eq!(s.intent().unwrap().update.stage(), Stage::Updated);
        let g = generation(&s);
        drop(s);
        let mut s = Store::open(&p, "default").unwrap();
        assert_eq!(generation(&s), g);
        assert_eq!(s.intent().unwrap().update.stage(), Stage::Updated);
        s.release_completed("update-1", g, now(&s)).unwrap();
        assert!(s.intent().is_none());
        assert_eq!(s.receipt().minimum_sequence, 2);
    }
    #[test]
    fn failed_update_restores_separate_candidate_and_requires_its_health() {
        for after_application in [false, true] {
            let (p, mut s, v) = prepared_fixture();
            begin(&mut s, &v);
            if after_application {
                finished(&mut s);
            }
            s.update_failed("update-1", generation(&s), now(&s))
                .unwrap();
            assert_eq!(s.intent().unwrap().update.stage(), Stage::RecoveryRequired);
            assert_eq!(
                s.image_only_rollback("update-1", generation(&s), now(&s))
                    .unwrap_err(),
                Error::PlanMismatch
            );
            s.begin_restore("update-1", generation(&s), "restore-1".into(), now(&s))
                .unwrap();
            let g = generation(&s);
            let mut bad = restore(&s, "restore-1");
            bad.inventory_digest = "0".repeat(64);
            assert_eq!(
                s.restore_finished("update-1", g, bad, now(&s)).unwrap_err(),
                Error::PlanMismatch
            );
            assert_eq!(generation(&s), g);
            s.restore_finished("update-1", g, restore(&s, "restore-1"), now(&s))
                .unwrap();
            assert_eq!(
                s.intent().unwrap().update.stage(),
                Stage::AwaitingRollbackHealth
            );
            assert_eq!(
                s.observe_health(
                    "update-1",
                    generation(&s),
                    health(&s, Some("source-1")),
                    now(&s)
                )
                .unwrap_err(),
                Error::PlanMismatch
            );
            s.observe_health(
                "update-1",
                generation(&s),
                health(&s, Some("restore-1")),
                now(&s),
            )
            .unwrap();
            let g = generation(&s);
            drop(s);
            let s = Store::open(&p, "default").unwrap();
            assert_eq!(generation(&s), g);
            assert_eq!(s.intent().unwrap().update.stage(), Stage::RolledBack);
        }
    }
    #[test]
    fn every_inflight_stage_reopens_as_interrupted_without_restore_or_health_proof() {
        for stage in [
            Stage::Applying,
            Stage::AwaitingHealth,
            Stage::Restoring,
            Stage::AwaitingRollbackHealth,
        ] {
            let (p, mut s, v) = prepared_fixture();
            begin(&mut s, &v);
            match stage {
                Stage::AwaitingHealth => finished(&mut s),
                Stage::Restoring | Stage::AwaitingRollbackHealth => {
                    s.update_failed("update-1", generation(&s), now(&s))
                        .unwrap();
                    s.begin_restore(
                        "update-1",
                        generation(&s),
                        "retained-candidate".into(),
                        now(&s),
                    )
                    .unwrap();
                    if stage == Stage::AwaitingRollbackHealth {
                        s.restore_finished(
                            "update-1",
                            generation(&s),
                            restore(&s, "retained-candidate"),
                            now(&s),
                        )
                        .unwrap();
                    }
                }
                _ => {}
            }
            let g = generation(&s);
            let original = fs::read(s.root.join(format!("{g:020}.json"))).unwrap();
            let root = s.root.clone();
            drop(s);
            let mut s = Store::open(&p, "default").unwrap();
            assert_eq!(generation(&s), g + 1);
            let u = serde_json::to_value(&s.intent().unwrap().update).unwrap();
            assert_eq!(u["stage"], "recovery_required");
            assert_eq!(u["failure"], "interrupted");
            for key in ["restore", "restoreCandidate", "health"] {
                assert!(u[key].is_null());
            }
            assert_eq!(
                fs::read(root.join(format!("{g:020}.json"))).unwrap(),
                original
            );
            if matches!(stage, Stage::Restoring | Stage::AwaitingRollbackHealth) {
                assert_eq!(
                    s.begin_restore(
                        "update-1",
                        generation(&s),
                        "retained-candidate".into(),
                        now(&s)
                    )
                    .unwrap_err(),
                    Error::TrustIdentityReused
                );
            }
            drop(s);
            assert_eq!(generation(&Store::open(&p, "default").unwrap()), g + 1);
        }
    }
    #[test]
    fn callbacks_cannot_overwrite_changed_generation_or_foreign_operation() {
        let (_, mut s, v) = prepared_fixture();
        begin(&mut s, &v);
        let g = generation(&s);
        assert_eq!(
            s.application_finished("other", g, now(&s)).unwrap_err(),
            Error::PlanMismatch
        );
        finished(&mut s);
        assert_eq!(
            s.update_failed("update-1", g, now(&s)).unwrap_err(),
            Error::TrustStaleOperation
        );
        assert_eq!(
            s.discard_prepared("update-1", generation(&s), now(&s))
                .unwrap_err(),
            Error::TrustOperationBusy
        );
        assert_eq!(
            s.release_completed("update-1", generation(&s), now(&s))
                .unwrap_err(),
            Error::TrustOperationBusy
        );
        assert_eq!(s.intent().unwrap().update.stage(), Stage::AwaitingHealth);
    }
    #[test]
    fn discard_preserves_floors_and_replay_reserves_prior_operation_and_target() {
        let (p, mut s, v) = prepared_fixture();
        let envelope = s.intent().unwrap().envelope.clone();
        let original = s.intent().unwrap().clone();
        let root = s.root.clone();
        s.discard_prepared("update-1", generation(&s), now(&s))
            .unwrap();
        assert!(s.intent().is_none());
        assert_eq!(s.receipt().minimum_sequence, 2);
        drop(s);
        let mut s = Store::open(&p, "default").unwrap();
        for same_operation in [true, false] {
            let mut p = plan();
            if same_operation {
                p.target_instance = "target-2".into();
            } else {
                p.operation_id = "update-2".into();
            }
            assert_eq!(
                s.prepare_update(envelope.as_bytes(), &v, p, 21)
                    .unwrap_err(),
                Error::TrustIdentityReused
            );
        }
        assert_eq!(generation(&s), 3);
        // Structurally valid/hash-linked history must still reject reused IDs on load.
        let mut next = s.current.clone();
        next.intent = Some(original);
        next.update_event = None;
        next.generation = 4;
        next.previous_sha256 = s.current_sha256.clone();
        next.observed_at = 21;
        write(&root, &next).unwrap();
        drop(s);
        assert!(matches!(
            Store::open(&p, "default"),
            Err(Error::TrustIdentityReused)
        ));
    }
    #[test]
    fn release_terminal_pointer_allows_fresh_operation_but_retains_history_and_ids() {
        let (p, mut s, v) = prepared_fixture();
        let envelope = s.intent().unwrap().envelope.clone();
        begin(&mut s, &v);
        updated(&mut s);
        s.request_rollback("update-1", generation(&s), now(&s))
            .unwrap();
        assert_eq!(
            s.release_completed("update-1", generation(&s), now(&s))
                .unwrap_err(),
            Error::TrustOperationBusy
        );
        s.begin_restore("update-1", generation(&s), "restore-1".into(), now(&s))
            .unwrap();
        s.recovery_failed("update-1", generation(&s), now(&s))
            .unwrap();
        assert_eq!(
            s.begin_restore("update-1", generation(&s), "restore-1".into(), now(&s))
                .unwrap_err(),
            Error::TrustIdentityReused
        );
        s.begin_restore("update-1", generation(&s), "restore-2".into(), now(&s))
            .unwrap();
        s.restore_finished(
            "update-1",
            generation(&s),
            restore(&s, "restore-2"),
            now(&s),
        )
        .unwrap();
        s.observe_health(
            "update-1",
            generation(&s),
            health(&s, Some("restore-2")),
            now(&s),
        )
        .unwrap();
        s.release_completed("update-1", generation(&s), now(&s))
            .unwrap();
        assert!(s.intent().is_none());
        let g = generation(&s);
        drop(s);
        let mut s = Store::open(&p, "default").unwrap();
        let mut new = plan();
        new.operation_id = "update-2".into();
        new.target_instance = "restore-2".into();
        assert_eq!(
            s.prepare_update(envelope.as_bytes(), &v, new.clone(), 21)
                .unwrap_err(),
            Error::TrustIdentityReused
        );
        new.target_instance = "target-2".into();
        s.prepare_update(envelope.as_bytes(), &v, new, 21).unwrap();
        assert_eq!(generation(&s), g + 1);
        assert_eq!(s.receipt().minimum_sequence, 2);
    }
    #[test]
    fn equal_schema_image_only_rollback_still_requires_explicit_compatibility() {
        for attested in [false, true] {
            let (p, k, policy, mut release) = fixture();
            release.artifact.schema_sha256 = "a".repeat(64);
            let envelope = seal(&k, &release);
            let mut s = Store::provision(&p, "default", policy, 10).unwrap();
            let mut v = s.verify_for_preparation(&envelope, 20).unwrap();
            v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
            let mut pl = plan();
            pl.target_schema = pl.source_schema.clone();
            s.prepare_update(&envelope, &v, pl.clone(), 20).unwrap();
            let mut e = observations();
            e.plan = pl;
            e.image_only_rollback_verified = attested;
            s.begin_update(e, &v, 21).unwrap();
            updated(&mut s);
            s.request_rollback("update-1", generation(&s), now(&s))
                .unwrap();
            let result = s.image_only_rollback("update-1", generation(&s), now(&s));
            if attested {
                result.unwrap();
                s.observe_health(
                    "update-1",
                    generation(&s),
                    health(&s, Some("source-1")),
                    now(&s),
                )
                .unwrap();
                assert_eq!(s.intent().unwrap().update.stage(), Stage::RolledBack);
            } else {
                assert_eq!(result.unwrap_err(), Error::PlanMismatch);
                assert_eq!(s.intent().unwrap().update.stage(), Stage::RecoveryRequired);
            }
        }
    }
    #[test]
    fn uncertain_completion_cannot_report_success_or_overwrite_original_record() {
        let (_, mut s, v) = prepared_fixture();
        begin(&mut s, &v);
        let collision = s.root.join("00000000000000000004.json");
        let mut f = private_file(&collision, true).unwrap();
        f.write_all(b"retained collision").unwrap();
        drop(f);
        assert_eq!(
            s.application_finished("update-1", generation(&s), now(&s))
                .unwrap_err(),
            Error::TrustWriteUncertain
        );
        assert!(s.receipt().write_uncertain);
        assert_eq!(s.intent().unwrap().update.stage(), Stage::Applying);
        assert_eq!(fs::read(collision).unwrap(), b"retained collision");
        assert_eq!(
            s.update_failed("update-1", generation(&s), now(&s))
                .unwrap_err(),
            Error::TrustWriteUncertain
        );
    }
    #[test]
    fn expired_or_revoked_target_cannot_be_completed_but_failure_can_be_recorded() {
        let (_, mut s, v) = prepared_fixture();
        begin(&mut s, &v);
        finished(&mut s);
        assert_eq!(
            s.observe_health("update-1", generation(&s), health(&s, None), 101)
                .unwrap_err(),
            Error::Expired
        );
        let mut policy = s.policy().clone();
        let key = ed25519_dalek::SigningKey::from_bytes(&[32; 32]);
        policy.public_keys = vec![
            key.verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ];
        s.replace_policy(policy, 1, now(&s)).unwrap();
        assert_eq!(
            s.observe_health("update-1", generation(&s), health(&s, None), now(&s))
                .unwrap_err(),
            Error::UntrustedKey
        );
        s.update_failed("update-1", generation(&s), now(&s))
            .unwrap();
        assert_eq!(s.intent().unwrap().update.stage(), Stage::RecoveryRequired);
        s.begin_restore(
            "update-1",
            generation(&s),
            "revoked-target-restore".into(),
            now(&s),
        )
        .unwrap();
        s.restore_finished(
            "update-1",
            generation(&s),
            restore(&s, "revoked-target-restore"),
            now(&s),
        )
        .unwrap();
        s.observe_health(
            "update-1",
            generation(&s),
            health(&s, Some("revoked-target-restore")),
            now(&s),
        )
        .unwrap();
        assert_eq!(s.intent().unwrap().update.stage(), Stage::RolledBack);
    }
}
