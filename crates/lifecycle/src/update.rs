// SPDX-License-Identifier: Apache-2.0
//! Pure update safety decisions. Evidence is supplied by trusted adapters, not verified here.
use serde::{Deserialize, Serialize};

pub const MODEL_VERSION: u32 = 1;
pub const MAX_RECORD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub operation_id: String,
    pub source_instance: String,
    pub target_instance: String,
    pub source_image: String,
    pub target_image: String,
    pub source_schema: String,
    pub target_schema: String,
    pub backup_id: String,
    pub backup_manifest: String,
    pub source_inventory: String,
    pub required_free_bytes: u64,
}

/// A trusted verifier must bind every result to this exact plan and current source snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preflight {
    pub plan: Plan,
    pub signature_verified: bool,
    pub artifact_verified: bool,
    pub compatibility_verified: bool,
    pub backup_restore_verified: bool,
    pub current_source_matches_backup: bool,
    pub available_free_bytes: u64,
    /// Explicit verifier proof of data compatibility and no migration/writes that
    /// invalidate old-runtime reads. Equal schema alone is insufficient. Unknown=false.
    pub image_only_rollback_verified: bool,
}

/// Produced only after readiness and actual image/schema observation by an executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthReceipt {
    pub operation_id: String,
    pub instance_id: String,
    pub image: String,
    pub schema: String,
    pub ready: bool,
}

/// A separate candidate must match the original full backup inventory before activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreReceipt {
    pub operation_id: String,
    pub backup_id: String,
    pub backup_manifest: String,
    pub inventory_digest: String,
    pub candidate_id: String,
    pub schema: String,
    pub inventory_verified: bool,
    pub separate_candidate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepared,
    Applying,
    AwaitingHealth,
    Updated,
    RecoveryRequired,
    Restoring,
    AwaitingRollbackHealth,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    UpdateFailed,
    HealthFailed,
    RestoreFailed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidRecord,
    InvalidPlan,
    EvidenceMismatch,
    SignatureUnverified,
    ArtifactUnverified,
    CompatibilityUnverified,
    BackupUnverified,
    SourceChanged,
    InsufficientSpace,
    InvalidTransition,
    ObservationUnverified,
    DataRestoreRequired,
}

/// Fields are private so callers cannot directly assign a successful stage.
/// Serialized records are not authenticated: store them in trusted private storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Update {
    version: u32,
    plan: Plan,
    stage: Stage,
    preflight: Option<Preflight>,
    restore: Option<RestoreReceipt>,
    restore_candidate: Option<String>,
    health: Option<HealthReceipt>,
    failure: Option<Failure>,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
