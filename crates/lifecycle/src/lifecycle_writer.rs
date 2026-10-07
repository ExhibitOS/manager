// SPDX-License-Identifier: Apache-2.0
//! Ordinary runtime writers consult immutable update history before any job or engine write.
use super::*;
use crate::{Action, LifecycleError};
use std::collections::BTreeMap;

fn unavailable() -> LifecycleError {
    crate::err("UPDATE_AUTHORITY_UNAVAILABLE")
}
fn present(path: &Path) -> crate::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(unavailable()),
    }
}
fn namespaces(profile: &Path, installation: &str) -> crate::Result<(PathBuf, PathBuf)> {
    let path = profile.to_str().ok_or_else(unavailable)?;
    let id = hash(format!("ExhibitOS-release-trust-v1\0{path}\0{installation}").as_bytes());
    let parent = profile.parent().ok_or_else(unavailable)?;
    Ok((
        parent.join(format!(".exhibitos-release-trust-{id}")),
        parent.join(format!(".exhibitos-release-recovery-{id}")),
    ))
}
fn disposition(update: &crate::update::Update, instance: &str) -> Option<bool> {
    use crate::update::Stage;
    let stage = update.stage();
    if update.plan().target_instance == instance {
        return Some(!matches!(stage, Stage::Prepared | Stage::Updated));
    }
    if update.plan().source_instance == instance {
        return Some(!matches!(
            stage,
            Stage::Prepared | Stage::Updated | Stage::RolledBack
        ));
    }
    if update.restore_candidate() == Some(instance) {
        return Some(stage != Stage::RolledBack);
    }
    None
}
fn check_store(store: &Store, instance: &str) -> crate::Result<()> {
    // Re-read the complete chain, including terminal intents later cleared from
    // the head. A failed migrated target must stay fenced after successful rollback.
    let mut previous = "0".repeat(64);
    let mut operations = BTreeMap::new();
    for generation in 1..=store.current.generation {
        let bytes = read_record(&store.root.join(format!("{generation:020}.json")))
            .map_err(|_| unavailable())?;
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        valid_record(&record, &store.scope).map_err(|_| unavailable())?;
        if record.generation != generation || record.previous_sha256 != previous {
            return Err(unavailable());
        }
        if let Some(intent) = &record.intent
            && let Some(blocked) = disposition(&intent.update, instance)
        {
            operations.insert(intent.update.plan().operation_id.clone(), blocked);
        }
        previous = hash(&bytes);
    }
    if previous != store.current_sha256 {
        return Err(unavailable());
    }
    store.check_root().map_err(|_| unavailable())?;
    if operations.values().any(|blocked| *blocked) {
        return Err(crate::err("UPDATE_WRITER_BLOCKED"));
    }
    Ok(())
}

