// SPDX-License-Identifier: Apache-2.0
//! Legacy candidate staging and fixed schema capability boundaries. No selection
//! change, source writer handoff, service activation or origin rewrite occurs here.
use super::*;
#[path = "native_legacy_apply.rs"]
mod application;
pub(crate) use application::AppliedSchema;
use crate::{
    LifecycleService,
    hosted_runtime_file::Pin,
    installations::{
        native_backup::{resource_budget, text},
        new_directory, private_directory,
    },
    maintenance_source::DirectoryPin,
    native_runtime::Session,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MARKER: &str = "native-legacy-migration-current.json";
fn changed() -> LifecycleError {
    err("NATIVE_LEGACY_STAGE_CHANGED")
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(changed()),
    }
}
fn sync(root: &Path) -> Result<()> {
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| changed())
}
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<(T, Pin)> {
    let (bytes, pin) = super::codec::private_read(path, 32768)?;
    Ok((
        serde_json::from_slice(&bytes.0).map_err(|_| changed())?,
        pin,
    ))
}
fn write_new<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| changed())?;
    crate::write_private_new(root, name, &bytes, 32768)?;
    sync(root)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    format: u32,
    id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    format: u32,
    id: String,
    target: PathBuf,
    archive: PathBuf,
    original_root: Option<PathBuf>,
    backup_id: String,
    manifest_sha256: String,
    cipher_manifest_sha256: String,
    payload_sha256: String,
    environment_sha256: String,
    original_origin: String,
    ports: Ports,
}
impl Intent {
    fn matches(
        &self,
        source: &AuthenticatedLegacyConfig,
        target: &Path,
        payload: &Payload,
    ) -> Result<()> {
        if self.format != 1
            || !crate::installations::uuid(&self.id)
            || self.target != target
            || self.archive != source.evidence.archive_root
            || self.original_root != source.evidence.original_root
            || self.backup_id != source.configuration.backup_id
            || self.manifest_sha256 != source.evidence.authenticated_manifest_sha256
            || self.cipher_manifest_sha256 != source.evidence.archive_manifest_sha256
            || self.payload_sha256 != payload.context().manifest_sha256
            || !hash_valid(&self.environment_sha256)
            || self.original_origin != source.configuration.original_origin
        {
            return Err(changed());
        }
        target_location_requirements(
            &source.configuration,
            &self.archive,
            self.original_root.as_deref(),
            target,
            self.ports,
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configured {
    format: u32,
    id: String,
    owner_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Database {
    format: u32,
    id: String,
    system_identifier: String,
}
fn system_identifier(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n > 0)
        && value.bytes().all(|b| b.is_ascii_digit())
        && !value.starts_with('0')
        && value.len() <= 20
}
impl Database {
    fn matches(&self, id: &str, system: &str) -> bool {
        self.format == 1
            && self.id == id
            && system_identifier(&self.system_identifier)
            && self.system_identifier == system
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Staged {
    format: u32,
    id: String,
    attempt: String,
    system_identifier: String,
    manifest_sha256: String,
    payload_sha256: String,
    environment_sha256: String,
    signing_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InspectionMigration {
    name: String,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InspectionFrame {
    format: u32,
    operation: String,
    candidate_id: String,
    backup_id: String,
    manifest_sha256: String,
    payload_sha256: String,
    system_identifier: String,
    source_inventory_sha256: String,
    source_schema_sha256: String,
    target_schema_sha256: String,
    target_migrations: Vec<InspectionMigration>,
    original_inventory_verified_after_rollback: bool,
}
/// In-memory authority is tied to the still-authenticated candidate and held
/// configuration identities. A serialized inspection cannot construct this.
pub(crate) struct SchemaInspection<'a> {
    candidate: &'a Candidate,
    frame: InspectionFrame,
    pins: Vec<Pin>,
}
impl SchemaInspection<'_> {
    pub(crate) fn summary(&self) -> Result<serde_json::Value> {
        self.candidate.check()?;
        self.candidate.payload.recheck()?;
        for pin in &self.pins {
            pin.check()?;
        }
        serde_json::to_value(&self.frame).map_err(|_| changed())
    }
}
impl Staged {
    fn matches(&self, intent: &Intent, system: &str, signing: &str) -> bool {
        self.format == 1
            && self.id == intent.id
            && crate::installations::uuid(&self.attempt)
            && self.system_identifier == system
            && system_identifier(system)
            && self.manifest_sha256 == intent.manifest_sha256
            && self.payload_sha256 == intent.payload_sha256
            && self.environment_sha256 == intent.environment_sha256
            && self.signing_sha256 == signing
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageSummary {
    id: String,
    backup_id: String,
    state: String,
}
struct NativeFields(BTreeMap<String, String>);
impl Drop for NativeFields {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
/// Cannot be deserialized or opened using an intent/receipt alone. Every reopen
/// consumes a new real codec-issued authentication capability.
pub(crate) struct Candidate {
    source: AuthenticatedLegacyConfig,
    intent: Intent,
    service: LifecycleService,
    payload: Arc<Payload>,
    job: PathBuf,
    root_pin: DirectoryPin,
    job_pin: DirectoryPin,
    pins: Vec<Pin>,
}
impl VerifiedMigrationPlan {
    pub(crate) fn prepare_candidate(self, payload: Arc<Payload>) -> Result<Candidate> {
        self.recheck()?;
        payload.recheck()?;
        if self.summary.target_payload_sha256 != payload.context().manifest_sha256 {
            return Err(changed());
        }
        let target = self.summary.target.clone();
        let parent_pin = DirectoryPin::open(target.parent().ok_or_else(changed)?)?;
        new_directory(&target)?;
        parent_pin.check()?;
        let root_pin = DirectoryPin::open(&target)?;
        let id = uuid::Uuid::new_v4().to_string();
        // First durable file blocks all service starts, including preparation
        // interruptions. An incomplete pre-intent candidate is retained/refused.
        write_new(
            &target,
            MARKER,
            &Pending {
                format: 1,
                id: id.clone(),
            },
        )?;
        let job = target.join(format!("legacy-migration-{id}"));
        new_directory(&job)?;
        crate::write_private_new(
            &job,
            "projected-runtime.env",
            &self.target_environment.0,
            8192,
        )?;
        sync(&job)?;
        let intent = Intent {
            format: 1,
            id,
            target: target.clone(),
            archive: self.source.evidence.archive_root.clone(),
            original_root: self.source.evidence.original_root.clone(),
            backup_id: self.summary.source_backup_id,
            manifest_sha256: self.summary.source_manifest_sha256,
            cipher_manifest_sha256: self.source.evidence.archive_manifest_sha256.clone(),
            payload_sha256: self.summary.target_payload_sha256,
            environment_sha256: crate::digest(&self.target_environment.0),
            original_origin: self.summary.original_origin,
            ports: self.summary.ports,
        };
        self.source.evidence.check()?;
        root_pin.check()?;
        write_new(&job, "intent.json", &intent)?;
        sync(&target)?;
        Candidate::open(self.source, payload, target)
    }
}
impl AuthenticatedLegacyConfig {
    /// Resume retains the exact persisted candidate secret; into_plan() is never
    /// called here and no owner, tenant or database credential is regenerated.
    pub(crate) fn resume_candidate(
        self,
        payload: Arc<Payload>,
        target: PathBuf,
    ) -> Result<Candidate> {
        Candidate::open(self, payload, target)
    }
}
impl Candidate {
    fn open(
        source: AuthenticatedLegacyConfig,
        payload: Arc<Payload>,
        target: PathBuf,
    ) -> Result<Self> {
        source.evidence.check()?;
        private_directory(&target)?;
        let root_pin = DirectoryPin::open(&target)?;
        let (pending, marker_pin) = read::<Pending>(&target.join(MARKER))?;
        if pending.format != 1 || !crate::installations::uuid(&pending.id) {
            return Err(changed());
        }
        let job = target.join(format!("legacy-migration-{}", pending.id));
        private_directory(&job)?;
        let (intent, intent_pin) = read::<Intent>(&job.join("intent.json"))?;
        intent.matches(&source, &target, &payload)?;
        if intent.id != pending.id {
            return Err(changed());
        }
        let job_pin = DirectoryPin::open(&job)?;
        let (environment, environment_pin) =
            super::codec::private_read(&job.join("projected-runtime.env"), 8192)?;
        if crate::digest(&environment.0) != intent.environment_sha256 {
            return Err(changed());
        }
        let credentials =
            publication_source::parse(&environment.0, intent.original_origin.clone())?;
        if credentials.tenant_id != source.configuration.credentials.tenant_id
            || credentials.subject != source.configuration.credentials.subject
            || credentials.password() != source.configuration.credentials.password()
        {
            return Err(changed());
        }
        let service = LifecycleService::bound_existing(target)?;
        let mut candidate = Self {
            source,
            intent,
            service,
            payload,
            job,
            root_pin,
            job_pin,
            pins: vec![marker_pin, intent_pin, environment_pin],
        };
        candidate.check()?;
        candidate.payload.recheck()?;
        if exists(&candidate.service.root.join("runtime.env"))? {
            let (bytes, pin) =
                super::codec::private_read(&candidate.service.root.join("runtime.env"), 8192)?;
            if bytes.0 != environment.0 {
                return Err(changed());
            }
            candidate.pins.push(pin);
        } else {
            crate::write_private_new(&candidate.service.root, "runtime.env", &environment.0, 8192)?;
            sync(&candidate.service.root)?;
        }
        candidate.check()?;
        // Existing API with input=None validates already-projected credentials.
        // It never enters its new-account branch when runtime.env exists.
        let prepared = candidate.service.prepare_native_installation(
            &candidate.payload,
            candidate.intent.ports,
            None,
        )?;
        if prepared.kind != "legacy_account"
            || prepared.tenant_id != candidate.source.configuration.credentials.tenant_id
            || prepared.subject != candidate.source.configuration.credentials.subject
        {
            return Err(changed());
        }
        let _fields = NativeFields(
            candidate
                .service
                .native_start_environment(&candidate.payload, candidate.intent.ports)?,
        );
        let (env, env_pin) =
            super::codec::private_read(&candidate.service.root.join("runtime.env"), 8192)?;
        let (owner, owner_pin) = super::codec::private_read(
            &candidate.service.root.join("native-installation.json"),
            4096,
        )?;
        if crate::digest(&env.0) != candidate.intent.environment_sha256 {
            return Err(changed());
        }
        let owner_sha256 = crate::digest(&owner.0);
        if exists(&candidate.job.join("configured.json"))? {
            let (configured, pin) = read::<Configured>(&candidate.job.join("configured.json"))?;
            if configured.format != 1
                || configured.id != candidate.intent.id
                || configured.owner_sha256 != owner_sha256
            {
                return Err(changed());
            }
            candidate.pins.push(pin);
        } else {
            write_new(
                &candidate.job,
                "configured.json",
                &Configured {
                    format: 1,
                    id: candidate.intent.id.clone(),
                    owner_sha256,
                },
            )?;
        }
        candidate.pins.extend([env_pin, owner_pin]);
        candidate.check()?;
        Ok(candidate)
    }
    fn check(&self) -> Result<()> {
        self.source.evidence.check()?;
        self.intent
            .matches(&self.source, &self.service.root, &self.payload)?;
        private_directory(&self.service.root)?;
        private_directory(&self.job)?;
        self.root_pin.check()?;
        self.job_pin.check()?;
        for pin in &self.pins {
            pin.check()?;
        }
        Ok(())
    }
    fn restored_configuration(&self, restored: &Path) -> Result<Vec<Pin>> {
        let (manifest, pin) =
            super::codec::private_read(&restored.join("manifest.json"), 16 * 1024 * 1024)?;
        if crate::digest(&manifest.0) != self.intent.manifest_sha256 {
            return Err(changed());
        }
        let configuration = restored.join("configuration");
        private_directory(&configuration)?;
        let names: BTreeSet<_> = fs::read_dir(&configuration)
            .map_err(|_| changed())?
            .map(|entry| {
                entry
                    .map_err(|_| changed())
                    .and_then(|e| e.file_name().into_string().map_err(|_| changed()))
            })
            .collect::<Result<_>>()?;
        if names != CONFIGURATION.into_iter().map(str::to_owned).collect() {
            return Err(changed());
        }
        let mut pins = vec![pin];
        for name in CONFIGURATION {
            let (bytes, pin) = super::codec::private_read(&configuration.join(name), 1024 * 1024)?;
            if self.source.configuration.configuration_sha256.get(name)
                != Some(&crate::digest(&bytes.0))
            {
                return Err(changed());
            }
            pins.push(pin);
        }
        Ok(pins)
    }
    fn observe(
        &self,
        session: &mut Session,
        restored: &Path,
        system: &str,
        budget: u64,
        scratch: &Path,
    ) -> Result<()> {
        self.check()?;
        self.payload.recheck()?;
        let pins = self.restored_configuration(restored)?;
        let value = session.backup_tool(
            vec![
                "check-restored-inventory".into(),
                "--manifest-file".into(),
                text(&restored.join("manifest.json"))?,
                "--manifest-sha256".into(),
                self.intent.manifest_sha256.clone(),
                "--snapshot-system-identifier".into(),
                system.into(),
                "--quiesced".into(),
            ],
            "{}".into(),
            "{}".into(),
            budget,
            scratch,
        )?;
        if value["operation"] != "restored-inventory-matched"
            || value["backupId"] != self.intent.backup_id
            || value["authenticatedManifestSha256"] != self.intent.manifest_sha256
        {
            return Err(changed());
        }
        let fields = BTreeMap::from([
            (
                "TENANT_ID".into(),
                self.source.configuration.credentials.tenant_id.clone(),
            ),
            (
                "ADMIN_SUBJECT".into(),
                self.source.configuration.credentials.subject.clone(),
            ),
        ]);
        session.maintenance_owner("legacy_account", &fields)?;
        for pin in pins {
            pin.check()?;
        }
        self.check()
    }
    /// Exact owned PG-only Session, with the existing host cancellation handle.
    /// Every exit path drains it normally before returning or persisting success.
    pub(crate) fn stage(
        &mut self,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<StageSummary> {
        self.check()?;
        self.payload.recheck()?;
        let free = fs2::available_space(&self.service.root).map_err(|_| changed())?;
        let budget = resource_budget(free, free)?;
        if self
            .source
            .evidence
            .archive
            .bounded_cipher_bytes()?
            .checked_mul(4)
            .is_none_or(|bytes| bytes > budget)
        {
            return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
        }
        let mut session = Session::new(&self.service, self.payload.clone(), self.intent.ports)?;
        let cancellation = session.cancellation();
        publish_cancel(cancellation.clone());
        let mut durable_pins = Vec::new();
        let result = (|| {
            session.start_database_only_cancellable()?;
            let system = session.maintenance_system_identifier()?;
            let empty = session.maintenance_empty_target()?;
            if exists(&self.job.join("database.json"))? {
                let (database, pin) = read::<Database>(&self.job.join("database.json"))?;
                if !database.matches(&self.intent.id, &system) {
                    return Err(changed());
                }
                pin.check()?;
                durable_pins.push(pin);
            } else {
                if !empty {
                    return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));
                }
                self.check()?;
                write_new(
                    &self.job,
                    "database.json",
                    &Database {
                        format: 1,
                        id: self.intent.id.clone(),
                        system_identifier: system.clone(),
                    },
                )?;
                let (_, pin) = read::<Database>(&self.job.join("database.json"))?;
                durable_pins.push(pin);
            }
            let scratch = self.job.join("scratch");
            if exists(&scratch)? {
                private_directory(&scratch)?;
            } else {
                new_directory(&scratch)?;
            }
            let signing_sha256 = crate::digest(&self.source.configuration.freeze_signing_key.0);
            let checkpoint = if exists(&self.job.join("staged.json"))? {
                let (staged, pin) = read::<Staged>(&self.job.join("staged.json"))?;
                if !staged.matches(&self.intent, &system, &signing_sha256) {
                    return Err(changed());
                }
                pin.check()?;
                durable_pins.push(pin);
                staged
            } else {
                let attempt = uuid::Uuid::new_v4().to_string();
                let restored = self.job.join(format!("restored-{attempt}"));
                self.check()?;
                self.payload.recheck()?;
                let operation = if empty { "restore" } else { "reconcile" };
                write_new(
                    &self.job,
                    &format!("attempt-{attempt}.json"),
                    &serde_json::json!({"format":1,"id":self.intent.id,"attempt":attempt,"operation":operation,"systemIdentifier":system}),
                )?;
                let value = session.backup_tool(
                    vec![
                        operation.into(),
                        "--key-file".into(),
                        text(&self.source.evidence.key_path)?,
                        "--source".into(),
                        text(&self.source.evidence.archive_root)?,
                        "--destination".into(),
                        text(&restored)?,
                        "--quiesced".into(),
                        "--fresh-destination".into(),
                    ],
                    "{}".into(),
                    "{}".into(),
                    budget,
                    &scratch,
                )?;
                if value != serde_json::json!({"operation":"restored-and-verified"}) {
                    return Err(changed());
                }
                Staged {
                    format: 1,
                    id: self.intent.id.clone(),
                    attempt,
                    system_identifier: system.clone(),
                    manifest_sha256: self.intent.manifest_sha256.clone(),
                    payload_sha256: self.intent.payload_sha256.clone(),
                    environment_sha256: self.intent.environment_sha256.clone(),
                    signing_sha256,
                }
            };
            self.observe(
                &mut session,
                &self.job.join(format!("restored-{}", checkpoint.attempt)),
                &system,
                budget,
                &scratch,
            )?;
            if cancellation.load(Ordering::SeqCst) {
                return Err(err("NATIVE_CANCELLED"));
            }
            Ok(checkpoint)
        })();
        loop {
            let stopped = session.stop()?;
            if stopped.postgres_pid.is_none()
                && stopped.node_pid.is_none()
                && stopped.auxiliary_pid.is_none()
            {
                break;
            }
        }
        let checkpoint = result?;
        for pin in durable_pins {
            pin.check()?;
        }
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        let _guard = self.service.lock()?;
        self.check()?;
        self.payload.recheck()?;
        let activation = self.job.join("activation");
        if exists(&activation)? {
            private_directory(&activation)?;
        } else {
            new_directory(&activation)?;
        }
        let signing = activation.join("freeze-signing-key.json");
        if exists(&signing)? {
            let (bytes, pin) = super::codec::private_read(&signing, 65536)?;
            if bytes.0 != self.source.configuration.freeze_signing_key.0 {
                return Err(changed());
            }
            pin.check()?;
        } else {
            crate::write_private_new(
                &activation,
                "freeze-signing-key.json",
                &self.source.configuration.freeze_signing_key.0,
                65536,
            )?;
            sync(&activation)?;
        }
        self.check()?;
        crate::write_json(&self.job, "staged.json", &checkpoint)?;
        sync(&self.job)?;
        Ok(StageSummary {
            id: self.intent.id.clone(),
            backup_id: self.intent.backup_id.clone(),
            state: "inventory_verified_not_activated".into(),
        })
    }
    /// Inspect only the pinned additive 015 -> 016 schema transition. The on-disk
    /// result is a journal, never a capability to commit or resume activation.
    pub(crate) fn inspect_schema(
        &self,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<SchemaInspection<'_>> {
        self.check()?;
        self.payload.recheck()?;
        let (staged, staged_pin) = read::<Staged>(&self.job.join("staged.json"))?;
        let (database, database_pin) = read::<Database>(&self.job.join("database.json"))?;
        let signing = crate::digest(&self.source.configuration.freeze_signing_key.0);
        if !staged.matches(&self.intent, &database.system_identifier, &signing)
            || !database.matches(&self.intent.id, &staged.system_identifier)
        {
            return Err(changed());
        }
        let restored = self.job.join(format!("restored-{}", staged.attempt));
        let mut configuration_pins = self.restored_configuration(&restored)?;
        let mut session = Session::new(&self.service, self.payload.clone(), self.intent.ports)?;
        let cancellation = session.cancellation();
        publish_cancel(cancellation.clone());
        let result = (|| {
            session.start_database_only_cancellable()?;
            let system = session.maintenance_system_identifier()?;
            if !database.matches(&self.intent.id, &system) {
                return Err(changed());
            }
            self.check()?;
            let request = serde_json::to_vec(&serde_json::json!({
                "candidateId": self.intent.id, "backupId": self.intent.backup_id,
                "manifestPath": restored.join("manifest.json"),
                "manifestSha256": self.intent.manifest_sha256,
                "payloadSha256": self.intent.payload_sha256, "systemIdentifier": system,
            }))
            .map_err(|_| changed())?;
            let value = session.inspect_legacy_schema(&request)?;
            let inspection: InspectionFrame =
                serde_json::from_value(value).map_err(|_| changed())?;
            if inspection.format != 1
                || inspection.operation != "legacy-schema-inspected-not-applied"
                || inspection.candidate_id != self.intent.id
                || inspection.backup_id != self.intent.backup_id
                || inspection.manifest_sha256 != self.intent.manifest_sha256
                || inspection.payload_sha256 != self.intent.payload_sha256
                || inspection.system_identifier != system
                || !inspection.original_inventory_verified_after_rollback
                || !hash_valid(&inspection.source_inventory_sha256)
                || !hash_valid(&inspection.source_schema_sha256)
                || !hash_valid(&inspection.target_schema_sha256)
                || inspection.target_schema_sha256 == inspection.source_schema_sha256
                || inspection.target_migrations.len() != 16
                || inspection
                    .target_migrations
                    .last()
                    .is_none_or(|m| m.name != "016_local_workspaces.sql")
                || inspection
                    .target_migrations
                    .iter()
                    .any(|m| !hash_valid(&m.sha256))
            {
                return Err(changed());
            }
            session.maintenance_owner(
                "legacy_account",
                &BTreeMap::from([
                    (
                        "TENANT_ID".into(),
                        self.source.configuration.credentials.tenant_id.clone(),
                    ),
                    (
                        "ADMIN_SUBJECT".into(),
                        self.source.configuration.credentials.subject.clone(),
                    ),
                ]),
            )?;
            Ok(inspection)
        })();
        loop {
            let stopped = session.stop()?;
            if stopped.postgres_pid.is_none()
                && stopped.node_pid.is_none()
                && stopped.auxiliary_pid.is_none()
            {
                break;
            }
        }
        let inspection = result?;
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        for pin in &configuration_pins {
            pin.check()?;
        }
        staged_pin.check()?;
        database_pin.check()?;
        let _guard = self.service.lock()?;
        self.check()?;
        self.payload.recheck()?;
        crate::write_json(&self.job, "schema-inspection.json", &inspection)?;
        sync(&self.job)?;
        configuration_pins.extend([staged_pin, database_pin]);
        Ok(SchemaInspection {
            candidate: self,
            frame: inspection,
            pins: configuration_pins,
        })
    }
    /// Preparation rollback is retention, never deletion or source activation.
    pub(crate) fn retain(&self) -> Result<StageSummary> {
        let _guard = self.service.lock()?;
        self.check()?;
        crate::write_json(
            &self.job,
            "retained.json",
            &serde_json::json!({"format":1,"id":self.intent.id,"state":"candidate_retained_not_activated"}),
        )?;
        sync(&self.job)?;
        Ok(StageSummary {
            id: self.intent.id.clone(),
            backup_id: self.intent.backup_id.clone(),
            state: "candidate_retained_not_activated".into(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires real authentication of the retained synthetic legacy backup, its existing stopped staged candidate, admitted runtime/inspection capsule, fresh workspace and one heavy owner"]
    fn actual_legacy_schema_inspection_rolls_back_same_candidate() {
        let input = |name| {
            let path = PathBuf::from(std::env::var_os(name).expect("explicit owned fixture input"));
            assert!(path.is_absolute() && fs::canonicalize(&path).unwrap() == path);
            path
        };
        let payload = Arc::new(Payload::open(&input("EXHIBITOS_LEGACY_TEST_PAYLOAD")).unwrap());
        let workspace = input("EXHIBITOS_LEGACY_TEST_WORKSPACE");
        private_directory(&workspace).unwrap();
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
        let target = input("EXHIBITOS_LEGACY_INSPECTION_TEST_TARGET");
        let source = super::super::authenticate(
            &payload,
            &input("EXHIBITOS_LEGACY_TEST_ARCHIVE"),
            &input("EXHIBITOS_LEGACY_TEST_KEY"),
            &workspace,
            Some(&input("EXHIBITOS_LEGACY_TEST_ORIGINAL")),
            &AtomicBool::new(false),
        )
        .unwrap();
        let candidate = source.resume_candidate(payload, target.clone()).unwrap();
        let secret_hash = candidate.intent.environment_sha256.clone();
        let (before_stage, _) = read::<Staged>(&candidate.job.join("staged.json")).unwrap();
        let cancelled = candidate
            .inspect_schema(|flag| flag.store(true, Ordering::SeqCst))
            .err()
            .expect("inspection cancellation must refuse");
        assert_eq!(cancelled.code, "NATIVE_CANCELLED");
        assert!(!candidate.job.join("schema-inspection.json").exists());
        let inspection = candidate.inspect_schema(|_| {}).unwrap();
        let result = inspection.summary().unwrap();
        assert_eq!(result["operation"], "legacy-schema-inspected-not-applied");
        assert_eq!(result["originalInventoryVerifiedAfterRollback"], true);
        assert_eq!(result["systemIdentifier"], before_stage.system_identifier);
        assert_eq!(result["targetMigrations"].as_array().unwrap().len(), 16);
        assert_ne!(result["sourceSchemaSha256"], result["targetSchemaSha256"]);
        let (persisted, _) =
            read::<serde_json::Value>(&candidate.job.join("schema-inspection.json")).unwrap();
        assert_eq!(persisted, result);
        let (after_stage, _) = read::<Staged>(&candidate.job.join("staged.json")).unwrap();
        assert_eq!(before_stage.attempt, after_stage.attempt);
        assert_eq!(
            before_stage.system_identifier,
            after_stage.system_identifier
        );
        let (environment, _) =
            super::super::codec::private_read(&target.join("runtime.env"), 8192).unwrap();
        assert_eq!(crate::digest(&environment.0), secret_hash);
        assert!(target.join(MARKER).exists());
        assert!(!target.join("config/freeze-signing-key.json").exists());
        let (current, _) =
            read::<serde_json::Value>(&target.join("native-current-run.json")).unwrap();
        let (state, _) = read::<crate::native_runtime::Context>(
            &target
                .join("native-runs")
                .join(current["id"].as_str().unwrap())
                .join("state.json"),
        )
        .unwrap();
        assert!(
            state.postgres_pid.is_none()
                && state.node_pid.is_none()
                && state.auxiliary_pid.is_none()
        );
        assert_eq!(state.exits.get("postgres"), Some(&0));
        assert_eq!(state.exits.get("auxiliary"), Some(&0));
        assert!(!state.readiness.ready);
        candidate.check().unwrap();
        inspection.summary().unwrap();
        write_new(&workspace, "result.json", &serde_json::json!({"scope":"ACTUAL_LEGACY_SCHEMA_INSPECTION_ROLLBACK_NOT_APPLY_ACTIVATION_FREEZE_GUI_OR_RELEASE","inspection":result,"cancelBeforePG":true,"sameClusterAndStagePreserved":true,"samePersistedDatabaseSecret":true,"ownedChildrenDrained":true,"activationStillBlocked":true})).unwrap();
    }
    #[test]
    fn resume_database_requires_exact_persisted_cluster_and_intent() {
        let id = uuid::Uuid::new_v4().to_string();
        let mut database = Database {
            format: 1,
            id: id.clone(),
            system_identifier: "1234567890123456789".into(),
        };
        assert!(database.matches(&id, "1234567890123456789"));
        assert!(!database.matches(&id, "1234567890123456790"));
        assert!(!database.matches(&uuid::Uuid::new_v4().to_string(), "1234567890123456789"));
        database.format = 2;
        assert!(!database.matches(&id, "1234567890123456789"));
        for value in ["0", "01", "+1", "-1", "18446744073709551616", "123\n"] {
            assert!(!system_identifier(value));
        }
    }
    #[test]
    fn marker_and_database_json_cannot_carry_credentials_or_source_authority() {
        assert!(serde_json::from_value::<Pending>(serde_json::json!({"format":1,"id":uuid::Uuid::new_v4().to_string(),"authenticated":true})).is_err());
        assert!(serde_json::from_value::<Database>(serde_json::json!({"format":1,"id":uuid::Uuid::new_v4().to_string(),"systemIdentifier":"123","password":"private"})).is_err());
        // No serde constructor exists for Candidate or AuthenticatedLegacyConfig.
    }
    #[test]
    #[ignore = "Requires the retained owned synthetic legacy backup/key, admitted R5 runtime/capsule, fresh private workspace and one global heavy owner"]
    fn actual_legacy_candidate_cancel_resume_and_lost_checkpoint_reconcile() {
        fn input(name: &str) -> PathBuf {
            let path = PathBuf::from(
                std::env::var_os(name).expect("explicit retained synthetic fixture input"),
            );
            assert!(path.is_absolute() && fs::canonicalize(&path).unwrap() == path);
            path
        }
        fn drained(target: &Path) {
            let (current, _) =
                read::<serde_json::Value>(&target.join("native-current-run.json")).unwrap();
            let id = current["id"].as_str().unwrap();
            assert!(crate::installations::uuid(id));
            let (state, _) = read::<crate::native_runtime::Context>(
                &target.join("native-runs").join(id).join("state.json"),
            )
            .unwrap();
            assert!(
                state.postgres_pid.is_none()
                    && state.node_pid.is_none()
                    && state.auxiliary_pid.is_none()
            );
            assert_eq!(state.exits.get("postgres"), Some(&0));
            assert!(!state.readiness.ready);
        }
        let payload = Arc::new(Payload::open(&input("EXHIBITOS_LEGACY_TEST_PAYLOAD")).unwrap());
        let archive = input("EXHIBITOS_LEGACY_TEST_ARCHIVE");
        let key = input("EXHIBITOS_LEGACY_TEST_KEY");
        let original = input("EXHIBITOS_LEGACY_TEST_ORIGINAL");
        let workspace = input("EXHIBITOS_LEGACY_TEST_WORKSPACE");
        let signing = input("EXHIBITOS_LEGACY_TEST_SIGNING_WITNESS");
        private_directory(&workspace).unwrap();
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
        let (original_environment, original_environment_pin) =
            super::super::codec::private_read(&original.join("runtime.env"), 8192).unwrap();
        let (original_signing, original_signing_pin) =
            super::super::codec::private_read(&signing, 65536).unwrap();
        let authenticate_source = || {
            super::super::authenticate(
                &payload,
                &archive,
                &key,
                &workspace,
                Some(&original),
                &AtomicBool::new(false),
            )
            .unwrap()
        };
        let source = authenticate_source();
        let expected_id = std::env::var("EXHIBITOS_LEGACY_TEST_BACKUP_ID").unwrap();
        let expected_sha = std::env::var("EXHIBITOS_LEGACY_TEST_MANIFEST_SHA256").unwrap();
        assert!(
            source.configuration.backup_id == expected_id
                && source.configuration.manifest_sha256 == expected_sha
        );
        let api = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let database = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let ports = Ports {
            web: source.configuration.manifest.ports[0],
            api: api.local_addr().unwrap().port(),
            database: database.local_addr().unwrap().port(),
        };
        assert!(ports.api != ports.web && ports.database != ports.web);
        drop(api);
        drop(database);
        let target = workspace.join("legacy-native-candidate");
        let plan = source.into_plan(target.clone(), &payload, ports).unwrap();
        let mut candidate = plan.prepare_candidate(payload.clone()).unwrap();
        let id = candidate.intent.id.clone();
        let job = candidate.job.clone();
        let secret_hash = candidate.intent.environment_sha256.clone();
        assert!(target.join(MARKER).exists() && !target.join("native-database").exists());
        let cancelled = candidate
            .stage(|flag| flag.store(true, Ordering::SeqCst))
            .err()
            .expect("cancellation must refuse staging");
        assert_eq!(cancelled.code, "NATIVE_CANCELLED");
        assert!(!target.join("native-database").exists());
        assert!(!target.join("native-current-run.json").exists());
        drop(candidate);
        let mut candidate = authenticate_source()
            .resume_candidate(payload.clone(), target.clone())
            .unwrap();
        assert!(candidate.intent.id == id && candidate.intent.environment_sha256 == secret_hash);
        let first = candidate.stage(|_| {}).unwrap();
        assert_eq!(first.state, "inventory_verified_not_activated");
        assert!(first.id == id && first.backup_id == expected_id);
        drained(&target);
        let (projected, _) =
            super::super::codec::private_read(&target.join("runtime.env"), 8192).unwrap();
        assert!(crate::digest(&projected.0) == secret_hash);
        let origin = candidate.intent.original_origin.clone();
        let source_credentials =
            publication_source::parse(&original_environment.0, origin.clone()).unwrap();
        let target_credentials = publication_source::parse(&projected.0, origin).unwrap();
        assert!(
            target_credentials.tenant_id == source_credentials.tenant_id
                && target_credentials.subject == source_credentials.subject
                && target_credentials.password() == source_credentials.password()
        );
        let (staged_signing, signing_pin) = super::super::codec::private_read(
            &job.join("activation/freeze-signing-key.json"),
            65536,
        )
        .unwrap();
        assert!(staged_signing.0 == original_signing.0);
        signing_pin.check().unwrap();
        assert!(!target.join("config/freeze-signing-key.json").exists());
        let (first_stage, _) = read::<Staged>(&job.join("staged.json")).unwrap();
        fs::rename(
            job.join("staged.json"),
            job.join("stage-before-checkpoint-fault.json"),
        )
        .unwrap();
        drop(candidate);
        let mut candidate = authenticate_source()
            .resume_candidate(payload.clone(), target.clone())
            .unwrap();
        assert!(candidate.intent.id == id && candidate.intent.environment_sha256 == secret_hash);
        let second = candidate.stage(|_| {}).unwrap();
        assert!(second.id == id);
        drained(&target);
        let (second_stage, _) = read::<Staged>(&job.join("staged.json")).unwrap();
        assert!(
            second_stage.system_identifier == first_stage.system_identifier
                && second_stage.attempt != first_stage.attempt
        );
        let (attempt, _) =
            read::<serde_json::Value>(&job.join(format!("attempt-{}.json", second_stage.attempt)))
                .unwrap();
        assert_eq!(attempt["operation"], "reconcile");
        let (persisted, _) =
            super::super::codec::private_read(&target.join("runtime.env"), 8192).unwrap();
        assert!(crate::digest(&persisted.0) == secret_hash);
        let mut blocked = Session::new(&candidate.service, payload.clone(), ports).unwrap();
        assert_eq!(
            blocked.start().err().unwrap().code,
            "NATIVE_RESTORE_ACTIVATION_REQUIRED"
        );
        assert_eq!(
            blocked.backup_available().err().unwrap().code,
            "NATIVE_RESTORE_PENDING"
        );
        drop(blocked);
        let retained = candidate.retain().unwrap();
        assert_eq!(retained.state, "candidate_retained_not_activated");
        assert!(target.join(MARKER).exists() && target.join("native-database").exists());
        assert!(!target.join("config/freeze-signing-key.json").exists());
        original_environment_pin.check().unwrap();
        original_signing_pin.check().unwrap();
        candidate.check().unwrap();
        let result = serde_json::json!({"scope":"ACTUAL_LEGACY_PG_STAGE_RECONCILE_NOT_ACTIVATION_SCHEMA_UPGRADE_GUI_OR_RELEASE","backupId":expected_id,"manifestSha256":expected_sha,"cancelBeforePG":true,"sameIdAndPersistedDatabaseSecretAfterReauthentication":true,"originalAccountPasswordAndSigningRetained":true,"sameClusterLostCheckpointReconciled":true,"reconcileAttemptRecorded":true,"fullServicesAndBackupBlocked":true,"ownedPostgresExitedZeroAndAllChildSlotsEmpty":true,"candidateRetainedAndNotActivated":true});
        write_new(&workspace, "result.json", &result).unwrap();
    }
}
