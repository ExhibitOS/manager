// SPDX-License-Identifier: Apache-2.0
//! Controlled changed Runtime failure followed by native original-data restoration.
use super::*;
use crate::{Result, Value, err};

pub struct InterruptedRecoveryInputs<'a> {
    pub runtime: RecoveryRuntimeInputs<'a>,
    pub migration: MigrationRuntimeInputs<'a>,
    pub destination: &'a Path,
    pub host_key_file: &'a Path,
    pub service_key_file: &'a Path,
    pub service_archive: &'a Path,
    pub port: u16,
}
fn output_boundary(
    profile: &Path,
    authority: &Path,
    i: &InterruptedRecoveryInputs<'_>,
) -> Result<()> {
    let d = i.destination;
    let parent = d.parent().ok_or_else(|| err("HOST_CHECKPOINT_INVALID"))?;
    installations::private_directory(parent)?;
    if !d.is_absolute()
        || d.exists()
        || fs::canonicalize(parent).ok().as_deref() != Some(parent)
        || d.starts_with(profile)
        || d.starts_with(authority)
        || i.port < 1024
        || [
            i.host_key_file,
            i.service_key_file,
            i.service_archive,
            i.runtime.checkpoint.host,
            i.runtime.checkpoint.trust,
            i.runtime.checkpoint.binding,
        ]
        .iter()
        .any(|p| p.starts_with(d))
    {
        return Err(err("HOST_CHECKPOINT_INVALID"));
    }
    Ok(())
}
impl ExecutionSession<'_> {
    /// Diagnostic only. Does not change the signed Prepared intent, live host,
    /// authority or selection, and does not return an admission/application permit.
    pub fn qualify_interrupted_migrated_recovery(
        &self,
        artifact: &mut PreparedArtifact,
        i: &InterruptedRecoveryInputs<'_>,
    ) -> Result<Value> {
        self.qualify_interrupted_recovery_mode(artifact, i, false)
    }
    /// Reobserve an already restored private candidate without copying or
    /// overwriting it. Native plan/host/trust/data/config/image checks all rerun.
    pub fn recheck_interrupted_original_recovery(
        &self,
        artifact: &mut PreparedArtifact,
        i: &InterruptedRecoveryInputs<'_>,
    ) -> Result<Value> {
        self.qualify_interrupted_recovery_mode(artifact, i, true)
    }
    fn qualify_interrupted_recovery_mode(
        &self,
        artifact: &mut PreparedArtifact,
        i: &InterruptedRecoveryInputs<'_>,
        reopening: bool,
    ) -> Result<Value> {
        self.check()?;
        if !i.runtime.external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if reopening {
            installations::private_directory(i.destination)?;
            if fs::canonicalize(i.destination).ok().as_deref() != Some(i.destination)
                || i.destination.starts_with(&self.store.profile)
                || i.destination.starts_with(&self.store.root)
                || self.store.profile.starts_with(i.destination)
                || self.store.root.starts_with(i.destination)
                || i.port < 1024
                || [i.host_key_file, i.service_key_file, i.service_archive, i.runtime.checkpoint.host, i.runtime.checkpoint.trust, i.runtime.checkpoint.binding].iter().any(|p| p.starts_with(i.destination))
            {
                return Err(err("HOST_CHECKPOINT_INVALID"));
            }
        } else {
            output_boundary(&self.store.profile, &self.store.root, i)?;
        }
        let cp = &i.runtime.checkpoint;
        let checkpoint = self
            .store
            .verify_checkpoint_pair(cp.binding, cp.host, cp.trust, cp.key)?;
        if !checkpoint.receipt().source_plan_bound
            || checkpoint.receipt().generation != artifact.generation
        {
            return Err(err("UPDATE_RECOVERY_CHECKPOINT_UNBOUND"));
        }
        if super::super::read_record(i.host_key_file)
            .map_err(|_| err("PROFILE_KEY_INVALID"))?
            .as_slice()
            != cp.key
        {
            return Err(err("PROFILE_KEY_INVALID"));
        }
        // Fresh restoration reserves all copies; reobservation only stages the signed
        // artifact. Keep the signed plan reserve and 6 GiB free floor in both modes.
        crate::maintenance::input_path(i.service_archive, true)?;
        let mut total = 0u64;
        let mut pending = vec![i.service_archive.to_owned()];
        let mut entries = 0;
        while let Some(p) = pending.pop() {
            let m = fs::symlink_metadata(&p).map_err(|_| err("BACKUP_PATH_INVALID"))?;
            if m.file_type().is_symlink() {
                return Err(err("BACKUP_PATH_INVALID"));
            }
            if m.is_dir() {
                for row in fs::read_dir(&p).map_err(|_| err("BACKUP_PATH_INVALID"))? {
                    entries += 1;
                    if entries > 20000 {
                        return Err(err("STORAGE_QUOTA"));
                    }
                    pending.push(row.map_err(|_| err("BACKUP_PATH_INVALID"))?.path());
                }
            } else if m.is_file() {
                total = total
                    .checked_add(m.len())
                    .ok_or_else(|| err("STORAGE_QUOTA"))?;
            } else {
                return Err(err("BACKUP_PATH_INVALID"));
            }
        }
        let additional = if reopening {
            artifact.verified.release.artifact.bytes
        } else {
            total.checked_mul(3).and_then(|n| n.checked_add(checkpoint.receipt().host_archive_bytes)).ok_or_else(|| err("STORAGE_QUOTA"))?
        };
        let peak = additional
            .checked_add(artifact.plan.required_free_bytes)
            .and_then(|n| n.checked_add(6 * 1024 * 1024 * 1024))
            .ok_or_else(|| err("STORAGE_QUOTA"))?;
        if fs2::available_space(i.destination.parent().unwrap())
            .map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < peak
        {
            return Err(err("RESTORE_SPACE_REQUIRED"));
        }
        let interruption = self.qualify_migrated_runtime_interruption(
            artifact,
            i.runtime.python,
            i.runtime.source_commit,
            i.runtime.maintenance_image,
            i.migration.catalog,
            i.migration.catalog_sha256,
            true,
        )?;
        let held = std::cell::RefCell::new(&mut *artifact);
        let mut result = None;
        self.inspect_restored_candidate_finalized(true,|ctx|{
            self.reverify_prepared_artifact(&mut held.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint,cp.binding,cp.host,cp.trust,cp.key)?;
            let before=ephemeral_inventory::observe_source(ctx,i.runtime.maintenance_image)?;
            let old_candidate=ephemeral_inventory::observe(ctx,i.runtime.maintenance_image)?;
            let (host,trust) = if reopening {
                let current=crate::profile_backup::verify_host_current_borrowed(&self.store.profile,cp.host,cp.key,&self._session,&checkpoint.receipt().host_manifest_sha256)?;
                let host=crate::profile_backup::recheck_extracted_host_current(&self.store.profile,&i.destination.join("host"),&current)?;
                let trust=self.store.verify_trust_checkpoint(&i.destination.join("trust")).map_err(|e|err(e.code()))?;
                (host,trust)
            } else {
                output_boundary(&self.store.profile,&self.store.root,i)?;
                installations::new_directory(i.destination)?;
                let host=crate::profile_backup::extract_host(&self.store.profile,i.host_key_file,cp.host,&i.destination.join("host"),true)?;
                if host.manifest_sha256!=checkpoint.receipt().host_manifest_sha256 {return Err(err("RECOVERY_PAIR_INVALID"));}
                let trust=self.store.extract_trust_checkpoint(cp.trust,&i.destination.join("trust"),cp.key).map_err(|e|err(e.code()))?;
                (host,trust)
            };
            let service_root=i.destination.join("services");
            if reopening { installations::private_directory(&service_root)?; }
            else { installations::new_directory(&service_root)?; }
            let service=LifecycleService::open_retry_diagnostics(service_root.clone())?;
            let guard=service.lock()?;
            let binding=crate::restoration::RestorationBinding::from_plan(ctx.plan)?;
            let restored:crate::restoration::RestorationReceipt=if reopening {
                let job:crate::restoration::RestorationJob=crate::read_json(&service_root.join("restoration.json"))?;
                if job.state!="completed" || job.stage!="complete" || !installations::uuid(&job.id) {return Err(err("UPDATE_ROLLBACK_PROOF_MISSING"));}
                let receipt:crate::restoration::RestorationReceipt=crate::read_json(&service_root.join(format!("restore-{}/receipt.json",job.id)))?;
                binding.check_receipt(&receipt)?;
                let m=service.manifest()?;
                service.validate_ownership(&m,"docker")?;service.validate_volumes(&m,"docker")?;
                let workspace=service_root.join(format!("restore-{}",receipt.id));
                check_restored_source_files(&self.source.root,&workspace,&receipt,ctx.plan)?;
                let raw=crate::installation_backup::source_bytes(&workspace.join("authenticated"),"manifest.json",16*1024*1024,true)?;
                binding.authenticated(&receipt.backup_id,&receipt.authenticated_manifest_sha256,&raw)?;
                if let Err(error)=service.operation(&crate::Action::Start) {
                    service.validate_ownership(&m,"docker")?;service.validate_volumes(&m,"docker")?;
                    crate::run("docker",&crate::compose_args(&m,&["stop","--timeout","30"]),Some(&service_root.join("bundle")),180)?;
                    service.check_restoration_guard(&guard)?;
                    return Err(error);
                }
                receipt
            } else {service.restore_update_candidate_locked(i.runtime.maintenance_image,i.service_key_file,i.service_archive,i.port,&binding,&guard)?};
            let attempt=(|| -> Result<Value>{
                binding.check_receipt(&restored)?;
                let workspace=service_root.join(format!("restore-{}",restored.id));
                check_restored_source_files(&self.source.root,&workspace,&restored,ctx.plan)?;
                let m=service.manifest()?;
                let raw=crate::installation_backup::source_bytes(&workspace.join("authenticated"),"manifest.json",16*1024*1024,true)?;
                // HTTP readiness may precede Docker's first scheduled healthcheck.
                // Retain all image/ownership/volume/readiness gates; only bounded waiting.
                let deadline=std::time::Instant::now()+std::time::Duration::from_secs(90);
                loop {
                    service.check_restoration_guard(&guard)?;
                    match super::super::rollback_runtime::pair(&service,&m,ctx.plan) {
                        Ok(_)=>break,
                        Err(e) if e.code=="UPDATE_ROLLBACK_HEALTH_FAILED" && std::time::Instant::now()<deadline=>std::thread::sleep(std::time::Duration::from_millis(250)),
                        Err(e)=>return Err(e),
                    }
                }
                let mut observations=Vec::new();
                for _ in 0..2 {
                    service.check_restoration_guard(&guard)?;
                    let pair=super::super::rollback_runtime::pair(&service,&m,ctx.plan)?;
                    check_rollback_configuration(i.runtime.maintenance_image,&pair.3[1],&raw)?;
                    let file=super::super::private_file(&workspace.join("authenticated/manifest.json"),false).map_err(|e|err(e.code()))?;
                    let inventory=native_inventory::observe(&native_inventory::Inputs{image:i.runtime.maintenance_image,root:&service_root,manifest:&m,app:&pair.1,blob_volume:&pair.3[0],expected_manifest:&ctx.plan.backup_manifest,expected_system:None},file,||service.check_restoration_guard(&guard))?;
                    check_rollback_inventory(&inventory,ctx.plan)?;
                    let repeated=super::super::rollback_runtime::pair(&service,&m,ctx.plan)?;
                    if pair.0!=repeated.0 || pair.2["Id"]!=repeated.2["Id"] || pair.3!=repeated.3 {return Err(err("UPDATE_TARGET_CHANGED"));}
                    observations.push(inventory);
                }
                Ok(serde_json::json!({"restoration":restored,"inventoryObservations":observations,"originalConfigurationVerified":true,"originalImageAndReadinessVerified":true,"freshServiceRoot":service_root}))
            })();
            // Stop only the new correctly owned project on both success and failure.
            let m=service.manifest()?;service.validate_ownership(&m,"docker")?;service.validate_volumes(&m,"docker")?;
            crate::run("docker",&crate::compose_args(&m,&["stop","--timeout","30"]),Some(&service_root.join("bundle")),180)?;
            service.check_restoration_guard(&guard)?;
            let services=attempt?;
            let after=ephemeral_inventory::observe_source(ctx,i.runtime.maintenance_image)?;
            let candidate_after=ephemeral_inventory::observe(ctx,i.runtime.maintenance_image)?;
            if !candidate_inventory::copies_match(&before.physical,&after.physical)
                || !candidate_inventory::copies_match(&old_candidate.physical,&candidate_after.physical){return Err(err("UPDATE_SOURCE_CHANGED"));}
            crate::profile_backup::verify_extracted_host(&self.store.profile,&i.destination.join("host"),&host)?;
            self.store.verify_trust_checkpoint(&i.destination.join("trust")).map_err(|e|err(e.code()))?;
            self.store.recheck_checkpoint_pair(&checkpoint,cp.binding,cp.host,cp.trust,cp.key)?;
            Ok(serde_json::json!({"interruption":interruption,"host":host,"trust":trust,"services":services,"sourceBefore":before,"sourceAfter":after,"candidateBefore":old_candidate,"candidateAfter":candidate_after,"inactiveHostAndFreshOriginalServicesVerified":true,"servicesStopped":true,"hostActivated":false,"selectionActivated":false,"wholeFailedUpdateRecoveryVerified":false,"preflightVerified":false,"updateExecuted":false}))
        },|_,receipt|{
            self.reverify_prepared_artifact(&mut held.borrow_mut())?;
            self.store.recheck_checkpoint_pair(&checkpoint,cp.binding,cp.host,cp.trust,cp.key)?;
            result=Some(receipt.clone());Ok(())
        })?;
        result.ok_or_else(|| err("UPDATE_RECOVERY_RUNTIME_MISMATCH"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_destination_refuses_existing_alias_and_protected_input_overlap() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-failed-recovery-boundary-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&root).unwrap();
        let profile = root.join("profile");
        let authority = root.join("authority");
        installations::new_directory(&profile).unwrap();
        installations::new_directory(&authority).unwrap();
        let destination = root.join("fresh");
        let key_file = root.join("key");
        let archive = root.join("archive");
        let host = root.join("host");
        let trust = root.join("trust");
        let binding = root.join("binding");
        let python = root.join("python");
        let catalog = root.join("catalog");
        let key = [0u8; 32];
        let mut inputs = InterruptedRecoveryInputs {
            runtime: RecoveryRuntimeInputs {
                checkpoint: super::super::CheckpointInputs {
                    host: &host,
                    trust: &trust,
                    binding: &binding,
                    key: &key,
                },
                export_parent: &root,
                python: &python,
                source_commit: "not-used",
                maintenance_image: "not-used",
                external_writers_quiesced: true,
            },
            migration: MigrationRuntimeInputs {
                catalog: &catalog,
                catalog_sha256: "not-used",
            },
            destination: &destination,
            host_key_file: &key_file,
            service_key_file: &key_file,
            service_archive: &archive,
            port: 20000,
        };
        output_boundary(&profile, &authority, &inputs).unwrap();
        installations::new_directory(&destination).unwrap();
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        let inside = profile.join("fresh");
        inputs.destination = &inside;
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        let hidden = authority.join("fresh");
        inputs.destination = &hidden;
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&profile, &alias).unwrap();
        let alias_output = alias.join("fresh");
        inputs.destination = &alias_output;
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        let fresh = root.join("fresh2");
        inputs.destination = &fresh;
        let contained_key = fresh.join("key");
        inputs.host_key_file = &contained_key;
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        inputs.host_key_file = &key_file;
        inputs.port = 80;
        assert!(output_boundary(&profile, &authority, &inputs).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "explicit signed small complete fixture; post-SIGKILL complete inactive host/trust and fresh encrypted source service restore"]
    fn actual_interrupted_migrated_original_recovery() {
        let args: Value = serde_json::from_slice(
            &fs::read(std::env::var("EXHIBITOS_FAILED_RECOVERY_COMMAND").unwrap()).unwrap(),
        )
        .unwrap();
        let command = args["command"].as_array().unwrap();
        let option = |name: &str| {
            command[command.iter().position(|x| x == name).unwrap() + 1]
                .as_str()
                .unwrap()
        };
        let profile = PathBuf::from(option("--profile"));
        let mut store = super::super::super::Store::open(&profile, "default").unwrap();
        let generation = store.current.generation;
        let plan = store.intent().unwrap().update.plan().clone();
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        let selection = installations::load(&profile).unwrap().unwrap().1;
        let parent = profile.parent().unwrap();
        let staging = parent.join(format!("failed-recovery-staging-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&staging).unwrap();
        let existing = std::env::var("EXHIBITOS_FAILED_RECOVERY_RECHECK_DESTINATION")
            .ok()
            .map(PathBuf::from);
        let destination = existing.clone().unwrap_or_else(|| {
            parent.join(format!("failed-recovery-output-{}", uuid::Uuid::new_v4()))
        });
        let host = PathBuf::from(option("--host-archive"));
        let trust = PathBuf::from(option("--trust-archive"));
        let binding = PathBuf::from(option("--pair-binding"));
        let host_key = PathBuf::from(option("--key"));
        let key = super::super::super::read_record(&host_key).unwrap();
        let key: [u8; 32] = key.try_into().unwrap();
        let python = PathBuf::from(option("--python"));
        let catalog = PathBuf::from(option("--target-catalog"));
        let exports = PathBuf::from(option("--export-parent"));
        let service_key = PathBuf::from(option("--service-key"));
        let archive = PathBuf::from(option("--service-archive"));
        let inputs = InterruptedRecoveryInputs {
            runtime: RecoveryRuntimeInputs {
                checkpoint: super::super::CheckpointInputs {
                    host: &host,
                    trust: &trust,
                    binding: &binding,
                    key: &key,
                },
                export_parent: &exports,
                python: &python,
                source_commit: option("--source-commit"),
                maintenance_image: option("--maintenance-image"),
                external_writers_quiesced: true,
            },
            migration: MigrationRuntimeInputs {
                catalog: &catalog,
                catalog_sha256: option("--target-catalog-sha256"),
            },
            destination: &destination,
            host_key_file: &host_key,
            service_key_file: &service_key,
            service_archive: &archive,
            port: option("--port").parse().unwrap(),
        };
        let session = store.execution().unwrap();
        let mut artifact = session
            .stage_prepared_artifact(&PathBuf::from(option("--artifact")), &staging)
            .unwrap();
        let result = if existing.is_some() {
            session.recheck_interrupted_original_recovery(&mut artifact, &inputs)
        } else {
            session.qualify_interrupted_migrated_recovery(&mut artifact, &inputs)
        };
        drop(session);
        let report = parent.join(format!(
            "failed-recovery-report-{}.json",
            uuid::Uuid::new_v4()
        ));
        let receipt = serde_json::json!({"status":if result.is_ok(){"PASS"}else{"FAIL"},"errorCode":result.as_ref().err().map(|e|e.code.as_str()),"receipt":result.as_ref().ok(),"destination":destination,"generation":generation,"wholeFailedUpdateRecoveryVerified":false});
        crate::restoration::private_bytes(&report, &serde_json::to_vec_pretty(&receipt).unwrap())
            .unwrap();
        let proof = result.unwrap();
        assert_eq!(proof["inactiveHostAndFreshOriginalServicesVerified"], true);
        assert_eq!(store.current.generation, generation);
        assert_eq!(store.intent().unwrap().update.plan(), &plan);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Prepared
        );
        assert_eq!(installations::load(&profile).unwrap().unwrap().1, selection);
    }
}
