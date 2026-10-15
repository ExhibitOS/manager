// SPDX-License-Identifier: Apache-2.0
//! App-closed handoff for the registered Prepared apply operation; saved references never replace fresh qualification.
use super::*;
pub use crate::execution_reference::ReferenceSummary;
#[cfg(any(unix, windows))]
use crate::signed_release::trust::Store;
const LIMIT: usize = 8192;
const MAX_RECORDS: usize = 384;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyInput {
    pub reference_id: String,
    pub operation_id: String,
    pub generation: u64,
    pub preserve_data: bool,
    pub close_app_and_apply: bool,
    pub external_writers_quiesced: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    id: String,
    installation_id: String,
    scope: String,
    registry_sha256: String,
    reference_sha256: String,
    input: ApplyInput,
    created_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub installation_id: String,
    pub state: String,
    pub code: Option<String>,
    pub created_at: u64,
    pub request_sha256: String,
}
fn directory(profile: &Path) -> PathBuf {
    profile.join("update-apply-handoffs")
}
fn persist<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
    #[cfg(windows)]
    {
        let directory = crate::windows_private::PrivateDirectory::inspect(root)?;
        let namespace = crate::windows_private::HostDirectory::inspect(root)?;
        namespace.check_private()?;
        let record = directory.write_new_record(name, &bytes, LIMIT)?;
        record
            .check()
            .map_err(|_| err("UPDATE_HANDOFF_UNCERTAIN"))?;
        // Actual file flush above; Windows directory sync is retained namespace evidence.
        namespace
            .sync_all()
            .map_err(|_| err("UPDATE_HANDOFF_UNCERTAIN"))?;
        directory
            .check()
            .map_err(|_| err("UPDATE_HANDOFF_UNCERTAIN"))
    }
    #[cfg(not(windows))]
    {
        write_private_new(root, name, &bytes, LIMIT)?;
        File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("UPDATE_HANDOFF_UNCERTAIN"))
    }
}
fn read<T: for<'a> Deserialize<'a>>(root: &Path, name: &str) -> Result<T> {
    serde_json::from_slice(&installation_backup::source_bytes(
        root,
        name,
        LIMIT as u64,
        true,
    )?)
    .map_err(|_| err("UPDATE_HANDOFF_INVALID"))
}
fn validate(v: &ApplyInput) -> Result<()> {
    if !v.preserve_data
        || !v.close_app_and_apply
        || !v.external_writers_quiesced
        || !installations::uuid(&v.reference_id)
        || v.generation == 0
        || v.generation > 9_007_199_254_740_991
        || v.operation_id.is_empty()
        || v.operation_id.len() > 128
        || !v
            .operation_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(err("UPDATE_HANDOFF_CONSENT_REQUIRED"));
    }
    Ok(())
}
pub(crate) fn prepare(
    profile: &Path,
    installation: &str,
    scope: &str,
    registry: &[u8],
    input: ApplyInput,
) -> Result<Receipt> {
    validate(&input)?;
    let status = crate::signed_release::trust::inspect_update_status(profile, scope)
        .map_err(|e| err(e.code()))?;
    if status.stage != Some(crate::update::Stage::Prepared)
        || status.generation != Some(input.generation)
        || status.operation_id.as_deref() != Some(&input.operation_id)
        || status.write_uncertain
    {
        return Err(err("UPDATE_HANDOFF_AUTHORITY_CHANGED"));
    }
    let summaries = crate::execution_reference::list(profile, installation, scope, registry)?;
    if !summaries.iter().any(|r| {
        r.reference_id == input.reference_id
            && r.operation_id == input.operation_id
            && r.generation == input.generation
            && !r.changed_schema
    }) {
        return Err(err("UPDATE_REFERENCE_STALE"));
    }
    let (_, reference_bytes) = crate::execution_reference::load(profile, &input.reference_id)?;
    let root = directory(profile);
    match fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => installations::new_directory(&root)?,
        Ok(_) => installations::private_directory(&root)?,
        Err(_) => return Err(err("UPDATE_HANDOFF_INVALID")),
    }
    if fs::read_dir(&root)
        .map_err(|_| err("UPDATE_HANDOFF_INVALID"))?
        .take(MAX_RECORDS)
        .count()
        >= MAX_RECORDS - 2
    {
        return Err(err("UPDATE_HANDOFF_QUOTA"));
    }
    let mut created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| err("UPDATE_CLOCK_UNVERIFIED"))?
        .as_millis() as u64;
    for entry in fs::read_dir(&root)
        .map_err(|_| err("UPDATE_HANDOFF_INVALID"))?
        .take(MAX_RECORDS)
    {
        let entry = entry.map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| err("UPDATE_HANDOFF_INVALID"))?;
        if name.ends_with(".request.json") {
            let previous: Request = read(&root, name)?;
            created_at = created_at.max(
                previous
                    .created_at
                    .checked_add(1)
                    .ok_or_else(|| err("UPDATE_HANDOFF_INVALID"))?,
            );
        }
    }
    if created_at > 9_007_199_254_740_991 {
        return Err(err("UPDATE_HANDOFF_INVALID"));
    }
    let request = Request {
        id: Uuid::new_v4().to_string(),
        installation_id: installation.into(),
        scope: scope.into(),
        registry_sha256: digest(registry),
        reference_sha256: digest(&reference_bytes),
        input,
        created_at,
    };
    let bytes = serde_json::to_vec(&request).map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
    let receipt = Receipt {
        id: request.id.clone(),
        installation_id: installation.into(),
        state: "awaiting-app-close".into(),
        code: None,
        created_at: request.created_at,
        request_sha256: digest(&bytes),
    };
    persist(&root, &format!("{}.request.json", request.id), &request)?;
    Ok(receipt)
}
pub(crate) fn latest(profile: &Path, installation: &str) -> Result<Option<Receipt>> {
    let root = directory(profile);
    if !root.exists() {
        return Ok(None);
    }
    installations::private_directory(&root)?;
    let mut latest: Option<Receipt> = None;
    let entries = fs::read_dir(&root).map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RECORDS {
            return Err(err("UPDATE_HANDOFF_QUOTA"));
        }
        let entry = entry.map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| err("UPDATE_HANDOFF_INVALID"))?;
        let Some(id) = name.strip_suffix(".request.json") else {
            continue;
        };
        if !installations::uuid(id) {
            return Err(err("UPDATE_HANDOFF_INVALID"));
        }
        let request: Request = read(&root, name)?;
        if request.id != id {
            return Err(err("UPDATE_HANDOFF_INVALID"));
        }
        if request.installation_id != installation {
            continue;
        }
        let bytes = installation_backup::source_bytes(&root, name, LIMIT as u64, true)?;
        let mut receipt = Receipt {
            id: id.into(),
            installation_id: installation.into(),
            state: if root.join(format!("{id}.claim.json")).exists() {
                "uncertain"
            } else {
                "awaiting-app-close"
            }
            .into(),
            code: None,
            created_at: request.created_at,
            request_sha256: digest(&bytes),
        };
        if root.join(format!("{id}.outcome.json")).exists() {
            let outcome: Receipt = read(&root, &format!("{id}.outcome.json"))?;
            if outcome.id != id
                || outcome.installation_id != installation
                || outcome.request_sha256 != receipt.request_sha256
                || outcome.created_at != request.created_at
                || !["updated", "failed"].contains(&outcome.state.as_str())
                || outcome.code.as_ref().is_some_and(|v| {
                    v.len() > 96
                        || !v
                            .bytes()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
                })
            {
                return Err(err("UPDATE_HANDOFF_INVALID"));
            }
            receipt = outcome;
        }
        if latest
            .as_ref()
            .is_none_or(|v| (receipt.created_at, &receipt.id) > (v.created_at, &v.id))
        {
            latest = Some(receipt);
        }
    }
    Ok(latest)
}
pub fn record_launch_failure(profile: &Path, id: &str) -> Result<()> {
    if !installations::uuid(id) {
        return Err(err("UPDATE_HANDOFF_INVALID"));
    }
    installations::private_directory(profile)?;
    let root = directory(profile);
    installations::private_directory(&root)?;
    let name = format!("{id}.request.json");
    let request: Request = read(&root, &name)?;
    let bytes = installation_backup::source_bytes(&root, &name, LIMIT as u64, true)?;
    let receipt = Receipt {
        id: id.into(),
        installation_id: request.installation_id,
        state: "failed".into(),
        code: Some("UPDATE_HELPER_START_FAILED".into()),
        created_at: request.created_at,
        request_sha256: digest(&bytes),
    };
    persist(&root, &format!("{id}.outcome.json"), &receipt)
}
/// Caller supplies only the native-derived profile and immutable request ID.
/// The exclusive anchor proves all Manager profile sessions are closed.
pub fn execute(profile: &Path, id: &str) -> Result<()> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (profile, id);
        Err(err("UPDATE_TRUST_PLATFORM_UNVERIFIED"))
    }
    #[cfg(any(unix, windows))]
    {
        if !installations::uuid(id) {
            return Err(err("UPDATE_HANDOFF_INVALID"));
        }
        installations::private_directory(profile)?;
        let root = directory(profile);
        installations::private_directory(&root)?;
        #[cfg(windows)]
        let profile_guard = crate::windows_private::PrivateDirectory::inspect(profile)?;
        #[cfg(windows)]
        let handoff_guard = crate::windows_private::PrivateDirectory::inspect(&root)?;
        let name = format!("{id}.request.json");
        #[cfg(windows)]
        let request_guard = handoff_guard.read_record(&name)?;
        let request: Request = read(&root, &name)?;
        validate(&request.input)?;
        if request.id != id
            || !installations::uuid(&request.installation_id)
            || !(request.scope == "default" || installations::uuid(&request.scope))
        {
            return Err(err("UPDATE_HANDOFF_INVALID"));
        }
        let bytes = installation_backup::source_bytes(&root, &name, LIMIT as u64, true)?;
        let mut receipt = Receipt {
            id: id.into(),
            installation_id: request.installation_id.clone(),
            state: "failed".into(),
            code: None,
            created_at: request.created_at,
            request_sha256: digest(&bytes),
        };
        if root.join(format!("{id}.outcome.json")).exists()
            || root.join(format!("{id}.claim.json")).exists()
        {
            return Err(err("UPDATE_HANDOFF_ALREADY_CLAIMED"));
        }
        // Claim once BEFORE waiting for the app anchor. A competing helper must
        // not time out and publish a failed outcome while this owner qualifies.
        // create-new + fsync is ownership only, never evidence of engine success.
        persist(&root, &format!("{id}.claim.json"), &receipt)?;
        // Wait for the parent app to exit; never recover another in-flight update.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut store = loop {
            match Store::open_for_prepared_handoff(profile, &request.scope) {
                Ok(store) => break store,
                Err(e)
                    if e.code() == "UPDATE_TRUST_BUSY" && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(100))
                }
                Err(e) => {
                    receipt.code = Some(e.code().into());
                    persist(&root, &format!("{id}.outcome.json"), &receipt)?;
                    return Err(err(e.code()));
                }
            }
        };
        let operation = match profile_backup::inspect_operation_lock(profile) {
            Ok(lock) => lock,
            Err(e) => {
                receipt.code = Some(e.code.clone());
                persist(&root, &format!("{id}.outcome.json"), &receipt)?;
                return Err(e);
            }
        };
        #[cfg(not(windows))]
        let operation = OperationGuard(operation);
        #[cfg(windows)]
        {
            request_guard.check()?;
            profile_guard.check_record(&operation, "operation.lock")?;
            handoff_guard.check()?;
        }
        let result = (|| -> Result<()> {
            let (registry, original) = installations::load(profile)?
                .ok_or_else(|| err("UPDATE_HANDOFF_SELECTION_CHANGED"))?;
            let entry = registry
                .installations
                .iter()
                .find(|e| e.id == request.installation_id)
                .ok_or_else(|| err("UPDATE_HANDOFF_SELECTION_CHANGED"))?;
            if registry.active_id != entry.id || digest(&original) != request.registry_sha256 {
                return Err(err("UPDATE_HANDOFF_SELECTION_CHANGED"));
            }
            store.verify_bound_selected(&registry)?;
            #[cfg(windows)]
            let reference_directory = crate::windows_private::PrivateDirectory::inspect(
                &profile.join("update-execution-references"),
            )?;
            #[cfg(windows)]
            let reference_guard =
                reference_directory.read_record(&format!("{}.json", request.input.reference_id))?;
            let (reference, reference_bytes) =
                crate::execution_reference::load(profile, &request.input.reference_id)?;
            if digest(&reference_bytes) != request.reference_sha256
                || reference.operation_id != request.input.operation_id
                || reference.generation != request.input.generation
            {
                return Err(err("UPDATE_REFERENCE_STALE"));
            }
            #[cfg(windows)]
            {
                request_guard.check()?;
                reference_guard.check()?;
                reference_directory.check()?;
                profile_guard.check_record(&operation, "operation.lock")?;
                handoff_guard.check()?;
            }
            let applied = crate::execution_reference::apply(
                profile,
                &reference,
                &mut store,
                request.input.external_writers_quiesced,
            );
            #[cfg(windows)]
            {
                request_guard.check()?;
                reference_guard.check()?;
                reference_directory.check()?;
                profile_guard.check_record(&operation, "operation.lock")?;
                handoff_guard.check()?;
            }
            applied
        })();
        drop(operation);
        match &result {
            Ok(()) => receipt.state = "updated".into(),
            Err(e) => receipt.code = Some(e.code.clone()),
        }
        persist(&root, &format!("{id}.outcome.json"), &receipt)?;
        result
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::execution_reference as refs;
    use crate::signed_release::{Artifact, DOMAIN, Envelope, Policy, Release};
    use crate::windows_private::PrivateDirectory;
    use ed25519_dalek::{Signer, SigningKey};

    // Synthetic storage fixture, not an actual runtime qualification/registration claim.
    fn fixture() -> (PrivateDirectory, PathBuf, String, u64, String) {
        let parent = PrivateDirectory::create(
            &PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
                .join(format!("exhibitos-apply-handoff-test-{}", Uuid::new_v4())),
        )
        .unwrap();
        let profile = parent.path().join("profile");
        let controller = installations::InstallationController::new(profile.clone(), None).unwrap();
        let installation = controller.context().unwrap().active_id;
        drop(controller);
        let signing = SigningKey::from_bytes(&[31; 32]);
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let policy = Policy {
            format: 1,
            channel: "development".into(),
            target: "linux-amd64".into(),
            protocol_version: 1,
            source_schema_sha256: "a".repeat(64),
            minimum_sequence: 1,
            minimum_issued_at: 10,
            public_keys: vec![hex(signing.verifying_key().as_bytes())],
        };
        let release = Release {
            format: 1,
            product: "ExhibitOS/runtime".into(),
            channel: policy.channel.clone(),
            target: policy.target.clone(),
            version: "0.2.0-dev.1".into(),
            sequence: 2,
            issued_at: 10,
            expires_at: 100,
            protocol_version: 1,
            source_schemas: vec!["a".repeat(64)],
            artifact: Artifact {
                name: "runtime.tar".into(),
                bytes: 7,
                sha256: digest(b"fixture"),
                runtime_image_sha256: "b".repeat(64),
                schema_sha256: "a".repeat(64),
            },
        };
        let payload = serde_json::to_string(&release).unwrap();
        let mut message = DOMAIN.to_vec();
        message.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        message.extend_from_slice(payload.as_bytes());
        let envelope = serde_json::to_vec(&Envelope {
            format: 1,
            algorithm: "ed25519".into(),
            key_id: digest(signing.verifying_key().as_bytes()),
            signature: hex(&signing.sign(&message).to_bytes()),
            payload,
        })
        .unwrap();
        let mut store = Store::provision(&profile, "default", policy, 10).unwrap();
        let mut verified = store.verify_for_preparation(&envelope, 20).unwrap();
        verified
            .verify_artifact(&mut b"fixture".as_slice())
            .unwrap();
        let plan = crate::update::Plan {
            operation_id: "update-1".into(),
            source_instance: installation.clone(),
            target_instance: Uuid::new_v4().to_string(),
            source_image: "d".repeat(64),
            target_image: "b".repeat(64),
            source_schema: "a".repeat(64),
            target_schema: "a".repeat(64),
            backup_id: Uuid::new_v4().to_string(),
            backup_manifest: "e".repeat(64),
            source_inventory: "f".repeat(64),
            required_free_bytes: 1,
        };
        store
            .prepare_update(&envelope, &verified, plan, 20)
            .unwrap();
        let generation = store.receipt().generation;
        drop(store);
        let registry = installations::load(&profile).unwrap().unwrap().1;
        let references = profile.join("update-execution-references");
        let directory = PrivateDirectory::create(&references).unwrap();
        let id = Uuid::new_v4().to_string();
        let external = parent.path();
        let reference = refs::Reference {
            format: 1,
            id: id.clone(),
            installation_id: installation.clone(),
            scope: "default".into(),
            operation_id: "update-1".into(),
            generation,
            registry_sha256: digest(&registry),
            artifact: external.join("unused-runtime.tar"),
            artifact_sha256: digest(b"fixture"),
            artifact_bytes: 7,
            python: external.join("unused-python"),
            source_commit: "a".repeat(40),
            maintenance_image: format!("sha256:{}", "a".repeat(64)),
            export_parent: external.into(),
            host_archive: external.join("unused-host"),
            trust_archive: external.join("unused-trust"),
            pair_binding: external.join("unused-pair"),
            key_file: external.join("unused-key"),
            qualification_destination: external.join("old-qualification"),
            qualification_sha256: "b".repeat(64),
            created_at: 20,
        };
        directory
            .write_new_record(
                &format!("{id}.json"),
                &serde_json::to_vec(&reference).unwrap(),
                16384,
            )
            .unwrap();
        drop(directory);
        (parent, profile, installation, generation, id)
    }
    fn input(id: &str, generation: u64) -> ApplyInput {
        ApplyInput {
            reference_id: id.into(),
            operation_id: "update-1".into(),
            generation,
            preserve_data: true,
            close_app_and_apply: true,
            external_writers_quiesced: true,
        }
    }
    fn retire(parent: PrivateDirectory) {
        parent.check().unwrap();
        let path = parent.path().to_owned();
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("exhibitos-apply-handoff-test-")
        );
        drop(parent);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn windows_apply_reference_and_request_guards_refuse_writers_and_overwrite() {
        let (parent, profile, installation, generation, id) = fixture();
        let (_, registry) = installations::load(&profile).unwrap().unwrap();
        let summaries = refs::list(&profile, &installation, "default", &registry).unwrap();
        assert_eq!(summaries.len(), 1);
        let safe = serde_json::to_value(&summaries[0]).unwrap();
        assert_eq!(safe.as_object().unwrap().len(), 9);
        assert!(safe.get("keyFile").is_none());
        let reference_root =
            PrivateDirectory::inspect(&profile.join("update-execution-references")).unwrap();
        let record = reference_root.read_record(&format!("{id}.json")).unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .open(
                    profile
                        .join("update-execution-references")
                        .join(format!("{id}.json"))
                )
                .is_err()
        );
        record.check().unwrap();
        drop(record);
        drop(reference_root);
        let receipt = prepare(
            &profile,
            &installation,
            "default",
            &registry,
            input(&id, generation),
        )
        .unwrap();
        let root = directory(&profile);
        let request = root.join(format!("{}.request.json", receipt.id));
        let original = fs::read(&request).unwrap();
        assert!(persist(&root, &format!("{}.request.json", receipt.id), &receipt).is_err());
        assert_eq!(fs::read(request).unwrap(), original);
        retire(parent);
    }
    #[test]
    fn windows_apply_waits_for_app_close_and_duplicate_cannot_publish_outcome() {
        let (parent, profile, installation, generation, id) = fixture();
        let registry = installations::load(&profile).unwrap().unwrap().1;
        let receipt = prepare(
            &profile,
            &installation,
            "default",
            &registry,
            input(&id, generation),
        )
        .unwrap();
        let app = profile_backup::session_lock(&profile, false).unwrap();
        let first_profile = profile.clone();
        let first_id = receipt.id.clone();
        let worker = std::thread::spawn(move || execute(&first_profile, &first_id));
        let root = directory(&profile);
        let claim = root.join(format!("{}.claim.json", receipt.id));
        let outcome = root.join(format!("{}.outcome.json", receipt.id));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !claim.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(claim.exists());
        assert!(!outcome.exists());
        assert_eq!(
            execute(&profile, &receipt.id).unwrap_err().code,
            "UPDATE_HANDOFF_ALREADY_CLAIMED"
        );
        assert_eq!(
            latest(&profile, &installation).unwrap().unwrap().state,
            "uncertain"
        );
        assert!(!outcome.exists());
        drop(app);
        assert!(worker.join().unwrap().is_err()); // Independent recovery is absent; no runtime/key access is admitted.
        assert_eq!(
            latest(&profile, &installation).unwrap().unwrap().state,
            "failed"
        );
        assert_eq!(
            crate::signed_release::trust::inspect_update_status(&profile, "default")
                .unwrap()
                .generation,
            Some(generation)
        );
        assert_eq!(installations::load(&profile).unwrap().unwrap().1, registry);
        assert!(!parent.path().join("unused-key").exists());
        retire(parent);
    }
    #[test]
    fn windows_apply_changed_reference_is_refused_without_authority_or_input_mutation() {
        let (parent, profile, installation, generation, id) = fixture();
        let registry = installations::load(&profile).unwrap().unwrap().1;
        let receipt = prepare(
            &profile,
            &installation,
            "default",
            &registry,
            input(&id, generation),
        )
        .unwrap();
        let path = profile
            .join("update-execution-references")
            .join(format!("{id}.json"));
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["createdAt"] = serde_json::json!(21);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let changed = fs::read(&path).unwrap();
        assert_eq!(
            execute(&profile, &receipt.id).unwrap_err().code,
            "UPDATE_REFERENCE_STALE"
        );
        assert_eq!(fs::read(path).unwrap(), changed);
        assert_eq!(installations::load(&profile).unwrap().unwrap().1, registry);
        assert_eq!(
            crate::signed_release::trust::inspect_update_status(&profile, "default")
                .unwrap()
                .generation,
            Some(generation)
        );
        retire(parent);
    }
}
