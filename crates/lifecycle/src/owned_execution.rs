// SPDX-License-Identifier: Apache-2.0
//! Existing registered source adapters under the borrowed exclusive trust fence.
use super::*;

// Fixed qualification stages only. Preserve every existing safety/refusal code;
// a generic Engine failure identifies this call boundary, not its root cause.
#[derive(Clone, Copy)]
enum RecoveryEngineStage {
    SourceImageExport,
    SourceConfiguration,
    SourceInventory,
    CandidatePreflight,
}
fn recovery_engine_failure(error: crate::LifecycleError, stage: RecoveryEngineStage) -> crate::LifecycleError {
    if error.code != "ENGINE_OPERATION_FAILED" {
        return error;
    }
    crate::err(match stage {
        RecoveryEngineStage::SourceImageExport => "UPDATE_SOURCE_IMAGE_EXPORT_ENGINE_FAILED",
        RecoveryEngineStage::SourceConfiguration => "UPDATE_SOURCE_CONFIGURATION_ENGINE_FAILED",
        RecoveryEngineStage::SourceInventory => "UPDATE_SOURCE_INVENTORY_ENGINE_FAILED",
        RecoveryEngineStage::CandidatePreflight => "UPDATE_CANDIDATE_PREFLIGHT_ENGINE_FAILED",
    })
}

#[cfg(test)]
fn native_test_image() -> String {
    native_test_fixture_image(
        "EXHIBITOS_NATIVE_TEST_IMAGE",
        "sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06",
    )
}

#[cfg(test)]
fn native_test_platform_image() -> String {
    native_test_fixture_image(
        "EXHIBITOS_NATIVE_TEST_PLATFORM_IMAGE",
        "sha256:335f8f2c1437841266c41e79912b1160b03ce500511acc94afa338c4c8f6215b",
    )
}

