// SPDX-License-Identifier: Apache-2.0
//! Inactive fresh target registration, preserving source selection and security history.
use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredUpdateTarget {
    pub source_instance: String,
    pub target_instance: String,
    pub target_path: String,
    pub active_instance: String,
    pub registry_sha256: String,
    pub activated: bool,
    pub runtime_started: bool,
}
impl Store {
    /// Operator action under the existing Store anchor. No arbitrary path/ID,
    /// active-selection change, release acceptance or Engine mutation.
    pub fn register_update_target(
        &self,
        preserve_active: bool,
    ) -> crate::Result<RegisteredUpdateTarget> {
        self.register_update_target_impl(preserve_active, false)
    }
    /// Register only the exact target reserved by the current Prepared plan.
    /// Existing names, candidates and aliases are never adopted or overwritten.
    pub fn register_planned_update_target(
        &self,
        preserve_active: bool,
    ) -> crate::Result<RegisteredUpdateTarget> {
        self.register_update_target_impl(preserve_active, true)
    }
    fn register_update_target_impl(
        &self,
        preserve_active: bool,
        planned: bool,
    ) -> crate::Result<RegisteredUpdateTarget> {
        self.check_root().map_err(|e| crate::err(e.code()))?;
        if !preserve_active {
            return Err(crate::err("UPDATE_TARGET_ACK_REQUIRED"));
        }
        if self
            .intent()
            .is_some_and(|i| i.update.stage() != crate::update::Stage::Prepared)
        {
            return Err(crate::err("UPDATE_CANDIDATE_STAGE_INVALID"));
        }
        if planned && self.intent().is_none() {
            return Err(crate::err("UPDATE_INTENT_MISSING"));
        }
        let anchor = self
            ._anchor
            .try_clone()
            .map_err(|_| crate::err("UPDATE_FENCE_UNAVAILABLE"))?;
        let _session = profile_backup::anchored_session(&self.profile, anchor, true)?;
        let profile = crate::LifecycleService::open_retry_diagnostics(self.profile.clone())?;
        let _profile = installations::profile_lock(&profile)?;
        let (mut registry, previous) = installations::load(&self.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| {
                if self.installation == "default" {
                    e.kind == "default"
                } else {
                    e.id == self.installation
                }
            })
            .ok_or_else(|| crate::err("UPDATE_SOURCE_UNREGISTERED"))?;
        let source_id = entry.id.clone();
        if self
            .intent()
            .is_some_and(|i| i.update.plan().source_instance != source_id)
        {
            return Err(crate::err("UPDATE_SOURCE_MISMATCH"));
        }
        let source_root = installations::root(&self.profile, entry);
        installations::private_directory(&source_root)?;
        let source = crate::LifecycleService::open_retry_diagnostics(source_root)?;
        let _source = source.lock()?;
        if registry.installations.len() >= 128 {
            return Err(crate::err("INSTALLATION_SELECTION_LIMIT"));
        }
        let parent = self.profile.join("installations");
        match fs::symlink_metadata(&parent) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                installations::new_directory(&parent)?
            }
            Ok(_) => installations::private_directory(&parent)?,
            Err(_) => return Err(crate::err("STATE_UNAVAILABLE")),
        }
        let target = installations::Entry {
            id: if planned {
                self.intent().ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
                    .update.plan().target_instance.clone()
            } else {
                uuid::Uuid::new_v4().to_string()
            },
            kind: "recovery".into(),
            created_at: crate::now(),
        };
        // Preparation already reserved this exact target after rejecting prior
        // history reuse. Only that current reservation may be materialized.
        if (!planned && self.used_instances.contains(&target.id))
            || registry.installations.iter().any(|e| e.id == target.id)
        {
            return Err(crate::err("UPDATE_IDENTITY_REUSED"));
        }
        let root = installations::root(&self.profile, &target);
        installations::new_directory(&root)?;
        let active = registry.active_id.clone();
        let id = target.id.clone();
        registry.installations.push(target);
        // Registry readback must still match the locked baseline before publication.
        if installations::load(&self.profile)?.map(|(_, b)| b) != Some(previous.clone()) {
            return Err(crate::err("UPDATE_SOURCE_CHANGED"));
        }
        self.check_root().map_err(|e| crate::err(e.code()))?;
        installations::save(&self.profile, &registry, Some(&previous))?;
        let (current, bytes) = installations::load(&self.profile)?
            .ok_or_else(|| crate::err("INSTALLATION_SELECTION_UNCERTAIN"))?;
        if current.active_id != active
            || current.installations.len() != registry.installations.len()
            || !current
                .installations
                .iter()
                .any(|e| e.id == id && e.kind == "recovery")
        {
            return Err(crate::err("INSTALLATION_SELECTION_UNCERTAIN"));
        }
        Ok(RegisteredUpdateTarget {
            source_instance: source_id,
            target_instance: id,
            target_path: root.to_string_lossy().into_owned(),
            active_instance: active,
            registry_sha256: crate::digest(&bytes),
            activated: false,
            runtime_started: false,
        })
    }
}
