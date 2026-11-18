// SPDX-License-Identifier: Apache-2.0
//! Continue only an ordinary reopen that failed the storage guard before spawning.
//! The already-consumed generation is retained; no clean completion is fabricated.
use super::*;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub(super) fn storage_available(bytes: u64) -> Result<()> {
    if bytes < 3 * 1024 * 1024 * 1024 { return Err(err("STORAGE_LOW")); }
    Ok(())
}

pub(super) fn check_storage(root: &Path) -> Result<()> {
    storage_available(fs2::available_space(root).map_err(|_| err("STORAGE_UNAVAILABLE"))?)
}

pub(super) struct Retry {
    state: Vec<u8>,
    pointer: Vec<u8>,
    predecessor: Vec<u8>,
    log: File,
    root: PathBuf,
    previous: String,
}

/// Caller holds both root-operation and physical writer fences and has checked
/// process/database quiescence. This does not admit another installation or job.
pub(super) fn admit(root: &Path, current: &Current, live: &Binding, ports: Ports) -> Result<Retry> {
    let invalid = || err(furnishing_lifecycle::ERROR);
    let source = current.furnishing.as_ref().ok_or_else(invalid)?;
    source.check()?;
    if current.format != 1 || current.id != source.generation
        || source.phase != Phase::Active || source.admission_kind != Admission::CleanReopen
        || source.restore.is_some() || !source.interrupted_claim_generations.is_empty()
        || live.version != 1 || !installations::uuid(&live.generation)
        || !live.interrupted_claim_generations.is_empty()
        || live.phase != Phase::Active || live.admission_kind != Admission::FreshEmpty
        || live.previous_generation.is_some() || live.restore.is_some()
        || live.generation == source.generation || !source.same_installation(live)
    { return Err(invalid()); }
    let (context, proof) = read_run(root, &source.generation)?;
    if proof.is_some() || context.state != "failed" || context.ports != ports
        || context.error_code.as_deref() != Some("STORAGE_LOW")
        || context.failure_code.as_deref() != Some("STORAGE_LOW")
        || context.readiness.ready || context.readiness.version.is_some()
        || context.readiness.protocol_version.is_some() || context.readiness.error_code.is_some()
        || context.node_pid.is_some() || context.postgres_pid.is_some() || context.auxiliary_pid.is_some()
        || !context.exits.is_empty() || context.log_truncated
    { return Err(invalid()); }
    let run = root.join("native-runs").join(&source.generation);
    let state = installation_backup::source_bytes(&run, "state.json", 8192, true)?;
    // Reject unknown nested readiness/schema fields as well as open outer records.
    if serde_json::from_slice::<Value>(&state).map_err(|_| invalid())?
        != serde_json::to_value(&context).map_err(|_| invalid())?
    { return Err(invalid()); }
    let pointer = installation_backup::source_bytes(root, "native-current-run.json", 4096, true)?;
    if serde_json::from_slice::<Current>(&pointer).map_err(|_| invalid())? != *current {
        return Err(invalid());
    }
    // This SAME attempt's durable Active binding is the authority: the original
    // start already authenticated and consumed its predecessor before publishing
    // Active. The old full binding is not retained here. Its completion hash is
    // syntax-checked, not reauthenticated; consumedBy is corroborating lineage,
    // never authority for a new generation, installation or admission.
    let previous = source.previous_generation.as_ref().ok_or_else(invalid)?;
    let (prior, proof) = read_run(root, previous)?;
    let proof = proof.ok_or_else(invalid)?;
    if prior.state != "stopped" || prior.error_code.is_some() || prior.failure_code.is_some()
        || prior.ports != ports || prior.node_pid.is_some() || prior.postgres_pid.is_some()
        || prior.auxiliary_pid.is_some() || prior.exits.get("node") != Some(&0)
        || prior.exits.get("postgres") != Some(&0)
        || proof.consumed_by.as_deref() != Some(source.generation.as_str())
        || prior.exits.values().any(|exit| *exit != 0)
        || !hash_valid(&proof.binding_sha256)
    { return Err(invalid()); }
    let predecessor = installation_backup::source_bytes(root,
        &format!("native-runs/{previous}/state.json"), 8192, true)?;
    let path = checked_path(&run, "process.log")?;
    let log = OpenOptions::new().append(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path).map_err(|_| invalid())?;
    let metadata = log.metadata().map_err(|_| invalid())?;
    let path_metadata = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() != 0 || metadata.nlink() != 1
        || metadata.uid() != fs::metadata(root).map_err(|_| invalid())?.uid()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.dev() != path_metadata.dev() || metadata.ino() != path_metadata.ino()
    { return Err(invalid()); }
    Ok(Retry { state, pointer, predecessor, log, root: root.into(), previous: previous.clone() })
}

