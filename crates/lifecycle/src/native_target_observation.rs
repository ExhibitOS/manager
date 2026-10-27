// SPDX-License-Identifier: Apache-2.0
//! Independent target observation and native plan binding, never an execution
//! permit. Existing OCI plans/records and exact-payload restore stay unchanged.
use super::*;
use crate::installations::native_backup::NativeTargetEvidence;
use crate::maintenance_source::DirectoryPin;

/// Issued by the authenticated backup + owned native candidate observation only.
/// No Clone/Deserialize or public constructor; a receipt cannot recreate it.
///
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::NativeTargetObservation;
/// let forged: NativeTargetObservation = serde_json::from_str("{}").unwrap();
/// ```
pub struct NativeTargetObservation {
    source_instance: String,
    target_instance: String,
    root: PathBuf,
    root_pin: DirectoryPin,
    target_manifest_sha256: String,
    evidence: NativeTargetEvidence,
    authority: SourceAuthority,
    verified: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTargetObservationReceipt<'a> {
    operation: &'static str,
    source_instance: &'a str,
    target_instance: &'a str,
    target_payload_manifest_sha256: &'a str,
    target_schema_sha256: &'a str,
    target_migrations_sha256: &'a str,
    target_system_identifier: &'a str,
    target_schema_observed: bool,
    original_data_preserved: bool,
    configuration_verified: bool,
    preflight_verified: bool,
    execution_permitted: bool,
}
impl NativeTargetObservation {
    pub fn receipt(&self) -> NativeTargetObservationReceipt<'_> {
        NativeTargetObservationReceipt {
            operation: "native-target-observed",
            source_instance: &self.source_instance,
            target_instance: &self.target_instance,
            target_payload_manifest_sha256: &self.target_manifest_sha256,
            target_schema_sha256: &self.evidence.target_schema_sha256,
            target_migrations_sha256: &self.evidence.target_migrations_sha256,
            target_system_identifier: &self.evidence.target_system_identifier,
            target_schema_observed: self.verified,
            original_data_preserved: self.verified,
            configuration_verified: false,
            preflight_verified: false,
            execution_permitted: false,
        }
    }
}
/// A typed, versioned native plan DATA boundary. It deliberately does not convert
/// to OCI Plan/Preflight, execute a transition, or write an authority intent.
pub struct NativeObservedPlanBoundary {
    receipt: NativeObservedPlanReceipt,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeObservedPlanReceipt {
    format: u32,
    kind: &'static str,
    stage: crate::update::Stage,
    operation_id: String,
    source_instance: String,
    target_instance: String,
    source_payload_manifest_sha256: String,
    target_payload_manifest_sha256: String,
    signed_release_payload_sha256: String,
    signed_artifact_sha256: String,
    source_schema_sha256: String,
    target_schema_sha256: String,
    target_migrations_sha256: String,
    backup_id: String,
    authenticated_backup_manifest_sha256: String,
    source_inventory_sha256: String,
    target_system_identifier: String,
    required_free_bytes: u64,
    configuration_verified: bool,
    preflight_verified: bool,
    execution_permitted: bool,
}
impl NativeObservedPlanBoundary {
    pub fn receipt(&self) -> &NativeObservedPlanReceipt {
        &self.receipt
    }
}
/// Successful same-catalog data preparation, never an activation/update permit.
/// A receipt or interrupted candidate journal cannot reconstruct this object.
///
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::NativeTargetPreparation;
/// let forged: NativeTargetPreparation = serde_json::from_str("{}").unwrap();
/// ```
pub struct NativeTargetPreparation {
    operation_id: String,
    observation: NativeTargetObservation,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTargetPreparationReceipt<'a> {
    operation: &'static str,
    operation_id: &'a str,
    state: &'static str,
    observation: NativeTargetObservationReceipt<'a>,
    configuration_verified: bool,
    preflight_verified: bool,
    execution_permitted: bool,
}
impl NativeTargetPreparation {
    pub fn receipt(&self) -> NativeTargetPreparationReceipt<'_> {
        NativeTargetPreparationReceipt {
            operation: "native-target-copy-prepared", operation_id: &self.operation_id,
            state: "PreparedInactive", observation: self.observation.receipt(),
            configuration_verified: false, preflight_verified: false, execution_permitted: false,
        }
    }
    /// The existing plan binder MUST re-observe this capability with both held
    /// sessions/current authority. This getter grants no execution permission.
    pub fn target_observation(&mut self) -> &mut NativeTargetObservation { &mut self.observation }
}
impl Store {
    /// Invoked by the existing authenticated backup pipeline while its profile
    /// operation lock is retained. The authority namespace is not a source UUID.
    fn check_native_target_selection(
        &self,
        profile: &Path,
        source_id: &str,
        source_root: &Path,
        target_id: &str,
        target_root: &Path,
    ) -> crate::Result<()> {
        self.check_native_source_selection(profile, source_id, source_root)?;
        let (registry, _) = installations::load(profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let candidate = registry
            .installations
            .iter()
            .find(|entry| entry.id == target_id && entry.kind == "recovery")
            .ok_or_else(|| crate::err("NATIVE_TARGET_SELECTION_INVALID"))?;
        if target_id == source_id
            || registry.active_id == target_id
            || installations::root(profile, candidate) != target_root
            || source_root.starts_with(target_root)
            || target_root.starts_with(source_root)
        {
            return Err(crate::err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        installations::private_directory(target_root)?;
        if fs::canonicalize(target_root)
            .map_err(|_| crate::err("NATIVE_TARGET_SELECTION_INVALID"))?
            != target_root
        {
            return Err(crate::err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        Ok(())
    }
    /// Additive authenticated-native-copy-v1 only. Ordinary exact-payload
    /// native restore, OCI update intents/reducers and activation are unchanged.
    pub fn prepare_native_target_copy(
        &self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        source_session: &mut Session, target_session: &mut Session, target_id: &str,
        operation_id: &str, now: u64,
    ) -> crate::Result<NativeTargetPreparation> {
        source.verified = false;
        self.recheck_native_admission(admitted,now).map_err(|e| crate::err(e.code()))?;
        if !source.authority.matches(self,admitted) { return Err(crate::err("UPDATE_POLICY_STALE")); }
        let operation = uuid::Uuid::parse_str(operation_id).map_err(|_| crate::err("UPDATE_PLAN_MISMATCH"))?.to_string();
        if self.used_operations.contains(&operation) || self.used_instances.contains(target_id)
            || source.evidence.schema_sha256 != admitted.release.release.artifact.schema_sha256
            || source.evidence.schema_sha256 != admitted.release.source_schema_sha256 { return Err(crate::err("NATIVE_TARGET_COPY_CATALOG_UNSUPPORTED")); }
        let root=target_session.target_observation_root().to_owned();
        let manifest=admitted.payload.context().manifest_sha256;
        if manifest==source.source.source_manifest() || !target_session.matches_native_source(&root,&manifest) { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let root_pin=DirectoryPin::open(&root)?;
        let authority=serde_json::json!({"scope":self.scope,"generation":self.current.generation,"policyGeneration":self.current.policy_generation,"recordSha256":self.current_sha256,"signedReleasePayloadSha256":admitted.release.payload_sha256,"signedArtifactSha256":admitted.release.release.artifact.sha256});
        let source_authority=&source.authority;
        let evidence=source.source.prepare_target_copy(source_session,target_session,&admitted.release.release.artifact.schema_sha256.clone(),&operation,authority,|profile,id,source_root,target_root| {
            self.recheck_native_admission(admitted,now).map_err(|e|crate::err(e.code()))?;
            if !source_authority.matches(self,admitted) || target_root!=root.as_path() { return Err(crate::err("UPDATE_POLICY_STALE")); }
            self.check_native_target_selection(profile,id,source_root,target_id,target_root)
        })?;
        root_pin.check()?;
        self.recheck_native_admission(admitted,now).map_err(|e|crate::err(e.code()))?;
        if !source.authority.matches(self,admitted) || !target_session.matches_native_source(&root,&manifest)
            || evidence.backup_id!=source.evidence.backup_id || evidence.authenticated_manifest_sha256!=source.evidence.manifest_sha256
            || evidence.source_inventory_sha256!=source.evidence.inventory_sha256 || evidence.source_schema_sha256!=source.evidence.schema_sha256 { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        source.verified=true;
        let observation=NativeTargetObservation {source_instance:self.bound_source_id(&installations::load(&self.profile)?.ok_or_else(||crate::err("UPDATE_SOURCE_UNREGISTERED"))?.0)?,target_instance:target_id.into(),root,root_pin,target_manifest_sha256:manifest,evidence,authority:SourceAuthority::capture(self,admitted),verified:true};
        Ok(NativeTargetPreparation {operation_id:operation,observation})
    }
    pub fn observe_native_target(
        &self,
        admitted: &mut AdmittedNativePayload,
        source: &mut NativeSourceObservation,
        source_session: &mut Session,
        target_session: &mut Session,
        target_id: &str,
        now: u64,
    ) -> Result<NativeTargetObservation, Error> {
        source.verified = false;
        self.recheck_native_admission(admitted, now)?;
        if !source.authority.matches(self, admitted) {
            return Err(Error::TrustStalePolicy);
        }
        let root = target_session.target_observation_root().to_owned();
        let manifest = admitted.payload.context().manifest_sha256;
        if !target_session.matches_native_source(&root, &manifest) {
            return Err(Error::PlanMismatch);
        }
        let root_pin = DirectoryPin::open(&root).map_err(|_| Error::PlanMismatch)?;
        let mut source_id = None;
        let observed = source
            .source
            .observe_target(
                source_session,
                target_session,
                &admitted.release.release.artifact.schema_sha256,
                |profile, id, source_root, target_root| {
                    self.check_native_target_selection(
                        profile,
                        id,
                        source_root,
                        target_id,
                        target_root,
                    )?;
                    if source_id.as_deref().is_some_and(|previous| previous != id) {
                        return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE"));
                    }
                    source_id = Some(id.to_owned());
                    if target_root != root.as_path() {
                        return Err(crate::err("NATIVE_TARGET_SELECTION_INVALID"));
                    }
                    Ok(())
                },
            )
            .map_err(|_| Error::PlanMismatch)?;
        root_pin.check().map_err(|_| Error::PlanMismatch)?;
        self.recheck_native_admission(admitted, now)?;
        if observed.source_schema_sha256 != source.evidence.schema_sha256
            || observed.source_schema_sha256 != admitted.release.source_schema_sha256
            || observed.backup_id != source.evidence.backup_id
            || observed.authenticated_manifest_sha256 != source.evidence.manifest_sha256
            || observed.source_inventory_sha256 != source.evidence.inventory_sha256
            || !target_session.matches_native_source(&root, &manifest)
        {
            return Err(Error::PlanMismatch);
        }
        source.verified = true;
        Ok(NativeTargetObservation {
            source_instance: source_id.ok_or(Error::PlanMismatch)?,
            target_instance: target_id.into(),
            root,
            root_pin,
            target_manifest_sha256: manifest,
            evidence: observed,
            authority: SourceAuthority::capture(self, admitted),
            verified: true,
        })
    }
    pub fn recheck_native_target(
        &self,
        admitted: &mut AdmittedNativePayload,
        source: &mut NativeSourceObservation,
        target: &mut NativeTargetObservation,
        source_session: &mut Session,
        target_session: &mut Session,
        now: u64,
    ) -> Result<(), Error> {
        target.verified = false;
        if !target.authority.matches(self, admitted) {
            return Err(Error::TrustStalePolicy);
        }
        target.root_pin.check().map_err(|_| Error::PlanMismatch)?;
        let fresh = self.observe_native_target(
            admitted,
            source,
            source_session,
            target_session,
            &target.target_instance,
            now,
        )?;
        target.root_pin.check().map_err(|_| Error::PlanMismatch)?;
        if fresh.root != target.root
            || fresh.source_instance != target.source_instance
            || fresh.target_manifest_sha256 != target.target_manifest_sha256
            || fresh.evidence != target.evidence
        {
            return Err(Error::PlanMismatch);
        }
        target.verified = true;
        Ok(())
    }
    pub fn bind_native_plan_boundary(
        &self,
        admitted: &mut AdmittedNativePayload,
        source: &mut NativeSourceObservation,
        target: &mut NativeTargetObservation,
        source_session: &mut Session,
        target_session: &mut Session,
        operation_id: &str,
        required_free_bytes: u64,
        now: u64,
    ) -> Result<NativeObservedPlanBoundary, Error> {
        let operation = uuid::Uuid::parse_str(operation_id)
            .map_err(|_| Error::PlanMismatch)?
            .to_string();
        if required_free_bytes == 0
            || required_free_bytes > 9_007_199_254_740_991
            || self.used_operations.contains(&operation)
            || self.used_instances.contains(&target.target_instance)
            || source.source.source_manifest() == admitted.payload.context().manifest_sha256
        {
            return Err(Error::PlanMismatch);
        }
        self.recheck_native_target(
            admitted,
            source,
            target,
            source_session,
            target_session,
            now,
        )?;
        Ok(NativeObservedPlanBoundary {
            receipt: NativeObservedPlanReceipt {
                format: 2,
                kind: "native-payload",
                stage: crate::update::Stage::Prepared,
                operation_id: operation,
                source_instance: target.source_instance.clone(),
                target_instance: target.target_instance.clone(),
                source_payload_manifest_sha256: source.source.source_manifest(),
                target_payload_manifest_sha256: target.target_manifest_sha256.clone(),
                signed_release_payload_sha256: admitted.release.payload_sha256.clone(),
                signed_artifact_sha256: admitted.release.release.artifact.sha256.clone(),
                source_schema_sha256: source.evidence.schema_sha256.clone(),
                target_schema_sha256: target.evidence.target_schema_sha256.clone(),
                target_migrations_sha256: target.evidence.target_migrations_sha256.clone(),
                backup_id: source.evidence.backup_id.clone(),
                authenticated_backup_manifest_sha256: source.evidence.manifest_sha256.clone(),
                source_inventory_sha256: source.evidence.inventory_sha256.clone(),
                target_system_identifier: target.evidence.target_system_identifier.clone(),
                required_free_bytes,
                configuration_verified: false,
                preflight_verified: false,
                execution_permitted: false,
            },
        })
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    #[test]
    fn target_selection_requires_inactive_registered_recovery_and_pinned_directory() {
        let (profile, store, payload, envelope, staged) =
            super::super::super::native_admission_tests::fixture();
        let source = uuid::Uuid::new_v4().to_string();
        let target = uuid::Uuid::new_v4().to_string();
        let source_root = profile.join("local-runtime");
        let parent = profile.join("installations");
        let target_root = parent.join(&target);
        installations::new_directory(&source_root).unwrap();
        installations::new_directory(&parent).unwrap();
        installations::new_directory(&target_root).unwrap();
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
                    id: target.clone(),
                    kind: "recovery".into(),
                    created_at: 1,
                },
            ],
        };
        installations::save(&profile, &registry, None).unwrap();
        let service = crate::LifecycleService::bound_existing(profile.clone()).unwrap();
        let guard = installations::profile_lock(&service).unwrap();
        store
            .check_native_target_selection(&profile, &source, &source_root, &target, &target_root)
            .unwrap();
        assert!(
            store
                .check_native_target_selection(
                    &profile,
                    &source,
                    &source_root,
                    &source,
                    &source_root
                )
                .is_err()
        );
        assert!(
            store
                .check_native_target_selection(
                    &profile,
                    &source,
                    &source_root,
                    &target,
                    &source_root
                )
                .is_err()
        );
        assert!(
            store
                .check_native_target_selection(
                    &profile,
                    &source,
                    &source_root,
                    &uuid::Uuid::new_v4().to_string(),
                    &target_root
                )
                .is_err()
        );
        let pin = DirectoryPin::open(&target_root).unwrap();
        let old = parent.join("retained-original");
        fs::rename(&target_root, &old).unwrap();
        installations::new_directory(&target_root).unwrap();
        assert!(pin.check().is_err());
        let (_, bytes) = installations::load(&profile).unwrap().unwrap();
        registry.active_id = target.clone();
        installations::save(&profile, &registry, Some(&bytes)).unwrap();
        assert!(
            store
                .check_native_target_selection(
                    &profile,
                    &source,
                    &source_root,
                    &target,
                    &target_root
                )
                .is_err()
        );
        drop(pin);
        drop(guard);
        drop(staged);
        drop(payload);
        drop(envelope);
        drop(store);
        fs::remove_dir_all(profile.parent().unwrap()).unwrap();
    }
}

/// Private, in-memory configuration proposal after fresh authenticated target
/// observation. Neither its receipt nor this type can activate a candidate.
/// No Clone/Deserialize or public constructor; installed configuration and full
/// update preflight remain unverified.
///
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::NativeTargetConfigurationPlan;
/// let forged: NativeTargetConfigurationPlan = serde_json::from_str("{}").unwrap();
/// ```
pub struct NativeTargetConfigurationPlan {
    receipt: NativeTargetConfigurationPlanReceipt,
    _configuration: installations::native_restore::ReboundConfigurationPlan,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTargetConfigurationPlanReceipt {
    format: u32,
    operation: &'static str,
    operation_id: String,
    state: &'static str,
    source_instance: String,
    target_instance: String,
    authenticated_backup_manifest_sha256: String,
    source_payload_manifest_sha256: String,
    target_payload_manifest_sha256: String,
    configuration_file_count: usize,
    configuration_planned: bool,
    configuration_verified: bool,
    preflight_verified: bool,
    execution_permitted: bool,
}
impl NativeTargetConfigurationPlan {
    pub fn receipt(&self) -> &NativeTargetConfigurationPlanReceipt { &self.receipt }
}
impl Store {
    /// Builds a closed secret proposal without writing target/profile files or
    /// invoking restore, schema changes, activation or an update execution path.
    pub fn plan_native_target_configuration(
        &self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, source_session: &mut Session,
        target_session: &mut Session, now: u64,
    ) -> crate::Result<NativeTargetConfigurationPlan> {
        self.recheck_native_target(admitted, source, &mut preparation.observation, source_session, target_session, now).map_err(|e| crate::err(e.code()))?;
        let operation = preparation.operation_id.clone();
        let target_id = preparation.observation.target_instance.clone();
        let root = preparation.observation.root.clone();
        let expected = preparation.observation.evidence.clone();
        if self.used_operations.contains(&operation) || self.used_instances.contains(&target_id) { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let authority = serde_json::json!({"scope":self.scope,"generation":self.current.generation,"policyGeneration":self.current.policy_generation,"recordSha256":self.current_sha256,"signedReleasePayloadSha256":admitted.release.payload_sha256,"signedArtifactSha256":admitted.release.release.artifact.sha256});
        let source_authority = source.authority.clone();
        let configuration = source.source.plan_target_configuration(source_session, target_session, &operation, &expected, &authority, |profile, id, source_root, target_root| {
            self.recheck_native_admission(admitted, now).map_err(|e| crate::err(e.code()))?;
            if !source_authority.matches(self, admitted) || target_root != root.as_path() { return Err(crate::err("UPDATE_POLICY_STALE")); }
            self.check_native_target_selection(profile, id, source_root, &target_id, target_root)
        })?;
        self.recheck_native_target(admitted, source, &mut preparation.observation, source_session, target_session, now).map_err(|e| crate::err(e.code()))?;
        configuration.recheck()?;
        if self.used_operations.contains(&operation) || self.used_instances.contains(&target_id)
            || preparation.observation.root != root || preparation.observation.evidence != expected { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let receipt = NativeTargetConfigurationPlanReceipt {
            format: 1, operation: "native-target-configuration-planned", operation_id: operation,
            state: "ConfigurationPlannedInactive", source_instance: preparation.observation.source_instance.clone(),
            target_instance: target_id, authenticated_backup_manifest_sha256: expected.authenticated_manifest_sha256,
            source_payload_manifest_sha256: source.source.source_manifest(), target_payload_manifest_sha256: admitted.payload.context().manifest_sha256,
            configuration_file_count: configuration.file_count(), configuration_planned: true,
            configuration_verified: false, preflight_verified: false, execution_permitted: false,
        };
        Ok(NativeTargetConfigurationPlan {receipt, _configuration: configuration})
    }
}
