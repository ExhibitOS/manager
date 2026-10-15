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
    #[cfg(not(unix))]
    { let _ = (profile, registry); Err(crate::err("UPDATE_TRUST_PLATFORM_UNVERIFIED")) }
    #[cfg(unix)]
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
            if store.bound_source_id(registry)? == registry.active_id {
                if matched.replace(scope).is_some() { return Err(crate::err("UPDATE_AUTHORITY_UNAVAILABLE")); }
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