impl Plan {
    fn validate(&self) -> Result<(), Error> {
        if !identifier(&self.source_instance)
            || !identifier(&self.target_instance)
            || self.source_instance == self.target_instance
            || !identifier(&self.operation_id)
            || !identifier(&self.backup_id)
            || !digest(&self.source_image)
            || !digest(&self.target_image)
            || !digest(&self.source_schema)
            || !digest(&self.target_schema)
            || !digest(&self.backup_manifest)
            || !digest(&self.source_inventory)
            || self.source_image == self.target_image
            || self.required_free_bytes == 0
        {
            return Err(Error::InvalidPlan);
        }
        Ok(())
    }
}
impl Update {
    pub fn new(plan: Plan) -> Result<Self, Error> {
        plan.validate()?;
        Ok(Self {
            version: MODEL_VERSION,
            plan,
            stage: Stage::Prepared,
            preflight: None,
            restore: None,
            restore_candidate: None,
            health: None,
            failure: None,
        })
    }
    pub fn stage(&self) -> Stage {
        self.stage
    }
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
    /// Read-only identity reserved by BeginRestore, never a restore proof.
    pub fn restore_candidate(&self) -> Option<&str> {
        self.restore_candidate.as_deref()
    }
    pub fn failure(&self) -> Option<Failure> {
        self.failure
    }
    /// Once applying starts, a changed schema is conservatively considered touched,
    /// even if the process crashed before reporting migration completion.
    pub fn requires_data_restore(&self) -> bool {
        self.stage != Stage::Prepared
            && (self.plan.source_schema != self.plan.target_schema
                || !self
                    .preflight
                    .as_ref()
                    .is_some_and(|e| e.image_only_rollback_verified))
    }
    fn check_preflight(&self, evidence: &Preflight) -> Result<(), Error> {
        if evidence.plan != self.plan {
            return Err(Error::EvidenceMismatch);
        }
        if !evidence.signature_verified {
            return Err(Error::SignatureUnverified);
        }
        if !evidence.artifact_verified {
            return Err(Error::ArtifactUnverified);
        }
        if !evidence.compatibility_verified {
            return Err(Error::CompatibilityUnverified);
        }
        if !evidence.backup_restore_verified {
            return Err(Error::BackupUnverified);
        }
        if !evidence.current_source_matches_backup {
            return Err(Error::SourceChanged);
        }
        if evidence.available_free_bytes < self.plan.required_free_bytes {
            return Err(Error::InsufficientSpace);
        }
        Ok(())
    }
    fn check_restore(&self, receipt: &RestoreReceipt) -> Result<(), Error> {
        if receipt.operation_id != self.plan.operation_id
            || receipt.backup_id != self.plan.backup_id
            || receipt.schema != self.plan.source_schema
            || receipt.backup_manifest != self.plan.backup_manifest
            || receipt.inventory_digest != self.plan.source_inventory
            || Some(&receipt.candidate_id) != self.restore_candidate.as_ref()
        {
            return Err(Error::EvidenceMismatch);
        }
        if !receipt.inventory_verified || !receipt.separate_candidate {
            return Err(Error::ObservationUnverified);
        }
        Ok(())
    }
    fn check_health(&self, receipt: &HealthReceipt, rollback: bool) -> Result<(), Error> {
        let (image, schema) = if rollback {
            (&self.plan.source_image, &self.plan.source_schema)
        } else {
            (&self.plan.target_image, &self.plan.target_schema)
        };
        let instance = if rollback {
            self.restore_candidate
                .as_ref()
                .unwrap_or(&self.plan.source_instance)
        } else {
            &self.plan.target_instance
        };
        if receipt.operation_id != self.plan.operation_id
            || &receipt.instance_id != instance
            || &receipt.image != image
            || &receipt.schema != schema
        {
            return Err(Error::EvidenceMismatch);
        }
        if !receipt.ready {
            return Err(Error::ObservationUnverified);
        }
        Ok(())
    }
    /// Persist this Applying state durably BEFORE allowing any engine/data mutation.
    pub fn begin_update(&mut self, evidence: Preflight) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::Prepared {
            return Err(Error::InvalidTransition);
        }
        self.check_preflight(&evidence)?;
        self.preflight = Some(evidence);
        self.stage = Stage::Applying;
        Ok(())
    }
    /// Executor says its application step finished; this is NOT successful update proof.
    pub fn application_finished(&mut self) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::Applying {
            return Err(Error::InvalidTransition);
        }
        self.stage = Stage::AwaitingHealth;
        Ok(())
    }
    pub fn observe_health(&mut self, receipt: HealthReceipt) -> Result<(), Error> {
        self.validate()?;
        let rollback = match self.stage {
            Stage::AwaitingHealth => false,
            Stage::AwaitingRollbackHealth => true,
            _ => return Err(Error::InvalidTransition),
        };
        self.check_health(&receipt, rollback)?;
        if rollback && self.requires_data_restore() && self.restore.is_none() {
            return Err(Error::DataRestoreRequired);
        }
        self.health = Some(receipt);
        self.stage = if rollback {
            Stage::RolledBack
        } else {
            Stage::Updated
        };
        Ok(())
    }
    pub fn update_failed(&mut self) -> Result<(), Error> {
        self.validate()?;
        if !matches!(self.stage, Stage::Applying | Stage::AwaitingHealth) {
            return Err(Error::InvalidTransition);
        }
        self.failure = Some(if self.stage == Stage::Applying {
            Failure::UpdateFailed
        } else {
            Failure::HealthFailed
        });
        self.stage = Stage::RecoveryRequired;
        Ok(())
    }
    /// A completed update may be explicitly reverted; failure does not imply prior success.
    pub fn request_rollback(&mut self) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::Updated {
            return Err(Error::InvalidTransition);
        }
        self.health = None;
        self.stage = Stage::RecoveryRequired;
        Ok(())
    }
    /// Persist Restoring before separate-candidate restore. Never overwrite original data.
    pub fn begin_restore(&mut self, candidate_id: String) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::RecoveryRequired {
            return Err(Error::InvalidTransition);
        }
        if !identifier(&candidate_id)
            || candidate_id == self.plan.source_instance
            || candidate_id == self.plan.target_instance
        {
            return Err(Error::EvidenceMismatch);
        }
        self.restore = None;
        self.restore_candidate = Some(candidate_id);
        self.stage = Stage::Restoring;
        Ok(())
    }
    pub fn restore_finished(&mut self, receipt: RestoreReceipt) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::Restoring {
            return Err(Error::InvalidTransition);
        }
        self.check_restore(&receipt)?;
        self.restore = Some(receipt);
        self.stage = Stage::AwaitingRollbackHealth;
        Ok(())
    }
    pub fn image_only_rollback(&mut self) -> Result<(), Error> {
        self.validate()?;
        if self.stage != Stage::RecoveryRequired {
            return Err(Error::InvalidTransition);
        }
        if self.requires_data_restore() {
            return Err(Error::DataRestoreRequired);
        }
        self.stage = Stage::AwaitingRollbackHealth;
        Ok(())
    }
    pub fn recovery_failed(&mut self) -> Result<(), Error> {
        self.validate()?;
        if !matches!(self.stage, Stage::Restoring | Stage::AwaitingRollbackHealth) {
            return Err(Error::InvalidTransition);
        }
        self.failure = Some(if self.stage == Stage::Restoring {
            Failure::RestoreFailed
        } else {
            Failure::HealthFailed
        });
        self.restore = None;
        self.restore_candidate = None;
        self.stage = Stage::RecoveryRequired;
        Ok(())
    }
    /// After restart, in-flight work is never presumed successful or automatically replayed.
    pub fn recover_after_restart(&mut self) -> Result<(), Error> {
        self.validate()?;
        if matches!(
            self.stage,
            Stage::Applying
                | Stage::AwaitingHealth
                | Stage::Restoring
                | Stage::AwaitingRollbackHealth
        ) {
            self.stage = Stage::RecoveryRequired;
            self.failure = Some(Failure::Interrupted);
            self.restore = None;
            self.restore_candidate = None;
            self.health = None;
        }
        Ok(())
    }
    /// Explicit validation also protects callers who use serde directly instead of from_json.
    pub fn validate(&self) -> Result<(), Error> {
        self.plan.validate()?;
        if self.version != MODEL_VERSION {
            return Err(Error::InvalidRecord);
        }
        if self.stage == Stage::Prepared {
            if self.preflight.is_some()
                || self.restore.is_some()
                || self.restore_candidate.is_some()
                || self.health.is_some()
                || self.failure.is_some()
            {
                return Err(Error::InvalidRecord);
            }
        } else {
            self.check_preflight(self.preflight.as_ref().ok_or(Error::InvalidRecord)?)?;
        }
        if let Some(candidate) = &self.restore_candidate
            && (!identifier(candidate)
                || candidate == &self.plan.source_instance
                || candidate == &self.plan.target_instance
                || !matches!(
                    self.stage,
                    Stage::Restoring | Stage::AwaitingRollbackHealth | Stage::RolledBack
                ))
        {
            return Err(Error::InvalidRecord);
        }
        if self.stage == Stage::Restoring && self.restore_candidate.is_none() {
            return Err(Error::InvalidRecord);
        }
        if self.restore_candidate.is_some()
            && self.stage != Stage::Restoring
            && self.restore.is_none()
        {
            return Err(Error::InvalidRecord);
        }
        if let Some(restore) = &self.restore {
            self.check_restore(restore)?;
        }
        let terminal = matches!(self.stage, Stage::Updated | Stage::RolledBack);
        if terminal != self.health.is_some() {
            return Err(Error::InvalidRecord);
        }
        if let Some(health) = &self.health {
            self.check_health(health, self.stage == Stage::RolledBack)?;
        }
        if self.restore.is_some()
            && !matches!(
                self.stage,
                Stage::AwaitingRollbackHealth | Stage::RolledBack
            )
        {
            return Err(Error::InvalidRecord);
        }
        if matches!(
            self.stage,
            Stage::AwaitingRollbackHealth | Stage::RolledBack
        ) && self.requires_data_restore()
            && self.restore.is_none()
        {
            return Err(Error::DataRestoreRequired);
        }
        if self.failure.is_some()
            && matches!(
                self.stage,
                Stage::Prepared | Stage::Applying | Stage::AwaitingHealth | Stage::Updated
            )
        {
            return Err(Error::InvalidRecord);
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InvalidRecord)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(Error::InvalidRecord);
        }
        Ok(bytes)
    }
    /// Strictly validates persisted data and marks in-flight operations Interrupted.
    /// No filesystem or executor activity is performed; persist the returned state.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(Error::InvalidRecord);
        }
        let mut value: Self = serde_json::from_slice(bytes).map_err(|_| Error::InvalidRecord)?;
        value.recover_after_restart()?;
        Ok(value)
    }
}
