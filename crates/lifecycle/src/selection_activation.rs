// SPDX-License-Identifier: Apache-2.0
//! Small independent write-ahead selection journal. Never replays Engine work or health.
use super::*;
use crate::update::{HealthReceipt, Plan, Stage};
const LIMIT: u64 = 256 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    format: u8,
    scope: String,
    generation: u64,
    head: String,
    plan: Plan,
    health: HealthReceipt,
    original_sha256: String,
    candidate_sha256: String,
}
pub(super) struct Activation {
    root: PathBuf,
    identity: Metadata,
    profile_identity: Metadata,
    intent: Intent,
    original: Vec<u8>,
    candidate: Vec<u8>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionActivationReceipt {
    pub operation_id: String,
    pub selection_completed: bool,
    pub original_selection_restored: bool,
    pub authority_generation: u64,
    pub authority_head_sha256: String,
    pub runtime_replayed: bool,
    pub health_replayed: bool,
}
fn private_read(path: &Path) -> crate::Result<Vec<u8>> {
    let mut f = private_file(path, false).map_err(|e| crate::err(e.code()))?;
    if f.metadata()
        .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?
        .len()
        > LIMIT
    {
        return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
    }
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)
        .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
    if bytes.len() as u64 > LIMIT {
        return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
    }
    Ok(bytes)
}
fn immutable(root: &Path, name: &str, bytes: &[u8]) -> crate::Result<()> {
    if bytes.len() as u64 > LIMIT {
        return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
    }
    let mut f = private_file(&root.join(name), true).map_err(|e| crate::err(e.code()))?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| crate::err("UPDATE_ACTIVATION_UNCERTAIN"))?;
    sync_dir(root).map_err(|e| crate::err(e.code()))
}
fn parent(store: &Store) -> crate::Result<PathBuf> {
    Ok(store
        .root
        .parent()
        .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?
        .join(format!(".exhibitos-release-activation-{}", store.scope)))
}
fn root(store: &Store, operation: &str) -> crate::Result<PathBuf> {
    Ok(parent(store)?.join(hash(operation.as_bytes())))
}
fn registry(raw: &[u8]) -> crate::Result<installations::Registry> {
    if raw.len() > 64 * 1024 {
        return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
    }
    let r = serde_json::from_slice(raw).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
    installations::valid(&r)?;
    Ok(r)
}
// Exact raw restoration also preserves existing whitespace and encoded metadata.
fn publish(
    profile: &Path,
    expected: &[u8],
    next: &[u8],
    profile_identity: &Metadata,
) -> crate::Result<()> {
    registry(next)?;
    let check = || -> crate::Result<()> {
        installations::private_directory(profile)?;
        if !identity(
            profile_identity,
            &fs::symlink_metadata(profile).map_err(|_| crate::err("UPDATE_ACTIVATION_CHANGED"))?,
        ) || installations::load(profile)?.is_none_or(|(_, bytes)| bytes != expected)
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        Ok(())
    };
    check()?;
    let pending = profile.join(format!(".selection-{}.pending", uuid::Uuid::new_v4()));
    let mut file = private_file(&pending, true).map_err(|e| crate::err(e.code()))?;
    file.write_all(next)
        .and_then(|_| file.sync_all())
        .map_err(|_| crate::err("UPDATE_ACTIVATION_UNCERTAIN"))?;
    check()?;
    fs::rename(&pending, profile.join("installation-selection.json"))
        .map_err(|_| crate::err("UPDATE_ACTIVATION_UNCERTAIN"))?;
    sync_dir(profile).map_err(|_| crate::err("UPDATE_ACTIVATION_UNCERTAIN"))?;
    if installations::load(profile)?.is_none_or(|(_, bytes)| bytes != next) {
        return Err(crate::err("UPDATE_ACTIVATION_UNCERTAIN"));
    }
    Ok(())
}
impl Activation {
    pub(super) fn prepare(
        store: &Store,
        original: &[u8],
        health: &HealthReceipt,
    ) -> crate::Result<Self> {
        store
            .require_authority_recovery()
            .map_err(|e| crate::err(e.code()))?;
        let current = store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?;
        let plan = current.update.plan();
        let mut selected = registry(original)?;
        if current.update.stage() != Stage::AwaitingHealth
            || selected.active_id != plan.source_instance
            || store.bound_source_id(&selected)? != plan.source_instance
            || health.operation_id != plan.operation_id
            || health.instance_id != plan.target_instance
            || health.image != plan.target_image
            || health.schema != plan.target_schema
            || !health.ready
            || !selected
                .installations
                .iter()
                .any(|e| e.id == plan.target_instance && e.kind == "recovery")
        {
            return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
        }
        if installations::load(&store.profile)?.is_none_or(|(_, bytes)| bytes != original) {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        selected.active_id = plan.target_instance.clone();
        let candidate =
            serde_json::to_vec(&selected).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
        let parent = parent(store)?;
        match fs::symlink_metadata(&parent) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                installations::new_directory(&parent)?
            }
            Ok(_) => installations::private_directory(&parent)?,
            Err(_) => return Err(crate::err("UPDATE_ACTIVATION_INVALID")),
        }
        let root = root(store, &plan.operation_id)?;
        installations::new_directory(&root)?;
        let intent = Intent {
            format: 1,
            scope: store.scope.clone(),
            generation: store.current.generation,
            head: store.current_sha256.clone(),
            plan: plan.clone(),
            health: health.clone(),
            original_sha256: hash(original),
            candidate_sha256: hash(&candidate),
        };
        immutable(&root, "original-selection.json", original)?;
        immutable(&root, "candidate-selection.json", &candidate)?;
        immutable(
            &root,
            "intent.json",
            &serde_json::to_vec(&intent).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?,
        )?;
        sync_dir(&parent).map_err(|e| crate::err(e.code()))?;
        let activation = Self {
            identity: fs::symlink_metadata(&root)
                .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?,
            profile_identity: fs::symlink_metadata(&store.profile)
                .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?,
            root,
            intent,
            original: original.to_vec(),
            candidate,
        };
        activation.check(store)?;
        Ok(activation)
    }
    fn check(&self, store: &Store) -> crate::Result<()> {
        store.check_root().map_err(|e| crate::err(e.code()))?;
        installations::private_directory(&self.root)?;
        installations::private_directory(
            self.root
                .parent()
                .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?,
        )?;
        if self.root != root(store, &self.intent.plan.operation_id)?
            || self.intent.scope != store.scope
            || !identity(
                &self.identity,
                &fs::symlink_metadata(&self.root)
                    .map_err(|_| crate::err("UPDATE_ACTIVATION_CHANGED"))?,
            )
            || !identity(
                &self.profile_identity,
                &fs::symlink_metadata(&store.profile)
                    .map_err(|_| crate::err("UPDATE_ACTIVATION_CHANGED"))?,
            )
            || private_read(&self.root.join("original-selection.json"))? != self.original
            || private_read(&self.root.join("candidate-selection.json"))? != self.candidate
            || private_read(&self.root.join("intent.json"))?
                != serde_json::to_vec(&self.intent)
                    .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        let raw = read_record(
            &store
                .root
                .join(format!("{:020}.json", self.intent.generation)),
        )
        .map_err(|e| crate::err(e.code()))?;
        let record: Record =
            serde_json::from_slice(&raw).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
        valid_record(&record, &store.scope).map_err(|e| crate::err(e.code()))?;
        if hash(&raw) != self.intent.head
            || record.intent.as_ref().is_none_or(|i| {
                i.update.stage() != Stage::AwaitingHealth || i.update.plan() != &self.intent.plan
            })
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        Ok(())
    }
    pub(super) fn publish_selection(&self, store: &Store) -> crate::Result<Vec<u8>> {
        self.check(store)?;
        if store.current.generation != self.intent.generation
            || store.current_sha256 != self.intent.head
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        immutable(
            &self.root,
            "selection-started.json",
            self.intent.head.as_bytes(),
        )?;
        publish(
            &store.profile,
            &self.original,
            &self.candidate,
            &self.profile_identity,
        )?;
        self.check(store)?;
        Ok(self.candidate.clone())
    }
    pub(super) fn complete(&self, store: &Store) -> crate::Result<SelectionActivationReceipt> {
        self.check(store)?;
        if store.current.generation
            != self
                .intent
                .generation
                .checked_add(1)
                .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?
            || store.current.previous_sha256 != self.intent.head
            || store.intent().is_none_or(|i| {
                i.update.stage() != Stage::Updated || i.update.plan() != &self.intent.plan
            })
            || !matches!(&store.current.update_event,Some(UpdateEvent::Health(health)) if **health==self.intent.health)
            || installations::load(&store.profile)?.is_none_or(|(_, bytes)| bytes != self.candidate)
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        if private_read(&self.root.join("selection-started.json"))? != self.intent.head.as_bytes()
            || self.root.join("selection-restored.json").exists()
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        let marker = store.current_sha256.as_bytes();
        match fs::symlink_metadata(self.root.join("completed.json")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                immutable(&self.root, "completed.json", marker)?
            }
            Ok(_) if private_read(&self.root.join("completed.json"))? == marker => {}
            _ => return Err(crate::err("UPDATE_ACTIVATION_CHANGED")),
        }
        Ok(self.receipt(store, true, false))
    }
    fn receipt(
        &self,
        store: &Store,
        completed: bool,
        restored: bool,
    ) -> SelectionActivationReceipt {
        SelectionActivationReceipt {
            operation_id: self.intent.plan.operation_id.clone(),
            selection_completed: completed,
            original_selection_restored: restored,
            authority_generation: store.current.generation,
            authority_head_sha256: store.current_sha256.clone(),
            runtime_replayed: false,
            health_replayed: false,
        }
    }
    fn load(store: &Store, operation: &str) -> crate::Result<Self> {
        let root = root(store, operation)?;
        installations::private_directory(&root)?;
        for entry in fs::read_dir(&root).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))? {
            let entry = entry.map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
            if ![
                "original-selection.json",
                "candidate-selection.json",
                "intent.json",
                "selection-started.json",
                "completed.json",
                "selection-restored.json",
            ]
            .iter()
            .any(|s| entry.file_name() == *s)
            {
                return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
            }
        }
        let intent: Intent = serde_json::from_slice(&private_read(&root.join("intent.json"))?)
            .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?;
        let original = private_read(&root.join("original-selection.json"))?;
        let candidate = private_read(&root.join("candidate-selection.json"))?;
        let old = registry(&original)?;
        let new = registry(&candidate)?;
        let mut expected = registry(&original)?;
        expected.active_id = intent.plan.target_instance.clone();
        if intent.format != 1
            || intent.plan.operation_id != operation
            || hash(&original) != intent.original_sha256
            || hash(&candidate) != intent.candidate_sha256
            || old.active_id != intent.plan.source_instance
            || new.active_id != intent.plan.target_instance
            || serde_json::to_vec(&expected).map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?
                != candidate
        {
            return Err(crate::err("UPDATE_ACTIVATION_INVALID"));
        }
        let activation = Self {
            identity: fs::symlink_metadata(&root)
                .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?,
            profile_identity: fs::symlink_metadata(&store.profile)
                .map_err(|_| crate::err("UPDATE_ACTIVATION_INVALID"))?,
            root,
            intent,
            original,
            candidate,
        };
        activation.check(store)?;
        Ok(activation)
    }
}
impl Store {
    /// Explicit metadata reconciliation only: never execute/replay runtime or health.
    /// A committed exact health record may finish its marker. Otherwise retain the
    /// failure and restore only the exact pre-activation selection. Unknown bytes refuse.
    pub fn reconcile_selection_activation(
        &mut self,
        operation: &str,
    ) -> crate::Result<SelectionActivationReceipt> {
        self.require_authority_recovery()
            .map_err(|e| crate::err(e.code()))?;
        let activation = Activation::load(self, operation)?;
        let selection = installations::load(&self.profile)?
            .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?
            .1;
        let original = registry(&activation.original)?;
        let source = original
            .installations
            .iter()
            .find(|e| e.id == activation.intent.plan.source_instance)
            .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?;
        let target = original
            .installations
            .iter()
            .find(|e| e.id == activation.intent.plan.target_instance)
            .ok_or_else(|| crate::err("UPDATE_ACTIVATION_INVALID"))?;
        let source_service = crate::LifecycleService::open_retry_diagnostics(installations::root(
            &self.profile,
            source,
        ))?;
        let target_service = crate::LifecycleService::open_retry_diagnostics(installations::root(
            &self.profile,
            target,
        ))?;
        let _source_guard = source_service.lock()?;
        let _target_guard = target_service.lock()?;
        activation.check(self)?;
        if self
            .intent()
            .is_some_and(|i| i.update.stage() == Stage::Updated)
        {
            return activation.complete(self);
        }
        if activation.root.join("completed.json").exists() {
            // A completion marker without its exact committed health record is
            // inconsistent evidence, never a permission to activate or restore.
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        if selection != activation.original && selection != activation.candidate {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        if self.current.generation == activation.intent.generation
            && self.current_sha256 == activation.intent.head
        {
            self.update_failed(
                operation,
                self.current.generation,
                super::owned_execution::release_now()?,
            )
            .map_err(|e| crate::err(e.code()))?;
        }
        if self.current.generation != activation.intent.generation + 1
            || self.current.previous_sha256 != activation.intent.head
            || self.intent().is_none_or(|i| {
                i.update.stage() != Stage::RecoveryRequired
                    || i.update.plan() != &activation.intent.plan
            })
            || !matches!(
                self.current.update_event,
                Some(UpdateEvent::Interrupted | UpdateEvent::UpdateFailed)
            )
        {
            return Err(crate::err("UPDATE_ACTIVATION_CHANGED"));
        }
        if selection == activation.candidate {
            publish(
                &self.profile,
                &activation.candidate,
                &activation.original,
                &activation.profile_identity,
            )?;
        }
        if installations::load(&self.profile)?.is_none_or(|(_, bytes)| bytes != activation.original)
        {
            return Err(crate::err("UPDATE_ACTIVATION_UNCERTAIN"));
        }
        let marker = self.current_sha256.as_bytes();
        match fs::symlink_metadata(activation.root.join("selection-restored.json")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                immutable(&activation.root, "selection-restored.json", marker)?
            }
            Ok(_) if private_read(&activation.root.join("selection-restored.json"))? == marker => {}
            _ => return Err(crate::err("UPDATE_ACTIVATION_CHANGED")),
        }
        Ok(activation.receipt(self, false, true))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fixture as trust_fixture, plan, seal};
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    const SOURCE: &str = "ab6a178b-a401-48c1-b0aa-ddf87b059e31";
    const TARGET: &str = "cc414c3c-dd99-45e2-8307-131a49f72d68";
    fn fixture() -> (PathBuf, Store, HealthReceipt) {
        let (p, key, policy, release) = trust_fixture();
        let mut store = Store::provision(&p, "default", policy, 10).unwrap();
        let raw = seal(&key, &release);
        let mut verified = store.verify_for_preparation(&raw, 20).unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        let mut plan = plan();
        plan.source_instance = SOURCE.into();
        plan.target_instance = TARGET.into();
        store
            .prepare_update(&raw, &verified, plan.clone(), 20)
            .unwrap();
        store
            .enroll_authority_recovery(&p.parent().unwrap().join("vault"), 20)
            .unwrap();
        let registry = installations::Registry {
            format: 1,
            active_id: SOURCE.into(),
            installations: vec![
                installations::Entry {
                    id: SOURCE.into(),
                    kind: "default".into(),
                    created_at: 0,
                },
                installations::Entry {
                    id: TARGET.into(),
                    kind: "recovery".into(),
                    created_at: 1,
                },
            ],
        };
        installations::save(&p, &registry, None).unwrap();
        installations::new_directory(&p.join("local-runtime")).unwrap();
        installations::new_directory(&p.join("installations")).unwrap();
        installations::new_directory(&p.join("installations").join(TARGET)).unwrap();
        store
            .begin_update(
                crate::update::Preflight {
                    plan: plan.clone(),
                    signature_verified: false,
                    artifact_verified: false,
                    compatibility_verified: true,
                    backup_restore_verified: true,
                    current_source_matches_backup: true,
                    available_free_bytes: 4096,
                    image_only_rollback_verified: false,
                },
                &verified,
                21,
            )
            .unwrap();
        store
            .application_finished(&plan.operation_id, store.current.generation, 22)
            .unwrap();
        let health = HealthReceipt {
            operation_id: plan.operation_id,
            instance_id: TARGET.into(),
            image: plan.target_image,
            schema: plan.target_schema,
            ready: true,
        };
        (p, store, health)
    }
    fn prepare(store: &Store, health: &HealthReceipt) -> Activation {
        let raw = installations::load(&store.profile).unwrap().unwrap().1;
        Activation::prepare(store, &raw, health).unwrap()
    }
    fn retire(p: &Path, store: Store) {
        drop(store);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn activation_selection_complete_preserves_original_and_same_authority_namespace() {
        let (p, mut store, health) = fixture();
        let authority = store.root.clone();
        let floors = (
            store.current.policy.minimum_sequence,
            store.current.policy.minimum_issued_at,
        );
        let activation = prepare(&store, &health);
        let originals = activation.original.clone();
        let head = store.current_sha256.clone();
        activation.publish_selection(&store).unwrap();
        assert_eq!(store.current_sha256, head);
        assert!(activation.complete(&store).is_err());
        store
            .observe_health(
                &health.operation_id,
                store.current.generation,
                health.clone(),
                23,
            )
            .unwrap();
        let receipt = activation.complete(&store).unwrap();
        assert!(
            receipt.selection_completed
                && !receipt.original_selection_restored
                && !receipt.health_replayed
                && !receipt.runtime_replayed
        );
        assert_eq!(
            private_read(&activation.root.join("original-selection.json")).unwrap(),
            originals
        );
        assert_eq!(store.root, authority);
        assert_eq!(
            (
                store.current.policy.minimum_sequence,
                store.current.policy.minimum_issued_at
            ),
            floors
        );
        assert_eq!(
            activation.complete(&store).unwrap().authority_generation,
            store.current.generation
        );
        assert_eq!(
            store
                .bound_source_id(&registry(&activation.candidate).unwrap())
                .unwrap(),
            TARGET
        );
        assert_eq!(
            store
                .reconcile_selection_activation(&health.operation_id)
                .unwrap()
                .authority_generation,
            store.current.generation
        );
        retire(&p, store);
    }
    #[test]
    fn activation_selection_abort_restores_exact_previous_raw_bytes_without_replaying_health() {
        for published in [false, true] {
            let (p, mut store, health) = fixture();
            let (_, mut raw) = installations::load(&p).unwrap().unwrap();
            raw.push(b'\n');
            fs::write(p.join("installation-selection.json"), &raw).unwrap();
            let activation = prepare(&store, &health);
            if published {
                activation.publish_selection(&store).unwrap();
            }
            let receipt = store
                .reconcile_selection_activation(&health.operation_id)
                .unwrap();
            assert!(
                receipt.original_selection_restored
                    && !receipt.selection_completed
                    && !receipt.runtime_replayed
                    && !receipt.health_replayed
            );
            assert_eq!(
                store.intent().unwrap().update.stage(),
                Stage::RecoveryRequired
            );
            assert_eq!(installations::load(&p).unwrap().unwrap().1, raw);
            assert_eq!(
                private_read(&activation.root.join("candidate-selection.json")).unwrap(),
                activation.candidate
            );
            let generation = store.current.generation;
            store
                .reconcile_selection_activation(&health.operation_id)
                .unwrap();
            assert_eq!(store.current.generation, generation);
            retire(&p, store);
        }
    }
    #[test]
    fn activation_selection_unknown_pointer_or_journal_refuses_without_mutation() {
        for change in [
            "pointer",
            "intent",
            "unknown-file",
            "permissions",
            "false-completion",
        ] {
            let (p, mut store, health) = fixture();
            let activation = prepare(&store, &health);
            activation.publish_selection(&store).unwrap();
            let head = store.current_sha256.clone();
            match change {
                "pointer" => {
                    let mut raw = activation.candidate.clone();
                    raw.push(b' ');
                    fs::write(p.join("installation-selection.json"), raw).unwrap();
                }
                "intent" => {
                    let mut raw = private_read(&activation.root.join("intent.json")).unwrap();
                    raw.push(b' ');
                    fs::write(activation.root.join("intent.json"), raw).unwrap();
                }
                "unknown-file" => immutable(&activation.root, "foreign.json", b"{}").unwrap(),
                "false-completion" => {
                    immutable(&activation.root, "completed.json", head.as_bytes()).unwrap()
                }
                "permissions" => fs::set_permissions(
                    activation.root.join("candidate-selection.json"),
                    fs::Permissions::from_mode(0o644),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let pointer = fs::read(p.join("installation-selection.json")).unwrap();
            assert!(
                store
                    .reconcile_selection_activation(&health.operation_id)
                    .is_err()
            );
            assert_eq!(store.current_sha256, head);
            assert_eq!(
                fs::read(p.join("installation-selection.json")).unwrap(),
                pointer
            );
            assert_eq!(
                private_read(&activation.root.join("original-selection.json")).unwrap(),
                activation.original
            );
            retire(&p, store);
        }
    }
    #[test]
    fn activation_selection_busy_service_fence_refuses_before_any_reconciliation_write() {
        let (p, mut store, health) = fixture();
        let activation = prepare(&store, &health);
        activation.publish_selection(&store).unwrap();
        let service =
            crate::LifecycleService::open_retry_diagnostics(p.join("local-runtime")).unwrap();
        let guard = service.lock().unwrap();
        let head = store.current_sha256.clone();
        assert_eq!(
            store
                .reconcile_selection_activation(&health.operation_id)
                .err()
                .unwrap()
                .code,
            "BUSY"
        );
        assert_eq!(store.current_sha256, head);
        assert_eq!(
            installations::load(&p).unwrap().unwrap().1,
            activation.candidate
        );
        drop(guard);
        drop(service);
        retire(&p, store);
    }
    #[test]
    #[ignore = "executed by activation_selection_abrupt_crash_matrix only"]
    fn activation_selection_crash_worker() {
        let report = PathBuf::from(std::env::var_os("EXHIBITOS_SELECTION_TEST_REPORT").unwrap());
        let phase = std::env::var("EXHIBITOS_SELECTION_TEST_PHASE").unwrap();
        installations::private_directory(report.parent().unwrap()).unwrap();
        let (p, mut store, health) = fixture();
        let source =
            crate::LifecycleService::open_retry_diagnostics(p.join("local-runtime")).unwrap();
        let target =
            crate::LifecycleService::open_retry_diagnostics(p.join("installations").join(TARGET))
                .unwrap();
        let _source = source.lock().unwrap();
        let _target = target.lock().unwrap();
        let activation = prepare(&store, &health);
        if phase != "prepared" {
            activation.publish_selection(&store).unwrap();
        }
        if phase == "health" {
            store
                .observe_health(
                    &health.operation_id,
                    store.current.generation,
                    health.clone(),
                    23,
                )
                .unwrap();
        }
        immutable(report.parent().unwrap(),report.file_name().unwrap().to_str().unwrap(),
            &serde_json::to_vec(&serde_json::json!({"profile":p,"operation":health.operation_id,"journal":activation.root,"generation":store.current.generation})).unwrap()).unwrap();
        // Abrupt termination while Store/profile/source/target descriptors are live.
        std::process::exit(77);
    }
    #[test]
    fn activation_selection_abrupt_crash_matrix_never_replays_candidate_or_health() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-selection-crash-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&root).unwrap();
        for phase in ["prepared", "selection", "health"] {
            let report = root.join(format!("{phase}.json"));
            let worker = format!(
                "{}::activation_selection_crash_worker",
                module_path!().split_once("::").unwrap().1
            );
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &worker,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("EXHIBITOS_SELECTION_TEST_REPORT", &report)
                .env("EXHIBITOS_SELECTION_TEST_PHASE", phase)
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let info: serde_json::Value =
                serde_json::from_slice(&private_read(&report).unwrap()).unwrap();
            let p = PathBuf::from(info["profile"].as_str().unwrap());
            let journal = PathBuf::from(info["journal"].as_str().unwrap());
            let original = private_read(&journal.join("original-selection.json")).unwrap();
            let candidate = private_read(&journal.join("candidate-selection.json")).unwrap();
            let mut reopened = Store::open(&p, "default").unwrap();
            let operation = info["operation"].as_str().unwrap();
            let generation = reopened.current.generation;
            let receipt = reopened.reconcile_selection_activation(operation).unwrap();
            if phase == "health" {
                assert!(receipt.selection_completed && !receipt.original_selection_restored);
                assert_eq!(reopened.intent().unwrap().update.stage(), Stage::Updated);
                assert_eq!(installations::load(&p).unwrap().unwrap().1, candidate);
            } else {
                assert!(receipt.original_selection_restored && !receipt.selection_completed);
                assert_eq!(
                    reopened.intent().unwrap().update.stage(),
                    Stage::RecoveryRequired
                );
                assert_eq!(installations::load(&p).unwrap().unwrap().1, original);
            }
            assert!(!receipt.runtime_replayed && !receipt.health_replayed);
            println!(
                "actual selection crash: phase={phase}, exit={}, completion={}, original_restored={}, runtime_replayed={}, health_replayed={}",
                result.status.code().unwrap(),
                receipt.selection_completed,
                receipt.original_selection_restored,
                receipt.runtime_replayed,
                receipt.health_replayed
            );
            assert_eq!(reopened.current.generation, generation);
            assert_eq!(
                private_read(&journal.join("original-selection.json")).unwrap(),
                original
            );
            assert_eq!(
                private_read(&journal.join("candidate-selection.json")).unwrap(),
                candidate
            );
            retire(&p, reopened);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
