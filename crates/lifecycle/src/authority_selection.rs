// SPDX-License-Identifier: Apache-2.0
//! A controller keeps its original authority namespace across selected instances.
//! Binding derives from the verified immutable completion history, never a route
//! supplied by a frontend or a newly provisioned candidate policy.
use super::*;
impl Store {
    pub(super) fn bound_source_id(
        &self,
        registry: &installations::Registry,
    ) -> crate::Result<String> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        installations::valid(registry)?;
        let original = registry
            .installations
            .iter()
            .find(|entry| {
                if self.installation == "default" {
                    entry.kind == "default"
                } else {
                    entry.id == self.installation
                }
            })
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let mut bound = original.id.clone();
        let mut head = "0".repeat(64);
        let mut previous = None;
        // Re-read every exact record while retaining the Store/profile fence.
        // Changes to historical health cannot redirect an already open executor.
        for generation in 1..=self.current.generation {
            let raw = read_record(&self.root.join(format!("{generation:020}.json")))
                .map_err(|e| crate::err(e.code()))?;
            let record: Record =
                serde_json::from_slice(&raw).map_err(|_| crate::err("UPDATE_AUTHORITY_CHANGED"))?;
            valid_record(&record, &self.scope).map_err(|e| crate::err(e.code()))?;
            if record.generation != generation || record.previous_sha256 != head {
                return Err(crate::err("UPDATE_AUTHORITY_CHANGED"));
            }
            if let Some(old) = &previous {
                transition(old, &record).map_err(|e| crate::err(e.code()))?;
            }
            if let Some(UpdateEvent::Health(health)) = &record.update_event {
                let intent = record
                    .intent
                    .as_ref()
                    .ok_or_else(|| crate::err("UPDATE_AUTHORITY_CHANGED"))?;
                if matches!(
                    intent.update.stage(),
                    crate::update::Stage::Updated | crate::update::Stage::RolledBack
                ) {
                    if !health.ready || !installations::uuid(&health.instance_id) {
                        return Err(crate::err("UPDATE_SOURCE_UNREGISTERED"));
                    }
                    bound = health.instance_id.clone();
                }
            }
            head = hash(&raw);
            previous = Some(record);
        }
        if head != self.current_sha256 {
            return Err(crate::err("UPDATE_AUTHORITY_CHANGED"));
        }
        if !registry.installations.iter().any(|entry| entry.id == bound) {
            return Err(crate::err("UPDATE_SOURCE_UNREGISTERED"));
        }
        self.check_root().map_err(|e| crate::err(e.code()))?;
        Ok(bound)
    }
}

/// Internal routing from existing, verified immutable completion history.
pub(crate) fn resolve_selected_scope(profile: &Path, registry: &installations::Registry) -> crate::Result<String> {
    #[cfg(not(any(unix, windows)))]
    { let _ = (profile, registry); Err(crate::err("UPDATE_TRUST_PLATFORM_UNVERIFIED")) }
    #[cfg(any(unix, windows))]
    {
        installations::valid(registry)?;
        let selected = registry.installations.iter().find(|e| e.id == registry.active_id)
            .ok_or_else(|| crate::err("INSTALLATION_SELECTION_INVALID"))?;
        let mut scopes = BTreeSet::from(["default".to_owned()]);
        scopes.extend(registry.installations.iter().map(|e| e.id.clone()));
        let mut matched = None;
        let mut selected_used = false;
        for scope in scopes {
            let (primary, recovery) = lifecycle_writer::namespaces(profile, &scope)?;
            if !lifecycle_writer::present(&primary)? && !lifecycle_writer::present(&recovery)? { continue; }
            let store = Store::open_for_inspection(profile, &scope).map_err(|e| crate::err(e.code()))?;
            selected_used |= store.used_instances.contains(&registry.active_id);
            if (store.bound_source_id(registry)? == registry.active_id
                || selection_activation::pending_activation(&store, registry)?.is_some())
                && matched.replace(scope).is_some() {
                return Err(crate::err("UPDATE_AUTHORITY_UNAVAILABLE"));
            }
        }
        if let Some(scope) = matched { return Ok(scope); }
        // A previously used candidate cannot escape its history as an absent authority.
        if selected_used { return Err(crate::err("UPDATE_SOURCE_MISMATCH")); }
        Ok(if selected.kind == "default" { "default".into() } else { selected.id.clone() })
    }
}
impl Store {
    pub(crate) fn verify_bound_selected(&self, registry: &installations::Registry) -> crate::Result<()> {
        if self.bound_source_id(registry)? != registry.active_id {
            return Err(crate::err("UPDATE_HANDOFF_SELECTION_CHANGED"));
        }
        Ok(())
    }
}