impl Retry {
    fn unchanged(&self, run: &Path) -> Result<()> {
        let invalid = || err(furnishing_lifecycle::ERROR);
        if installation_backup::source_bytes(run, "state.json", 8192, true)? != self.state
            || installation_backup::source_bytes(&self.root, "native-current-run.json", 4096, true)? != self.pointer
            || installation_backup::source_bytes(&self.root,
                &format!("native-runs/{}/state.json", self.previous), 8192, true)? != self.predecessor
        { return Err(invalid()); }
        let metadata = self.log.metadata().map_err(|_| invalid())?;
        let path_metadata = fs::symlink_metadata(run.join("process.log")).map_err(|_| invalid())?;
        if metadata.len() != 0 || metadata.nlink() != 1 || metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.dev() != path_metadata.dev() || metadata.ino() != path_metadata.ino()
        { return Err(invalid()); }
        Ok(())
    }
    /// All historical bytes are durable before normal lifecycle state can change.
    /// A partial archive is retained on error; the failed state is never rewritten here.
    pub(super) fn preserve(self, run: &Path) -> Result<File> {
        self.unchanged(run)?;
        let parent = directory(run, "preprocess-failures")?;
        let archive = parent.join(Uuid::new_v4().to_string());
        installations::new_directory(&archive)?;
        write_private_new(&archive, "state.json", &self.state, 8192)?;
        write_private_new(&archive, "native-current-run.json", &self.pointer, 4096)?;
        write_private_new(&archive, "predecessor-state.json", &self.predecessor, 8192)?;
        write_private_new(&archive, "process.log", b"", 0)?;
        for path in [&archive, &parent, run] {
            File::open(path).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        self.unchanged(run)?;
        Ok(self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture { root: PathBuf, current: Current, live: Binding, context: Context }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }
    impl Fixture {
        fn new() -> Self {
            let root = fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("native-preprocess-{}", Uuid::new_v4()));
            installations::new_directory(&root).unwrap();
            let prior = Uuid::new_v4().to_string();
            let source = Binding { version: 1, phase: Phase::Active, generation: Uuid::new_v4().to_string(),
                previous_generation: Some(prior), admission_kind: Admission::CleanReopen,
                owner: FurnishingOwner { workspace_id: Uuid::new_v4().to_string(), tenant_id: Uuid::new_v4().to_string(),
                    principal_id: Uuid::new_v4().to_string(), owner_uid: unsafe { libc::getuid() } },
                source_commit: "a".repeat(40), producer_sha256: "b".repeat(64), payload_manifest_sha256: "c".repeat(64),
                catalog: 41, physical: FurnishingWriterIdentity { root_device: 1, root_inode: 2, lock_device: 1, lock_inode: 3 },
                restore: None, interrupted_claim_generations: vec![] };
            source.check().unwrap();
            let mut live = source.clone(); live.generation = Uuid::new_v4().to_string();
            live.previous_generation = None; live.admission_kind = Admission::FreshEmpty;
            let context = Context { format: 1, job_id: Some(source.generation.clone()), state: "failed".into(),
                ports: Ports::default(), readiness: Readiness::default(), error_code: Some("STORAGE_LOW".into()),
                failure_code: Some("STORAGE_LOW".into()), postgres_pid: None, node_pid: None, auxiliary_pid: None,
                exits: BTreeMap::new(), log_truncated: false };
            let current = Current { format: 1, id: source.generation.clone(), furnishing: Some(source) };
            let fixture = Self { root, current, live, context };
            fixture.persist(); fixture
        }
        fn run(&self) -> PathBuf { self.root.join("native-runs").join(&self.current.id) }
        fn persist(&self) {
            let parent = directory(&self.root, "native-runs").unwrap();
            let run = directory(&parent, &self.current.id).unwrap();
            write_run(&run, &self.context, None).unwrap();
            if !run.join("process.log").exists() { write_private_new(&run, "process.log", b"", 0).unwrap(); }
            write_json(&self.root, "native-current-run.json", &self.current).unwrap();
            let previous = self.current.furnishing.as_ref().unwrap().previous_generation.as_ref().unwrap();
            let prior_run = directory(&parent, previous).unwrap();
            let mut prior = self.context.clone(); prior.job_id = Some(previous.clone()); prior.state = "stopped".into();
            prior.error_code = None; prior.failure_code = None;
            prior.exits = BTreeMap::from([("postgres".into(), 0), ("node".into(), 0)]);
            write_run(&prior_run, &prior, Some(FurnishingCompletion {
                binding_sha256: "d".repeat(64), consumed_by: Some(self.current.id.clone()),
            })).unwrap();
        }
        fn admit(&self) -> Result<Retry> { admit(&self.root, &self.current, &self.live, self.context.ports) }
    }
    #[test]
    fn same_generation_retry_preserves_exact_failure_and_consumed_lineage_each_time() {
        let f = Fixture::new();
        let state = fs::read(f.run().join("state.json")).unwrap();
        let pointer = fs::read(f.root.join("native-current-run.json")).unwrap();
        for _ in 0..2 {
            let mut log = f.admit().unwrap().preserve(&f.run()).unwrap();
            log.write_all(b"").unwrap();
            assert_eq!(fs::read(f.run().join("state.json")).unwrap(), state);
            assert_eq!(fs::read(f.root.join("native-current-run.json")).unwrap(), pointer);
        }
        let archives: Vec<_> = fs::read_dir(f.run().join("preprocess-failures")).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(archives.len(), 2);
        for path in archives {
            assert_eq!(fs::read(path.join("state.json")).unwrap(), state);
            assert_eq!(fs::read(path.join("native-current-run.json")).unwrap(), pointer);
            let predecessor: Value = serde_json::from_slice(&fs::read(path.join("predecessor-state.json")).unwrap()).unwrap();
            assert_eq!(predecessor["furnishingCompletion"]["consumedBy"], f.current.id);
            assert_eq!(fs::metadata(path.join("process.log")).unwrap().len(), 0);
        }
        assert_eq!(read_run(&f.root, &f.current.id).unwrap().0.state, "failed");
    }
    #[test]
    fn retry_refuses_process_readiness_exit_and_nonstorage_failures() {
        for mutation in 0..13 {
            let mut f = Fixture::new();
            match mutation {
                0 => f.context.node_pid = Some(123),
                1 => f.context.postgres_pid = Some(123),
                2 => f.context.auxiliary_pid = Some(123),
                3 => f.context.readiness.ready = true,
                4 => f.context.readiness.version = Some("1".into()),
                5 => f.context.readiness.protocol_version = Some("1".into()),
                6 => f.context.readiness.error_code = Some("failure".into()),
                7 => { f.context.exits.insert("postgres".into(), 0); },
                8 => f.context.failure_code = Some("NATIVE_CANCELLED".into()),
                9 => f.context.error_code = None,
                10 => f.context.state = "starting".into(),
                11 => f.context.log_truncated = true,
                12 => f.context.format = 2,
                _ => unreachable!(),
            }
            f.persist(); assert!(f.admit().is_err(), "mutation {mutation}");
            assert!(!f.run().join("preprocess-failures").exists());
        }
    }
    #[test]
    fn retry_refuses_foreign_owner_runtime_schema_physical_identity_and_pointer() {
        for mutation in 0..13 {
            let mut f = Fixture::new();
            match mutation {
                0 => f.live.owner.workspace_id = Uuid::new_v4().to_string(),
                1 => f.live.owner.tenant_id = Uuid::new_v4().to_string(),
                2 => f.live.owner.principal_id = Uuid::new_v4().to_string(),
                3 => f.live.owner.owner_uid += 1,
                4 => f.live.source_commit = "e".repeat(40),
                5 => f.live.producer_sha256 = "e".repeat(64),
                6 => f.live.payload_manifest_sha256 = "e".repeat(64),
                7 => f.live.catalog = 40,
                8 => f.live.physical.root_inode += 1,
                9 => f.live.physical.lock_inode += 1,
                10 => f.live.physical.root_device += 1,
                11 => f.live.physical.lock_device += 1,
                12 => f.current.id = Uuid::new_v4().to_string(),
                _ => unreachable!(),
            }
            assert!(f.admit().is_err(), "mutation {mutation}");
        }
    }
    #[test]
    fn retry_refuses_bad_journal_schema_ports_completion_log_and_foreign_consumption() {
        for mutation in 0..9 {
            let f = Fixture::new();
            let state_path = f.run().join("state.json");
            let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            match mutation {
                0 => state["readiness"]["unknown"] = true.into(),
                1 => state["unknown"] = true.into(),
                2 => state["jobId"] = Uuid::new_v4().to_string().into(),
                3 => state["ports"]["web"] = 1.into(),
                4 => state["furnishingCompletion"] = serde_json::json!({"bindingSha256":"d".repeat(64),"consumedBy":null}),
                5 => fs::write(f.run().join("process.log"), b"process started").unwrap(),
                6 => {
                    let previous = f.current.furnishing.as_ref().unwrap().previous_generation.as_ref().unwrap();
                    let path = f.root.join("native-runs").join(previous).join("state.json");
                    let mut prior: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    prior["furnishingCompletion"]["consumedBy"] = Uuid::new_v4().to_string().into();
                    fs::write(path, serde_json::to_vec(&prior).unwrap()).unwrap();
                },
                7 => fs::set_permissions(f.run().join("process.log"), fs::Permissions::from_mode(0o644)).unwrap(),
                8 => { fs::remove_file(f.run().join("process.log")).unwrap(); std::os::unix::fs::symlink(&state_path, f.run().join("process.log")).unwrap(); },
                _ => unreachable!(),
            }
            fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
            assert!(f.admit().is_err(), "mutation {mutation}");
        }
    }
    #[test]
    fn archive_failure_or_changed_claim_preserves_original_failed_journal() {
        for changed in [false, true] {
            let f = Fixture::new(); let retry = f.admit().unwrap();
            let original = fs::read(f.run().join("state.json")).unwrap();
            if changed {
                let mut current = f.current.clone(); current.id = Uuid::new_v4().to_string();
                write_json(&f.root, "native-current-run.json", &current).unwrap();
            } else { write_private_new(&f.run(), "preprocess-failures", b"blocked", 16).unwrap(); }
            assert!(retry.preserve(&f.run()).is_err());
            assert_eq!(fs::read(f.run().join("state.json")).unwrap(), original);
            assert_eq!(read_run(&f.root, &f.current.id).unwrap().0.exits.len(), 0);
        }
    }
    #[test]
    fn predecessor_hash_is_corroboration_and_cannot_mint_admission() {
        let mut f = Fixture::new();
        let previous = f.current.furnishing.as_ref().unwrap().previous_generation.as_ref().unwrap();
        let path = f.root.join("native-runs").join(previous).join("state.json");
        let mut prior: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        prior["furnishingCompletion"]["bindingSha256"] = "e".repeat(64).into();
        let altered = serde_json::to_vec(&prior).unwrap(); fs::write(&path, &altered).unwrap();
        let current = f.current.clone();
        f.admit().unwrap().preserve(&f.run()).unwrap();
        assert_eq!(current_run(&f.root).unwrap().unwrap(), current);
        assert_eq!(fs::read(&path).unwrap(), altered);
        let archive = fs::read_dir(f.run().join("preprocess-failures")).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(archive.join("predecessor-state.json")).unwrap(), altered);
        assert_eq!(fs::read_dir(f.root.join("native-runs")).unwrap().count(), 2);
        for mutation in 0..5 {
            let original = f.live.clone();
            match mutation {
                0 => f.live.owner.principal_id = Uuid::new_v4().to_string(),
                1 => f.live.payload_manifest_sha256 = "e".repeat(64),
                2 => f.live.catalog = 40,
                3 => f.live.physical.root_inode += 1,
                4 => f.live.admission_kind = Admission::CleanReopen,
                _ => unreachable!(),
            }
            assert!(f.admit().is_err()); f.live = original;
        }
        prior["furnishingCompletion"]["consumedBy"] = f.live.generation.clone().into();
        fs::write(&path, serde_json::to_vec(&prior).unwrap()).unwrap();
        assert!(f.admit().is_err());
    }
    #[test]
    fn storage_guard_retains_existing_three_gib_floor() {
        let floor = 3 * 1024 * 1024 * 1024;
        assert_eq!(storage_available(floor - 1).unwrap_err().code, "STORAGE_LOW");
        storage_available(floor).unwrap();
    }
}
