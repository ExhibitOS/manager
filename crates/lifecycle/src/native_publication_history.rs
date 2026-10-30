// SPDX-License-Identifier: Apache-2.0
//! Closed native configuration-publication history, not an execution permit.
//! This does not construct a writer, revive an observation, or install files.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativePublicationBinding {
    format: u8,
    pub(super) operation_id: String,
    attempt_id: String,
    pub(super) source_instance: String,
    pub(super) target_instance: String,
    source_root_sha256: String,
    target_root_sha256: String,
    source_payload_sha256: String,
    target_payload_sha256: String,
    release_payload_sha256: String,
    artifact_sha256: String,
    backup_id: String,
    backup_manifest_sha256: String,
    source_inventory_sha256: String,
    source_schema_sha256: String,
    target_schema_sha256: String,
    target_migrations_sha256: String,
    source_system_identifier: String,
    target_system_identifier: String,
    configuration_manifest_sha256: String,
    beforeimage_archive_sha256: String,
    pub(super) authority_generation: u64,
    authority_sha256: String,
    pub(super) policy_generation: u64,
    state: NativePublicationState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativePublicationState { Pending, PublishedInactive, RestoredGeneratedInactive }

fn canonical_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).ok().is_some_and(|id| id.to_string() == s)
}
fn system_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u64>().is_ok()
}
impl NativePublicationBinding {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.format != 1
            || ![&self.operation_id, &self.attempt_id, &self.source_instance, &self.target_instance, &self.backup_id].into_iter().all(|s| canonical_uuid(s))
            || self.source_instance == self.target_instance || self.operation_id == self.attempt_id
            || !system_id(&self.source_system_identifier) || !system_id(&self.target_system_identifier)
            || self.source_system_identifier == self.target_system_identifier
            || self.authority_generation == 0 || self.policy_generation == 0 || self.policy_generation > self.authority_generation
            || ![&self.source_root_sha256, &self.target_root_sha256, &self.source_payload_sha256, &self.target_payload_sha256,
                &self.release_payload_sha256, &self.artifact_sha256, &self.backup_manifest_sha256, &self.source_inventory_sha256,
                &self.source_schema_sha256, &self.target_schema_sha256, &self.target_migrations_sha256,
                &self.configuration_manifest_sha256, &self.beforeimage_archive_sha256, &self.authority_sha256]
                .into_iter().all(|s| hex::<32>(s).is_some())
            || self.source_root_sha256 == self.target_root_sha256 || self.source_payload_sha256 == self.target_payload_sha256
            || self.source_schema_sha256 != self.target_schema_sha256 {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn pending(&self) -> bool { self.state == NativePublicationState::Pending }
    pub(super) fn in_flight(&self) -> bool { self.state != NativePublicationState::RestoredGeneratedInactive }
    pub(super) fn current_predecessor(&self, record: &Record) -> bool {
        let offset = match self.state {
            NativePublicationState::Pending => 1,
            NativePublicationState::PublishedInactive => 2,
            NativePublicationState::RestoredGeneratedInactive => return false,
        };
        self.authority_generation.checked_add(offset) == Some(record.generation)
            && self.policy_generation == record.policy_generation
    }
    pub(super) fn starts_after(&self, old: &Record, next: &Record) -> bool {
        self.pending() && self.authority_generation == old.generation && self.authority_sha256 == next.previous_sha256
            && self.policy_generation == old.policy_generation && self.source_schema_sha256 == old.policy.source_schema_sha256 && old.intent.is_none()
            && old.policy.format == 2 && old.policy.target == "macos-arm64"
    }
    pub(super) fn completes(&self, old: &Self) -> bool {
        let allowed = matches!((old.state, self.state),
            (NativePublicationState::Pending, NativePublicationState::PublishedInactive | NativePublicationState::RestoredGeneratedInactive)
            | (NativePublicationState::PublishedInactive, NativePublicationState::RestoredGeneratedInactive));
        if !allowed { return false; }
        let mut expected = old.clone(); expected.state = self.state;
        &expected == self
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn binding(store: &Store) -> NativePublicationBinding {
        let id = || uuid::Uuid::new_v4().to_string();
        NativePublicationBinding {
            format: 1, operation_id: id(), attempt_id: id(), source_instance: id(), target_instance: id(),
            source_root_sha256: "1".repeat(64), target_root_sha256: "2".repeat(64),
            source_payload_sha256: "3".repeat(64), target_payload_sha256: "4".repeat(64),
            release_payload_sha256: "5".repeat(64), artifact_sha256: "6".repeat(64), backup_id: id(),
            backup_manifest_sha256: "7".repeat(64), source_inventory_sha256: "8".repeat(64),
            source_schema_sha256: store.current.policy.source_schema_sha256.clone(), target_schema_sha256: store.current.policy.source_schema_sha256.clone(),
            target_migrations_sha256: "b".repeat(64), source_system_identifier: "12345".into(),
            target_system_identifier: "67890".into(), configuration_manifest_sha256: "c".repeat(64),
            beforeimage_archive_sha256: "d".repeat(64), authority_generation: store.current.generation,
            authority_sha256: store.current_sha256.clone(), policy_generation: store.current.policy_generation,
            state: NativePublicationState::Pending,
        }
    }
    #[test]
    fn closed_native_history_preserves_legacy_bytes_and_refuses_tamper_and_reuse() {
        let (profile, _key, mut policy, _release) = super::super::tests::fixture();
        policy.format = 2; policy.target = "macos-arm64".into();
        let mut store = Store::provision(&profile, "default", policy, 10).unwrap();
        let legacy = serde_json::to_value(&store.current).unwrap();
        assert!(legacy.get("nativePublication").is_none());
        let roundtrip: Record = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(roundtrip).unwrap(), legacy);
        let pending = binding(&store);
        let mut next = store.current.clone(); next.native_publication = Some(pending.clone());
        let mut wrong = next.clone(); wrong.policy.minimum_sequence += 1;
        assert!(store.commit(wrong, 20).is_err());
        store.commit(next, 20).unwrap();
        // An old deny-unknown reader cannot silently consume a native record.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct OldRecord {
            format: u8, scope: String, generation: u64, previous_sha256: String,
            policy_generation: u64, observed_at: u64, policy: Policy, revoked_keys: Vec<String>,
            acceptance: Option<Accepted>, intent: Option<UpdateIntent>, update_event: Option<UpdateEvent>, recovery_binding: Option<String>,
        }
        assert!(serde_json::from_value::<OldRecord>(legacy).is_ok());
        assert!(serde_json::from_value::<OldRecord>(serde_json::to_value(&store.current).unwrap()).is_err());
        let mut changed = store.current.clone();
        changed.native_publication.as_mut().unwrap().target_root_sha256 = "f".repeat(64);
        changed.native_publication.as_mut().unwrap().state = NativePublicationState::PublishedInactive;
        assert!(store.commit(changed, 21).is_err());
        let mut completed = store.current.clone();
        completed.native_publication.as_mut().unwrap().state = NativePublicationState::PublishedInactive;
        store.commit(completed, 21).unwrap();
        let mut overlapping = store.current.clone(); overlapping.native_publication = Some(binding(&store));
        assert!(store.commit(overlapping, 22).is_err());
        let mut restored = store.current.clone();
        restored.native_publication.as_mut().unwrap().state = NativePublicationState::RestoredGeneratedInactive;
        store.commit(restored, 22).unwrap();
        assert!(serde_json::from_value::<OldRecord>(serde_json::to_value(&store.current).unwrap()).is_err());
        drop(store);
        let mut store = Store::open(&profile, "default").unwrap();
        let mut reused = binding(&store); reused.operation_id = pending.operation_id.clone();
        let mut attempt = store.current.clone(); attempt.native_publication = Some(reused);
        assert!(store.commit(attempt, 23).is_err());
        let mut reused_target = binding(&store); reused_target.target_instance = pending.target_instance.clone();
        let mut attempt = store.current.clone(); attempt.native_publication = Some(reused_target);
        assert!(matches!(store.commit(attempt, 23), Err(Error::TrustIdentityReused)));
        // OCI and native IDs share the same retained audit even across cold open.
        let mut oci_plan = super::super::tests::plan();
        oci_plan.operation_id = pending.operation_id.clone();
        let mut oci = store.current.clone();
        oci.intent = Some(UpdateIntent { envelope: String::new(), key_id: String::new(), payload_sha256: String::new(), artifact_sha256: String::new(), update: crate::update::Update::new(oci_plan.clone()).unwrap() });
        assert!(matches!(update_journal::validate_new_ids(Some(&store.current), &oci, &store.used_operations, &store.used_instances), Err(Error::TrustIdentityReused)));
        oci_plan.operation_id = uuid::Uuid::new_v4().to_string();
        oci_plan.target_instance = pending.target_instance.clone();
        oci.intent.as_mut().unwrap().update = crate::update::Update::new(oci_plan).unwrap();
        assert!(matches!(update_journal::validate_new_ids(Some(&store.current), &oci, &store.used_operations, &store.used_instances), Err(Error::TrustIdentityReused)));
        let root = profile.parent().unwrap().to_path_buf(); drop(store); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn policy_rotation_invalidates_pending_completion_after_cold_open() {
        let (profile, _key, mut policy, _release) = super::super::tests::fixture();
        policy.format = 2; policy.target = "macos-arm64".into();
        let mut store = Store::provision(&profile, "default", policy.clone(), 10).unwrap();
        let mut next = store.current.clone(); next.native_publication = Some(binding(&store));
        store.commit(next, 20).unwrap();
        store.replace_policy(policy, 1, 21).unwrap();
        drop(store);
        let mut store = Store::open(&profile, "default").unwrap();
        for state in [NativePublicationState::PublishedInactive, NativePublicationState::RestoredGeneratedInactive] {
            let mut next = store.current.clone(); next.native_publication.as_mut().unwrap().state = state;
            assert!(store.commit(next, 22).is_err());
        }
        let root = profile.parent().unwrap().to_path_buf(); drop(store); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_binding_survives_vault_commit_and_cold_open_without_execution() {
        let (profile, _key, mut policy, _release) = super::super::tests::fixture();
        let controller = crate::installations::InstallationController::new(profile.clone(), None).unwrap();
        drop(controller);
        policy.format = 2; policy.target = "macos-arm64".into();
        let mut store = Store::provision(&profile, "default", policy, 10).unwrap();
        let vault = profile.parent().unwrap().join("native-history-test-vault");
        store.enroll_authority_recovery(&vault, 11).unwrap();
        let pending = binding(&store);
        let mut next = store.current.clone(); next.native_publication = Some(pending.clone());
        store.commit(next, 20).unwrap();
        let head = store.current_sha256.clone();
        drop(store);
        let store = Store::open(&profile, "default").unwrap();
        assert_eq!(store.current_sha256, head);
        assert_eq!(store.current.native_publication.as_ref(), Some(&pending));
        assert!(store.used_operations.contains(&pending.operation_id));
        assert!(store.used_instances.contains(&pending.target_instance));
        let root = profile.parent().unwrap().to_path_buf(); drop(store); fs::remove_dir_all(root).unwrap();
    }
}
