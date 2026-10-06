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
