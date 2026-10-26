// SPDX-License-Identifier: Apache-2.0
//! Same-catalog authenticated data copy into an inactive maintenance candidate.
//! No source configuration is installed and no journal can recreate authority.
use super::*;
const MARKER: &str = "native-target-preparation-current.json";

/// Preparation permits a fresh EMPTY config directory. If a signing key is
/// already present, preserve and pin its identity AND bytes without exposing it.
/// The source-backup ConfigurationDirectory contract is deliberately unchanged.
struct CandidateConfiguration {
    root: PathBuf,
    directory: crate::maintenance_source::DirectoryPin,
    files: Vec<(String, String, crate::hosted_runtime_file::Pin)>,
}
impl CandidateConfiguration {
    fn names(root: &Path) -> Result<Vec<String>> {
        let mut names = fs::read_dir(root).map_err(|_| err("NATIVE_TARGET_CONFIGURATION_CHANGED"))?
            .take(3).map(|e| e.map_err(|_| err("NATIVE_TARGET_CONFIGURATION_CHANGED"))?
                .file_name().into_string().map_err(|_| err("NATIVE_TARGET_CONFIGURATION_CHANGED")))
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        if !names.is_empty() && names != ["freeze-signing-key.json"] {
            return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
        }
        Ok(names)
    }
    fn capture(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let directory = crate::maintenance_source::DirectoryPin::open(root)?;
        let mut files = Vec::new();
        for name in Self::names(root)? {
            let original = Secret(installation_backup::source_bytes(root, &name, 65536, true)?);
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&root.join(&name), 65536)?;
            let bytes = Secret(bytes);
            if bytes.0 != original.0 { return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            files.push((name, digest(&bytes.0), pin));
        }
        let snapshot = Self {root: root.into(), directory, files};
        snapshot.check()?; Ok(snapshot)
    }
    fn check(&self) -> Result<()> {
        self.directory.check()?;
        if Self::names(&self.root)? != self.files.iter().map(|(n,_,_)| n.clone()).collect::<Vec<_>>() {
            return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
        }
        for (name, hash, pin) in &self.files {
            pin.check()?;
            let bytes = Secret(installation_backup::source_bytes(&self.root, name, 65536, true)?);
            if digest(&bytes.0) != *hash { return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED")); }
            pin.check()?;
        }
        self.directory.check()
    }
}
fn durable(root: &Path, name: &str, value: &Value) -> Result<()> {
    write_json(root, name, value)?;
    std::fs::File::open(root).and_then(|f| f.sync_all()).map_err(|_| err("NATIVE_TARGET_PREPARATION_UNCERTAIN"))
}
fn authenticated_catalog(manifest: &[u8]) -> Result<Vec<u8>> {
    let value: Value = serde_json::from_slice(manifest).map_err(|_| err("NATIVE_TARGET_COPY_UNAVAILABLE"))?;
    let rows = value["inventory"]["migrations"].as_array().filter(|r| !r.is_empty() && r.len() <= 10000).ok_or_else(|| err("NATIVE_TARGET_COPY_UNAVAILABLE"))?;
    let rows: Vec<_> = rows.iter().map(|r| serde_json::json!({"name":r["name"],"sha256":r["sha256"]})).collect();
    serde_json::to_vec(&rows).map_err(|_| err("NATIVE_TARGET_COPY_UNAVAILABLE"))
}
impl AuthenticatedNativeBackup {
    pub(crate) fn prepare_target_copy(
        &self, source: &mut Session, target: &mut Session, target_schema: &str,
        operation: &str, authority: Value,
        mut check_selection: impl FnMut(&Path, &str, &Path, &Path) -> Result<()>,
    ) -> Result<NativeTargetEvidence> {
        let profile = LifecycleService::bound_existing(self.profile_root.clone())?;
        let _profile = profile_lock(&profile)?;
        let root = target.target_observation_root().to_owned();
        if [&self.source_root, &self.archive, &self.verified, &self.scratch].iter().any(|p| root.starts_with(p) || p.starts_with(&root)) { return Err(err("NATIVE_TARGET_SELECTION_INVALID")); }
        source.check_native_observation_fence()?; target.check_native_observation_fence()?;
        let root_pin = crate::maintenance_source::DirectoryPin::open(&root)?;
        check_selection(&self.profile_root, &self.installation_id, &self.source_root, &root)?;
        self.check(source)?;
        let source_resources = self.payload.native_copy_resources()?;
        let target_payload = target.maintenance_payload();
        let target_resources = target_payload.native_copy_resources()?;
        let catalog = authenticated_catalog(&self.manifest)?;
        if catalog != source_resources.inventory.migrations || catalog != target_resources.inventory.migrations
            || source_resources.recipe_sha256 != target_resources.recipe_sha256
            || self.evidence.schema_sha256 != target_schema { return Err(err("NATIVE_TARGET_COPY_CATALOG_UNSUPPORTED")); }
        // Refuse all unsupported resources/catalogs BEFORE any reservation/write.
        let marker = root.join(MARKER);
        if fs::symlink_metadata(&marker).is_ok() { return Err(err("NATIVE_TARGET_PREPARATION_PENDING")); }
        target.maintenance_available()?; target.backup_source_current()?;
        if !target.maintenance_empty_target()? { return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY")); }
        let system = target.maintenance_system_identifier()?;
        if system == self.evidence.system_identifier { return Err(err("NATIVE_TARGET_SELECTION_INVALID")); }
        let config = CandidateConfiguration::capture(&root.join("config"))?;
        let before = self.observe_locked(source, &mut |profile, id, source_root| check_selection(profile, id, source_root, &root))?;
        source_resources.recheck()?; target_resources.recheck()?;
        check_selection(&self.profile_root, &self.installation_id, &self.source_root, &root)?;
        let attempt = Uuid::new_v4().to_string();
        let directory = root.join(format!("native-target-preparation-{operation}"));
        new_directory(&directory)?;
        let mut intent = serde_json::json!({"format":1,"recipe":"authenticated-native-copy-v1","operationId":operation,"attemptId":attempt,"state":"Reserved","sourceRoot":self.source_root,"targetRoot":root,"sourcePayloadManifestSha256":self.source_manifest(),"targetPayloadManifestSha256":target_payload.context().manifest_sha256,"catalogSha256":target_resources.inventory.migrations_sha256,"recipeSha256":target_resources.recipe_sha256,"backupId":self.evidence.backup_id,"authenticatedBackupManifestSha256":self.evidence.manifest_sha256,"encryptedBackupManifestSha256":self.receipt.manifest_sha256,"backupCompleteSha256":self.complete_sha256,"sourceInventorySha256":self.evidence.inventory_sha256,"sourceSystemIdentifier":self.evidence.system_identifier,"targetSystemIdentifier":system,"authority":authority,"configurationVerified":false,"preflightVerified":false,"executionPermitted":false});
        durable(&directory, "intent.json", &intent)?;
        write_private_new(&root, MARKER, &serde_json::to_vec(&serde_json::json!({"format":1,"operationId":operation,"attemptId":attempt})).map_err(|_| err("STATE_INVALID"))?, 4096)?;
        std::fs::File::open(&root).and_then(|f| f.sync_all()).map_err(|_| err("NATIVE_TARGET_PREPARATION_UNCERTAIN"))?;
        let result = (|| {
            let scratch = directory.join("scratch"); new_directory(&scratch)?;
            let restored = directory.join(format!("restored-{attempt}"));
            let budget = self.budget.min(resource_budget(fs2::available_space(&self.source_root).map_err(|_| err("STORAGE_UNAVAILABLE"))?, fs2::available_space(&root).map_err(|_| err("STORAGE_UNAVAILABLE"))?)?);
            intent["state"] = Value::String("Restoring".into()); durable(&directory,"intent.json",&intent)?;
            self.check(source)?; config.check()?; source_resources.recheck()?; target_resources.recheck()?;
            source.check_native_observation_fence()?; target.check_native_observation_fence()?;
            check_selection(&self.profile_root,&self.installation_id,&self.source_root,&root)?;
            let output = target.backup_tool(vec!["restore".into(),"--key-file".into(),text(&self.key_file)?,"--source".into(),text(&self.archive)?,"--destination".into(),text(&restored)?,"--quiesced".into(),"--fresh-destination".into()],"{}".into(),"{}".into(),budget,&scratch)?;
            if output["operation"].as_str()!=Some("restored-and-verified") || installation_backup::source_bytes(&restored,"manifest.json",16*1024*1024,true)? != self.manifest { return Err(err("NATIVE_TARGET_RESULT_INVALID")); }
            self.check(source)?; config.check()?; root_pin.check()?;
            source_resources.recheck()?; target_resources.recheck()?;
            check_selection(&self.profile_root,&self.installation_id,&self.source_root,&root)?;
            intent["state"] = Value::String("Observing".into()); durable(&directory,"intent.json",&intent)?;
            // Existing observer includes BOTH before/after source observations,
            // fixed target inventory, distinct cluster and full data comparison.
            let evidence = self.observe_target_locked(source,target,target_schema,&mut check_selection)?;
            let after = self.observe_locked(source,&mut |profile,id,source_root| check_selection(profile,id,source_root,&root))?;
            if before != after || evidence.target_system_identifier != system { return Err(err("NATIVE_SOURCE_OBSERVATION_STALE")); }
            config.check()?; root_pin.check()?; source_resources.recheck()?; target_resources.recheck()?;
            source.check_native_observation_fence()?; target.check_native_observation_fence()?;
            check_selection(&self.profile_root,&self.installation_id,&self.source_root,&root)?;
            intent["state"] = Value::String("PreparedInactive".into()); durable(&directory,"intent.json",&intent)?;
            Ok(evidence)
        })();
        if let Err(error) = &result {
            // backup_tool drains owned writers before returning any failure.
            // Any nonterminal journal after a crash is Interrupted/Uncertain,
            // never replayed; marker remains and blocks ordinary app start.
            intent["state"] = Value::String(if error.code=="NATIVE_CANCELLED" {"Cancelled"} else {"Failed"}.into());
            intent["errorCode"] = Value::String(error.code.clone());
            if durable(&directory,"intent.json",&intent).is_err() { return Err(err("NATIVE_TARGET_PREPARATION_UNCERTAIN")); }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config_root() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("exhibitos-copy-config-{}",Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap(); fs::canonicalize(root).unwrap()
    }
    #[test]
    fn native_copy_empty_candidate_config_needs_no_generated_key() {
        let root=config_root(); let snapshot=CandidateConfiguration::capture(&root).unwrap();
        snapshot.check().unwrap(); assert_eq!(fs::read_dir(&root).unwrap().count(),0);
        write_private_new(&root,"freeze-signing-key.json",b"synthetic newly added key",65536).unwrap();
        assert!(snapshot.check().is_err()); fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_copy_candidate_existing_key_contents_and_identity_stay_pinned() {
        let root=config_root(); let name="freeze-signing-key.json";
        write_private_new(&root,name,b"synthetic-original",65536).unwrap();
        let snapshot=CandidateConfiguration::capture(&root).unwrap(); snapshot.check().unwrap();
        fs::write(root.join(name),b"synthetic-modified").unwrap();
        assert!(snapshot.check().is_err(),"Same-length in-place key rewrite must refuse");
        let snapshot=CandidateConfiguration::capture(&root).unwrap();
        write_private_new(&root,"replacement",b"synthetic-modified",65536).unwrap();
        fs::rename(root.join("replacement"),root.join(name)).unwrap();
        assert!(snapshot.check().is_err(),"Byte-identical replacement at a name must refuse");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_copy_catalog_keeps_order_and_byte_identity() {
        let source=serde_json::json!({"inventory":{"migrations":[{"name":"001.sql","sha256":"a".repeat(64)},{"name":"002.sql","sha256":"b".repeat(64)}]}});
        let catalog=authenticated_catalog(&serde_json::to_vec(&source).unwrap()).unwrap();
        let mut changed=source.clone(); changed["inventory"]["migrations"][0]["sha256"]=Value::String("c".repeat(64));
        assert_ne!(catalog,authenticated_catalog(&serde_json::to_vec(&changed).unwrap()).unwrap());
        let mut reordered=source; reordered["inventory"]["migrations"].as_array_mut().unwrap().reverse();
        assert_ne!(catalog,authenticated_catalog(&serde_json::to_vec(&reordered).unwrap()).unwrap());
        assert!(authenticated_catalog(b"{}").is_err());
    }
}