#[cfg(test)]
fn native_test_fixture_image(variable: &str, default: &str) -> String {
    let image = std::env::var(variable).unwrap_or_else(|_| default.into());
    assert!(
        image.starts_with("sha256:")
            && image.len() == 71
            && image[7..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let row = crate::backup_creation::inspected(
        "docker",
        &["image".into(), "inspect".into(), image.clone()],
    )
    .unwrap();
    assert_eq!(row["Id"], image);
    image
}

#[path = "candidate_configuration.rs"]
mod candidate_configuration;
#[path = "candidate_recovery.rs"]
mod candidate_recovery;
pub use candidate_recovery::{CandidateRecoveryReceipt, EphemeralCandidateRecoveryReceipt};
#[path = "candidate_inventory.rs"]
mod candidate_inventory;
pub use candidate_configuration::CandidateConfigurationReceipt;
pub use candidate_inventory::CandidateInventoryReceipt;
#[path = "migration_catalog.rs"]
mod migration_catalog;
/// Native input discovery only; the full qualifier must retain and recheck it.
#[cfg(unix)]
pub(crate) fn validate_preparation_catalog(path:&Path,pin:&str,signed_schema:&str)->crate::Result<()> {
 let mut input=migration_catalog::CatalogInput::read(path,pin,signed_schema)?;input.recheck()
}
#[path = "prepared_oci.rs"]
mod prepared_oci;
#[path = "runtime_compatibility.rs"]
mod runtime_compatibility;
use crate::{LifecycleService, Status, maintenance::VerificationReceipt};
pub use prepared_oci::PreparedOciReceipt;
pub use runtime_compatibility::RuntimeCompatibility;
#[cfg(any(unix, windows))]
#[path = "interrupted_recovery.rs"]
mod interrupted_recovery;
#[cfg(any(unix, windows))]
pub use interrupted_recovery::InterruptedRecoveryInputs;
#[path = "current_recovery_runtime.rs"]
mod current_recovery_runtime;
pub use current_recovery_runtime::{
    CurrentRecoveryRuntime, MigrationRuntimeInputs, RecoveryRuntimeInputs,
};
#[cfg(any(unix, windows))]
pub use current_recovery_runtime::{OwnedPreflight, ReadyCandidate, StartedUpdate};
#[path = "source_stopped.rs"]
mod source_stopped;
pub use source_stopped::SourceStoppedReceipt;
#[path = "source_deployment.rs"]
pub(crate) mod source_deployment;
pub use source_deployment::SourceDeploymentReceipt;
#[path = "source_configuration.rs"]
mod source_configuration;
pub use source_configuration::NativeConfigurationReceipt;
#[path = "source_images.rs"]
pub(crate) mod source_images;
pub use source_images::SourceImageReceipt;
#[path = "source_database.rs"]
mod source_database;
pub use source_database::DatabaseSnapshotReceipt;
#[path = "combined_recovery.rs"]
mod combined_recovery;
#[path = "ephemeral_inventory.rs"]
mod ephemeral_inventory;
#[path = "ephemeral_source_recovery.rs"]
mod ephemeral_source_recovery;
pub use combined_recovery::{CheckpointInputs, CombinedRecoveryReceipt};
pub use ephemeral_source_recovery::EphemeralSourceRecoveryReceipt;
#[cfg(any(unix, windows))]
pub(super) mod native_inventory;
#[path = "source_inventory.rs"]
mod source_inventory;
pub use ephemeral_inventory::EphemeralCandidateInventoryReceipt;
pub use source_inventory::SourceInventoryReceipt;
#[path = "recovery_space.rs"]
mod recovery_space;
#[path = "source_full_configuration.rs"]
mod source_full_configuration;
#[path = "source_image_bytes.rs"]
mod source_image_bytes;
pub use source_full_configuration::ConfigurationInventoryReceipt;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecoveryReceipt {
    pub inventory: SourceInventoryReceipt,
    pub configuration: ConfigurationInventoryReceipt,
    pub repeated_inventory: SourceInventoryReceipt,
    pub preflight_verified: bool,
    pub update_executed: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHostTrustReceipt {
    pub host: profile_backup::HostReceipt,
    pub trust: TrustCheckpointReceipt,
    pub source: SourceRecoveryReceipt,
    pub after_archive_inventory: SourceInventoryReceipt,
    pub after_archive_configuration: ConfigurationInventoryReceipt,
    pub external_volumes_saved: bool,
    pub live_authority_restored: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
#[cfg(any(unix, windows))]
pub(super) fn check_rollback_configuration(
    image: &str,
    volume: &str,
    manifest: &[u8],
) -> crate::Result<()> {
    let expected = source_configuration::expected(manifest)?;
    source_configuration::observe(image, volume, &expected).map(|_| ())
}
#[cfg(any(unix, windows))]
pub(super) fn check_rollback_inventory(
    proof: &source_inventory::InventoryProof,
    plan: &crate::update::Plan,
) -> crate::Result<()> {
    source_inventory::matched_candidate(proof, plan)
}
/// Holds the Store borrow and both profile fences. No arbitrary root, controller
/// bootstrap, activation, journal transition, or externally supplied success flag.
#[cfg(not(windows))]
pub(super) type DirectoryIdentity = Metadata;
#[cfg(windows)]
pub(super) type DirectoryIdentity = crate::windows_private::HostDirectory;
pub(super) fn directory_identity(path: impl AsRef<Path>) -> crate::Result<DirectoryIdentity> {
    let path = path.as_ref();
    #[cfg(not(windows))]
    { fs::symlink_metadata(path).map_err(|_| crate::err("UPDATE_TARGET_CHANGED")) }
    #[cfg(windows)]
    {
        let directory = crate::windows_private::HostDirectory::inspect(path)?;
        directory.check_private()?;
        Ok(directory)
    }
}
pub(super) fn directory_matches(expected: &DirectoryIdentity, path: impl AsRef<Path>) -> crate::Result<bool> {
    let path = path.as_ref();
    #[cfg(not(windows))]
    { Ok(identity(expected, &fs::symlink_metadata(path).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?)) }
    #[cfg(windows)]
    {
        expected.check_private()?;
        Ok(expected.path() == path)
    }
}
pub(super) fn same_directory_identity(a: &DirectoryIdentity, b: &DirectoryIdentity) -> crate::Result<bool> {
    #[cfg(not(windows))]
    { Ok(identity(a, b)) }
    #[cfg(windows)]
    { a.check_private()?; b.check_private()?; Ok(a.path() == b.path()) }
}
pub struct ExecutionSession<'a> {
    store: &'a mut Store,
    source: LifecycleService,
    profile_identity: DirectoryIdentity,
    source_identity: DirectoryIdentity,
    registry: Vec<u8>,
    _session: profile_backup::ProfileSession,
}
/// Internal rollback bridge to the existing authenticated five-file comparison.
pub(super) fn check_restored_source_files(
    source: &Path,
    workspace: &Path,
    receipt: &crate::restoration::RestorationReceipt,
    plan: &crate::update::Plan,
) -> crate::Result<()> {
    source_deployment::authenticated_files(source, workspace, receipt, plan).map(|_| ())
}
impl Store {
    pub fn execution(&mut self) -> crate::Result<ExecutionSession<'_>> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        installations::private_directory(&self.profile)?;
        // Clone the existing open file description; never re-lock, downgrade or
        // explicitly unlock it. The Store borrow keeps the exclusive owner alive.
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| crate::err("UPDATE_FENCE_UNAVAILABLE"))?;
        let session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let (registry, bytes) = installations::load(&self.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let bound_source = self.bound_source_id(&registry)?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == bound_source)
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let intent = self
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if entry.id != intent.update.plan().source_instance {
            return Err(crate::err("UPDATE_SOURCE_MISMATCH"));
        }
        let root = installations::root(&self.profile, entry);
        installations::private_directory(&root)?;
        let profile_identity =
            directory_identity(&self.profile).map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let source_identity =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let source = LifecycleService::open_retry_diagnostics(root)?;
        let lease = ExecutionSession {
            store: self,
            source,
            profile_identity,
            source_identity,
            registry: bytes,
            _session: session,
        };
        lease.check()?;
        Ok(lease)
    }
}
/// Opaque retained bytes for the executor. This is not a preflight or apply permit.
pub struct PreparedArtifact {
    staged: crate::signed_release::artifact::StagedArtifact,
    verified: VerifiedRelease,
    plan: crate::update::Plan,
    generation: u64,
    envelope: String,
    profile: PathBuf,
    profile_identity: DirectoryIdentity,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedArtifactReceipt {
    pub operation_id: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub staged_path: PathBuf,
    pub trust_generation: u64,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
// Lifecycle log timestamps use milliseconds; signed release times are Unix seconds.
pub(super) fn release_now() -> crate::Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| crate::err("UPDATE_CLOCK_UNVERIFIED"))
}
/// Raw pre-stop ownership observations, not a fabricated stopped-state receipt.
#[cfg(not(windows))]
fn candidate_stop_isolated(
    source: &SourceStoppedReceipt,
    bundle: &str,
    project: &str,
    source_project: &str,
    containers: [&str; 2],
    volumes: [&str; 3],
) -> crate::Result<()> {
    let source_containers = [
        source.platform_container.as_str(),
        source.database_container.as_str(),
    ];
    let source_volumes = [
        source.blob_volume.as_str(),
        source.configuration_volume.as_str(),
        source.database_volume.as_str(),
    ];
    if bundle == source.bundle_id
        || project == source_project
        || containers[0] == containers[1]
        || containers.iter().any(|id| source_containers.contains(id))
        || volumes[0] == volumes[1]
        || volumes[0] == volumes[2]
        || volumes[1] == volumes[2]
        || volumes.iter().any(|name| source_volumes.contains(name))
    {
        return Err(crate::err("UPDATE_TARGET_CHANGED"));
    }
    Ok(())
}
impl ExecutionSession<'_> {
    /// Stage the current intent's exact signed artifact under the borrowed execution
    /// fence. No caller policy/envelope/plan or success booleans are accepted.
    pub fn stage_prepared_artifact(
        &self,
        source: &Path,
        parent: &Path,
    ) -> crate::Result<PreparedArtifact> {
        let mut artifact = self.stage_prepared_artifact_at(source, parent, release_now()?)?;
        self.reverify_prepared_artifact(&mut artifact)?;
        Ok(artifact)
    }
    fn stage_prepared_artifact_at(
        &self,
        source: &Path,
        parent: &Path,
        now: u64,
    ) -> crate::Result<PreparedArtifact> {
        self.check()?;
        if parent.starts_with(&self.store.profile) || parent.starts_with(&self.store.root) {
            return Err(crate::err("UPDATE_ARTIFACT_UNAVAILABLE"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan().clone();
        let envelope = intent.envelope.clone();
        let mut verified = self
            .store
            .verify_for_preparation(envelope.as_bytes(), now)
            .map_err(|e| crate::err(e.code()))?;
        if !verified.binds(&plan)
            || verified.payload_sha256 != intent.payload_sha256
            || verified.key_id != intent.key_id
            || verified.release.artifact.sha256 != intent.artifact_sha256
        {
            return Err(crate::err("UPDATE_PLAN_MISMATCH"));
        }
        let _source_lock = self.source.lock()?;
        let staged = crate::signed_release::artifact::stage(&mut verified, source, parent, now)
            .map_err(|e| crate::err(e.code()))?;
        let mut result = PreparedArtifact {
            staged,
            verified,
            plan,
            generation: self.store.receipt().generation,
            envelope,
            profile: self.store.profile.clone(),
            profile_identity: self.profile_identity.clone(),
        };
        self.reverify_prepared_artifact_at(&mut result, now)?;
        self.check()?;
        Ok(result)
    }
    /// Revalidate current trust/time/plan and the retained handle before later use.
    /// A returned receipt deliberately never authorizes an Applying transition.
    pub fn reverify_prepared_artifact(
        &self,
        artifact: &mut PreparedArtifact,
    ) -> crate::Result<PreparedArtifactReceipt> {
        self.reverify_prepared_artifact_at(artifact, release_now()?)
    }
    fn reverify_prepared_artifact_at(
        &self,
        artifact: &mut PreparedArtifact,
        now: u64,
    ) -> crate::Result<PreparedArtifactReceipt> {
        self.check()?;
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if artifact.profile != self.store.profile
            || !same_directory_identity(&artifact.profile_identity, &self.profile_identity).map_err(|_|crate::err("UPDATE_OPERATION_STALE"))?
            || intent.update.stage() != crate::update::Stage::Prepared
            || intent.update.plan() != &artifact.plan
            || intent.envelope != artifact.envelope
            || self.store.receipt().generation != artifact.generation
        {
            return Err(crate::err("UPDATE_OPERATION_STALE"));
        }
        let mut fresh = self
            .store
            .verify_for_preparation(intent.envelope.as_bytes(), now)
            .map_err(|e| crate::err(e.code()))?;
        if !fresh.binds(&artifact.plan)
            || fresh.payload_sha256 != artifact.verified.payload_sha256
            || fresh.key_id != artifact.verified.key_id
        {
            return Err(crate::err("UPDATE_PLAN_MISMATCH"));
        }
        artifact
            .staged
            .reverify(&mut fresh, now)
            .map_err(|e| crate::err(e.code()))?;
        self.check()?;
        artifact.verified = fresh;
        Ok(PreparedArtifactReceipt {
            operation_id: artifact.plan.operation_id.clone(),
            artifact_sha256: artifact.verified.release.artifact.sha256.clone(),
            artifact_bytes: artifact.verified.release.artifact.bytes,
            staged_path: artifact.staged.path().to_owned(),
            trust_generation: artifact.generation,
            preflight_verified: false,
            update_executed: false,
        })
    }
    fn check(&self) -> crate::Result<()> {
        self.store.check_root().map_err(|e| crate::err(e.code()))?;
        installations::private_directory(&self.store.profile)?;
        installations::private_directory(&self.source.root)?;
        let (_, bytes) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        if !directory_matches(&self.profile_identity, &self.store.profile).map_err(|_|crate::err("UPDATE_SOURCE_CHANGED"))?
            || !directory_matches(&self.source_identity, &self.source.root).map_err(|_|crate::err("UPDATE_SOURCE_CHANGED"))?
            || bytes != self.registry
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        Ok(())
    }
    #[cfg(any(unix, windows))]
    fn require_selected_source(&self) -> crate::Result<()> {
        self.check()?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let bound = self.store.bound_source_id(&registry)?;
        let source = &self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
            .update
            .plan()
            .source_instance;
        if registry.active_id != bound || source != &bound {
            return Err(crate::err("UPDATE_SELECTION_AUTHORITY_MISMATCH"));
        }
        Ok(())
    }
    /// Observed service status, not plan-bound update health or preflight proof.
    pub fn source_status(&self) -> crate::Result<Status> {
        self.check()?;
        let result = {
            let _lock = self.source.lock()?;
            self.source.status()
        };
        self.check()?;
        result
    }
    /// Fresh direct engine observations; not a current data snapshot or full preflight.
    pub fn verify_source_stopped(
        &self,
        external_writers_quiesced: bool,
    ) -> crate::Result<SourceStoppedReceipt> {
        self.check()?;
        if !external_writers_quiesced {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let _lock = self.source.lock()?;
        let result = source_stopped::observe(
            &self.source,
            self.store
                .intent()
                .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                .update
                .plan(),
        );
        self.check()?;
        result
    }
    /// Current host deployment bytes against the exact authenticated restored backup.
    /// DB/blob/config-volume equality and full preflight remain separate.
    pub fn verify_source_deployment(
        &self,
        acknowledged: bool,
    ) -> crate::Result<SourceDeploymentReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceDeploymentReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                files,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Current native freeze key only; full configuration/image/data gates remain.
    pub fn verify_source_configuration(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<NativeConfigurationReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let expected = source_configuration::expected(&raw)?;
            let observed =
                source_configuration::observe(image, &before.configuration_volume, &expected)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(NativeConfigurationReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                configuration_volume: before.configuration_volume,
                maintenance_image: image.into(),
                file: observed,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Complete supported configuration scope with newly exported current image bytes.
    pub fn verify_configuration_inventory(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_scoped(image, acknowledged, false)
    }
    /// Fresh full verification with an explicit successful-export retirement policy.
    /// Failure candidates and small verified image receipts remain private.
    pub fn verify_configuration_inventory_transient(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_at(image, acknowledged, false, None, false)
    }
    fn verify_configuration_inventory_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.verify_configuration_inventory_at(image, acknowledged, roots_held, None, true)
    }
    fn verify_configuration_inventory_at(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
        export_parent: Option<&Path>,
        retain_images: bool,
    ) -> crate::Result<ConfigurationInventoryReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target_lock = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let manifest = source_full_configuration::scope(&raw)?;
            let expected = source_configuration::expected(&raw)?;
            let observed =
                source_configuration::observe(image, &before.configuration_volume, &expected)?;
            let mappings = source_images::observe(&self.source, &workspace, &raw)?;
            let inventory_bytes = crate::installation_backup::source_bytes(
                &workspace.join("configuration"),
                "manager-image-inventory.json",
                1048576,
                true,
            )?;
            let preserved = source_images::bound_inventory(
                &inventory_bytes,
                &manifest,
                &self.source.manifest()?,
            )?;
            let exported = export_parent
                .unwrap_or(&self.source.root)
                .join(format!("source-image-observation-{}", uuid::Uuid::new_v4()));
            #[cfg(windows)]
            installations::new_directory(&exported)?;
            #[cfg(not(windows))]
            fs::create_dir(&exported).map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&exported, fs::Permissions::from_mode(0o700))
                    .map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
            }
            installations::private_directory(&exported)?;
            let images = source_image_bytes::export(&exported, &preserved)
                .map_err(|error| recovery_engine_failure(error, RecoveryEngineStage::SourceImageExport))?;
            let generated = source_full_configuration::generated_inventory(&images, &manifest)?;
            if source_configuration::observe(image, &before.configuration_volume, &expected)?
                != observed
                || source_images::observe(&self.source, &workspace, &raw)? != mappings
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            let full_files = source_full_configuration::files(&files, observed, generated)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(ConfigurationInventoryReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                configuration_volume: before.configuration_volume,
                files: full_files,
                images,
                export_workspace: exported.to_string_lossy().into_owned(),
                image_archives_retained: true,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let mut receipt = result?;
        if !retain_images {
            source_image_bytes::retire_verified(
                Path::new(&receipt.export_workspace),
                &receipt.images,
            )?;
            receipt.image_archives_retained = false;
        }
        Ok(receipt)
    }
    /// DB/blob before and after full configuration/image bytes under the same
    /// source+registered-target locks. Does not authorize Applying or prove recovery.
    pub fn verify_source_recovery_bundle(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.verify_source_recovery_scoped(image, acknowledged, false, true)
    }
    /// Full DB/blob/configuration observation; successful fresh image exports are
    /// retired only after the repeated data inventory and all root checks pass.
    pub fn verify_source_recovery_transient(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.verify_source_recovery_scoped(image, acknowledged, false, false)
    }
    fn verify_source_recovery_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
        retain_images: bool,
    ) -> crate::Result<SourceRecoveryReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let plan = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
            .update
            .plan();
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&root)?;
        let original =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let inventory = self.verify_source_inventory_scoped(image, true, true)?;
        let mut configuration = self.verify_configuration_inventory_scoped(image, true, true)?;
        let repeated_inventory = self.verify_source_inventory_scoped(image, true, true)?;
        if inventory.source_content_sha256 != repeated_inventory.source_content_sha256
            || inventory.inventory.inventory_sha256 != repeated_inventory.inventory.inventory_sha256
            || inventory.inventory.schema_sha256 != repeated_inventory.inventory.schema_sha256
            || inventory.inventory.authenticated_manifest_sha256
                != configuration.authenticated_manifest_sha256
            || inventory.source_instance != configuration.source_instance
            || inventory.target_instance != configuration.target_instance
            || inventory.source_database_volume != repeated_inventory.source_database_volume
            || inventory.source_blob_volume != repeated_inventory.source_blob_volume
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&original, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        if !retain_images {
            source_image_bytes::retire_verified(
                Path::new(&configuration.export_workspace),
                &configuration.images,
            )?;
            configuration.image_archives_retained = false;
        }
        Ok(SourceRecoveryReceipt {
            inventory,
            configuration,
            repeated_inventory,
            preflight_verified: false,
            update_executed: false,
        })
    }
    /// Cooperative host/root fences span fresh source observations, host+trust
    /// archives and a final read-only source comparison. External volumes are
    /// observed against the existing backup, not newly archived by this method.
    /// The completed pre-archive observation retires its fresh image exports;
    /// final recovery exports remain retained outside the host profile.
    pub fn checkpoint_source_host_trust(
        &self,
        image: &str,
        key_file: &Path,
        target: &Path,
        external_writers_quiesced: bool,
        host_writers_stopped: bool,
    ) -> crate::Result<SourceHostTrustReceipt> {
        self.check()?;
        if !external_writers_quiesced {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !host_writers_stopped {
            return Err(crate::err("HOST_WRITER_ACK_REQUIRED"));
        }
        let parent = target
            .parent()
            .ok_or_else(|| crate::err("PROFILE_PATH_INVALID"))?;
        if !target.is_absolute()
            || fs::canonicalize(parent).ok().as_deref() != Some(parent)
            || target.starts_with(&self.store.profile)
            || target.starts_with(&self.store.root)
            || target.exists()
        {
            return Err(crate::err("PROFILE_PATH_INVALID"));
        }
        installations::private_directory(parent)?;
        if !key_file.is_absolute()
            || fs::canonicalize(key_file).ok().as_deref() != Some(key_file)
            || key_file.starts_with(&self.store.profile)
            || key_file.starts_with(&self.store.root)
        {
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let mut key = read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?;
        if key.len() != 32 {
            key.fill(0);
            return Err(crate::err("PROFILE_KEY_INVALID"));
        }
        let result = (|| {
            let stage = parent.join(format!(
                "pending-source-host-trust-{}",
                uuid::Uuid::new_v4()
            ));
            #[cfg(windows)]
            installations::new_directory(&stage)?;
            #[cfg(not(windows))]
            {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&stage).map_err(|_|crate::err("HOST_WRITE_UNCERTAIN"))?;
            }
            let source = std::cell::RefCell::new(None);
            let (host, (), (trust, source, after_archive_inventory, after_archive_configuration)) =
                profile_backup::checkpoint_host_borrowed(
                    &self.store.profile,
                    key_file,
                    &stage.join("host.bin"),
                    &self._session,
                    || {
                        *source.borrow_mut() =
                            Some(self.verify_source_recovery_scoped(image, true, true, false)?);
                        Ok(())
                    },
                    || {
                        self.check()?;
                        if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))?
                            != key
                        {
                            return Err(crate::err("PROFILE_KEY_INVALID"));
                        }
                        let trust = self
                            .store
                            .archive_trust_checkpoint(
                                &stage.join("trust.bin"),
                                key.as_slice()
                                    .try_into()
                                    .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                            )
                            .map_err(|e| crate::err(e.code()))?;
                        let after = self.verify_source_inventory_scoped(image, true, true)?;
                        let source = source
                            .borrow_mut()
                            .take()
                            .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
                        let before = &source.repeated_inventory;
                        if before.source_content_sha256 != after.source_content_sha256
                            || before.inventory.inventory_sha256 != after.inventory.inventory_sha256
                            || before.inventory.schema_sha256 != after.inventory.schema_sha256
                            || before.inventory.authenticated_manifest_sha256
                                != after.inventory.authenticated_manifest_sha256
                            || before.source_instance != after.source_instance
                            || before.target_instance != after.target_instance
                            || before.source_database_volume != after.source_database_volume
                            || before.source_blob_volume != after.source_blob_volume
                        {
                            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
                        }
                        let after_configuration = self.verify_configuration_inventory_at(
                            image,
                            true,
                            true,
                            Some(&stage),
                            true,
                        )?;
                        let before_configuration = &source.configuration;
                        if before_configuration.configuration_volume
                            != after_configuration.configuration_volume
                            || serde_json::to_value(&before_configuration.files).ok()
                                != serde_json::to_value(&after_configuration.files).ok()
                            || serde_json::to_value(&before_configuration.images).ok()
                                != serde_json::to_value(&after_configuration.images).ok()
                            || before_configuration.authenticated_manifest_sha256
                                != after_configuration.authenticated_manifest_sha256
                        {
                            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
                        }
                        Ok((trust, source, after, after_configuration))
                    },
                )?;
            self.check()?;
            if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key {
                return Err(crate::err("PROFILE_KEY_INVALID"));
            }
            let mut after_archive_configuration = after_archive_configuration;
            let export_relative = Path::new(&after_archive_configuration.export_workspace)
                .strip_prefix(&stage)
                .map_err(|_| crate::err("UPDATE_SOURCE_CHANGED"))?;
            after_archive_configuration.export_workspace =
                target.join(export_relative).to_string_lossy().into_owned();
            self.store.seal_recovery_pair(
                &stage,
                key.as_slice()
                    .try_into()
                    .map_err(|_| crate::err("PROFILE_KEY_INVALID"))?,
                &host,
                Some(super::trust_checkpoint::binding::SourceBinding::from_plan(
                    self.store
                        .intent()
                        .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                        .update
                        .plan(),
                )),
            )?;
            if read_record(key_file).map_err(|_| crate::err("PROFILE_KEY_INVALID"))? != key {
                return Err(crate::err("PROFILE_KEY_INVALID"));
            }
            let receipt = SourceHostTrustReceipt {
                host,
                trust,
                source,
                after_archive_inventory,
                after_archive_configuration,
                external_volumes_saved: false,
                live_authority_restored: false,
                preflight_verified: false,
                update_executed: false,
            };
            let mut marker = private_file(&stage.join("verified.json"), true)
                .map_err(|e| crate::err(e.code()))?;
            marker
                .write_all(
                    &serde_json::to_vec(&receipt)
                        .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?,
                )
                .map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            marker.sync_all().map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            #[cfg(windows)]
            marker.check_private().map_err(|_| crate::err("HOST_WRITE_UNCERTAIN"))?;
            drop(marker);
            sync_dir(&stage).map_err(|e| crate::err(e.code()))?;
            publish(&stage, target).map_err(|e| crate::err(e.code()))?;
            sync_dir(parent).map_err(|e| crate::err(e.code()))?;
            Ok(receipt)
        })();
        key.fill(0);
        result
    }
    /// Retained physical stopped-source copy; no logical inventory or full preflight proof.
    pub fn snapshot_source_database(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<DatabaseSnapshotReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            if fs2::available_space(&self.source.root)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 4 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
            let (snapshot_volume, proof) = source_database::copy(image, &before.database_volume)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(DatabaseSnapshotReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                source_volume: before.database_volume,
                snapshot_volume,
                maintenance_image: image.into(),
                proof,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Current logical DB/blob inventory from isolated source snapshots; full preflight remains.
    pub fn verify_source_inventory(
        &self,
        image: &str,
        acknowledged: bool,
    ) -> crate::Result<SourceInventoryReceipt> {
        self.verify_source_inventory_scoped(image, acknowledged, false)
    }
    fn verify_source_inventory_scoped(
        &self,
        image: &str,
        acknowledged: bool,
        roots_held: bool,
    ) -> crate::Result<SourceInventoryReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = if roots_held {
            None
        } else {
            Some(self.source.lock()?)
        };
        let _target_lock = if roots_held {
            None
        } else {
            Some(target.lock()?)
        };
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            if fs2::available_space(&self.source.root)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                < 6 * 1024 * 1024 * 1024
            {
                return Err(crate::err("RESTORE_SPACE_REQUIRED"));
            }
            let (snapshot_volume, proof) = source_database::copy(image, &before.database_volume)?;
            let manifest_path =
                crate::checked_path(&workspace.join("authenticated"), "manifest.json")?;
            let inventory = source_inventory::compare(
                image,
                &snapshot_volume,
                &before.blob_volume,
                &manifest_path,
                &receipt.authenticated_manifest_sha256,
            )?;
            source_inventory::matched(&inventory, plan)?;
            let (repeated_snapshot_volume, repeated_copy) =
                source_database::copy(image, &before.database_volume)?;
            if proof.content_sha256 != repeated_copy.content_sha256
                || proof.bytes != repeated_copy.bytes
                || proof.entries != repeated_copy.entries
                || proof.files != repeated_copy.files
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceInventoryReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                source_database_volume: before.database_volume,
                source_blob_volume: before.blob_volume,
                snapshot_volume,
                repeated_snapshot_volume,
                source_content_sha256: proof.content_sha256,
                inventory,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Fresh current Engine image references; not image byte or full preflight proof.
    pub fn verify_source_images(&self, acknowledged: bool) -> crate::Result<SourceImageReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let result = (|| {
            let job = target
                .restoration_status()?
                .filter(|j| j.state == "completed")
                .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
            let workspace = root.join(format!("restore-{}", job.id));
            let receipt: crate::restoration::RestorationReceipt =
                crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
            let files = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let before = source_stopped::observe(&self.source, plan)?;
            let raw = crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
            binding.authenticated(
                &receipt.backup_id,
                &receipt.authenticated_manifest_sha256,
                &raw,
            )?;
            let observed = source_images::observe(&self.source, &workspace, &raw)?;
            let repeated = source_deployment::authenticated_files(
                &self.source.root,
                &workspace,
                &receipt,
                plan,
            )?;
            let after = source_stopped::observe(&self.source, plan)?;
            if files != repeated
                || before.platform_container != after.platform_container
                || before.database_container != after.database_container
                || before.blob_volume != after.blob_volume
                || before.configuration_volume != after.configuration_volume
                || before.database_volume != after.database_volume
            {
                return Err(crate::err("UPDATE_SOURCE_CHANGED"));
            }
            Ok(SourceImageReceipt {
                source_instance: plan.source_instance.clone(),
                target_instance: plan.target_instance.clone(),
                backup_id: receipt.backup_id,
                authenticated_manifest_sha256: receipt.authenticated_manifest_sha256,
                images: observed,
                observed_at: crate::now(),
            })
        })();
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        result
    }
    /// Restore the exact planned source backup into the already-registered fresh
    /// target. Prepared intent has reserved this target identity durably; no
    /// journal transition, signed-update activation or selection change occurs here.
    pub fn prepare_target_candidate(
        &self,
        image: &str,
        key: &Path,
        archive: &Path,
        port: u16,
        acknowledged: bool,
    ) -> crate::Result<crate::restoration::RestorationReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        // Preserve no-overwrite precedence even when the source is unavailable.
        {
            let _lock = target.lock()?;
            crate::restoration::fresh_root(&root)?;
        }
        let before_source = source_stopped::observe(&self.source, plan)?;
        let result = target.restore_update_candidate(image, key, archive, port, &binding);
        let post_check = (|| -> crate::Result<()> {
            if let Ok(receipt) = &result {
                let _target_lock = target.lock()?;
                source_deployment::authenticated_files(
                    &self.source.root,
                    &root.join(format!("restore-{}", receipt.id)),
                    receipt,
                    plan,
                )?;
            }
            Ok(())
        })();
        let after_source = source_stopped::observe(&self.source, plan);
        self.check()?;
        installations::private_directory(&root)?;
        if !directory_matches(&before, &root).map_err(|_|crate::err("UPDATE_TARGET_CHANGED"))? {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let after_source = after_source?;
        if before_source.platform_container != after_source.platform_container
            || before_source.database_container != after_source.database_container
            || before_source.blob_volume != after_source.blob_volume
            || before_source.configuration_volume != after_source.configuration_volume
            || before_source.database_volume != after_source.database_volume
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        post_check?;
        result
    }
    /// Normally stop only the exact completed recovery candidate reserved by the
    /// current Prepared plan. Source selection and update generation do not change.
    /// Call this before the final host/trust checkpoint: the normal Stop job writes
    /// candidate history and therefore invalidates an earlier whole-profile snapshot.
    /// A completed Stop and fresh stopped observation are not update qualification.
    #[cfg(not(windows))]
    pub fn stop_target_candidate(
        &self,
        acknowledged: bool,
    ) -> crate::Result<(crate::Job, SourceStoppedReceipt)> {
        self.require_selected_source()?;
        if !acknowledged {
            return Err(crate::err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        let intent = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != crate::update::Stage::Prepared {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        let plan = intent.update.plan();
        let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        if plan.target_instance == plan.source_instance
            || registry.active_id == plan.target_instance
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let entry = registry
            .installations
            .iter()
            .find(|entry| entry.id == plan.target_instance && entry.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&self.store.profile.join("installations"))?;
        installations::private_directory(&root)?;
        let identity_before =
            directory_identity(&root).map_err(|_| crate::err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let source_guard = self.source.lock()?;
        let target_guard = target.lock()?;
        self.source.check_restoration_guard(&source_guard)?;
        target.check_restoration_guard(&target_guard)?;
        // The profile/trust session and both original operation guards remain
        // held; retained external helpers must not be bypassed by this new entry.
        LifecycleService::open_retry_diagnostics(self.store.profile.clone())?
            .ensure_external_archive_reconciled()?;
        self.source.ensure_external_archive_reconciled()?;
        target.ensure_external_archive_reconciled()?;
        let restored = target
            .restoration_status()?
            .filter(|job| {
                job.state == "completed" && job.stage == "complete" && job.error_code.is_none()
            })
            .ok_or_else(|| crate::err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
        let workspace = root.join(format!("restore-{}", restored.id));
        let receipt_bytes = crate::installation_backup::source_bytes(
            &workspace,
            "receipt.json",
            1024 * 1024,
            true,
        )?;
        let receipt: crate::restoration::RestorationReceipt =
            serde_json::from_slice(&receipt_bytes)
                .map_err(|_| crate::err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
        let manifest = target.manifest()?;
        candidate_inventory::receipt_matches(&receipt, &restored.id, &manifest, plan)?;
        let raw = crate::installation_backup::source_bytes(
            &workspace.join("authenticated"),
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )?;
        binding.authenticated(
            &receipt.backup_id,
            &receipt.authenticated_manifest_sha256,
            &raw,
        )?;
        source_deployment::authenticated_files(&self.source.root, &workspace, &receipt, plan)?;
        let source_before = source_stopped::observe(&self.source, plan)?;
        let result = (|| {
            let source_manifest = self.source.manifest()?;
            let engine = target.engine(&manifest, false)?;
            if engine != "docker" {
                return Err(crate::err("BACKUP_PLATFORM_UNVERIFIED"));
            }
            target.ensure_backup_helpers_idle(&engine)?;
            target.validate_compose(&manifest, &engine)?;
            let config: serde_json::Value = serde_json::from_slice(&crate::run(
                &engine,
                &crate::compose_args(&manifest, &["config", "--format", "json"]),
                Some(&target.root.join("bundle")),
                30,
            )?)
            .map_err(|_| crate::err("ENGINE_OUTPUT_INVALID"))?;
            let (app_id, app) =
                crate::backup_creation::one_container(&target, &manifest, &engine, "platform")?;
            let (db_id, db) =
                crate::backup_creation::one_container(&target, &manifest, &engine, "database")?;
            if app["Image"] != format!("sha256:{}", plan.source_image) {
                return Err(crate::err("UPDATE_SOURCE_IMAGE_MISMATCH"));
            }
            let blobs = crate::backup_creation::volume_for(
                &engine,
                &manifest,
                &config,
                "platform",
                "/data/blobs",
                &app,
            )?;
            let configuration = crate::backup_creation::volume_for(
                &engine,
                &manifest,
                &config,
                "platform",
                "/data/config",
                &app,
            )?;
            let database = crate::backup_creation::volume_for(
                &engine,
                &manifest,
                &config,
                "database",
                "/var/lib/postgresql",
                &db,
            )?;
            candidate_stop_isolated(
                &source_before,
                &manifest.bundle_id,
                &manifest.project_name,
                &source_manifest.project_name,
                [&app_id, &db_id],
                [&blobs, &configuration, &database],
            )?;
            self.check()?;
            target.check_restoration_guard(&target_guard)?;
            if !directory_matches(&identity_before, &root)? {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
            let job = target.stop_with_retained_guard(&target_guard)?;
            if job.action != crate::Action::Stop || job.state != crate::JobState::Completed {
                return Err(crate::err(
                    job.error_code
                        .as_deref()
                        .unwrap_or("UPDATE_SOURCE_NOT_STOPPED"),
                ));
            }
            let jobs = target.job_history()?;
            if jobs.last().is_none_or(|stored| {
                serde_json::to_value(stored).ok() != serde_json::to_value(&job).ok()
            }) {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
            let mut candidate_plan = plan.clone();
            candidate_plan.source_instance = plan.target_instance.clone();
            let stopped = source_stopped::observe(&target, &candidate_plan)?;
            if stopped.platform_container != app_id
                || stopped.database_container != db_id
                || stopped.blob_volume != blobs
                || stopped.configuration_volume != configuration
                || stopped.database_volume != database
                || !candidate_inventory::isolated(&source_before, &stopped)
                || target.restoration_status()?.is_none_or(|job| {
                    job.id != restored.id || job.state != "completed" || job.error_code.is_some()
                })
                || crate::installation_backup::source_bytes(
                    &workspace,
                    "receipt.json",
                    1024 * 1024,
                    true,
                )? != receipt_bytes
                || crate::installation_backup::source_bytes(
                    &workspace.join("authenticated"),
                    "manifest.json",
                    16 * 1024 * 1024,
                    true,
                )? != raw
                || serde_json::to_value(target.manifest()?).ok()
                    != serde_json::to_value(&manifest).ok()
            {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
            source_deployment::authenticated_files(&self.source.root, &workspace, &receipt, plan)?;
            Ok((job, stopped))
        })();
        // Observe the original even if stopping/observing the candidate failed.
        let source_after = source_stopped::observe(&self.source, plan);
        self.check()?;
        self.source.check_restoration_guard(&source_guard)?;
        target.check_restoration_guard(&target_guard)?;
        installations::private_directory(&root)?;
        if !directory_matches(&identity_before, &root)?
            || !candidate_inventory::same_stopped(&source_before, &source_after?)
        {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        result
    }
    /// Actual authenticated decryption, bound to the prepared manifest hash.
    /// Authentication alone cannot authorize update or prove complete restoration.
    pub fn verify_backup(
        &self,
        image: &str,
        key: &Path,
        archive: &Path,
    ) -> crate::Result<VerificationReceipt> {
        self.check()?;
        let result = self.source.verify_backup(image, key, archive);
        self.check()?;
        let receipt = result?;
        if receipt.authenticated_manifest_sha256
            != self
                .store
                .intent()
                .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                .update
                .plan()
                .backup_manifest
        {
            return Err(crate::err("UPDATE_BACKUP_MISMATCH"));
        }
        Ok(receipt)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{fixture, plan, seal};
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn recovery_engine_failure_preserves_existing_refusals_and_nested_stage() {
        for stage in [RecoveryEngineStage::SourceImageExport, RecoveryEngineStage::SourceConfiguration,
                      RecoveryEngineStage::SourceInventory, RecoveryEngineStage::CandidatePreflight] {
            for code in ["ENGINE_PERMISSION", "ENGINE_TIMEOUT", "ENGINE_OUTPUT_LIMIT", "IMAGE_INTEGRITY",
                         "OWNERSHIP_CONFLICT", "UPDATE_SOURCE_CHANGED", "UPDATE_SOURCE_IMAGES_MISMATCH",
                         "RESTORE_SPACE_REQUIRED", "UPDATE_SOURCE_IMAGE_EXPORT_ENGINE_FAILED"] {
                let original = crate::LifecycleError {code: code.into(), guidance: "existing safe guidance".into()};
                let failure = recovery_engine_failure(original, stage);
                assert_eq!(failure.code, code);
                assert_eq!(failure.guidance, "existing safe guidance");
            }
        }
    }

    const SOURCE: &str = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
    fn prepared(scope: &str) -> (PathBuf, Store) {
        prepared_with_budget(scope, 1024)
    }
    fn prepared_with_budget(scope: &str, budget: u64) -> (PathBuf, Store) {
        let (p, k, policy, r) = fixture();
        let mut s = Store::provision(&p, scope, policy, 10).unwrap();
        let e = seal(&k, &r);
        let mut v = s.verify_for_preparation(&e, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut plan = plan();
        plan.source_instance = SOURCE.into();
        plan.required_free_bytes = budget;
        plan.target_instance = "cc414c3c-dd99-45e2-8307-131a49f72d68".into();
        plan.backup_id = "45ec39e9-5c19-47e6-9aaf-176978521a73".into();
        s.prepare_update(&e, &v, plan, 20).unwrap();
        (p, s)
    }
    fn registered(p: &Path, id: &str, kind: &str) -> PathBuf {
        let mut entries = vec![installations::Entry {
            id: SOURCE.into(),
            kind: "default".into(),
            created_at: 0,
        }];
        if kind != "default" {
            entries.push(installations::Entry {
                id: id.into(),
                kind: kind.into(),
                created_at: 0,
            });
        } else {
            entries[0].id = id.into();
        }
        let r = installations::Registry {
            format: 1,
            active_id: entries[0].id.clone(),
            installations: entries,
        };
        let path = p.join("installation-selection.json");
        fs::write(&path, serde_json::to_vec(&r).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        let root = if kind == "default" {
            p.join("local-runtime")
        } else {
            fs::create_dir(p.join("installations")).unwrap();
            fs::set_permissions(p.join("installations"), fs::Permissions::from_mode(0o700))
                .unwrap();
            p.join("installations").join(id)
        };
        installations::new_directory(&root).unwrap();
        root
    }
    fn completed_selection(store: &mut Store) -> crate::update::Plan {
        let intent = store.intent().unwrap().clone();
        let plan = intent.update.plan().clone();
        let mut verified = store
            .verify_for_preparation(intent.envelope.as_bytes(), 21)
            .unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        store
            .begin_update(
                crate::update::Preflight {
                    plan: plan.clone(),
                    signature_verified: false,
                    artifact_verified: false,
                    compatibility_verified: true,
                    backup_restore_verified: true,
                    current_source_matches_backup: true,
                    available_free_bytes: 4096,
                    image_only_rollback_verified: false,
                },
                &verified,
                21,
            )
            .unwrap();
        store
            .application_finished(&plan.operation_id, store.receipt().generation, 22)
            .unwrap();
        store
            .observe_health(
                &plan.operation_id,
                store.receipt().generation,
                crate::update::HealthReceipt {
                    operation_id: plan.operation_id.clone(),
                    instance_id: plan.target_instance.clone(),
                    image: plan.target_image.clone(),
                    schema: plan.target_schema.clone(),
                    ready: true,
                },
                23,
            )
            .unwrap();
        plan
    }
    #[test]
    fn stable_authority_selection_survives_completion_release_and_reopen_without_new_policy() {
        let (p, mut store) = prepared("default");
        let target = store
            .intent()
            .unwrap()
            .update
            .plan()
            .target_instance
            .clone();
        registered(&p, &target, "recovery");
        let (mut registry, _) = installations::load(&p).unwrap().unwrap();
        assert_eq!(store.bound_source_id(&registry).unwrap(), SOURCE);
        let plan = completed_selection(&mut store);
        assert_eq!(store.bound_source_id(&registry).unwrap(), target);
        // Registry disagreement is never silently adopted as a new authority.
        assert_eq!(
            store.execution().err().unwrap().code,
            "UPDATE_SOURCE_MISMATCH"
        );
        registry.active_id = target.clone();
        installations::save(&p, &registry, None).unwrap();
        let session = store.execution().err().unwrap();
        assert_eq!(session.code, "UPDATE_SOURCE_MISMATCH"); // completed plan still names original
        store
            .release_completed(&plan.operation_id, store.receipt().generation, 24)
            .unwrap();
        let root = store.root.clone();
        let floors = (
            store.current.policy.minimum_sequence,
            store.current.policy.minimum_issued_at,
        );
        let head = store.current_sha256.clone();
        let generation = store.current.generation;
        // A later target registration follows authenticated completed selection,
        // not the original default-kind entry or an unverified active pointer.
        let next = store.register_update_target(true).unwrap();
        assert_eq!(next.source_instance, target);
        assert_eq!(next.active_instance, target);
        assert_eq!(store.current_sha256, head);
        drop(store);
        let reopened = Store::open(&p, "default").unwrap();
        assert_eq!(reopened.root, root);
        assert_eq!(reopened.current_sha256, head);
        assert_eq!(reopened.current.generation, generation);
        assert_eq!(
            (
                reopened.current.policy.minimum_sequence,
                reopened.current.policy.minimum_issued_at
            ),
            floors
        );
        assert!(reopened.used_instances.contains(&target));
        assert_eq!(reopened.bound_source_id(&registry).unwrap(), target);
        let candidate_scope =
            hash(format!("ExhibitOS-release-trust-v1\0{}\0{target}", p.display()).as_bytes());
        assert!(
            !p.parent()
                .unwrap()
                .join(format!(".exhibitos-release-trust-{candidate_scope}"))
                .exists()
        );
        drop(reopened);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn stable_authority_selection_rejects_uncompleted_selected_candidate_without_mutation() {
        let (p, mut store) = prepared("default");
        let target = store
            .intent()
            .unwrap()
            .update
            .plan()
            .target_instance
            .clone();
        registered(&p, &target, "recovery");
        installations::new_directory(&p.join("local-runtime")).unwrap();
        let (mut registry, _) = installations::load(&p).unwrap().unwrap();
        registry.active_id = target.clone();
        installations::save(&p, &registry, None).unwrap();
        let head = store.current_sha256.clone();
        assert_eq!(store.bound_source_id(&registry).unwrap(), SOURCE);
        let diagnostic = store.execution().unwrap();
        assert_eq!(
            diagnostic.require_selected_source().unwrap_err().code,
            "UPDATE_SELECTION_AUTHORITY_MISMATCH"
        );
        drop(diagnostic);
        assert_eq!(store.current_sha256, head);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        drop(store);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn stable_authority_selection_rechecks_historical_record_bytes_of_an_open_store() {
        let (p, store) = prepared("default");
        registered(&p, SOURCE, "default");
        let (registry, _) = installations::load(&p).unwrap().unwrap();
        let path = store.root.join("00000000000000000001.json");
        let original = fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed.push(b' ');
        fs::write(&path, &changed).unwrap();
        assert!(store.bound_source_id(&registry).is_err());
        assert_eq!(fs::read(&path).unwrap(), changed);
        fs::write(&path, &original).unwrap();
        assert_eq!(store.bound_source_id(&registry).unwrap(), SOURCE);
        drop(store);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn owned_preflight_without_independent_authority_never_extracts_or_transitions() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p.parent().unwrap().to_owned();
        let file = parent.join("runtime.tar");
        fs::write(&file, b"fixture").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let head = store.current_sha256.clone();
        let intent = serde_json::to_vec(&store.current.intent).unwrap();
        let mut session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&file, &parent, 21)
            .unwrap();
        let destination = parent.join("never-created");
        let inputs = RecoveryRuntimeInputs {
            checkpoint: CheckpointInputs {
                binding: Path::new("/missing-binding"),
                host: Path::new("/missing-host"),
                trust: Path::new("/missing-trust"),
                key: &[4; 32],
            },
            export_parent: &parent,
            python: Path::new("/missing-python"),
            source_commit: "invalid",
            maintenance_image: "untrusted",
            external_writers_quiesced: true,
        };
        assert!(
            session
                .prepare_owned_update(
                    &mut artifact,
                    &inputs,
                    &destination,
                    Path::new("/missing-key")
                )
                .is_err()
        );
        assert!(session.prepare_owned_migrated_update_reusing_host(
            &mut artifact,&inputs,&MigrationRuntimeInputs{catalog:Path::new("/missing-catalog"),catalog_sha256:&"a".repeat(64)},
            Path::new("/missing-retained-host"),&destination,Path::new("/missing-key")).is_err());
        assert!(!destination.exists());
        assert!(!p.join("update-execution-references").exists());
        assert_eq!(fs::read(&file).unwrap(), b"fixture");
        drop(artifact);
        drop(session);
        assert_eq!(store.current_sha256, head);
        assert_eq!(serde_json::to_vec(&store.current.intent).unwrap(), intent);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        drop(store);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn owned_artifact_rechecks_expiry_and_retained_path_without_transition() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-owned-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"fixture").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let original = store.current_sha256.clone();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&source, &parent, 21)
            .unwrap();
        let receipt = session
            .reverify_prepared_artifact_at(&mut artifact, 22)
            .unwrap();
        assert!(!receipt.preflight_verified && !receipt.update_executed);
        assert_eq!(
            session
                .reverify_prepared_artifact_at(&mut artifact, 100)
                .unwrap_err()
                .code,
            "UPDATE_RELEASE_EXPIRED"
        );
        let staged = artifact.staged.path().to_owned();
        fs::rename(&staged, staged.with_extension("original")).unwrap();
        fs::write(&staged, b"fixture").unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(
            session
                .reverify_prepared_artifact_at(&mut artifact, 22)
                .unwrap_err()
                .code,
            "UPDATE_ARTIFACT_MISMATCH"
        );
        drop(session);
        assert_eq!(store.current_sha256, original);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
    }
    #[test]
    fn identical_plan_in_another_profile_cannot_reuse_retained_artifact() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-scope-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"fixture").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&source, &parent, 21)
            .unwrap();
        drop(session);
        let (other, mut second) = prepared("default");
        registered(&other, SOURCE, "default");
        let second_session = second.execution().unwrap();
        assert_eq!(
            second_session
                .reverify_prepared_artifact_at(&mut artifact, 21)
                .unwrap_err()
                .code,
            "UPDATE_OPERATION_STALE"
        );
        assert_eq!(
            second_session.store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
    }
    #[test]
    fn owned_artifact_rejects_source_bytes_and_profile_staging() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let parent = p
            .parent()
            .unwrap()
            .join(format!("artifact-refusal-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let source = parent.join("runtime.tar");
        fs::write(&source, b"changed").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let original = store.current_sha256.clone();
        let session = store.execution().unwrap();
        assert_eq!(
            session
                .stage_prepared_artifact_at(&source, &p, 21)
                .err()
                .unwrap()
                .code,
            "UPDATE_ARTIFACT_UNAVAILABLE"
        );
        assert_eq!(
            session
                .stage_prepared_artifact_at(&source, &parent, 21)
                .err()
                .unwrap()
                .code,
            "UPDATE_ARTIFACT_MISMATCH"
        );
        drop(session);
        assert_eq!(store.current_sha256, original);
    }
    #[test]
    fn missing_registry_or_wrong_source_cannot_bootstrap_execution() {
        let (p, mut s) = prepared("default");
        assert_eq!(
            s.execution().err().unwrap().code,
            "UPDATE_SOURCE_UNREGISTERED"
        );
        assert!(!p.join("local-runtime").exists());
        registered(&p, "e7980e6c-cbae-454f-906b-1f4924cc2a4e", "default");
        assert_eq!(s.execution().err().unwrap().code, "UPDATE_SOURCE_MISMATCH");
        assert_eq!(s.receipt().generation, 2);
    }
    #[test]
    fn owned_fence_excludes_other_sessions_without_downgrade_and_keeps_prepared() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        {
            let lease = s.execution().unwrap();
            assert_eq!(
                profile_backup::session_lock(&p, false).err().unwrap().code,
                "PROFILE_BUSY"
            );
            assert!(matches!(Store::open(&p, "default"), Err(Error::TrustBusy)));
            assert!(!lease.source_status().unwrap().installed);
        }
        assert_eq!(
            profile_backup::session_lock(&p, false).err().unwrap().code,
            "PROFILE_BUSY"
        );
        assert_eq!(
            s.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        assert_eq!(s.receipt().generation, 2);
        drop(s);
        assert!(profile_backup::session_lock(&p, false).is_ok());
    }
    #[test]
    fn registry_and_source_replacements_invalidate_existing_lease() {
        for registry in [true, false] {
            let (p, mut s) = prepared("default");
            let root = registered(&p, SOURCE, "default");
            let lease = s.execution().unwrap();
            if registry {
                let path = p.join("installation-selection.json");
                let mut b = fs::read(&path).unwrap();
                b.push(b' ');
                fs::write(path, b).unwrap();
            } else {
                fs::rename(&root, p.join("retained-source")).unwrap();
                installations::new_directory(&root).unwrap();
            }
            assert_eq!(
                lease.source_status().unwrap_err().code,
                "UPDATE_SOURCE_CHANGED"
            );
        }
    }
    #[test]
    fn alias_and_unregistered_explicit_scope_are_rejected() {
        let (p, mut s) = prepared("default");
        let root = registered(&p, SOURCE, "default");
        let retained = p.join("retained-source");
        fs::rename(&root, &retained).unwrap();
        symlink(&retained, &root).unwrap();
        assert!(s.execution().is_err());
        let (p, mut s) = prepared("e7980e6c-cbae-454f-906b-1f4924cc2a4e");
        registered(&p, SOURCE, "default");
        assert_eq!(
            s.execution().err().unwrap().code,
            "UPDATE_SOURCE_UNREGISTERED"
        );
    }
    #[test]
    fn explicit_recovery_scope_does_not_change_active_selection() {
        let (p, mut s) = prepared(SOURCE);
        let root = registered(&p, SOURCE, "recovery");
        // The helper's duplicate default ID must be replaced by a distinct UUID.
        let path = p.join("installation-selection.json");
        let mut r: installations::Registry =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        r.installations[0].id = "e7980e6c-cbae-454f-906b-1f4924cc2a4e".into();
        r.active_id = r.installations[0].id.clone();
        fs::write(&path, serde_json::to_vec(&r).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(lease.source.root, root);
        assert!(!lease.source_status().unwrap().installed);
        assert_eq!(fs::read(path).unwrap(), before);
    }
    fn register_target(p: &Path) -> PathBuf {
        let (mut r, _) = installations::load(p).unwrap().unwrap();
        let id = "cc414c3c-dd99-45e2-8307-131a49f72d68";
        r.installations.push(installations::Entry {
            id: id.into(),
            kind: "recovery".into(),
            created_at: 0,
        });
        fs::write(
            p.join("installation-selection.json"),
            serde_json::to_vec(&r).unwrap(),
        )
        .unwrap();
        let parent = p.join("installations");
        installations::new_directory(&parent).unwrap();
        let target = parent.join(id);
        installations::new_directory(&target).unwrap();
        target
    }
    #[test]
    fn candidate_stop_requires_ack_registered_private_target_and_preserves_selection() {
        let (p, mut store) = prepared("default");
        let source = registered(&p, SOURCE, "default");
        fs::write(source.join("original-witness"), b"keep source").unwrap();
        let registry = fs::read(p.join("installation-selection.json")).unwrap();
        {
            let lease = store.execution().unwrap();
            assert_eq!(
                lease.stop_target_candidate(false).unwrap_err().code,
                "BACKUP_OPERATOR_ACK_REQUIRED"
            );
            assert_eq!(
                lease.stop_target_candidate(true).unwrap_err().code,
                "UPDATE_TARGET_UNREGISTERED"
            );
        }
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            registry
        );
        let target = register_target(&p);
        let retained = p.join("retained-target");
        fs::rename(&target, &retained).unwrap();
        symlink(&retained, &target).unwrap();
        let registered = fs::read(p.join("installation-selection.json")).unwrap();
        {
            let lease = store.execution().unwrap();
            assert!(lease.stop_target_candidate(true).is_err());
        }
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            registered
        );
        assert_eq!(
            fs::read(source.join("original-witness")).unwrap(),
            b"keep source"
        );
        assert_eq!(fs::read_dir(retained).unwrap().count(), 0);
        assert_eq!(store.receipt().generation, 2);
    }
    #[test]
    fn candidate_stop_rejects_applying_and_an_active_candidate_before_mutation() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        fs::write(target.join("candidate-witness"), b"keep candidate").unwrap();
        let envelope = store.intent().unwrap().envelope.as_bytes().to_vec();
        let mut verified = store.verify_for_preparation(&envelope, 20).unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        let mut observations = super::super::tests::observations();
        observations.plan = store.intent().unwrap().update.plan().clone();
        store.begin_update(observations, &verified, 21).unwrap();
        {
            let lease = store.execution().unwrap();
            assert_eq!(
                lease.stop_target_candidate(true).unwrap_err().code,
                "UPDATE_CANDIDATE_STAGE_INVALID"
            );
        }
        assert_eq!(
            fs::read(target.join("candidate-witness")).unwrap(),
            b"keep candidate"
        );
        assert!(!target.join("jobs.json").exists());
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        let (mut registry, _) = installations::load(&p).unwrap().unwrap();
        registry.active_id = store
            .intent()
            .unwrap()
            .update
            .plan()
            .target_instance
            .clone();
        fs::write(
            p.join("installation-selection.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        let before = fs::read(p.join("installation-selection.json")).unwrap();
        let lease = store.execution().unwrap();
        assert_eq!(
            lease.stop_target_candidate(true).unwrap_err().code,
            "UPDATE_SELECTION_AUTHORITY_MISMATCH"
        );
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            before
        );
        assert!(!target.join("jobs.json").exists());
    }
    #[test]
    fn candidate_stop_rejects_wrong_backup_receipt_before_source_or_target_engine_action() {
        let (p, mut store) = prepared("default");
        let source = registered(&p, SOURCE, "default");
        fs::write(source.join("original-witness"), b"keep source").unwrap();
        let target = register_target(&p);
        let bundle = target.join("bundle");
        installations::new_directory(&bundle).unwrap();
        crate::write_private_new(&bundle, "compose.yaml", b"{}", 1024).unwrap();
        let m: crate::BundleManifest = serde_json::from_value(serde_json::json!({"schemaVersion":"1.0.0-draft.1","bundleId":"recovered-fixture","version":"1","protocolVersion":"1","composeSha256":crate::digest(b"{}"),"projectName":"exhibitos-recovered-fixture","services":["platform","database"],"images":[{"reference":format!("sha256:{}","a".repeat(64))}],"ports":[13200],"openUrl":"http://127.0.0.1:13200","readinessUrl":"http://127.0.0.1:13200/health","minimumFreeBytes":1})).unwrap();
        crate::write_json(&bundle, "manifest.json", &m).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let restored = crate::restoration::RestorationJob {
            id: id.clone(),
            state: "completed".into(),
            stage: "complete".into(),
            error_code: None,
            created_at: 1,
            updated_at: 1,
        };
        crate::write_json(&target, "restoration.json", &restored).unwrap();
        let workspace = target.join(format!("restore-{id}"));
        installations::new_directory(&workspace).unwrap();
        let plan = store.intent().unwrap().update.plan();
        // Intentionally mismatched fixture; never accepted as actual restoration.
        let receipt = crate::restoration::RestorationReceipt {
            id,
            operation: "restored-and-running".into(),
            backup_id: uuid::Uuid::new_v4().to_string(),
            authenticated_manifest_sha256: plan.backup_manifest.clone(),
            bundle_id: m.bundle_id.clone(),
            project_name: m.project_name.clone(),
            open_url: m.open_url.clone(),
            at: 1,
            network_subnet: None,
            source_verification: Some(crate::restoration::RestorationProof {
                inventory_sha256: plan.source_inventory.clone(),
                schema_sha256: plan.source_schema.clone(),
                runtime_image_sha256: plan.source_image.clone(),
            }),
        };
        crate::write_json(&workspace, "receipt.json", &receipt).unwrap();
        let original = fs::read(workspace.join("receipt.json")).unwrap();
        let registry = fs::read(p.join("installation-selection.json")).unwrap();
        {
            let lease = store.execution().unwrap();
            assert_eq!(
                lease.stop_target_candidate(true).unwrap_err().code,
                "UPDATE_CANDIDATE_PROOF_MISSING"
            );
        }
        assert_eq!(fs::read(workspace.join("receipt.json")).unwrap(), original);
        assert_eq!(
            fs::read(p.join("installation-selection.json")).unwrap(),
            registry
        );
        assert_eq!(
            fs::read(source.join("original-witness")).unwrap(),
            b"keep source"
        );
        assert!(!target.join("jobs.json").exists());
        assert_eq!(store.receipt().generation, 2);
    }
    #[test]
    fn candidate_stop_pre_stop_isolation_rejects_every_original_alias() {
        let source = SourceStoppedReceipt {
            source_instance: SOURCE.into(),
            bundle_id: "original".into(),
            platform_container: "app".into(),
            database_container: "db".into(),
            runtime_image_sha256: "a".repeat(64),
            blob_volume: "blob".into(),
            configuration_volume: "config".into(),
            database_volume: "data".into(),
            observed_at: 1,
        };
        assert!(
            candidate_stop_isolated(
                &source,
                "candidate",
                "candidate-project",
                "source-project",
                ["new-app", "new-db"],
                ["new-blob", "new-config", "new-data"]
            )
            .is_ok()
        );
        for mode in 0..5 {
            let (bundle, project, ids, volumes) = match mode {
                0 => (
                    "original",
                    "candidate-project",
                    ["new-app", "new-db"],
                    ["new-blob", "new-config", "new-data"],
                ),
                1 => (
                    "candidate",
                    "source-project",
                    ["new-app", "new-db"],
                    ["new-blob", "new-config", "new-data"],
                ),
                2 => (
                    "candidate",
                    "candidate-project",
                    ["db", "new-db"],
                    ["new-blob", "new-config", "new-data"],
                ),
                3 => (
                    "candidate",
                    "candidate-project",
                    ["new-app", "new-db"],
                    ["new-blob", "config", "new-data"],
                ),
                _ => (
                    "candidate",
                    "candidate-project",
                    ["new-app", "new-db"],
                    ["new-blob", "new-blob", "new-data"],
                ),
            };
            assert_eq!(
                candidate_stop_isolated(&source, bundle, project, "source-project", ids, volumes)
                    .unwrap_err()
                    .code,
                "UPDATE_TARGET_CHANGED"
            );
        }
    }
    #[test]
    fn candidate_requires_ack_existing_registered_target_and_private_namespace() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert!(!p.join("installations").exists());
        drop(lease);
        assert_eq!(s.receipt().generation, 2);
        let target = register_target(&p);
        let retained = p.join("retained-target");
        fs::rename(&target, &retained).unwrap();
        symlink(&retained, &target).unwrap();
        let lease = s.execution().unwrap();
        assert!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .is_err()
        );
        assert_eq!(fs::read_dir(retained).unwrap().count(), 0);
    }
    #[test]
    fn candidate_preparation_rejects_applying_before_any_target_mutation() {
        let (p, mut s) = prepared("default");
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        let envelope = s.intent().unwrap().envelope.as_bytes().to_vec();
        let mut v = s.verify_for_preparation(&envelope, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut observations = super::super::tests::observations();
        observations.plan = s.intent().unwrap().update.plan().clone();
        s.begin_update(observations, &v, 21).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate("tag", &p, &p, 13200, true)
                .unwrap_err()
                .code,
            "UPDATE_CANDIDATE_STAGE_INVALID"
        );
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }
    #[test]
    fn existing_target_is_refused_before_an_unsatisfiable_new_operation_budget() {
        let (p, mut s) = prepared_with_budget("default", u64::MAX);
        registered(&p, SOURCE, "default");
        let target = register_target(&p);
        fs::write(target.join("retained-candidate"), b"keep").unwrap();
        let key = p.parent().unwrap().join("synthetic-test-key.bin");
        fs::write(&key, [7u8; 32]).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let archive = p.parent().unwrap().join("synthetic-archive");
        installations::new_directory(&archive).unwrap();
        let lease = s.execution().unwrap();
        assert_eq!(
            lease
                .prepare_target_candidate(
                    &format!("sha256:{}", "a".repeat(64)),
                    &key,
                    &archive,
                    13200,
                    true
                )
                .unwrap_err()
                .code,
            "RESTORE_FRESH_ROOT_REQUIRED"
        );
        assert_eq!(
            fs::read(target.join("retained-candidate")).unwrap(),
            b"keep"
        );
        assert!(!target.join("restoration.json").exists());
        drop(lease);
        assert_eq!(s.receipt().generation, 2);
    }
    #[test]
    fn fresh_target_registration_preserves_active_source_intent_and_existing_bytes() {
        let (p, store) = prepared("default");
        let root = registered(&p, SOURCE, "default");
        fs::write(root.join("original-witness"), b"keep source").unwrap();
        let trust = serde_json::to_vec(&store.receipt()).unwrap();
        let intent = serde_json::to_vec(&store.intent()).unwrap();
        assert_eq!(
            store.register_update_target(false).unwrap_err().code,
            "UPDATE_TARGET_ACK_REQUIRED"
        );
        assert!(!p.join("installations").exists());
        let first = store.register_update_target(true).unwrap();
        let second = store.register_update_target(true).unwrap();
        assert_eq!(first.source_instance, SOURCE);
        assert_eq!(first.active_instance, SOURCE);
        assert_ne!(first.target_instance, second.target_instance);
        assert!(!first.activated && !first.runtime_started);
        assert_eq!(
            fs::read(root.join("original-witness")).unwrap(),
            b"keep source"
        );
        assert_eq!(fs::read_dir(&first.target_path).unwrap().count(), 0);
        assert_eq!(serde_json::to_vec(&store.receipt()).unwrap(), trust);
        assert_eq!(serde_json::to_vec(&store.intent()).unwrap(), intent);
        let (registry, _) = installations::load(&p).unwrap().unwrap();
        assert_eq!(registry.active_id, SOURCE);
        assert_eq!(registry.installations.len(), 3);
    }
    #[test]
    fn planned_target_registration_materializes_only_reserved_id_and_refuses_reopen() {
        let (p, store) = prepared("default");
        registered(&p, SOURCE, "default");
        let before = serde_json::to_vec(&store.intent()).unwrap();
        let trust = serde_json::to_vec(&store.receipt()).unwrap();
        assert_eq!(store.register_planned_update_target(false).unwrap_err().code,
            "UPDATE_TARGET_ACK_REQUIRED");
        let first = store.register_planned_update_target(true).unwrap();
        assert_eq!(first.target_instance, store.intent().unwrap().update.plan().target_instance);
        assert_eq!(first.active_instance, SOURCE);
        assert!(!first.activated && !first.runtime_started);
        let witness = Path::new(&first.target_path).join("retained-candidate");
        fs::write(&witness, b"preserve").unwrap();
        let registry = fs::read(p.join("installation-selection.json")).unwrap();
        assert_eq!(store.register_planned_update_target(true).unwrap_err().code,
            "UPDATE_IDENTITY_REUSED");
        assert_eq!(fs::read(&witness).unwrap(), b"preserve");
        assert_eq!(fs::read(p.join("installation-selection.json")).unwrap(), registry);
        assert_eq!(serde_json::to_vec(&store.intent()).unwrap(), before);
        assert_eq!(serde_json::to_vec(&store.receipt()).unwrap(), trust);
        drop(store);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn fresh_registration_refuses_source_mismatch_and_aliased_namespace() {
        let (p, store) = prepared("default");
        registered(&p, "e7980e6c-cbae-454f-906b-1f4924cc2a4e", "default");
        assert_eq!(
            store.register_update_target(true).unwrap_err().code,
            "UPDATE_SOURCE_MISMATCH"
        );
        let (p, store) = prepared("default");
        registered(&p, SOURCE, "default");
        let other = p.parent().unwrap().join("retained-other");
        installations::new_directory(&other).unwrap();
        symlink(&other, p.join("installations")).unwrap();
        assert!(store.register_update_target(true).is_err());
        assert_eq!(fs::read_dir(other).unwrap().count(), 0);
    }
    #[test]
    fn integrated_checkpoint_refuses_both_missing_acknowledgements_before_writes() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let session = store.execution().unwrap();
        let destination = p.parent().unwrap().join("unpublished-pair");
        let key = p.parent().unwrap().join("absent-key");
        let image = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            session
                .checkpoint_source_host_trust(&image, &key, &destination, false, true)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .checkpoint_source_host_trust(&image, &key, &destination, true, false)
                .unwrap_err()
                .code,
            "HOST_WRITER_ACK_REQUIRED"
        );
        assert!(!destination.exists());
    }
    #[test]
    fn combined_recovery_refuses_missing_ack_and_candidate_without_engine_work() {
        let (p, mut store) = prepared("default");
        registered(&p, SOURCE, "default");
        let session = store.execution().unwrap();
        assert_eq!(
            session
                .verify_source_recovery_bundle(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_source_recovery_bundle(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert_eq!(
            session
                .verify_source_recovery_transient(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_source_recovery_transient(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_recovery(
                    "untrusted-image",
                    Path::new("/untrusted-parent"),
                    false
                )
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_recovery(
                    "untrusted-image",
                    Path::new("/untrusted-parent"),
                    true
                )
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_configuration(
                    "untrusted-image",
                    Path::new("/untrusted-parent"),
                    false
                )
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_configuration(
                    "untrusted-image",
                    Path::new("/untrusted-parent"),
                    true
                )
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_inventory(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            session
                .verify_restored_candidate_inventory(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_TARGET_UNREGISTERED"
        );
        for acknowledged in [false, true] {
            let key = [0u8; 32];
            let inputs = CheckpointInputs {
                binding: Path::new("/untrusted"),
                host: Path::new("/untrusted"),
                trust: Path::new("/untrusted"),
                key: &key,
            };
            assert_eq!(
                session
                    .verify_combined_recovery_ephemeral(
                        "untrusted",
                        Path::new("/untrusted"),
                        &inputs,
                        acknowledged
                    )
                    .unwrap_err()
                    .code,
                if acknowledged {
                    "UPDATE_TARGET_UNREGISTERED"
                } else {
                    "BACKUP_OPERATOR_ACK_REQUIRED"
                }
            );
        }
        let external = p.with_extension("binding-output");
        installations::new_directory(&external).unwrap();
        let output = external.join("binding.bin");
        let key = [0u8; 32];
        let inputs = CheckpointInputs {
            binding: Path::new("/untrusted"),
            host: Path::new("/untrusted"),
            trust: Path::new("/untrusted"),
            key: &key,
        };
        for acknowledged in [false, true] {
            assert_eq!(
                session
                    .bind_combined_recovery_ephemeral(
                        "untrusted",
                        &external,
                        &inputs,
                        &output,
                        acknowledged
                    )
                    .unwrap_err()
                    .code,
                if acknowledged {
                    "UPDATE_TARGET_UNREGISTERED"
                } else {
                    "BACKUP_OPERATOR_ACK_REQUIRED"
                }
            );
            assert!(!output.exists());
        }
        fs::write(&output, b"preserve existing catalog").unwrap();
        assert_eq!(
            session
                .bind_combined_recovery_ephemeral("untrusted", &external, &inputs, &output, true)
                .unwrap_err()
                .code,
            "RECOVERY_PAIR_INVALID"
        );
        assert_eq!(fs::read(&output).unwrap(), b"preserve existing catalog");
        fs::remove_file(&output).unwrap();
        fs::remove_dir(&external).unwrap();
        for acknowledged in [false, true] {
            assert_eq!(
                session
                    .verify_source_recovery_ephemeral(
                        "untrusted",
                        Path::new("/untrusted-parent"),
                        acknowledged
                    )
                    .unwrap_err()
                    .code,
                if acknowledged {
                    "UPDATE_TARGET_UNREGISTERED"
                } else {
                    "BACKUP_OPERATOR_ACK_REQUIRED"
                }
            );
        }
        for acknowledged in [false, true] {
            assert_eq!(
                session
                    .verify_restored_candidate_recovery_ephemeral(
                        "untrusted",
                        Path::new("/untrusted-parent"),
                        acknowledged
                    )
                    .unwrap_err()
                    .code,
                if acknowledged {
                    "UPDATE_TARGET_UNREGISTERED"
                } else {
                    "BACKUP_OPERATOR_ACK_REQUIRED"
                }
            );
        }
        for acknowledged in [false, true] {
            assert_eq!(
                session
                    .verify_restored_candidate_inventory_ephemeral("untrusted", acknowledged)
                    .unwrap_err()
                    .code,
                if acknowledged {
                    "UPDATE_TARGET_UNREGISTERED"
                } else {
                    "BACKUP_OPERATOR_ACK_REQUIRED"
                }
            );
        }
        drop(session);
        assert_eq!(store.receipt().generation, 2);
    }
    #[test]
    fn actual_unbound_checkpoint_refuses_runtime_before_engine_and_preserves_inputs() {
        let (profile, mut store) = prepared("default");
        registered(&profile, SOURCE, "default");
        let parent = profile.parent().unwrap().to_owned();
        let key_file = parent.join("key.bin");
        fs::write(&key_file, [4u8; 32]).unwrap();
        fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
        let pair = parent.join("pair");
        store.checkpoint_host_trust(&key_file, &pair, true).unwrap();
        let original = store.current_sha256.clone();
        let saved = fs::read(pair.join("host.bin")).unwrap();
        let artifact_file = parent.join("runtime.tar");
        fs::write(&artifact_file, b"fixture").unwrap();
        fs::set_permissions(&artifact_file, fs::Permissions::from_mode(0o600)).unwrap();
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact_at(&artifact_file, &parent, 21)
            .unwrap();
        let key = [4u8; 32];
        let binding = pair.join("pair-binding.bin");
        let host = pair.join("host.bin");
        let trust = pair.join("trust.bin");
        let mut inputs = RecoveryRuntimeInputs {
            checkpoint: CheckpointInputs {
                binding: &binding,
                host: &host,
                trust: &trust,
                key: &key,
            },
            export_parent: &parent,
            python: Path::new("/untrusted-python"),
            source_commit: "invalid",
            maintenance_image: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            external_writers_quiesced: false,
        };
        assert_eq!(
            session
                .qualify_current_recovery_runtime(&mut artifact, &inputs)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        inputs.external_writers_quiesced = true;
        assert_eq!(
            session
                .qualify_current_recovery_runtime(&mut artifact, &inputs)
                .unwrap_err()
                .code,
            "UPDATE_RECOVERY_CHECKPOINT_UNBOUND"
        );
        assert_eq!(fs::read(&host).unwrap(), saved);
        assert_eq!(fs::read(&artifact_file).unwrap(), b"fixture");
        assert_eq!(fs::read(&key_file).unwrap(), key);
        drop(artifact);
        drop(session);
        assert_eq!(store.current_sha256, original);
        drop(store);
        // Only this wholly new successful synthetic fixture; no existing roots.
        assert!(
            parent
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("exhibitos-release-trust-")
        );
        fs::remove_dir_all(parent).unwrap();
    }
}

#[cfg(all(test, unix))]
mod source_stop_tests {
    use super::super::tests::{fixture, plan, seal};
    use super::*;
    #[test]
    fn missing_installed_source_and_ack_refuse_without_engine_bootstrap() {
        let (p, k, policy, r) = fixture();
        let mut store = Store::provision(&p, "default", policy, 10).unwrap();
        let envelope = seal(&k, &r);
        let mut v = store.verify_for_preparation(&envelope, 20).unwrap();
        v.verify_artifact(&mut b"fixture".as_slice()).unwrap();
        let mut planned = plan();
        let id = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
        planned.source_instance = id.into();
        store.prepare_update(&envelope, &v, planned, 20).unwrap();
        fs::write(p.join("installation-selection.json"),serde_json::to_vec(&serde_json::json!({"format":1,"activeId":id,"installations":[{"id":id,"kind":"default","createdAt":0}]})).unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            p.join("installation-selection.json"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let root = p.join("local-runtime");
        installations::new_directory(&root).unwrap();
        let lease = store.execution().unwrap();
        assert_eq!(
            lease.verify_source_stopped(false).unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease.verify_source_stopped(true).unwrap_err().code,
            "UPDATE_SOURCE_NOT_INSTALLED"
        );
        assert_eq!(
            lease
                .verify_source_configuration(&format!("sha256:{}", "a".repeat(64)), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease
                .verify_source_configuration(&format!("sha256:{}", "a".repeat(64)), true)
                .unwrap_err()
                .code,
            "UPDATE_RESTORE_BINDING_INVALID"
        );
        assert_eq!(
            lease.verify_source_images(false).unwrap_err().code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            lease.verify_source_images(true).unwrap_err().code,
            "UPDATE_RESTORE_BINDING_INVALID"
        );
        assert!(!root.join("engine.json").exists());
        assert!(!root.join("installed.json").exists());
        drop(lease);
        assert_eq!(store.receipt().generation, 2);
    }
}