/// Existing-only projection. It cannot create an authority, recover a journal or replay health.
pub(crate) fn inspect_rollback_context(profile: &Path, scope: &str, installation: &str)
    -> crate::Result<Option<crate::update_rollback_handoff::RollbackContext>> {
    #[cfg(not(any(unix, windows)))]
    { let _=(profile,scope,installation); Err(crate::err("UPDATE_TRUST_PLATFORM_UNVERIFIED")) }
    #[cfg(any(unix, windows))]
    {
        let store=match Store::open_for_inspection(profile,scope) {
            Ok(s)=>s, Err(Error::TrustMissing)=>return Ok(None), Err(e)=>return Err(crate::err(e.code())),
        };
        store.rollback_handoff_context(installation)
    }
}
#[cfg(any(unix, windows))]
impl Store {
    pub(crate) fn rollback_handoff_context(&self, installation: &str)
        -> crate::Result<Option<crate::update_rollback_handoff::RollbackContext>> {
        use crate::update::Stage;
        self.check_root().map_err(|e|crate::err(e.code()))?;
        if self.receipt().write_uncertain {return Err(crate::err("UPDATE_HANDOFF_AUTHORITY_CHANGED"));}
        let (registry,_)=installations::load(&self.profile)?.ok_or_else(||crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        if registry.active_id!=installation {return Err(crate::err("UPDATE_HANDOFF_SELECTION_CHANGED"));}
        let pending=selection_activation::pending_activation(self,&registry)?;
        if self.bound_source_id(&registry)?!=installation && pending.is_none() {return Err(crate::err("UPDATE_HANDOFF_SELECTION_CHANGED"));}
        let Some(intent)=self.intent() else {return Ok(None)};
        let u=&intent.update;
        let stage=u.stage();
        if stage==Stage::Prepared {return Ok(None);}
        let p=u.plan();
        let can_complete=stage==Stage::AwaitingRollbackHealth
            || (stage==Stage::RecoveryRequired && u.interrupted_restoration().is_some())
            || (stage==Stage::RolledBack && pending.is_some());
        let can_restore=matches!(stage,Stage::Updated|Stage::Applying|Stage::AwaitingHealth|Stage::Restoring|Stage::RecoveryRequired);
        if !can_restore && !can_complete {return Ok(None);}
        Ok(Some(crate::update_rollback_handoff::RollbackContext {
            installation_id:installation.into(),operation_id:p.operation_id.clone(),generation:self.current.generation,stage,
            backup_id:p.backup_id.clone(),source_image:p.source_image.clone(),source_schema:p.source_schema.clone(),
            backup_manifest:p.backup_manifest.clone(),source_inventory:p.source_inventory.clone(),
            can_restore_original:can_restore,can_complete_restored:can_complete,selection_reconciliation_required:pending.is_some(),
        }))
    }
    /// The caller first verifies its frozen context inside this same exclusive Store.
    /// Existing methods retain their own namespace/operation guards throughout native work.
    pub(crate) fn execute_rollback_handoff(&mut self, input:&crate::update_rollback_handoff::RollbackInput)->crate::Result<()> {
        use crate::update::Stage;
        use crate::update_rollback_handoff::RollbackAction;
        crate::update_rollback_handoff::validate(input)?;
        let (registry,_)=installations::load(&self.profile)?.ok_or_else(||crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let context=self.rollback_handoff_context(&registry.active_id)?.ok_or_else(||crate::err("UPDATE_INTENT_MISSING"))?;
        if context.operation_id!=input.operation_id || context.generation!=input.generation {
            return Err(crate::err("UPDATE_HANDOFF_AUTHORITY_CHANGED"));
        }
        if match input.action {RollbackAction::RestoreOriginal=>!context.can_restore_original,RollbackAction::CompleteRestored=>!context.can_complete_restored} {
            return Err(crate::err("UPDATE_ROLLBACK_STAGE_INVALID"));
        }
        let pending=selection_activation::pending_activation(self,&registry)?;
        self.recover_rollback_handoff_interruption().map_err(|e|crate::err(e.code()))?;
        if let Some((operation,rollback))=pending {
            if rollback {self.reconcile_rollback_selection_activation(&operation)?;}
            else {self.reconcile_selection_activation(&operation)?;}
        }
        match input.action {
            RollbackAction::RestoreOriginal=>{
                if self.intent().is_some_and(|i|i.update.stage()==Stage::Updated) {
                    self.request_rollback(&input.operation_id,self.current.generation,owned_execution::release_now()?)
                        .map_err(|e|crate::err(e.code()))?;
                }
                self.register_rollback_candidate(input.preserve_data)?;
                self.restore_registered_rollback_candidate(&input.maintenance_image,
                    input.key_file.as_deref().ok_or_else(||crate::err("UPDATE_HANDOFF_INVALID"))?,
                    input.archive.as_deref().ok_or_else(||crate::err("UPDATE_HANDOFF_INVALID"))?,
                    input.port.ok_or_else(||crate::err("UPDATE_HANDOFF_INVALID"))?,input.external_writers_quiesced)?;
                self.activate_restored_rollback(&input.maintenance_image,input.external_writers_quiesced)?;
            },
            RollbackAction::CompleteRestored=>{
                if !self.intent().is_some_and(|i|i.update.stage()==Stage::RolledBack) {
                    self.activate_restored_rollback(&input.maintenance_image,input.external_writers_quiesced)?;
                }
            },
        }
        if !self.intent().is_some_and(|i|i.update.stage()==Stage::RolledBack) {return Err(crate::err("UPDATE_ROLLBACK_UNCERTAIN"));}
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod windows_rollback_handoff_tests {
    use super::super::windows_status_handoff_tests::cleanup;
    use super::*;
    use crate::update_rollback_handoff as handoff;
    use crate::windows_private::PrivateDirectory;

    fn fixture() -> (PrivateDirectory, PathBuf, u64) {
        let (parent, profile, mut store, verified) =
            super::super::windows_status_handoff_tests::prepared();
        store
            .begin_update(
                super::super::windows_status_handoff_tests::observations(),
                &verified,
                21,
            )
            .unwrap();
        store
            .update_failed("update-1", store.receipt().generation, 22)
            .unwrap();
        let generation = store.receipt().generation;
        drop(store);
        (parent, profile, generation)
    }
    fn input(profile: &Path, generation: u64) -> handoff::RollbackInput {
        handoff::RollbackInput {
            action: handoff::RollbackAction::RestoreOriginal,
            operation_id: "update-1".into(),
            generation,
            maintenance_image: format!("sha256:{}", "a".repeat(64)),
            key_file: Some(profile.parent().unwrap().join("unused-key.bin")),
            archive: Some(profile.parent().unwrap().join("unused-backup")),
            port: Some(50449),
            preserve_data: true,
            close_app_and_rollback: true,
            external_writers_quiesced: true,
        }
    }
    #[test]
    fn windows_rollback_context_and_consent_preserve_native_authority_without_key_or_engine_access()
    {
        let (parent, profile, generation) = fixture();
        let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
        let selected = controller.context().unwrap();
        let context = controller
            .update_rollback_context(&selected.selection_token)
            .unwrap()
            .unwrap();
        assert_eq!(context.installation_id, selected.active_id);
        assert_eq!(context.generation, generation);
        assert_eq!(context.operation_id, "update-1");
        assert!(context.can_restore_original && !context.can_complete_restored);
        let json = serde_json::to_value(&context).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 12);
        for forbidden in ["keyFile", "archive", "scope", "candidatePath", "health"] {
            assert!(json.get(forbidden).is_none());
        }
        let store = Store::open_for_inspection(&profile, "default").unwrap();
        let head = store.current_sha256.clone();
        drop(store);
        let mut denied = input(&profile, generation);
        denied.external_writers_quiesced = false;
        assert!(
            controller
                .update_rollback_handoff(&selected.selection_token, Some(denied))
                .is_err()
        );
        assert!(!profile.join("update-rollback-handoffs").exists());
        let store = Store::open_for_inspection(&profile, "default").unwrap();
        assert_eq!(store.current_sha256, head);
        assert_eq!(store.receipt().generation, generation);
        drop(store);
        drop(controller);
        cleanup(parent);
    }
    #[test]
    fn windows_rollback_claim_waits_for_app_close_and_stale_generation_cannot_recover_or_publish_success()
     {
        let (parent, profile, generation) = fixture();
        let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
        let selected = controller.context().unwrap();
        let (_, receipt) = controller
            .update_rollback_handoff(&selected.selection_token, Some(input(&profile, generation)))
            .unwrap();
        let receipt = receipt.unwrap();
        drop(controller);
        let mut store = Store::open_for_prepared_handoff(&profile, "default").unwrap();
        store
            .begin_restore("update-1", generation, uuid::Uuid::new_v4().to_string(), 23)
            .unwrap();
        let changed_generation = store.receipt().generation;
        let head = store.current_sha256.clone();
        drop(store);
        let (_, app) = profile_backup::anchor_lock(&profile, false).unwrap();
        let worker_profile = profile.clone();
        let worker_id = receipt.id.clone();
        let worker = std::thread::spawn(move || handoff::execute(&worker_profile, &worker_id));
        let mailbox = profile.join("update-rollback-handoffs");
        let claim = mailbox.join(format!("{}.claim.json", receipt.id));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !claim.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(claim.exists());
        assert_eq!(
            handoff::latest(&profile, &selected.active_id)
                .unwrap()
                .unwrap()
                .state,
            "uncertain"
        );
        assert_eq!(
            handoff::execute(&profile, &receipt.id).unwrap_err().code,
            "UPDATE_HANDOFF_ALREADY_CLAIMED"
        );
        assert!(
            !mailbox
                .join(format!("{}.outcome.json", receipt.id))
                .exists()
        );
        drop(app);
        assert_eq!(
            worker.join().unwrap().unwrap_err().code,
            "UPDATE_HANDOFF_AUTHORITY_CHANGED"
        );
        let store = Store::open_for_inspection(&profile, "default").unwrap();
        assert_eq!(store.receipt().generation, changed_generation);
        assert_eq!(store.current_sha256, head);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Restoring
        );
        drop(store);
        let outcome = handoff::latest(&profile, &selected.active_id)
            .unwrap()
            .unwrap();
        assert_eq!(outcome.state, "failed");
        assert_eq!(
            outcome.code.as_deref(),
            Some("UPDATE_HANDOFF_AUTHORITY_CHANGED")
        );
        assert!(handoff::execute(&profile, &receipt.id).is_err());
        cleanup(parent);
    }
}
