// SPDX-License-Identifier: Apache-2.0
//! Native source observation only. The selected target's publisher schema
//! declaration is never treated as an independently observed target schema.
use super::*;
use crate::installations::native_backup::{AuthenticatedNativeBackup, NativeSourceEvidence};
use crate::native_runtime::Session;
use crate::signed_release::native::AdmittedNativePayload;

/// Obtained only from the authenticated native backup pipeline and a fresh
/// quiesced native maintenance observation under the current release authority.
/// Not Clone/Deserialize, not an execution or target-compatibility permit.
pub struct NativeSourceObservation {
    source: AuthenticatedNativeBackup,
    evidence: NativeSourceEvidence,
    authority: SourceAuthority,
    verified: bool,
}
#[derive(Clone)]
struct SourceAuthority {
    target_payload_sha256: String,
    target_manifest_sha256: String,
    scope: String,
    generation: u64,
    policy_generation: u64,
    authority_sha256: String,
}
impl SourceAuthority {
    fn capture(store: &Store, target: &AdmittedNativePayload) -> Self {
        Self {
            target_payload_sha256: target.release.payload_sha256.clone(),
            target_manifest_sha256: target.payload.context().manifest_sha256,
            scope: store.scope.clone(),
            generation: store.current.generation,
            policy_generation: store.current.policy_generation,
            authority_sha256: store.current_sha256.clone(),
        }
    }
    fn matches(&self, store: &Store, target: &AdmittedNativePayload) -> bool {
        self.scope == store.scope
            && self.generation == store.current.generation
            && self.policy_generation == store.current.policy_generation
            && self.authority_sha256 == store.current_sha256
            && self.target_payload_sha256 == target.release.payload_sha256
            && self.target_manifest_sha256 == target.payload.context().manifest_sha256
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourceObservationReceipt<'a> {
    operation: &'static str,
    backup_id: &'a str,
    authenticated_manifest_sha256: &'a str,
    encrypted_manifest_sha256: &'a str,
    source_runtime_manifest_sha256: String,
    source_inventory_sha256: &'a str,
    source_schema_sha256: &'a str,
    source_schema_observed: bool,
    target_schema_observed: bool,
    execution_permitted: bool,
}
impl NativeSourceObservation {
    pub fn receipt(&self) -> NativeSourceObservationReceipt<'_> {
        NativeSourceObservationReceipt {
            operation: "native-source-observed",
            backup_id: &self.evidence.backup_id,
            authenticated_manifest_sha256: &self.evidence.manifest_sha256,
            encrypted_manifest_sha256: &self.source.receipt().manifest_sha256,
            source_runtime_manifest_sha256: self.source.source_manifest(),
            source_inventory_sha256: &self.evidence.inventory_sha256,
            source_schema_sha256: &self.evidence.schema_sha256,
            source_schema_observed: self.verified,
            target_schema_observed: false,
            execution_permitted: false,
        }
    }
}
impl Store {
    /// The caller retains the existing profile operation lock through both
    /// registry/history checks and the source observation. Authority namespaces
    /// deliberately outlive the UUID of the currently selected source.
    pub(super) fn check_native_source_selection(
        &self,
        profile: &Path,
        id: &str,
        root: &Path,
    ) -> crate::Result<()> {
        if profile != self.profile {
            return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE"));
        }
        let (registry, _) = installations::load(profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let bound = self.bound_source_id(&registry)?;
        if bound != registry.active_id || bound != id {
            return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE"));
        }
        let entry = registry
            .installations
            .iter()
            .find(|entry| entry.id == bound)
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        if installations::root(profile, entry) != root {
            return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE"));
        }
        installations::private_directory(root)?;
        if fs::canonicalize(root).map_err(|_| crate::err("NATIVE_SOURCE_OBSERVATION_STALE"))?
            != root
        {
            return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE"));
        }
        Ok(())
    }
    /// No caller-supplied observation/hash can construct the source capability.
    /// The same Store/profile/installation and staged signed target admission
    /// must remain current on both sides of the native database observation.
    pub fn observe_native_source(
        &self,
        target: &mut AdmittedNativePayload,
        source: AuthenticatedNativeBackup,
        session: &mut Session,
        now: u64,
    ) -> Result<NativeSourceObservation, Error> {
        self.recheck_native_admission(target, now)?;
        let evidence = source
            .observe(session, |profile, id, root| {
                self.check_native_source_selection(profile, id, root)
            })
            .map_err(|_| Error::PlanMismatch)?;
        if evidence.schema_sha256 != target.release.source_schema_sha256 {
            return Err(Error::PlanMismatch);
        }
        self.recheck_native_admission(target, now)?;
        Ok(NativeSourceObservation {
            source,
            evidence,
            authority: SourceAuthority::capture(self, target),
            verified: true,
        })
    }
    pub fn recheck_native_source(
        &self,
        target: &mut AdmittedNativePayload,
        observed: &mut NativeSourceObservation,
        session: &mut Session,
        now: u64,
    ) -> Result<(), Error> {
        observed.verified = false;
        self.recheck_native_admission(target, now)?;
        if !observed.authority.matches(self, target) {
            return Err(Error::TrustStalePolicy);
        }
        let current = observed
            .source
            .observe(session, |profile, id, root| {
                self.check_native_source_selection(profile, id, root)
            })
            .map_err(|_| Error::PlanMismatch)?;
        if current != observed.evidence
            || current.schema_sha256 != target.release.source_schema_sha256
        {
            return Err(Error::PlanMismatch);
        }
        self.recheck_native_admission(target, now)?;
        observed.verified = true;
        Ok(())
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    #[test]
    fn native_source_selection_resolves_default_and_refuses_changed_or_foreign_sources() {
        let (profile, store, payload, envelope, staged) =
            super::super::native_admission_tests::fixture();
        let source = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        let root = profile.join("local-runtime");
        installations::new_directory(&root).unwrap();
        let mut registry = installations::Registry {
            format: 1,
            active_id: source.clone(),
            installations: vec![
                installations::Entry {
                    id: source.clone(),
                    kind: "default".into(),
                    created_at: 0,
                },
                installations::Entry {
                    id: other.clone(),
                    kind: "recovery".into(),
                    created_at: 1,
                },
            ],
        };
        installations::save(&profile, &registry, None).unwrap();
        let service = crate::LifecycleService::bound_existing(profile.clone()).unwrap();
        let guard = installations::profile_lock(&service).unwrap();
        assert_eq!(store.installation, "default");
        store
            .check_native_source_selection(&profile, &source, &root)
            .unwrap();
        assert!(
            store
                .check_native_source_selection(&profile, &other, &root)
                .is_err()
        );
        assert!(
            store
                .check_native_source_selection(&profile, &source, &profile)
                .is_err()
        );
        assert!(
            store
                .check_native_source_selection(profile.parent().unwrap(), &source, &root)
                .is_err()
        );
        let (_, bytes) = installations::load(&profile).unwrap().unwrap();
        registry.active_id = other;
        installations::save(&profile, &registry, Some(&bytes)).unwrap();
        assert!(
            store
                .check_native_source_selection(&profile, &source, &root)
                .is_err()
        );
        drop(guard);
        drop(staged);
        drop(payload);
        drop(envelope);
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
    #[test]
    fn native_source_authority_cannot_follow_new_scope_epoch_or_target_declaration() {
        let (profile, store, payload, envelope, staged) =
            super::super::native_admission_tests::fixture();
        let target = store
            .admit_native_payload(&envelope, staged, payload, 20)
            .unwrap();
        let bound = SourceAuthority::capture(&store, &target);
        assert!(bound.matches(&store, &target));
        for field in 0..6 {
            let mut stale = bound.clone();
            match field {
                0 => stale.scope = "other-installation".into(),
                1 => stale.generation += 1,
                2 => stale.policy_generation += 1,
                3 => stale.authority_sha256 = "d".repeat(64),
                4 => stale.target_payload_sha256 = "d".repeat(64),
                _ => stale.target_manifest_sha256 = "d".repeat(64),
            }
            assert!(!stale.matches(&store, &target));
        }
        drop(target);
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
}
