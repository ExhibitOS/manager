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
    // Original generated copy attempt, retained only in this opaque capability.
    attempt_id: String,
    observation: NativeTargetObservation,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePublicationRepairReceipt {
    operation_id: String,
    state: &'static str,
    authority_sha256: String,
    execution_permitted: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeConfigurationPublishedReceipt {
    operation_id: String,
    state: &'static str,
    authority_sha256: String,
    preflight_verified: bool,
    execution_permitted: bool,
}
fn check_offline_guard(root: &Path, guard: &crate::OperationGuard) -> crate::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let held = guard.metadata().map_err(|_| crate::err("NATIVE_TARGET_SELECTION_INVALID"))?;
    let named = fs::symlink_metadata(root.join("operation.lock")).map_err(|_| crate::err("NATIVE_TARGET_SELECTION_INVALID"))?;
    if !held.is_file() || named.is_symlink() || held.dev() != named.dev() || held.ino() != named.ino()
        || held.nlink() != 1 || held.uid() != unsafe { libc::geteuid() } || held.mode() & 0o777 != 0o600 {
        return Err(crate::err("NATIVE_TARGET_SELECTION_INVALID"));
    }
    Ok(())
}
fn check_durable_head(store: &Store) -> crate::Result<()> {
    store.check_root().map_err(|e| crate::err(e.code()))?;
    let bytes = read_record(&store.root.join(format!("{:020}.json", store.current.generation))).map_err(|e| crate::err(e.code()))?;
    if hash(&bytes) != store.current_sha256 { return Err(crate::err("TRUST_WRITE_UNCERTAIN")); }
    Ok(())
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
    /// Offline repair of an interrupted inactive publication. The supplied key
    /// must authenticate the archive already bound by this Store; disk markers
    /// and caller paths alone never confer repair authority.
    pub fn repair_native_configuration_publication(&mut self, key_file: &Path) -> crate::Result<NativePublicationRepairReceipt> {
        use super::super::native_publication_history::NativePublicationState;
        self.check_root().map_err(|e| crate::err(e.code()))?;
        let binding = self.current.native_publication.clone().filter(|v| v.in_flight() && v.current_predecessor(&self.current))
            .ok_or_else(|| crate::err("UPDATE_POLICY_STALE"))?;
        let head = self.current_sha256.clone();
        let profile = self.profile.clone();
        let profile_service = crate::LifecycleService::bound_existing(profile.clone())?;
        let profile_guard = installations::profile_lock(&profile_service)?;
        let (registry, _) = installations::load(&profile)?.ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let source = registry.installations.iter().find(|v| v.id == binding.source_instance).ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let target = registry.installations.iter().find(|v| v.id == binding.target_instance).ok_or_else(|| crate::err("NATIVE_TARGET_SELECTION_INVALID"))?;
        let source_root = installations::root(&profile, source);
        let target_root = installations::root(&profile, target);
        self.check_native_target_selection(&profile, &binding.source_instance, &source_root, &binding.target_instance, &target_root)?;
        if hash(source_root.as_os_str().as_encoded_bytes()) != binding.source_root_sha256
            || hash(target_root.as_os_str().as_encoded_bytes()) != binding.target_root_sha256 { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let target_service = crate::LifecycleService::bound_existing(target_root.clone())?;
        let target_guard = installations::profile_lock(&target_service)?;
        crate::native_runtime::check_interrupted_root(&target_root)?;
        let target_pin = DirectoryPin::open(&target_root)?;
        let profile_pin = DirectoryPin::open(&profile)?;
        let directory = target_root.join(format!("native-target-preparation-{}", binding.operation_id));
        installations::private_directory(&directory)?;
        let directory_pin = DirectoryPin::open(&directory)?;
        let read = |root: &Path, name: &str| -> crate::Result<(serde_json::Value, crate::hosted_runtime_file::Pin)> {
            let private = crate::installations::native_backup::Secret(crate::installation_backup::source_bytes(root, name, 65536, true)?);
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&root.join(name), 65536)?;
            let bytes = crate::installations::native_backup::Secret(bytes);
            if private.0 != bytes.0 { return Err(crate::err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            Ok((serde_json::from_slice(&bytes.0).map_err(|_| crate::err("NATIVE_TARGET_CONFIGURATION_CHANGED"))?, pin))
        };
        let (marker, marker_pin) = read(&target_root, "native-target-preparation-current.json")?;
        let (intent, intent_pin) = read(&directory, "intent.json")?;
        if marker["format"] != 1 || marker["operationId"] != binding.operation_id || marker["attemptId"] != binding.attempt_id
            || intent["format"] != 1 || intent["state"] != "PreparedInactive" || intent["operationId"] != binding.operation_id
            || intent["attemptId"] != binding.attempt_id || intent["sourceRoot"].as_str() != source_root.to_str()
            || intent["targetRoot"].as_str() != target_root.to_str() || intent["backupId"] != binding.backup_id
            || intent["authenticatedBackupManifestSha256"] != binding.backup_manifest_sha256
            || intent["sourcePayloadManifestSha256"] != binding.source_payload_sha256
            || intent["targetPayloadManifestSha256"] != binding.target_payload_sha256
            || intent["sourceInventorySha256"] != binding.source_inventory_sha256
            || intent["targetSystemIdentifier"] != binding.target_system_identifier
            || intent["authority"]["generation"] != binding.authority_generation
            || intent["authority"]["policyGeneration"] != binding.policy_generation
            || intent["authority"]["recordSha256"] != binding.authority_sha256
            || intent["authority"]["scope"] != self.scope
            || intent["authority"]["signedReleasePayloadSha256"] != binding.release_payload_sha256
            || intent["authority"]["signedArtifactSha256"] != binding.artifact_sha256 { return Err(crate::err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
        if !key_file.is_absolute() || key_file.starts_with(&profile) || profile.starts_with(key_file)
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file) { return Err(crate::err("BACKUP_KEY_INVALID")); }
        let key_parent = key_file.parent().ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
        let key_name = key_file.file_name().and_then(|v| v.to_str()).ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
        let key = crate::installations::native_backup::Secret(crate::installation_backup::source_bytes(key_parent, key_name, 32, true)?);
        let (bounded, key_pin) = crate::hosted_runtime_file::bounded(key_file, 32)?;
        let bounded = crate::installations::native_backup::Secret(bounded);
        if key.0.len() != 32 || key.0 != bounded.0 { return Err(crate::err("BACKUP_KEY_INVALID")); }
        let staged = directory.join(format!("publication-{}", binding.attempt_id));
        let before = installations::native_restore::open_beforeimages(&staged, &binding.operation_id, &binding.attempt_id, &key.0,
            &binding.configuration_manifest_sha256, &binding.beforeimage_archive_sha256)?;
        let owner = before.owner()?;
        let mut validate = || -> crate::Result<()> {
            check_durable_head(self)?;
            if self.current_sha256 != head || self.current.native_publication.as_ref() != Some(&binding)
                || !binding.current_predecessor(&self.current) { return Err(crate::err("UPDATE_POLICY_STALE")); }
            self.check_native_target_selection(&profile, &binding.source_instance, &source_root, &binding.target_instance, &target_root)?;
            crate::native_runtime::check_interrupted_root(&target_root)?;
            check_offline_guard(&profile, &profile_guard)?; check_offline_guard(&target_root, &target_guard)?;
            target_pin.check()?; profile_pin.check()?; directory_pin.check()?; marker_pin.check()?; intent_pin.check()?; key_pin.check()?;
            before.recheck()
        };
        before.restore_beforeimages(&profile, &target_root, owner, &mut validate)?;
        validate()?;
        let mut next = self.current.clone();
        next.native_publication.as_mut().ok_or_else(|| crate::err("TRUST_INVALID"))?.state = NativePublicationState::RestoredGeneratedInactive;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| crate::err("TRUST_CLOCK_ROLLBACK"))?.as_secs();
        self.commit(next, now).map_err(|e| crate::err(e.code()))?;
        self.check_root().map_err(|e| crate::err(e.code()))?;
        let bytes = read_record(&self.root.join(format!("{:020}.json", self.current.generation))).map_err(|e| crate::err(e.code()))?;
        let recorded: Record = serde_json::from_slice(&bytes).map_err(|_| crate::err("TRUST_INVALID"))?;
        let mut restored = binding.clone(); restored.state = NativePublicationState::RestoredGeneratedInactive;
        if hash(&bytes) != self.current_sha256 || recorded.native_publication.as_ref() != Some(&restored) { return Err(crate::err("TRUST_WRITE_UNCERTAIN")); }
        Ok(NativePublicationRepairReceipt { operation_id: binding.operation_id, state: "RestoredGeneratedInactive", authority_sha256: self.current_sha256.clone(), execution_permitted: false })
    }
    #[cfg(test)]
    pub(crate) fn begin_native_configuration_publication_test_clock(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, held: NativeTargetConfigurationPlan,
        source_session: &mut Session, target_session: &mut Session, now: u64,
    ) -> crate::Result<NativePublicationPermit> {
        self.begin_native_configuration_publication_with_clock(admitted, source, preparation, held, source_session, target_session, &|| Ok(now))
    }
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
        let (evidence,attempt_id)=source.source.prepare_target_copy(source_session,target_session,&admitted.release.release.artifact.schema_sha256.clone(),&operation,authority,|profile,id,source_root,target_root| {
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
        Ok(NativeTargetPreparation {operation_id:operation,attempt_id,observation})
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
    _authority: SourceAuthority,
    _attempt_id: String,
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
/// A postcommit in-process capability. It cannot be deserialized or converted
/// into preflight, file-publication, activation or execution authority.
pub struct NativePublicationPermit {
    receipt: NativePublicationPermitReceipt,
    _plan: installations::native_restore::ReboundConfigurationPlan,
    _staged: installations::native_restore::StagedNativeConfiguration,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePublicationPermitReceipt {
    operation_id: String,
    state: &'static str,
    authority_generation: u64,
    authority_sha256: String,
    configuration_manifest_sha256: String,
    beforeimage_archive_sha256: String,
    configuration_published: bool,
    preflight_verified: bool,
    execution_permitted: bool,
}
impl NativePublicationPermit {
    pub fn receipt(&self) -> &NativePublicationPermitReceipt { &self.receipt }
    #[cfg(test)]
    pub(crate) fn simulate_partial_for_test(&self, count: usize) -> crate::Result<()> { self._plan.simulate_partial_for_test(count) }
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
        let configuration = source.source.plan_target_configuration(source_session, target_session, &operation, &preparation.attempt_id, &expected, &authority, |profile, id, source_root, target_root| {
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
        Ok(NativeTargetConfigurationPlan {receipt, _configuration: configuration,
            _authority: SourceAuthority::capture(self, admitted), _attempt_id: preparation.attempt_id.clone()})
    }

    /// Consume the exact in-process postcommit capability and publish only its
    /// inactive configuration. An interrupted write retains the pending binding
    /// and marker for separately authenticated offline repair.
    pub fn publish_native_configuration(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, permit: NativePublicationPermit,
        source_session: &mut Session, target_session: &mut Session,
    ) -> crate::Result<NativeConfigurationPublishedReceipt> {
        self.publish_native_configuration_with_clock(admitted, source, preparation, permit, source_session, target_session,
            &|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_secs()).map_err(|_| crate::err("TRUST_CLOCK_ROLLBACK")))
    }
    #[cfg(test)]
    pub(crate) fn publish_native_configuration_test_clock(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, permit: NativePublicationPermit,
        source_session: &mut Session, target_session: &mut Session, now: u64,
    ) -> crate::Result<NativeConfigurationPublishedReceipt> {
        self.publish_native_configuration_with_clock(admitted, source, preparation, permit, source_session, target_session, &|| Ok(now))
    }
    fn publish_native_configuration_with_clock(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, permit: NativePublicationPermit,
        source_session: &mut Session, target_session: &mut Session, clock: &dyn Fn() -> crate::Result<u64>,
    ) -> crate::Result<NativeConfigurationPublishedReceipt> {
        use std::cell::RefCell;
        use super::super::native_publication_history::NativePublicationState;
        let binding = self.current.native_publication.clone().filter(|b| b.pending() && b.current_predecessor(&self.current))
            .ok_or_else(|| crate::err("UPDATE_POLICY_STALE"))?;
        let head = self.current_sha256.clone();
        if permit.receipt.authority_generation != self.current.generation || permit.receipt.authority_sha256 != head
            || permit.receipt.operation_id != binding.operation_id || preparation.operation_id != binding.operation_id
            || preparation.attempt_id != binding.attempt_id || source.authority.generation != binding.authority_generation
            || source.authority.policy_generation != binding.policy_generation || source.authority.authority_sha256 != binding.authority_sha256
            || source.authority.scope != self.scope || source.authority.target_payload_sha256 != binding.release_payload_sha256
            || source.authority.target_manifest_sha256 != binding.target_payload_sha256
            || permit.receipt.configuration_manifest_sha256 != binding.configuration_manifest_sha256
            || permit.receipt.beforeimage_archive_sha256 != binding.beforeimage_archive_sha256 { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let source_root = source_session.target_observation_root().to_owned();
        let target_root = preparation.observation.root.clone();
        if hash(source_root.as_os_str().as_encoded_bytes()) != binding.source_root_sha256
            || hash(target_root.as_os_str().as_encoded_bytes()) != binding.target_root_sha256
            || preparation.observation.source_instance != binding.source_instance || preparation.observation.target_instance != binding.target_instance { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
        let authority = serde_json::json!({"scope":self.scope,"generation":binding.authority_generation,"policyGeneration":binding.policy_generation,"recordSha256":binding.authority_sha256,"signedReleasePayloadSha256":binding.release_payload_sha256,"signedArtifactSha256":binding.artifact_sha256});
        let expected = preparation.observation.evidence.clone();
        let store = RefCell::new(self); let admission = RefCell::new(admitted);
        let (_, (published_head, operation)) = source.source.with_target_configuration(source_session, target_session,
            &binding.operation_id, &binding.attempt_id, &expected, &authority,
            |profile, id, from, to| {
                let store = store.borrow(); let mut admitted = admission.borrow_mut();
                check_durable_head(&store)?;
                if store.current_sha256 != head || store.current.native_publication.as_ref() != Some(&binding) || !binding.current_predecessor(&store.current)
                    || id != binding.source_instance || from != source_root || to != target_root
                    || admitted.scope != store.scope || admitted.generation != binding.authority_generation
                    || admitted.policy_generation != binding.policy_generation || admitted.authority_sha256 != binding.authority_sha256 { return Err(crate::err("UPDATE_POLICY_STALE")); }
                let mut fresh = store.verify_native(&admitted.envelope, clock()?).map_err(|e| crate::err(e.code()))?;
                admitted.staged.reverify(&mut fresh, clock()?).map_err(|e| crate::err(e.code()))?;
                admitted.payload.recheck().map_err(|_| crate::err("UPDATE_PLAN_MISMATCH"))?;
                if fresh.payload_sha256 != binding.release_payload_sha256 || fresh.release.artifact.sha256 != binding.artifact_sha256
                    || admitted.payload.context().manifest_sha256 != binding.target_payload_sha256 { return Err(crate::err("UPDATE_PLAN_MISMATCH")); }
                store.check_native_target_selection(profile, id, from, &binding.target_instance, to)
            },
            |fresh, profile, root, directory, key_file, revalidate| {
                permit._plan.recheck()?; permit._staged.recheck()?;
                if !permit._plan.matches(fresh) { return Err(crate::err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
                let key_parent = key_file.parent().ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
                let key_name = key_file.file_name().and_then(|v| v.to_str()).ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
                let key = crate::installations::native_backup::Secret(crate::installation_backup::source_bytes(key_parent, key_name, 32, true)?);
                let (bytes, key_pin) = crate::hosted_runtime_file::bounded(key_file, 32)?;
                let bytes = crate::installations::native_backup::Secret(bytes);
                if key.0.len() != 32 || key.0 != bytes.0 { return Err(crate::err("BACKUP_KEY_INVALID")); }
                let staged = directory.join(format!("publication-{}", binding.attempt_id));
                let before = installations::native_restore::open_beforeimages(&staged, &binding.operation_id, &binding.attempt_id, &key.0,
                    &binding.configuration_manifest_sha256, &binding.beforeimage_archive_sha256)?;
                let owner = before.owner()?;
                revalidate(true, true)?;
                before.publish_afterimages(&permit._plan, profile, root, owner, || {
                    revalidate(true, false)?; permit._plan.recheck_source()?; permit._staged.recheck()?; key_pin.check()
                })?;
                revalidate(true, false)?; permit._plan.recheck_source()?; permit._staged.recheck()?; key_pin.check()?;
                let mut store = store.borrow_mut(); let admitted = admission.borrow();
                let now = clock()?;
                let fresh_release = store.verify_native(&admitted.envelope, now).map_err(|e| crate::err(e.code()))?;
                if fresh_release.payload_sha256 != binding.release_payload_sha256 || store.current_sha256 != head
                    || store.current.native_publication.as_ref() != Some(&binding) { return Err(crate::err("UPDATE_POLICY_STALE")); }
                let mut next = store.current.clone(); next.native_publication.as_mut().ok_or_else(|| crate::err("TRUST_INVALID"))?.state = NativePublicationState::PublishedInactive;
                store.commit(next, now).map_err(|e| crate::err(e.code()))?;
                store.check_root().map_err(|e| crate::err(e.code()))?;
                let bytes = read_record(&store.root.join(format!("{:020}.json", store.current.generation))).map_err(|e| crate::err(e.code()))?;
                let record: Record = serde_json::from_slice(&bytes).map_err(|_| crate::err("TRUST_INVALID"))?;
                let mut published = binding.clone(); published.state = NativePublicationState::PublishedInactive;
                if hash(&bytes) != store.current_sha256 || record.native_publication.as_ref() != Some(&published) { return Err(crate::err("TRUST_WRITE_UNCERTAIN")); }
                revalidate(false, false)?; permit._plan.recheck_source()?; permit._staged.recheck()?; key_pin.check()?;
                Ok((store.current_sha256.clone(), binding.operation_id.clone()))
            })?;
        Ok(NativeConfigurationPublishedReceipt { operation_id: operation, state: "PublishedInactive", authority_sha256: published_head, preflight_verified: false, execution_permitted: false })
    }

    /// Commit a source-bound inactive publication intent. This stages only an
    /// authenticated beforeimage archive and a private manifest; it never
    /// replaces target configuration, starts services or permits execution.
    pub fn begin_native_configuration_publication(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, held: NativeTargetConfigurationPlan,
        source_session: &mut Session, target_session: &mut Session,
    ) -> crate::Result<NativePublicationPermit> {
        self.begin_native_configuration_publication_with_clock(admitted, source, preparation, held, source_session, target_session, &|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).map_err(|_| crate::err("TRUST_CLOCK_ROLLBACK"))
        })
    }

    fn begin_native_configuration_publication_with_clock(
        &mut self, admitted: &mut AdmittedNativePayload, source: &mut NativeSourceObservation,
        preparation: &mut NativeTargetPreparation, held: NativeTargetConfigurationPlan,
        source_session: &mut Session, target_session: &mut Session, clock: &dyn Fn() -> crate::Result<u64>,
    ) -> crate::Result<NativePublicationPermit> {
        use std::cell::RefCell;
        use super::super::native_publication_history::{NativePublicationBinding, NativePublicationState};
        if self.current.intent.is_some() || self.current.native_publication.as_ref().is_some_and(|v| v.in_flight()) {
            return Err(crate::err("TRUST_OPERATION_BUSY"));
        }
        self.recheck_native_target(admitted, source, &mut preparation.observation, source_session, target_session, clock()?).map_err(|e| crate::err(e.code()))?;
        let operation = preparation.operation_id.clone();
        let attempt = preparation.attempt_id.clone();
        let target_id = preparation.observation.target_instance.clone();
        let source_id = preparation.observation.source_instance.clone();
        let target_root = preparation.observation.root.clone();
        let source_root = source_session.target_observation_root().to_owned();
        let expected = preparation.observation.evidence.clone();
        if !held._authority.matches(self, admitted) || held._attempt_id != attempt
            || held.receipt.operation_id != operation || held.receipt.target_instance != target_id
            || held.receipt.source_instance != source_id || held.receipt.authenticated_backup_manifest_sha256 != expected.authenticated_manifest_sha256
            || held.receipt.source_payload_manifest_sha256 != source.source.source_manifest()
            || held.receipt.target_payload_manifest_sha256 != admitted.payload.context().manifest_sha256
            || self.used_operations.contains(&operation) || self.used_instances.contains(&target_id) {
            return Err(crate::err("UPDATE_PLAN_MISMATCH"));
        }
        held._configuration.recheck()?;
        let authority = serde_json::json!({"scope":self.scope,"generation":self.current.generation,"policyGeneration":self.current.policy_generation,"recordSha256":self.current_sha256,"signedReleasePayloadSha256":admitted.release.payload_sha256,"signedArtifactSha256":admitted.release.release.artifact.sha256});
        let source_authority = source.authority.clone();
        let source_evidence = source.evidence.clone();
        let source_manifest = source.source.source_manifest();
        let store = RefCell::new(self);
        let admission = RefCell::new(admitted);
        let (fresh, (staged, binding, generation, head)) = source.source.with_target_configuration(
            source_session, target_session, &operation, &attempt, &expected, &authority,
            |profile, id, from, to| {
                let store = store.borrow(); let mut admission = admission.borrow_mut();
                store.recheck_native_admission(&mut admission, clock()?).map_err(|e| crate::err(e.code()))?;
                if !source_authority.matches(&store, &admission) || to != target_root { return Err(crate::err("UPDATE_POLICY_STALE")); }
                store.check_native_target_selection(profile, id, from, &target_id, to)
            },
            |fresh, profile, root, directory, key_file, revalidate| {
                held._configuration.recheck()?;
                if !held._configuration.matches(fresh) { return Err(crate::err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
                let parent = key_file.parent().ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
                let name = key_file.file_name().and_then(|v| v.to_str()).ok_or_else(|| crate::err("BACKUP_KEY_INVALID"))?;
                let key = crate::installations::native_backup::Secret(crate::installation_backup::source_bytes(parent, name, 32, true)?);
                let (bounded_key, key_pin) = crate::hosted_runtime_file::bounded(key_file, 32)?;
                let bounded_key = crate::installations::native_backup::Secret(bounded_key);
                if key.0.len() != 32 || key.0 != bounded_key.0 { return Err(crate::err("BACKUP_KEY_INVALID")); }
                let staged = fresh.stage_beforeimages(profile, root, directory, &operation, &attempt, &key.0)?;
                revalidate(true, true)?; held._configuration.recheck()?; staged.recheck()?; key_pin.check()?;
                {
                    let store = store.borrow(); let mut admission = admission.borrow_mut();
                    store.recheck_native_admission(&mut admission, clock()?).map_err(|e| crate::err(e.code()))?;
                    if !source_authority.matches(&store, &admission) || store.used_operations.contains(&operation) || store.used_instances.contains(&target_id) {
                        return Err(crate::err("UPDATE_POLICY_STALE"));
                    }
                }
                revalidate(true, true)?; held._configuration.recheck()?; staged.recheck()?; key_pin.check()?;
                let mut store = store.borrow_mut(); let admission = admission.borrow_mut();
                let now = clock()?;
                let verified = store.verify_native(&admission.envelope, now).map_err(|e| crate::err(e.code()))?;
                if verified.payload_sha256 != admission.release.payload_sha256 || !source_authority.matches(&store, &admission) {
                    return Err(crate::err("UPDATE_POLICY_STALE"));
                }
                let binding = NativePublicationBinding {
                    format: 1, operation_id: operation.clone(), attempt_id: attempt.clone(), source_instance: source_id.clone(), target_instance: target_id.clone(),
                    source_root_sha256: hash(source_root.as_os_str().as_encoded_bytes()), target_root_sha256: hash(root.as_os_str().as_encoded_bytes()),
                    source_payload_sha256: source_manifest.clone(), target_payload_sha256: admission.payload.context().manifest_sha256,
                    release_payload_sha256: admission.release.payload_sha256.clone(), artifact_sha256: admission.release.release.artifact.sha256.clone(),
                    backup_id: source_evidence.backup_id.clone(), backup_manifest_sha256: source_evidence.manifest_sha256.clone(), source_inventory_sha256: source_evidence.inventory_sha256.clone(),
                    source_schema_sha256: source_evidence.schema_sha256.clone(), target_schema_sha256: expected.target_schema_sha256.clone(), target_migrations_sha256: expected.target_migrations_sha256.clone(),
                    source_system_identifier: source_evidence.system_identifier.clone(), target_system_identifier: expected.target_system_identifier.clone(),
                    configuration_manifest_sha256: staged.manifest_sha256.clone(), beforeimage_archive_sha256: staged.beforeimage_archive_sha256.clone(),
                    authority_generation: store.current.generation, authority_sha256: store.current_sha256.clone(), policy_generation: store.current.policy_generation,
                    state: NativePublicationState::Pending,
                };
                let mut next = store.current.clone(); next.update_event = None; next.native_publication = Some(binding.clone());
                store.commit(next, now).map_err(|e| crate::err(e.code()))?;
                // The old admission is now stale. Read back the actual durable
                // record and retain only a new in-process, postcommit permit.
                store.check_root().map_err(|e| crate::err(e.code()))?;
                let bytes = read_record(&store.root.join(format!("{:020}.json", store.current.generation))).map_err(|e| crate::err(e.code()))?;
                let recorded: Record = serde_json::from_slice(&bytes).map_err(|_| crate::err("TRUST_INVALID"))?;
                if hash(&bytes) != store.current_sha256 || recorded.native_publication.as_ref() != Some(&binding) { return Err(crate::err("TRUST_WRITE_UNCERTAIN")); }
                revalidate(false, true)?; held._configuration.recheck()?; staged.recheck()?; key_pin.check()?;
                store.check_native_target_selection(profile, &source_id, &source_root, &target_id, root)?;
                Ok((staged, binding, store.current.generation, store.current_sha256.clone()))
            },
        )?;
        let receipt = NativePublicationPermitReceipt {
            operation_id: operation, state: "PublicationIntentInactive", authority_generation: generation, authority_sha256: head,
            configuration_manifest_sha256: binding.configuration_manifest_sha256,
            beforeimage_archive_sha256: binding.beforeimage_archive_sha256,
            configuration_published: false, preflight_verified: false, execution_permitted: false,
        };
        Ok(NativePublicationPermit { receipt, _plan: fresh, _staged: staged })
    }
}
