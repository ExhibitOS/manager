// SPDX-License-Identifier: Apache-2.0
//! Owned candidate application and actual health. Activation remains a separate gate.
use super::*;
use crate::{BundleManifest, run, run_observed, run_observed_input};
const LIMIT: usize = 256 * 1024;
const READER: &str = include_str!("candidate_execution_reader.mjs");

/// Actual candidate application/health observation, not selected-host activation.
/// Keeps every admission/recovery fence. No caller health receipt can construct it.
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::ReadyCandidate;
/// let replay = serde_json::from_str::<ReadyCandidate<'_, '_, '_>>("{}");
/// ```
/// ```compile_fail
/// use exhibitos_lifecycle::signed_release::trust::ReadyCandidate;
/// fn replay(ready: ReadyCandidate<'_, '_, '_>) {
///     let first = ready.activate();
///     let second = ready.activate();
/// }
/// ```
pub struct ReadyCandidate<'session, 'store, 'inputs> {
    started: StartedUpdate<'session, 'store, 'inputs>,
    health: crate::update::HealthReceipt,
    execution: CandidateState,
}
struct CandidateState {
    deployment: Deployment,
    database_image: String,
    containers: [String; 2],
    frozen: Vec<(String, String)>,
}
impl ReadyCandidate<'_, '_, '_> {
    pub fn receipt(&self) -> &Value {
        &self.started.admission.receipt
    }
    pub fn observed_health(&self) -> &crate::update::HealthReceipt {
        &self.health
    }
}
fn immutable(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err(err("UPDATE_DEPLOYMENT_INVALID"));
    }
    let mut file = private_file(&root.join(name), true).map_err(|e| err(e.code()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| err("UPDATE_DEPLOYMENT_UNCERTAIN"))?;
    sync_dir(root).map_err(|e| err(e.code()))
}
fn phase(root: &Path, name: &str, plan: &crate::update::Plan, generation: u64) -> Result<()> {
    immutable(root,&format!("{name}.json"),&serde_json::to_vec(&serde_json::json!({"format":1,"operationId":plan.operation_id,"targetInstance":plan.target_instance,"backupId":plan.backup_id,"applyingGeneration":generation,"phase":name})).map_err(|_|err("UPDATE_DEPLOYMENT_INVALID"))?)
}
fn replacement(
    manifest: &BundleManifest,
    config: &Value,
    plan: &crate::update::Plan,
) -> Result<(BundleManifest, Value)> {
    if plan.source_schema != plan.target_schema {
        return Err(err("UPDATE_RUNTIME_MIGRATION_UNQUALIFIED"));
    }
    let services = config["services"]
        .as_object()
        .filter(|s| s.len() == 2 && s.contains_key("platform") && s.contains_key("database"))
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    if manifest.services.len() != 2
        || !manifest.services.iter().any(|s| s == "platform")
        || !manifest.services.iter().any(|s| s == "database")
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let old = services["platform"]["image"]
        .as_str()
        .ok_or_else(|| err("UPDATE_DEPLOYMENT_INVALID"))?;
    if !crate::hash_valid(&plan.target_image)
        || manifest
            .images
            .iter()
            .filter(|i| i.reference == old)
            .count()
            != 1
    {
        return Err(err("UPDATE_DEPLOYMENT_INVALID"));
    }
    let mut config = config.clone();
    config["services"]["platform"]["image"] =
        serde_json::json!(format!("sha256:{}", plan.target_image));
    let bytes = serde_json::to_vec(&config).map_err(|_| err("UPDATE_DEPLOYMENT_INVALID"))?;
    if bytes.len() > LIMIT {
        return Err(err("UPDATE_DEPLOYMENT_INVALID"));
    }
    let mut next = manifest.clone();
    let image = next
        .images
        .iter_mut()
        .find(|i| i.reference == old)
        .ok_or_else(|| err("UPDATE_DEPLOYMENT_INVALID"))?;
    image.reference = format!("sha256:{}", plan.target_image);
    image.archive = None;
    next.compose_sha256 = crate::digest(&bytes);
    // Preserve the observed unchanged API/OED version; signed package version is
    // retained by the authority intent, not guessed from a packaging label.
    Ok((next, config))
}
struct Deployment {
    root: PathBuf,
    bundle_identity: Metadata,
    manifest: BundleManifest,
    compose: Value,
    originals: Vec<(String, Vec<u8>)>,
}
impl Deployment {
    fn stage(
        root: &Path,
        workspace: &Path,
        manifest: &BundleManifest,
        config: &Value,
        plan: &crate::update::Plan,
    ) -> Result<Self> {
        installations::private_directory(root)?;
        let bundle = root.join("bundle");
        installations::private_directory(&bundle)?;
        let (next, compose) = replacement(manifest, config, plan)?;
        let mut originals = Vec::new();
        for (path, name) in [
            ("bundle/manifest.json", "original-manifest.json"),
            ("bundle/compose.yaml", "original-compose.yaml"),
            ("installed.json", "original-installed.json"),
        ] {
            let bytes = crate::installation_backup::source_bytes(root, path, LIMIT as u64, true)?;
            immutable(workspace, name, &bytes)?;
            originals.push((path.to_owned(), bytes));
        }
        immutable(
            workspace,
            "new-compose.json",
            &serde_json::to_vec(&compose).map_err(|_| err("UPDATE_DEPLOYMENT_INVALID"))?,
        )?;
        immutable(
            workspace,
            "new-manifest.json",
            &serde_json::to_vec(&next).map_err(|_| err("UPDATE_DEPLOYMENT_INVALID"))?,
        )?;
        let staged = Self {
            root: root.to_owned(),
            bundle_identity: fs::symlink_metadata(&bundle)
                .map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
            manifest: next,
            compose,
            originals,
        };
        staged.check_originals()?;
        Ok(staged)
    }
    fn check_parent(&self) -> Result<()> {
        installations::private_directory(&self.root.join("bundle"))?;
        if !identity(
            &self.bundle_identity,
            &fs::symlink_metadata(self.root.join("bundle"))
                .map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
        ) {
            return Err(err("UPDATE_TARGET_CHANGED"));
        }
        Ok(())
    }
    fn check_originals(&self) -> Result<()> {
        self.check_parent()?;
        for (path, bytes) in &self.originals {
            if crate::installation_backup::source_bytes(&self.root, path, LIMIT as u64, true)?
                != *bytes
            {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
        }
        Ok(())
    }
    fn publish(&self) -> Result<()> {
        self.check_originals()?;
        crate::write_json(&self.root.join("bundle"), "compose.yaml", &self.compose)?;
        self.check_parent()?;
        crate::write_json(&self.root.join("bundle"), "manifest.json", &self.manifest)?;
        crate::write_json(&self.root, "installed.json", &self.manifest)?;
        sync_dir(&self.root.join("bundle"))
            .and_then(|_| sync_dir(&self.root))
            .map_err(|e| err(e.code()))?;
        self.check_published()
    }
    fn check_published(&self) -> Result<()> {
        self.check_parent()?;
        for (path, bytes) in [
            ("bundle/compose.yaml", serde_json::to_vec(&self.compose)),
            ("bundle/manifest.json", serde_json::to_vec(&self.manifest)),
            ("installed.json", serde_json::to_vec(&self.manifest)),
        ] {
            if crate::installation_backup::source_bytes(&self.root, path, LIMIT as u64, true)?
                != bytes.map_err(|_| err("UPDATE_DEPLOYMENT_INVALID"))?
            {
                return Err(err("UPDATE_DEPLOYMENT_UNCERTAIN"));
            }
        }
        Ok(())
    }
}
fn container_matches(
    row: &Value,
    manifest: &BundleManifest,
    service: &str,
    image: &str,
    mounts: &[(&str, &str)],
) -> bool {
    let labels = &row["Config"]["Labels"];
    row["Id"]
        .as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        && row["Image"] == image
        && row["State"]["Running"] == true
        && row["State"]["Restarting"] == false
        && row["State"]["Health"]["Status"] == "healthy"
        && labels["com.docker.compose.service"] == service
        && labels["com.docker.compose.project"] == manifest.project_name
        && labels["com.exhibitos.bundle"] == manifest.bundle_id
        && labels["com.exhibitos.project"] == manifest.project_name
        && labels["com.exhibitos.schema"] == manifest.schema_version
        && row["Mounts"].as_array().is_some_and(|actual| {
            actual.len() == mounts.len()
                && mounts.iter().all(|(destination, name)| {
                    actual
                        .iter()
                        .filter(|m| {
                            m["Destination"] == *destination
                                && m["Name"] == *name
                                && m["Type"] == "volume"
                                && m["RW"] == true
                        })
                        .count()
                        == 1
                })
        })
}
fn target_writer_census(app: &Value, db: &Value, before: &SourceStoppedReceipt) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for volume in [
        &before.database_volume,
        &before.blob_volume,
        &before.configuration_volume,
    ] {
        let data = run(
            "docker",
            &[
                "ps".into(),
                "--all".into(),
                "--no-trunc".into(),
                "--filter".into(),
                format!("volume={volume}"),
                "--format".into(),
                "{{.ID}}".into(),
            ],
            None,
            15,
        )?;
        let text = std::str::from_utf8(&data).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        for id in text.lines() {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || ids.len() > 10000 {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            ids.insert(id.to_owned());
        }
    }
    for row in [app, db] {
        if !ids.contains(
            row["Id"]
                .as_str()
                .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?,
        ) {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
    }
    for id in ids {
        if app["Id"] == id || db["Id"] == id {
            continue;
        }
        let row = crate::backup_creation::inspected("docker", &["inspect".into(), id])?;
        if row["State"]["Running"] != false
            || row["State"]["Paused"] != false
            || row["State"]["Restarting"] != false
        {
            return Err(err("UPDATE_CANDIDATE_FOREIGN_WRITER"));
        }
    }
    Ok(())
}
impl<'session, 'store, 'inputs> StartedUpdate<'session, 'store, 'inputs> {
    fn quick_check(&self) -> Result<()> {
        self.admission.session.check()?;
        self.admission
            .lease
            .check_fences(&self.admission.session.source)
    }
    fn check_applying(&mut self) -> Result<()> {
        self.check_stage(crate::update::Stage::Applying, 1)
    }
    fn check_stage(&mut self, stage: crate::update::Stage, offset: u64) -> Result<()> {
        self.quick_check()?;
        let a = &mut self.admission;
        let intent = a
            .session
            .store
            .intent()
            .ok_or_else(|| err("UPDATE_INTENT_MISSING"))?;
        if intent.update.stage() != stage
            || intent.update.plan() != &a.artifact.plan
            || intent.envelope != a.artifact.envelope
            || a.session.store.receipt().generation
                != a.artifact
                    .generation
                    .checked_add(offset)
                    .ok_or_else(|| err("UPDATE_OPERATION_STALE"))?
        {
            return Err(err("UPDATE_OPERATION_STALE"));
        }
        let now = super::super::super::release_now()?;
        let mut fresh = a
            .session
            .store
            .verify_for_preparation(intent.envelope.as_bytes(), now)
            .map_err(|e| err(e.code()))?;
        if !fresh.binds(&a.artifact.plan)
            || fresh.payload_sha256 != a.artifact.verified.payload_sha256
            || fresh.key_id != a.artifact.verified.key_id
        {
            return Err(err("UPDATE_PLAN_MISMATCH"));
        }
        a.artifact
            .staged
            .reverify(&mut fresh, now)
            .map_err(|e| err(e.code()))?;
        a.artifact.verified = fresh;
        Ok(())
    }
    fn source_unchanged(&self) -> Result<()> {
        self.quick_check()?;
        let observed = source_stopped::observe(
            &self.admission.session.source,
            &self.admission.artifact.plan,
        )?;
        let (original, _) = self.admission.lease.stopped();
        if observed.platform_container != original.platform_container
            || observed.database_container != original.database_container
            || observed.database_volume != original.database_volume
            || observed.blob_volume != original.blob_volume
            || observed.configuration_volume != original.configuration_volume
            || observed.runtime_image_sha256 != original.runtime_image_sha256
        {
            return Err(err("UPDATE_SOURCE_CHANGED"));
        }
        Ok(())
    }
    fn observed_pair(&self, m: &BundleManifest, database_image: &str) -> Result<(Value, Value)> {
        let target = self.admission.lease.target();
        let (app, app_row) =
            crate::backup_creation::one_container(target, m, "docker", "platform")?;
        let (database, db_row) =
            crate::backup_creation::one_container(target, m, "docker", "database")?;
        let (_, before) = self.admission.lease.stopped();
        let target_pin = format!("sha256:{}", self.admission.artifact.plan.target_image);
        if !container_matches(
            &app_row,
            m,
            "platform",
            &target_pin,
            &[
                ("/data/blobs", &before.blob_volume),
                ("/data/config", &before.configuration_volume),
            ],
        ) || !container_matches(
            &db_row,
            m,
            "database",
            database_image,
            &[("/var/lib/postgresql", &before.database_volume)],
        ) || app == database
            || database != before.database_container
        {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        target_writer_census(&app_row, &db_row, before)?;
        Ok((app_row, db_row))
    }
    fn apply_inner(&mut self) -> Result<(crate::update::HealthReceipt, CandidateState)> {
        self.check_applying()?;
        self.admission.lease.check(&self.admission.session.source)?;
        let plan = self.admission.artifact.plan.clone();
        let target = self.admission.lease.target();
        let m = target.manifest()?;
        if target.engine(&m, false)? != "docker" {
            return Err(err("UPDATE_OCI_PLATFORM_UNVERIFIED"));
        }
        let image = format!("sha256:{}", plan.target_image);
        let loaded = crate::backup_creation::inspected(
            "docker",
            &["image".into(), "inspect".into(), image.clone()],
        )?;
        if loaded["Id"] != image
            || loaded["Os"] != "linux"
            || loaded["Architecture"] != "arm64"
            || loaded["RootFS"]["Layers"] != self.admission.oci.proof["layerDiffIds"]
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
        let (_, before) = self.admission.lease.stopped();
        let db_image = crate::backup_creation::inspected(
            "docker",
            &["inspect".into(), before.database_container.clone()],
        )?["Image"]
            .as_str()
            .filter(|s| s.starts_with("sha256:"))
            .ok_or_else(|| err("IMAGE_INTEGRITY"))?
            .to_owned();
        let data = run(
            "docker",
            &crate::compose_args(&m, &["config", "--format", "json"]),
            Some(&target.root.join("bundle")),
            30,
        )?;
        if data.len() > LIMIT {
            return Err(err("UPDATE_DEPLOYMENT_INVALID"));
        }
        let config: Value =
            serde_json::from_slice(&data).map_err(|_| err("UPDATE_DEPLOYMENT_INVALID"))?;
        // The internally produced admission fixes all five configuration hashes.
        // A later filesystem change cannot be adopted as new execution input.
        let proofs = self.admission.receipt["candidate"]["configuration"]["files"]
            .as_array()
            .filter(|f| f.len() == source_deployment::FILES.len())
            .ok_or_else(|| err("UPDATE_CANDIDATE_PROOF_MISSING"))?;
        for ((_, path, limit), proof) in source_deployment::FILES.iter().zip(proofs) {
            let mut bytes =
                crate::installation_backup::source_bytes(&target.root, path, *limit, true)?;
            let matched = proof["path"] == *path
                && proof["bytes"] == bytes.len() as u64
                && proof["sha256"] == crate::digest(&bytes);
            bytes.fill(0);
            if !matched {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
        }
        let frozen: Vec<_> = ["runtime.env", "engine.json"]
            .into_iter()
            .map(|path| {
                let bytes =
                    crate::installation_backup::source_bytes(&target.root, path, 8192, true)?;
                Ok((path.to_owned(), crate::digest(&bytes)))
            })
            .collect::<Result<_>>()?;

        let workspace = self.admission.destination.join("execution");
        installations::new_directory(&workspace)?;
        let staged = Deployment::stage(&target.root, &workspace, &m, &config, &plan)?;
        let generation = self.admission.session.store.receipt().generation;
        phase(&workspace, "publication-started", &plan, generation)?;
        self.check_applying()?;
        self.source_unchanged()?;
        staged.publish()?;
        self.admission
            .lease
            .target()
            .validate_compose(&staged.manifest, "docker")?;
        phase(&workspace, "runtime-started", &plan, generation)?;
        let args = crate::compose_args(
            &staged.manifest,
            &["up", "--detach", "--no-build", "--pull", "never"],
        );
        run_observed(
            "docker",
            &args,
            Some(&staged.root.join("bundle")),
            180,
            || {
                self.quick_check()?;
                staged.check_published()
            },
        )?;
        self.check_applying()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let pair = loop {
            self.quick_check()?;
            staged.check_published()?;
            if let Ok(pair) = self.observed_pair(&staged.manifest, &db_image)
                && crate::readiness(&staged.manifest).ready
            {
                break pair;
            }
            if std::time::Instant::now() >= deadline {
                return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };
        self.source_unchanged()?;
        phase(&workspace, "health-started", &plan, generation)?;
        let system=self.admission.receipt["candidate"]["inventory"]["observation"]["physical"]["systemIdentifier"].as_str().filter(|s|source_database::valid_system_identifier(s)).ok_or_else(||err("UPDATE_CANDIDATE_PROOF_MISSING"))?.to_owned();
        let id = pair.0["Id"]
            .as_str()
            .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?
            .to_owned();
        let input = self.admission.lease.manifest_input()?;
        let args = vec![
            "exec".into(),
            "--interactive".into(),
            id.clone(),
            "node".into(),
            "--input-type=module".into(),
            "-e".into(),
            READER.into(),
            plan.backup_manifest.clone(),
            system,
        ];
        let raw = run_observed_input("docker", &args, None, 180, Some(input), || {
            self.quick_check()?;
            staged.check_published()
        })?;
        let inventory: source_inventory::InventoryProof =
            serde_json::from_slice(&raw).map_err(|_| err("UPDATE_CANDIDATE_HEALTH_FAILED"))?;
        source_inventory::matched_candidate(&inventory, &plan)?;
        if inventory.schema_sha256 != plan.target_schema {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        let after = self.observed_pair(&staged.manifest, &db_image)?;
        if after.0["Id"] != pair.0["Id"]
            || after.1["Id"] != pair.1["Id"]
            || !crate::readiness(&staged.manifest).ready
        {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        self.source_unchanged()?;
        self.check_applying()?;
        staged.check_published()?;
        for (path, sha) in &frozen {
            if crate::digest(&crate::installation_backup::source_bytes(
                &staged.root,
                path,
                8192,
                true,
            )?) != *sha
            {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
        }
        let (_, before) = self.admission.lease.stopped();
        let expected = source_configuration::expected(self.admission.lease.authenticated_raw()?)?;
        source_configuration::observe(
            self.admission.inputs.maintenance_image,
            &before.configuration_volume,
            &expected,
        )?;
        // Configuration observation invokes a bounded helper; recheck authority,
        // deployment and both service identities after it, before any success journal.
        self.check_applying()?;
        self.source_unchanged()?;
        staged.check_published()?;
        let final_pair = self.observed_pair(&staged.manifest, &db_image)?;
        if final_pair.0["Id"] != pair.0["Id"]
            || final_pair.1["Id"] != pair.1["Id"]
            || !crate::readiness(&staged.manifest).ready
        {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        phase(&workspace, "health-observed", &plan, generation)?;
        let health = crate::update::HealthReceipt {
            operation_id: plan.operation_id.clone(),
            instance_id: plan.target_instance.clone(),
            image: plan.target_image.clone(),
            schema: inventory.schema_sha256,
            ready: true,
        };
        let finished = self
            .admission
            .session
            .store
            .application_finished(
                &plan.operation_id,
                generation,
                super::super::super::release_now()?,
            )
            .map_err(|e| err(e.code()))?;
        self.admission.receipt["candidateRuntimeApplied"] = serde_json::json!(true);
        self.admission.receipt["candidateHealthObserved"] =
            serde_json::to_value(&health).map_err(|_| err("UPDATE_RESULT_INVALID"))?;
        self.admission.receipt["awaitingHealthGeneration"] = serde_json::json!(finished.generation);
        self.admission.receipt["executionWorkspace"] = serde_json::json!(workspace);
        // Do NOT journal Updated before coherent selection/authority activation.
        self.admission.receipt["hostActivated"] = serde_json::json!(false);
        self.admission.receipt["updateExecuted"] = serde_json::json!(false);
        Ok((
            health,
            CandidateState {
                deployment: staged,
                database_image: db_image,
                containers: [
                    final_pair.0["Id"]
                        .as_str()
                        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?
                        .to_owned(),
                    final_pair.1["Id"]
                        .as_str()
                        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?
                        .to_owned(),
                ],
                frozen,
            },
        ))
    }
    /// Consuming runtime execution. No raw command, path, image or health flag.
    /// Failure durably requires recovery; original source and all recovery inputs
    /// remain retained. Engine uncertainty is never retried or relabeled success.
    pub fn apply_candidate(mut self) -> Result<ReadyCandidate<'session, 'store, 'inputs>> {
        match self.apply_inner() {
            Ok((health, execution)) => Ok(ReadyCandidate {
                started: self,
                health,
                execution,
            }),
            Err(error) => {
                let store = &mut self.admission.session.store;
                if store.intent().is_some_and(|i| {
                    matches!(
                        i.update.stage(),
                        crate::update::Stage::Applying | crate::update::Stage::AwaitingHealth
                    )
                }) {
                    let generation = store.receipt().generation;
                    store
                        .update_failed(
                            &self.admission.artifact.plan.operation_id,
                            generation,
                            super::super::super::release_now()?,
                        )
                        .map_err(|_| err("UPDATE_EXECUTION_UNCERTAIN"))?;
                }
                Err(error)
            }
        }
    }
}

impl ReadyCandidate<'_, '_, '_> {
    fn check_ready(&mut self) -> Result<()> {
        self.started
            .check_stage(crate::update::Stage::AwaitingHealth, 2)?;
        self.execution.deployment.check_published()?;
        self.started.source_unchanged()?;
        for (path, sha) in &self.execution.frozen {
            if crate::digest(&crate::installation_backup::source_bytes(
                &self.execution.deployment.root,
                path,
                8192,
                true,
            )?) != *sha
            {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
        }
        let pair = self.started.observed_pair(
            &self.execution.deployment.manifest,
            &self.execution.database_image,
        )?;
        if pair.0["Id"] != self.execution.containers[0]
            || pair.1["Id"] != self.execution.containers[1]
            || !crate::readiness(&self.execution.deployment.manifest).ready
        {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        Ok(())
    }
    fn refresh_health(&mut self) -> Result<()> {
        self.started.admission.session.require_selected_source()?;
        self.check_ready()?;
        let plan = self.started.admission.artifact.plan.clone();
        let system=self.started.admission.receipt["candidate"]["inventory"]["observation"]["physical"]["systemIdentifier"]
            .as_str().filter(|s|source_database::valid_system_identifier(s))
            .ok_or_else(||err("UPDATE_CANDIDATE_PROOF_MISSING"))?.to_owned();
        let input = self.started.admission.lease.manifest_input()?;
        let args = vec![
            "exec".into(),
            "--interactive".into(),
            self.execution.containers[0].clone(),
            "node".into(),
            "--input-type=module".into(),
            "-e".into(),
            READER.into(),
            plan.backup_manifest.clone(),
            system,
        ];
        let raw = run_observed_input("docker", &args, None, 180, Some(input), || {
            self.started.quick_check()?;
            self.execution.deployment.check_published()
        })?;
        let inventory: source_inventory::InventoryProof =
            serde_json::from_slice(&raw).map_err(|_| err("UPDATE_CANDIDATE_HEALTH_FAILED"))?;
        source_inventory::matched_candidate(&inventory, &plan)?;
        if inventory.schema_sha256 != plan.target_schema {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        let (_, before) = self.started.admission.lease.stopped();
        let expected =
            source_configuration::expected(self.started.admission.lease.authenticated_raw()?)?;
        source_configuration::observe(
            self.started.admission.inputs.maintenance_image,
            &before.configuration_volume,
            &expected,
        )?;
        self.check_ready()?;
        if self.health.operation_id != plan.operation_id
            || self.health.instance_id != plan.target_instance
            || self.health.image != plan.target_image
            || self.health.schema != inventory.schema_sha256
            || !self.health.ready
        {
            return Err(err("UPDATE_CANDIDATE_HEALTH_FAILED"));
        }
        Ok(())
    }
    fn activate_inner(
        &mut self,
    ) -> Result<crate::signed_release::trust::SelectionActivationReceipt> {
        self.refresh_health()?;
        let activation = crate::signed_release::trust::selection_activation::Activation::prepare(
            self.started.admission.session.store,
            &self.started.admission.session.registry,
            &self.health,
        )?;
        // The original profile/authority and both operation locks are still held.
        let next = activation.publish_selection(self.started.admission.session.store)?;
        self.started.admission.session.registry = next;
        self.check_ready()?;
        let plan = &self.started.admission.artifact.plan;
        let store = &mut self.started.admission.session.store;
        store
            .observe_health(
                &plan.operation_id,
                store.receipt().generation,
                self.health.clone(),
                super::super::super::release_now()?,
            )
            .map_err(|e| err(e.code()))?;
        activation.complete(store)
    }
    /// Consume actual candidate observations; reobserve before selected-host and
    /// authority completion. No frontend health flag, target, command or route.
    /// Failure preserves both selections and journal for explicit reconciliation.
    pub fn activate(mut self) -> Result<crate::signed_release::trust::SelectionActivationReceipt> {
        match self.activate_inner() {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                let store = &mut self.started.admission.session.store;
                if store
                    .intent()
                    .is_some_and(|i| i.update.stage() == crate::update::Stage::AwaitingHealth)
                {
                    store
                        .update_failed(
                            &self.started.admission.artifact.plan.operation_id,
                            store.receipt().generation,
                            super::super::super::release_now()?,
                        )
                        .map_err(|_| err("UPDATE_EXECUTION_UNCERTAIN"))?;
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture() -> (PathBuf, PathBuf, BundleManifest, Value, crate::update::Plan) {
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-execution-fixture-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&parent).unwrap();
        let root = parent.join("target");
        installations::new_directory(&root).unwrap();
        installations::new_directory(&root.join("bundle")).unwrap();
        let workspace = parent.join("execution");
        installations::new_directory(&workspace).unwrap();
        let m:BundleManifest=serde_json::from_value(serde_json::json!({"schemaVersion":"1.0.0-draft.1","bundleId":"synthetic","version":"0.1.0","protocolVersion":"1","composeSha256":"a".repeat(64),"projectName":"synthetic","services":["platform","database"],"images":[{"reference":format!("sha256:{}","a".repeat(64)),"archive":{"path":"runtime.tar","bytes":7,"sha256":"a".repeat(64)}},{"reference":format!("sha256:{}","b".repeat(64)),"archive":null}],"ports":[19001],"openUrl":"http://127.0.0.1:19001","readinessUrl":"http://127.0.0.1:19001/health/ready","minimumFreeBytes":1024})).unwrap();
        let config = serde_json::json!({"name":"synthetic","services":{"platform":{"image":m.images[0].reference,"environment":{"EXHIBITOS_SYNTHETIC":"fixture"},"volumes":[{"type":"volume","source":"blobs","target":"/data/blobs"}]},"database":{"image":m.images[1].reference,"environment":{"SYNTHETIC":"unchanged"}}},"volumes":{"blobs":{"name":"synthetic_blobs"}}});
        for (path, bytes) in [
            ("bundle/manifest.json", serde_json::to_vec(&m).unwrap()),
            ("installed.json", serde_json::to_vec(&m).unwrap()),
            ("bundle/compose.yaml", serde_json::to_vec(&config).unwrap()),
        ] {
            fs::write(root.join(path), bytes).unwrap();
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut plan = super::super::super::super::super::tests::plan();
        plan.target_image = "c".repeat(64);
        plan.target_schema = plan.source_schema.clone();
        (root, workspace, m, config, plan)
    }
    fn retire(root: &Path) {
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    #[test]
    fn candidate_execution_changes_only_qualified_platform_image_and_keeps_original_copies() {
        let (root, workspace, m, config, p) = fixture();
        let staged = Deployment::stage(&root, &workspace, &m, &config, &p).unwrap();
        let mut expected = config.clone();
        expected["services"]["platform"]["image"] =
            serde_json::json!(format!("sha256:{}", p.target_image));
        assert_eq!(staged.compose, expected);
        assert_eq!(staged.manifest.images[1].reference, m.images[1].reference);
        assert_eq!(staged.manifest.version, m.version);
        assert_eq!(staged.manifest.bundle_id, m.bundle_id);
        assert_eq!(staged.manifest.project_name, m.project_name);
        assert!(staged.manifest.images[0].archive.is_none());
        let original = staged.originals.clone();
        staged.publish().unwrap();
        staged.check_published().unwrap();
        for ((_, bytes), name) in original.iter().zip([
            "original-manifest.json",
            "original-compose.yaml",
            "original-installed.json",
        ]) {
            assert_eq!(fs::read(workspace.join(name)).unwrap(), *bytes);
        }
        assert!(staged.publish().is_err());
        staged.check_published().unwrap();
        assert_eq!(
            crate::digest(&fs::read(root.join("bundle/compose.yaml")).unwrap()),
            staged.manifest.compose_sha256
        );
        retire(&root);
    }
    #[test]
    fn candidate_execution_refuses_changed_original_before_any_publication() {
        let (root, workspace, m, config, p) = fixture();
        let staged = Deployment::stage(&root, &workspace, &m, &config, &p).unwrap();
        let old = fs::read(root.join("installed.json")).unwrap();
        fs::write(root.join("bundle/compose.yaml"), b"preserve user change").unwrap();
        assert_eq!(staged.publish().unwrap_err().code, "UPDATE_TARGET_CHANGED");
        assert_eq!(fs::read(root.join("installed.json")).unwrap(), old);
        assert_eq!(
            fs::read(root.join("bundle/compose.yaml")).unwrap(),
            b"preserve user change"
        );
        assert_eq!(
            fs::read(workspace.join("original-installed.json")).unwrap(),
            old
        );
        retire(&root);
    }
    #[test]
    fn candidate_execution_rejects_replaced_bundle_parent_without_adopting_it() {
        let (root, workspace, m, config, p) = fixture();
        let staged = Deployment::stage(&root, &workspace, &m, &config, &p).unwrap();
        fs::rename(root.join("bundle"), root.join("retained-bundle")).unwrap();
        installations::new_directory(&root.join("bundle")).unwrap();
        assert_eq!(staged.publish().unwrap_err().code, "UPDATE_TARGET_CHANGED");
        assert!(fs::read_dir(root.join("bundle")).unwrap().next().is_none());
        assert_eq!(
            fs::read(root.join("retained-bundle/manifest.json")).unwrap(),
            staged.originals[0].1
        );
        retire(&root);
    }
    #[test]
    fn candidate_execution_immutable_phase_cannot_be_reused_or_rewritten() {
        let (root, workspace, _, _, p) = fixture();
        phase(&workspace, "runtime-started", &p, 4).unwrap();
        let saved = fs::read(workspace.join("runtime-started.json")).unwrap();
        assert!(phase(&workspace, "runtime-started", &p, 5).is_err());
        assert_eq!(
            fs::read(workspace.join("runtime-started.json")).unwrap(),
            saved
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&saved).unwrap()["applyingGeneration"],
            4
        );
        retire(&root);
    }
    #[test]
    fn candidate_execution_unknown_migration_layout_or_image_never_produces_deployment() {
        let (root, _, m, c, p) = fixture();
        let mut changed = p.clone();
        changed.target_schema = "f".repeat(64);
        assert!(replacement(&m, &c, &changed).is_err());
        let mut unknown = c.clone();
        unknown["services"]["foreign"] = serde_json::json!({"image":"unknown"});
        assert!(replacement(&m, &unknown, &p).is_err());
        let mut wrong = c.clone();
        wrong["services"]["platform"]["image"] = serde_json::json!("unknown");
        assert!(replacement(&m, &wrong, &p).is_err());
        assert_eq!(
            fs::read(root.join("installed.json")).unwrap(),
            serde_json::to_vec(&m).unwrap()
        );
        retire(&root);
    }
    #[test]
    fn candidate_execution_running_flag_is_insufficient_without_exact_owned_healthy_mounts() {
        let (root, _, m, _, _) = fixture();
        let image = format!("sha256:{}", "c".repeat(64));
        let good = serde_json::json!({"Id":"d".repeat(64),"Image":image,"State":{"Running":true,"Restarting":false,"Health":{"Status":"healthy"}},"Config":{"Labels":{"com.docker.compose.service":"platform","com.docker.compose.project":"synthetic","com.exhibitos.bundle":"synthetic","com.exhibitos.project":"synthetic","com.exhibitos.schema":"1.0.0-draft.1"}},"Mounts":[{"Destination":"/data/blobs","Name":"candidate_blobs","Type":"volume","RW":true}]});
        assert!(container_matches(
            &good,
            &m,
            "platform",
            &image,
            &[("/data/blobs", "candidate_blobs")]
        ));
        for pointer in [
            "/Image",
            "/State/Running",
            "/State/Health/Status",
            "/Config/Labels/com.exhibitos.project",
            "/Mounts/0/Name",
            "/Mounts/0/RW",
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(!container_matches(
                &bad,
                &m,
                "platform",
                &image,
                &[("/data/blobs", "candidate_blobs")]
            ));
        }
        retire(&root);
    }
}