pub(crate) fn admit(root: &Path, action: &Action) -> crate::Result<()> {
    if *action == Action::Stop {
        return Ok(());
    }
    let profile = if root.file_name().is_some_and(|n| n == "local-runtime") {
        root.parent()
    } else if root
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "installations")
    {
        root.parent().and_then(Path::parent)
    } else {
        return Ok(());
    }
    .ok_or_else(unavailable)?;
    let loaded = installations::load(profile).map_err(|_| unavailable())?;
    let mut scopes = vec!["default".to_owned()];
    if let Some((registry, _)) = &loaded {
        scopes.extend(registry.installations.iter().map(|e| e.id.clone()));
    }
    let mut enrolled = Vec::new();
    for installation in scopes {
        let (primary, recovery) = namespaces(profile, &installation)?;
        if present(&primary)? || present(&recovery)? {
            enrolled.push(installation);
        }
    }
    if enrolled.is_empty() {
        return Ok(());
    }
    // Shared admission is compatible with a live desktop profile session. Update
    // executors take the exclusive anchor and the same runtime operation lock.
    let (_, _anchor) = profile_backup::anchor_lock(profile, false).map_err(|_| unavailable())?;
    let (registry, original_bytes) = loaded.ok_or_else(unavailable)?;
    let entry = registry
        .installations
        .iter()
        .find(|e| installations::root(profile, e) == root)
        .ok_or_else(unavailable)?;
    for installation in enrolled {
        let store = Store::open_mode_with_anchor(profile, &installation, false, false)
            .map_err(|_| unavailable())?;
        check_store(&store, &entry.id)?;
    }
    if installations::load(profile)
        .map_err(|_| unavailable())?
        .is_none_or(|(_, bytes)| bytes != original_bytes)
    {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_writer_prepared_and_applying_history_gate_without_recovery_write() {
        let (profile, mut store, verified) = super::super::tests::prepared_fixture();
        assert!(check_store(&store, "source-1").is_ok());
        assert!(check_store(&store, "target-1").is_ok());
        store
            .begin_update(super::super::tests::observations(), &verified, 21)
            .unwrap();
        let head = store.current_sha256.clone();
        assert_eq!(
            check_store(&store, "source-1").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
        assert_eq!(
            check_store(&store, "target-1").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
        assert!(check_store(&store, "unrelated").is_ok());
        drop(store);
        let store = Store::open_mode_with_anchor(&profile, "default", false, false).unwrap();
        assert_eq!(store.current_sha256, head);
        assert_eq!(
            store.intent().unwrap().update.stage(),
            crate::update::Stage::Applying
        );
        assert_eq!(
            check_store(&store, "target-1").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
    }
    #[test]
    fn lifecycle_writer_inspection_is_compatible_with_shared_profile_and_refuses_exclusive() {
        let (profile, store, _) = super::super::tests::prepared_fixture();
        drop(store);
        let (_, shared) = profile_backup::anchor_lock(&profile, false).unwrap();
        let store = Store::open_mode_with_anchor(&profile, "default", false, false).unwrap();
        assert!(check_store(&store, "source-1").is_ok());
        drop(store);
        drop(shared);
        let (_, exclusive) = profile_backup::anchor_lock(&profile, true).unwrap();
        assert!(Store::open_mode_with_anchor(&profile, "default", false, false).is_err());
        drop(exclusive);
    }
    #[test]
    fn lifecycle_writer_changed_record_refuses_and_stop_is_available() {
        let (_, store, _) = super::super::tests::prepared_fixture();
        let record = store.root.join("00000000000000000002.json");
        fs::write(&record, b"{}").unwrap();
        assert_eq!(
            check_store(&store, "source-1").unwrap_err().code,
            "UPDATE_AUTHORITY_UNAVAILABLE"
        );
        assert!(admit(&store.profile.join("local-runtime"), &Action::Stop).is_ok());
    }
    #[test]
    fn lifecycle_writer_terminal_history_survives_pointer_release_and_allows_restored_original() {
        let (_, mut store, verified) = super::super::tests::prepared_fixture();
        let plan = super::super::tests::plan();
        store
            .begin_update(super::super::tests::observations(), &verified, 21)
            .unwrap();
        store.update_failed(&plan.operation_id, 3, 22).unwrap();
        store
            .begin_restore(&plan.operation_id, 4, "original-clone".into(), 23)
            .unwrap();
        assert_eq!(
            check_store(&store, "original-clone").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
        store
            .restore_finished(
                &plan.operation_id,
                5,
                crate::update::RestoreReceipt {
                    operation_id: plan.operation_id.clone(),
                    backup_id: plan.backup_id.clone(),
                    backup_manifest: plan.backup_manifest.clone(),
                    inventory_digest: plan.source_inventory.clone(),
                    candidate_id: "original-clone".into(),
                    schema: plan.source_schema.clone(),
                    inventory_verified: true,
                    separate_candidate: true,
                },
                24,
            )
            .unwrap();
        store
            .observe_health(
                &plan.operation_id,
                6,
                crate::update::HealthReceipt {
                    operation_id: plan.operation_id.clone(),
                    instance_id: "original-clone".into(),
                    image: plan.source_image.clone(),
                    schema: plan.source_schema.clone(),
                    ready: true,
                },
                25,
            )
            .unwrap();
        assert!(check_store(&store, "source-1").is_ok());
        assert!(check_store(&store, "original-clone").is_ok());
        assert_eq!(
            check_store(&store, "target-1").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
        store.release_completed(&plan.operation_id, 7, 26).unwrap();
        assert!(store.intent().is_none());
        assert!(check_store(&store, "original-clone").is_ok());
        assert_eq!(
            check_store(&store, "target-1").unwrap_err().code,
            "UPDATE_WRITER_BLOCKED"
        );
    }
    #[test]
    fn lifecycle_writer_successful_target_remains_allowed_after_pointer_release() {
        let (_, mut store, verified) = super::super::tests::prepared_fixture();
        let plan = super::super::tests::plan();
        store
            .begin_update(super::super::tests::observations(), &verified, 21)
            .unwrap();
        store
            .application_finished(&plan.operation_id, 3, 22)
            .unwrap();
        store
            .observe_health(
                &plan.operation_id,
                4,
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
        assert!(check_store(&store, "target-1").is_ok());
        store.release_completed(&plan.operation_id, 5, 24).unwrap();
        assert!(check_store(&store, "target-1").is_ok());
    }
    #[test]
    fn lifecycle_writer_missing_primary_with_recovery_locator_refuses_without_registry_change() {
        let (profile, store, _) = super::super::tests::prepared_fixture();
        let primary = store.root.clone();
        drop(store);
        let id = uuid::Uuid::new_v4().to_string();
        let registry = installations::Registry {
            format: 1,
            active_id: id.clone(),
            installations: vec![installations::Entry {
                id,
                kind: "default".into(),
                created_at: 0,
            }],
        };
        installations::save(&profile, &registry, None).unwrap();
        let before = installations::load(&profile).unwrap().unwrap().1;
        let (_, locator) = namespaces(&profile, "default").unwrap();
        installations::new_directory(&locator).unwrap();
        fs::rename(
            &primary,
            primary.with_file_name("preserved-missing-primary"),
        )
        .unwrap();
        assert_eq!(
            admit(&profile.join("local-runtime"), &Action::Start)
                .unwrap_err()
                .code,
            "UPDATE_AUTHORITY_UNAVAILABLE"
        );
        assert_eq!(installations::load(&profile).unwrap().unwrap().1, before);
        assert!(!primary.exists());
        assert!(admit(&profile.join("local-runtime"), &Action::Stop).is_ok());
    }
}
