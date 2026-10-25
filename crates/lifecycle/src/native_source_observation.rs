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
        if source.scope() != (self.profile.as_path(), self.installation.as_str()) {
            return Err(Error::TrustStalePolicy);
        }
        let evidence = source.observe(session).map_err(|_| Error::PlanMismatch)?;
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
        if !observed.authority.matches(self, target)
            || observed.source.scope() != (self.profile.as_path(), self.installation.as_str())
        {
            return Err(Error::TrustStalePolicy);
        }
        let current = observed
            .source
            .observe(session)
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
