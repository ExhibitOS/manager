// SPDX-License-Identifier: Apache-2.0
//! Native reobservation and same-fence coherent rollback completion, no health endpoint.
use super::*;
use crate::{Result as LifecycleResult, err};
fn ready_row(
    row: &serde_json::Value,
    m: &crate::BundleManifest,
    service: &str,
    image: &str,
) -> LifecycleResult<()> {
    let l = &row["Config"]["Labels"];
    if row["Image"] != image
        || row["State"]["Running"] != true
        || row["State"]["Paused"] != false
        || row["State"]["Restarting"] != false
        || row["State"]["Health"]["Status"] != "healthy"
        || l["com.docker.compose.service"] != service
        || l["com.docker.compose.project"] != m.project_name
        || l["com.exhibitos.bundle"] != m.bundle_id
        || l["com.exhibitos.project"] != m.project_name
        || l["com.exhibitos.schema"] != m.schema_version
    {
        return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
    }
    Ok(())
}
fn volumes(row: &serde_json::Value, expected: &[&str]) -> LifecycleResult<Vec<String>> {
    let mounts = row["Mounts"]
        .as_array()
        .ok_or_else(|| err("UPDATE_ROLLBACK_HEALTH_FAILED"))?;
    if mounts.len() != expected.len() {
        return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
    }
    let mut names = Vec::new();
    for destination in expected {
        let matches = mounts
            .iter()
            .filter(|m| m["Destination"] == *destination)
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0]["Type"] != "volume" || matches[0]["RW"] != true {
            return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
        }
        names.push(
            matches[0]["Name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| err("UPDATE_ROLLBACK_HEALTH_FAILED"))?
                .to_owned(),
        );
    }
    Ok(names)
}
fn pair(
    target: &crate::LifecycleService,
    m: &crate::BundleManifest,
    plan: &crate::update::Plan,
) -> LifecycleResult<(String, serde_json::Value, serde_json::Value, Vec<String>)> {
    target.validate_ownership(m, "docker")?;
    target.validate_volumes(m, "docker")?;
    let (app, a) = crate::backup_creation::one_container(target, m, "docker", "platform")?;
    let (db, d) = crate::backup_creation::one_container(target, m, "docker", "database")?;
    let pin = format!("sha256:{}", plan.source_image);
    let database = m
        .images
        .iter()
        .filter(|i| i.reference != pin)
        .collect::<Vec<_>>();
    if app == db
        || database.len() != 1
        || m.images.len() != 2
        || m.images.iter().filter(|i| i.reference == pin).count() != 1
    {
        return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
    }
    ready_row(&a, m, "platform", &pin)?;
    ready_row(&d, m, "database", &database[0].reference)?;
    let mut names = volumes(&a, &["/data/blobs", "/data/config"])?;
    names.extend(volumes(&d, &["/var/lib/postgresql"])?);
    if names
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != 3
    {
        return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
    }
    for volume in &names {
        let raw = crate::run(
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
        let ids = std::str::from_utf8(&raw).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        if ids.lines().count() > 10000 {
            return Err(err("ENGINE_OUTPUT_INVALID"));
        }
        for id in ids.lines() {
            if id == app || id == db {
                continue;
            }
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            let r = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
            if r["State"]["Running"] != false
                || r["State"]["Paused"] != false
                || r["State"]["Restarting"] != false
            {
                return Err(err("UPDATE_ROLLBACK_FOREIGN_WRITER"));
            }
        }
    }
    if !crate::readiness(m).ready {
        return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
    }
    Ok((app, a, d, names))
}
impl Store {
    /// Reobserve the reserved restored candidate twice and consume actual native
    /// inventory/image/schema/configuration health into coherent selected-host
    /// rollback completion. Keeps all profile/installation fences; never accepts
    /// caller health receipts, candidate IDs, commands, URLs or success flags.
    pub fn activate_restored_rollback(
        &mut self,
        maintenance_image: &str,
        external_writers_quiesced: bool,
    ) -> LifecycleResult<SelectionActivationReceipt> {
        if !external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        self.check_root().map_err(|e| err(e.code()))?;
        self.require_authority_recovery()
            .map_err(|e| err(e.code()))?;
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| err("UPDATE_FENCE_UNAVAILABLE"))?;
        let _session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let controller = crate::LifecycleService::open_retry_diagnostics(self.profile.clone())?;
        let _profile = installations::profile_lock(&controller)?;
        let intent = self.intent().ok_or_else(|| err("UPDATE_INTENT_MISSING"))?;
        let interrupted = intent.update.stage() == crate::update::Stage::RecoveryRequired
            && intent.update.interrupted_restoration().is_some();
        if intent.update.stage() != crate::update::Stage::AwaitingRollbackHealth && !interrupted {
            return Err(err("UPDATE_ROLLBACK_STAGE_INVALID"));
        }
        let historical = intent.update.interrupted_restoration().cloned();
        let plan = intent.update.plan().clone();
        let id = intent
            .update
            .restore_candidate()
            .or_else(|| historical.as_ref().map(|r| r.candidate_id.as_str()))
            .filter(|id| installations::uuid(id))
            .ok_or_else(|| err("UPDATE_ROLLBACK_CANDIDATE_MISSING"))?
            .to_owned();
        let generation = std::cell::Cell::new(self.current.generation);
        let (registry, previous) =
            installations::load(&self.profile)?.ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
        let bound = self.bound_source_id(&registry)?;
        if registry.active_id != bound
            || (bound != plan.source_instance && bound != plan.target_instance)
            || !self.used_instances.contains(&id)
        {
            return Err(err("UPDATE_SOURCE_MISMATCH"));
        }
        let mut protected = Vec::new();
        for name in [&plan.source_instance, &plan.target_instance, &id] {
            let e = registry
                .installations
                .iter()
                .find(|e| &e.id == name)
                .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
            if name == &id && e.kind != "recovery" {
                return Err(err("UPDATE_ROLLBACK_CANDIDATE_MISSING"));
            }
            protected.push(crate::LifecycleService::open_retry_diagnostics(
                installations::root(&self.profile, e),
            )?);
        }
        protected.sort_by(|a, b| a.root.cmp(&b.root));
        let guards = protected
            .iter()
            .map(|s| s.lock())
            .collect::<LifecycleResult<Vec<_>>>()?;
        let root = self.profile.join("installations").join(&id);
        let target = protected
            .iter()
            .find(|s| s.root == root)
            .ok_or_else(|| err("UPDATE_TARGET_CHANGED"))?;
        let check = |store: &Store, selection: &[u8]| -> LifecycleResult<()> {
            store.check_root().map_err(|e| err(e.code()))?;
            if store.current.generation != generation.get()
                || installations::load(&store.profile)?.is_none_or(|(_, b)| b != selection)
            {
                return Err(err("UPDATE_ROLLBACK_CHANGED"));
            }
            for (service, guard) in protected.iter().zip(&guards) {
                service.check_restoration_guard(guard)?;
            }
            Ok(())
        };
        let m = target.manifest()?;
        if target.engine(&m, false)? != "docker" {
            return Err(err("UPDATE_OCI_PLATFORM_UNVERIFIED"));
        }
        // Bind the exact original restore workspace, never search arbitrary journals.
        let records = fs::read_dir(&root)
            .map_err(|_| err("UPDATE_ROLLBACK_PROOF_MISSING"))?
            .take(1001)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| err("UPDATE_ROLLBACK_PROOF_MISSING"))?;
        if records.len() > 1000 {
            return Err(err("UPDATE_ROLLBACK_PROOF_MISSING"));
        }
        let mut receipts = Vec::new();
        for e in records {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(suffix) = name.strip_prefix("restore-")
                && installations::uuid(suffix)
            {
                receipts.push(e.path());
            }
        }
        if receipts.len() != 1 {
            return Err(err("UPDATE_ROLLBACK_PROOF_MISSING"));
        }
        let workspace = &receipts[0];
        let bytes =
            crate::installation_backup::source_bytes(workspace, "receipt.json", 256 * 1024, true)?;
        let r: crate::restoration::RestorationReceipt =
            serde_json::from_slice(&bytes).map_err(|_| err("UPDATE_ROLLBACK_PROOF_MISSING"))?;
        crate::restoration::RestorationBinding::from_plan(&plan)?.check_receipt(&r)?;
        if r.operation != "restored-and-running"
            || workspace.file_name().and_then(|s| s.to_str())
                != Some(format!("restore-{}", r.id).as_str())
            || r.bundle_id != m.bundle_id
            || r.project_name != m.project_name
        {
            return Err(err("UPDATE_ROLLBACK_PROOF_MISSING"));
        }
        let raw = crate::installation_backup::source_bytes(
            &workspace.join("authenticated"),
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )?;
        crate::restoration::RestorationBinding::from_plan(&plan)?.authenticated(
            &r.backup_id,
            &r.authenticated_manifest_sha256,
            &raw,
        )?;
        let source = registry
            .installations
            .iter()
            .find(|e| e.id == plan.source_instance)
            .ok_or_else(|| err("UPDATE_SOURCE_UNREGISTERED"))?;
        owned_execution::check_restored_source_files(
            &installations::root(&self.profile, source),
            workspace,
            &r,
            &plan,
        )?;
        let frozen = [
            "installed.json",
            "engine.json",
            "runtime.env",
            "bundle/manifest.json",
            "bundle/compose.yaml",
        ]
        .iter()
        .map(|p| {
            crate::installation_backup::source_bytes(&root, p, 1024 * 1024, true)
                .map(|b| (*p, hash(&b)))
        })
        .collect::<LifecycleResult<Vec<_>>>()?;
        let inspect = |store: &Store,
                       selection: &[u8]|
         -> LifecycleResult<crate::update::HealthReceipt> {
            check(store, selection)?;
            for (p, sha) in &frozen {
                if hash(&crate::installation_backup::source_bytes(
                    &root,
                    p,
                    1024 * 1024,
                    true,
                )?) != *sha
                {
                    return Err(err("UPDATE_TARGET_CHANGED"));
                }
            }
            let before = pair(target, &m, &plan)?;
            owned_execution::check_rollback_configuration(maintenance_image, &before.3[1], &raw)?;
            let manifest_path = workspace.join("authenticated/manifest.json");
            let mut input = private_file(&manifest_path, false).map_err(|e| err(e.code()))?;
            let mut supplied = Vec::new();
            (&mut input)
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut supplied)
                .map_err(|_| err("UPDATE_INPUT_INVALID"))?;
            if supplied != raw {
                return Err(err("UPDATE_ROLLBACK_CHANGED"));
            }
            std::io::Seek::rewind(&mut input).map_err(|_| err("UPDATE_INPUT_INVALID"))?;
            let proof = owned_execution::native_inventory::observe(
                &owned_execution::native_inventory::Inputs {
                    image: maintenance_image,
                    root: &root,
                    manifest: &m,
                    app: &before.1,
                    blob_volume: &before.3[0],
                    expected_manifest: &plan.backup_manifest,
                    expected_system: None,
                },
                input,
                || check(store, selection),
            )?;
            owned_execution::check_rollback_inventory(&proof, &plan)?;
            let after = pair(target, &m, &plan)?;
            if before.1["Id"] != after.1["Id"]
                || before.2["Id"] != after.2["Id"]
                || before.3 != after.3
            {
                return Err(err("UPDATE_TARGET_CHANGED"));
            }
            if crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )? != raw
            {
                return Err(err("UPDATE_ROLLBACK_CHANGED"));
            }
            owned_execution::check_restored_source_files(
                &installations::root(&store.profile, source),
                workspace,
                &r,
                &plan,
            )?;
            check(store, selection)?;
            Ok(crate::update::HealthReceipt {
                operation_id: plan.operation_id.clone(),
                instance_id: id.clone(),
                image: plan.source_image.clone(),
                schema: plan.source_schema.clone(),
                ready: true,
            })
        };
        let health = inspect(self, &previous)?;
        if let Some(receipt) = historical {
            self.resume_restored_runtime(
                &plan.operation_id,
                generation.get(),
                receipt,
                owned_execution::release_now()?,
            )
            .map_err(|e| err(e.code()))?;
            generation.set(self.current.generation);
        }
        let activation =
            selection_activation::Activation::prepare_rollback(self, &previous, &health)?;
        let selected = activation.publish_selection(self)?;
        let fresh = inspect(self, &selected)?;
        if fresh != health {
            return Err(err("UPDATE_ROLLBACK_HEALTH_FAILED"));
        }
        self.observe_health(
            &plan.operation_id,
            generation.get(),
            fresh,
            owned_execution::release_now()?,
        )
        .map_err(|e| err(e.code()))?;
        for (service, guard) in protected.iter().zip(&guards) {
            service.check_restoration_guard(guard)?;
        }
        activation.complete(self)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_mounts_refuse_aliases_binds_and_unexpected_writable_scope() {
        let row = serde_json::json!({"Mounts":[{"Destination":"/data/blobs","Type":"volume","RW":true,"Name":"owned"}]});
        assert_eq!(volumes(&row, &["/data/blobs"]).unwrap(), vec!["owned"]);
        for changed in [
            serde_json::json!({"Mounts":[{"Destination":"/data/blobs","Type":"bind","RW":true,"Name":"owned"}]}),
            serde_json::json!({"Mounts":[{"Destination":"/data/blobs","Type":"volume","RW":false,"Name":"owned"}]}),
            serde_json::json!({"Mounts":[{"Destination":"/data/blobs","Type":"volume","RW":true,"Name":""}]}),
            serde_json::json!({"Mounts":[{"Destination":"/elsewhere","Type":"volume","RW":true,"Name":"owned"}]}),
        ] {
            assert!(volumes(&changed, &["/data/blobs"]).is_err());
        }
        assert!(volumes(&row, &["/data/blobs", "/data/config"]).is_err());
    }
    #[test]
    fn rollback_runtime_reopen_preserves_only_diagnostic_candidate_and_refuses_unobserved_completion()
     {
        let (p, mut store) = super::super::rollback_registration::tests::fixture();
        let r = store.register_rollback_candidate(true).unwrap();
        let plan = store.intent().unwrap().update.plan().clone();
        let proof = crate::update::RestoreReceipt {
            operation_id: plan.operation_id.clone(),
            backup_id: plan.backup_id.clone(),
            backup_manifest: plan.backup_manifest.clone(),
            inventory_digest: plan.source_inventory.clone(),
            candidate_id: r.candidate_id.clone(),
            schema: plan.source_schema.clone(),
            inventory_verified: true,
            separate_candidate: true,
        };
        store
            .restore_finished(
                &plan.operation_id,
                store.current.generation,
                proof.clone(),
                owned_execution::release_now().unwrap(),
            )
            .unwrap();
        let raw = installations::load(&p).unwrap().unwrap().1;
        let authority = store.root.clone();
        drop(store);
        let mut store = Store::open(&p, "default").unwrap();
        assert_eq!(store.root, authority);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        assert!(store.intent().unwrap().update.restore_candidate().is_none());
        assert_eq!(
            store.intent().unwrap().update.interrupted_restoration(),
            Some(&proof)
        );
        let core = &store.intent().unwrap().update;
        let bytes = core.to_json().unwrap();
        assert_eq!(
            crate::update::Update::from_json(&bytes)
                .unwrap()
                .interrupted_restoration(),
            Some(&proof)
        );
        let mut forged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        forged["interruptedRestoration"]["inventoryDigest"] = "0".repeat(64).into();
        assert!(crate::update::Update::from_json(&serde_json::to_vec(&forged).unwrap()).is_err());
        let generation = store.current.generation;
        assert_eq!(
            store
                .activate_restored_rollback("untrusted", false)
                .err()
                .unwrap()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert!(store.activate_restored_rollback("untrusted", true).is_err());
        assert_eq!(store.current.generation, generation);
        assert_eq!(installations::load(&p).unwrap().unwrap().1, raw);
        let mut wrong = proof.clone();
        wrong.candidate_id = uuid::Uuid::new_v4().to_string();
        assert!(
            store
                .resume_restored_runtime(
                    &plan.operation_id,
                    generation,
                    wrong,
                    owned_execution::release_now().unwrap()
                )
                .is_err()
        );
        assert_eq!(store.current.generation, generation);
        drop(store);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    #[ignore = "explicit local Docker qualification on already restored synthetic candidate; no new data copies"]
    fn rollback_runtime_actual_reused_candidate_completes_same_authority_after_reobservation() {
        let root =
            fs::canonicalize(std::env::var("EXHIBITOS_ROLLBACK_RUNTIME_FIXTURE").unwrap()).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("report.json")).unwrap()).unwrap();
        assert_eq!(report["status"], "PASS");
        assert_eq!(report["rollbackCompleted"], false);
        let profile = root.join("profile");
        let target_root = PathBuf::from(report["targetRoot"].as_str().unwrap());
        assert!(target_root.starts_with(&profile));
        let mut store = Store::open(&profile, "default").unwrap();
        let authority = store.root.clone();
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::RecoveryRequired
        );
        let id = store
            .intent()
            .unwrap()
            .update
            .interrupted_restoration()
            .unwrap()
            .candidate_id
            .clone();
        assert_eq!(target_root, profile.join("installations").join(&id));
        let old = installations::load(&profile).unwrap().unwrap().0.active_id;
        let files = [
            "installed.json",
            "engine.json",
            "runtime.env",
            "bundle/manifest.json",
            "bundle/compose.yaml",
        ];
        let before = files
            .iter()
            .map(|p| {
                (
                    *p,
                    hash(&fs::read(profile.join("local-runtime").join(p)).unwrap()),
                )
            })
            .collect::<Vec<_>>();
        let target = crate::LifecycleService::open_retry_diagnostics(target_root.clone()).unwrap();
        let m = target.manifest().unwrap();
        {
            let guard = target.lock().unwrap();
            target.validate_ownership(&m, "docker").unwrap();
            target.validate_volumes(&m, "docker").unwrap();
            crate::run(
                "docker",
                &crate::compose_args(&m, &["up", "--detach"]),
                Some(&target.root.join("bundle")),
                180,
            )
            .unwrap();
            target.check_restoration_guard(&guard).unwrap();
        }
        for _ in 0..120 {
            if pair(&target, &m, store.intent().unwrap().update.plan()).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let result = store.activate_restored_rollback(
            "sha256:4658383ee50284d85c20bbbd742feee684c094a60b02033f6bc60e9c56b0aad2",
            true,
        );
        // Always stop only the already-qualified candidate; retain its volumes/data.
        {
            let guard = target.lock().unwrap();
            crate::run(
                "docker",
                &crate::compose_args(&m, &["stop", "--timeout", "30"]),
                Some(&target.root.join("bundle")),
                180,
            )
            .unwrap();
            target.check_restoration_guard(&guard).unwrap();
        }
        let error = result.as_ref().err().map(|e| e.code.as_str());
        let result_file = root.join(format!("runtime-result-{}.json", uuid::Uuid::new_v4()));
        println!("RUNTIME_RESULT_FILE={}", result_file.display());
        crate::restoration::private_bytes(&result_file,&serde_json::to_vec_pretty(&serde_json::json!({"status":if result.is_ok(){"PASS"}else{"FAIL"},"errorCode":error,"receipt":result.as_ref().ok(),"candidateStopped":true,"newLargeCopies":0,"classification":"Actual native inventory/configuration/image/readiness and rollback selection/authority; preceding failed update journal synthetic, changed migration/native target apply not verified"})).unwrap()).unwrap();
        let receipt = result.unwrap();
        assert!(
            receipt.selection_completed && !receipt.runtime_replayed && !receipt.health_replayed
        );
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::RolledBack
        );
        assert_eq!(
            installations::load(&profile).unwrap().unwrap().0.active_id,
            id
        );
        assert_ne!(old, id);
        assert_eq!(store.root, authority);
        assert_eq!(
            store
                .bound_source_id(&installations::load(&profile).unwrap().unwrap().0)
                .unwrap(),
            id
        );
        for (p, sha) in before {
            assert_eq!(
                hash(&fs::read(profile.join("local-runtime").join(p)).unwrap()),
                sha
            );
        }
        let op = store.intent().unwrap().update.plan().operation_id.clone();
        drop(store);
        let mut store = Store::open(&profile, "default").unwrap();
        assert_eq!(store.root, authority);
        assert!(
            store
                .reconcile_rollback_selection_activation(&op)
                .unwrap()
                .selection_completed
        );
        println!("PASS_NATIVE_REUSED_ROLLBACK_RUNTIME");
    }
}
