// SPDX-License-Identifier: Apache-2.0
//! Fresh native candidate restoration and explicit, replayable activation.
//! Archive crypto, database restore and inventory checks remain Platform-owned.
use super::native_backup::{Secret, SourceMap, resource_budget, text};
use super::*;
use crate::{native_installation::Ports, native_payload::Payload, native_runtime::Session};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::MetadataExt,
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub source: PathBuf,
    pub key_file: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub backup_id: String,
    pub state: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    format: u32,
    id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    format: u32,
    id: String,
    source: PathBuf,
    source_manifest_sha256: String,
    system_identifier: String,
    target_environment_sha256: String,
    target_owner_sha256: String,
}
impl Intent {
    fn check(
        &self,
        id: &str,
        source: &Path,
        source_manifest: &str,
        system: &str,
        target: &Path,
        owner: &str,
    ) -> Result<()> {
        if self.format != 1
            || self.id != id
            || self.source != source
            || self.source_manifest_sha256 != source_manifest
            || self.system_identifier != system
            || file_hash(target, "runtime.env")?.as_deref()
                != Some(self.target_environment_sha256.as_str())
            || file_hash(target, owner)?.as_deref() != Some(self.target_owner_sha256.as_str())
        {
            return Err(err("NATIVE_RESTORE_CANDIDATE_CHANGED"));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActivationFile {
    profile: bool,
    name: String,
    staged: String,
    sha256: String,
    bytes: u64,
    previous: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    format: u32,
    id: String,
    backup_id: String,
    manifest_sha256: String,
    payload_sha256: String,
    system_identifier: String,
    kind: String,
    ports: Ports,
    files: Vec<ActivationFile>,
    attempt: String,
}
struct Fields(BTreeMap<String, String>);
impl Drop for Fields {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
    }
}
const MARKER: &str = "native-restore-current.json";
fn bad() -> LifecycleError {
    err("NATIVE_RESTORE_INVALID")
}
fn json<T: for<'a> Deserialize<'a>>(root: &Path, name: &str, limit: u64) -> Result<T> {
    serde_json::from_slice(&installation_backup::source_bytes(root, name, limit, true)?)
        .map_err(|_| bad())
}
fn exists(root: &Path, name: &str) -> Result<bool> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(bad()),
    }
}
fn key(input: &Input, profile: &Path, target: &Path) -> Result<()> {
    private_directory(&input.source)?;
    if fs::canonicalize(&input.source).ok().as_deref() != Some(input.source.as_path())
        || !input.key_file.is_absolute()
        || input.source.starts_with(target)
        || target.starts_with(&input.source)
        || input.key_file.starts_with(&input.source)
        || input.key_file.starts_with(target)
        || input.key_file.starts_with(profile)
        || fs::canonicalize(&input.key_file).ok().as_deref() != Some(input.key_file.as_path())
    {
        return Err(bad());
    }
    let parent = input.key_file.parent().ok_or_else(bad)?;
    private_directory(parent)?;
    let name = input
        .key_file
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(bad)?;
    let bytes = Secret(installation_backup::source_bytes(parent, name, 32, true)?);
    if bytes.0.len() != 32 {
        return Err(err("BACKUP_KEY_INVALID"));
    }
    Ok(())
}
fn archive_bytes(source: &Path) -> Result<u64> {
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in fs::read_dir(source.join("files")).map_err(|_| bad())? {
        count += 1;
        if count > 100000 {
            return Err(bad());
        }
        let path = entry.map_err(|_| bad())?.path();
        let meta = fs::symlink_metadata(path).map_err(|_| bad())?;
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o777 != 0o600
        {
            return Err(bad());
        }
        total = total.checked_add(meta.len()).ok_or_else(bad)?;
    }
    if count == 0 {
        return Err(bad());
    }
    total.checked_add(32 * 1024 * 1024).ok_or_else(bad)
}
fn names(root: &Path, expected: &BTreeSet<String>) -> Result<()> {
    private_directory(root)?;
    let actual: BTreeSet<_> = fs::read_dir(root)
        .map_err(|_| bad())?
        .take(expected.len() + 1)
        .map(|e| {
            e.map_err(|_| bad())
                .and_then(|v| v.file_name().into_string().map_err(|_| bad()))
        })
        .collect::<Result<_>>()?;
    if &actual != expected {
        return Err(bad());
    }
    Ok(())
}
fn owner_name(kind: &str) -> Result<&'static str> {
    match kind {
        "personal" => Ok("native-personal.json"),
        "legacy_account" => Ok("native-installation.json"),
        _ => Err(bad()),
    }
}
fn journal(source: &Path, logical: &str) -> Result<String> {
    let kind = match logical {
        "document.json" => "document",
        "upload.json" => "upload",
        "oex-import.json" => "oex-import",
        "publication.json" => "publication",
        "static-export.json" => "static-export",
        _ => return Err(bad()),
    };
    Ok(format!(
        "native-{kind}-{}.json",
        digest(source.as_os_str().as_encoded_bytes())
    ))
}
fn file_hash(root: &Path, name: &str) -> Result<Option<String>> {
    if !exists(root, name)? {
        return Ok(None);
    }
    installation_backup::source_bytes(root, name, 2_200_000, true).map(|bytes| Some(digest(&bytes)))
}
fn rebound_environment(source: &[u8], target: &BTreeMap<String, String>) -> Result<Secret> {
    let mut output = String::new();
    for line in std::str::from_utf8(source).map_err(|_| bad())?.lines() {
        let (key, value) = line.split_once('=').ok_or_else(bad)?;
        let value = if [
            "POSTGRES_PASSWORD",
            "DATABASE_URL",
            "EXHIBITOS_PORT",
            "EXHIBITOS_API_PORT",
            "NATIVE_RUNTIME_MANIFEST_SHA256",
        ]
        .contains(&key)
        {
            target.get(key).ok_or_else(bad)?.as_str()
        } else {
            value
        };
        output.push_str(key);
        output.push('=');
        output.push_str(value);
        output.push('\n');
    }
    Ok(Secret(output.into_bytes()))
}
fn staged_file(
    directory: &Path,
    root: &Path,
    profile: bool,
    name: &str,
    logical: &str,
    bytes: &[u8],
) -> Result<ActivationFile> {
    write_private_new(directory, logical, bytes, 2_200_000)?;
    Ok(ActivationFile {
        profile,
        name: name.into(),
        staged: logical.into(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        previous: file_hash(root, name)?,
    })
}
fn journal_value(logical: &str, bytes: &[u8]) -> Result<Value> {
    match logical {
        "document.json" => {
            native_document::validate(&serde_json::from_slice(bytes).map_err(|_| bad())?)?
        }
        "upload.json" => {
            native_upload::validate(&serde_json::from_slice(bytes).map_err(|_| bad())?)?
        }
        "publication.json" => {
            native_publication::validate(&serde_json::from_slice(bytes).map_err(|_| bad())?)?
        }
        "static-export.json" => {
            let v: native_static_export::Status =
                serde_json::from_slice(bytes).map_err(|_| bad())?;
            if v.format != 1
                || !uuid(&v.job_id)
                || !uuid(&v.publication_id)
                || !hash_valid(&v.revision_sha256)
                || !v.output.is_absolute()
                || !matches!(
                    v.state.as_str(),
                    "running" | "completed" | "cancelled" | "failed" | "interrupted"
                )
            {
                return Err(bad());
            }
            text(&v.output)?;
            if (v.state == "completed") != v.receipt.is_some() {
                return Err(bad());
            }
            if let Some(receipt) = &v.receipt
                && (receipt.publication_id != v.publication_id
                    || receipt.revision_sha256 != v.revision_sha256
                    || receipt.output != v.output
                    || !hash_valid(&receipt.index_sha256)
                    || !hash_valid(&receipt.manifest_sha256)
                    || receipt.files == 0
                    || receipt.files > 20000
                    || receipt.bytes == 0)
            {
                return Err(bad());
            }
        }
        _ => return Err(bad()),
    }
    serde_json::from_slice(bytes).map_err(|_| bad())
}
fn authenticated_configuration(restored: &Path, kind: &str) -> Result<()> {
    let owner = owner_name(kind)?;
    let expected: BTreeSet<String> = [
        "native-source.json",
        "native-runtime.env",
        owner,
        "freeze-signing-key.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let manifest: Value = json(restored, "manifest.json", 16 * 1024 * 1024)?;
    let entries = manifest["files"].as_array().ok_or_else(bad)?;
    let configuration = restored.join("configuration");
    names(&configuration, &expected)?;
    let mut found = BTreeSet::new();
    for entry in entries.iter().filter(|v| v["role"] == "configuration") {
        let name = entry["name"].as_str().ok_or_else(bad)?;
        if !expected.contains(name) || !found.insert(name.to_string()) {
            return Err(bad());
        }
        let bytes = Secret(installation_backup::source_bytes(
            &configuration,
            name,
            1024 * 1024,
            true,
        )?);
        if entry["sha256"].as_str() != Some(digest(&bytes.0).as_str())
            || entry["bytes"].as_u64() != Some(bytes.0.len() as u64)
        {
            return Err(bad());
        }
    }
    if found != expected {
        return Err(bad());
    }
    Ok(())
}
fn authenticated_journals(restored: &Path, source: &SourceMap) -> Result<()> {
    let manifest: Value = json(restored, "manifest.json", 16 * 1024 * 1024)?;
    let entries = manifest["files"].as_array().ok_or_else(bad)?;
    let mut found = BTreeSet::new();
    for entry in entries.iter().filter(|v| v["role"] == "deployment") {
        let name = entry["name"]
            .as_str()
            .and_then(|v| v.strip_prefix("native-authoring/"))
            .ok_or_else(bad)?;
        if !source.journal_names.contains_key(name) || !found.insert(name.to_owned()) {
            return Err(bad());
        }
        let bytes = installation_backup::source_bytes(
            &restored.join("deployment/native-authoring"),
            name,
            2_200_000,
            true,
        )?;
        if entry["sha256"].as_str() != Some(digest(&bytes).as_str())
            || entry["bytes"].as_u64() != Some(bytes.len() as u64)
        {
            return Err(bad());
        }
    }
    if found != source.journal_names.keys().cloned().collect() {
        return Err(bad());
    }
    Ok(())
}
fn rebound_journal(logical: &str, bytes: &[u8], fields: &Fields) -> Result<Vec<u8>> {
    let mut value = journal_value(logical, bytes)?;
    if value["workspaceId"].as_str() != fields.0.get("WORKSPACE_ID").map(String::as_str)
        || value["tenantId"].as_str() != fields.0.get("TENANT_ID").map(String::as_str)
        || value["principalId"].as_str() != fields.0.get("PRINCIPAL_ID").map(String::as_str)
    {
        return Err(bad());
    }
    if logical == "static-export.json" {
        value["state"] = Value::String("interrupted".into());
        value["receipt"] = Value::Null;
        value["errorCode"] = Value::String("NATIVE_STATIC_RESTORED_REVERIFY_REQUIRED".into());
        serde_json::to_vec(&value).map_err(|_| bad())
    } else {
        Ok(bytes.into())
    }
}
fn rebound_identity(kind: &str, original: &Fields, current: &Fields) -> Result<()> {
    let keys: &[&str] = match kind {
        "personal" => &["WORKSPACE_ID", "PRINCIPAL_ID", "OWNER_UID", "TENANT_ID"],
        "legacy_account" => &["TENANT_ID", "ADMIN_SUBJECT", "ADMIN_PASSWORD"],
        _ => return Err(bad()),
    };
    if keys
        .iter()
        .any(|key| !original.0.contains_key(*key) || original.0.get(*key) != current.0.get(*key))
    {
        return Err(err("NATIVE_RESTORE_OWNER_MISMATCH"));
    }
    Ok(())
}
fn valid_plan(plan: &Plan, target: &Path) -> Result<()> {
    if !uuid(&plan.attempt)
        || plan.format != 1
        || !uuid(&plan.id)
        || !uuid(&plan.backup_id)
        || !hash_valid(&plan.manifest_sha256)
        || !hash_valid(&plan.payload_sha256)
        || plan.files.len() < 3
        || plan.files.len() > 7
    {
        return Err(bad());
    }
    let owner = owner_name(&plan.kind)?;
    let mut root_names = BTreeSet::new();
    let mut profile_names = BTreeSet::new();
    for file in &plan.files {
        if !hash_valid(&file.sha256)
            || file.bytes == 0
            || file.bytes > 2_200_000
            || file.previous.as_ref().is_some_and(|v| !hash_valid(v))
        {
            return Err(bad());
        }
        if file.profile {
            if journal(target, &file.staged)? != file.name
                || file.previous.is_some()
                || !profile_names.insert(&file.name)
            {
                return Err(bad());
            }
        } else {
            let logical = match file.name.as_str() {
                "runtime.env" => "runtime.env",
                "config/freeze-signing-key.json" => "freeze-signing-key.json",
                name if name == owner => owner,
                _ => return Err(bad()),
            };
            if file.staged != logical || !root_names.insert(file.name.as_str()) {
                return Err(bad());
            }
        }
    }
    if root_names != BTreeSet::from(["runtime.env", owner, "config/freeze-signing-key.json"]) {
        return Err(bad());
    }
    Ok(())
}
fn install_file(
    directory: &Path,
    activation: &Path,
    root: &Path,
    file: &ActivationFile,
    index: usize,
) -> Result<()> {
    let bytes = Secret(installation_backup::source_bytes(
        activation,
        &file.staged,
        2_200_000,
        true,
    )?);
    if digest(&bytes.0) != file.sha256 || bytes.0.len() as u64 != file.bytes {
        return Err(bad());
    }
    let current = file_hash(root, &file.name)?;
    if current.as_deref() == Some(file.sha256.as_str()) {
        return Ok(());
    }
    if current != file.previous {
        return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT"));
    }
    if let Some(expected) = &file.previous {
        let previous = Secret(installation_backup::source_bytes(
            root, &file.name, 2_200_000, true,
        )?);
        if digest(&previous.0) != *expected {
            return Err(bad());
        }
        let name = format!("previous-{index}.bin");
        if exists(directory, &name)? {
            if file_hash(directory, &name)?.as_ref() != Some(expected) {
                return Err(bad());
            }
        } else {
            write_private_new(directory, &name, &previous.0, 2_200_000)?;
        }
        File::open(directory)
            .and_then(|v| v.sync_all())
            .map_err(|_| bad())?;
    }
    let target = root.join(&file.name);
    let parent = target.parent().ok_or_else(bad)?;
    private_directory(parent)?;
    let temp = format!(".native-restore-{}", Uuid::new_v4());
    write_private_new(parent, &temp, &bytes.0, 2_200_000)?;
    if file_hash(root, &file.name)? != file.previous {
        return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT"));
    }
    if file.previous.is_none() {
        use std::os::unix::ffi::OsStrExt;
        let from =
            std::ffi::CString::new(parent.join(&temp).as_os_str().as_bytes()).map_err(|_| bad())?;
        let to = std::ffi::CString::new(target.as_os_str().as_bytes()).map_err(|_| bad())?;
        if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(err("NATIVE_RESTORE_CONFIGURATION_CONFLICT"));
        }
    } else {
        fs::rename(parent.join(temp), &target).map_err(|_| bad())?;
    }
    File::open(parent)
        .and_then(|v| v.sync_all())
        .map_err(|_| bad())
}
impl InstallationController {
    /// The caller supplies an explicitly separate, empty PG-only candidate.
    /// Nothing starts Node or changes selection; activation is another action.
    pub fn stage_native_restore(
        &self,
        token: &str,
        session: &mut Session,
        input: Input,
    ) -> Result<Receipt> {
        self.with_current(token, |service| {
            if !session.belongs_to(service) || session.status()?.state != "database_ready" {
                return Err(err("NATIVE_NOT_READY"));
            }
            session.maintenance_available()?;
            session.backup_source_current()?;
            let profile = self.profile.as_ref().ok_or_else(bad)?;
            let _profile = profile_lock(profile)?;
            signed_release::trust::lifecycle_writer::admit(&service.root,&Action::Install)?;
            key(&input, &profile.root, &service.root)?;
            let prepared = service.native_prepared()?.ok_or_else(bad)?;
            let owner = owner_name(&prepared.kind)?;
            let payload = session.maintenance_payload();
            let free =
                fs2::available_space(&service.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?;
            let budget = resource_budget(free, free)?;
            // Quarantine input-copy budget only, not a bound on physical PG
            // expansion. Session separately monitors actual free space while
            // the owned restore/reconciliation child is active.
            if archive_bytes(&input.source)?
                .checked_mul(4)
                .filter(|n| *n <= budget)
                .is_none()
            {
                return Err(err("NATIVE_BACKUP_RESOURCE_BUDGET"));
            }
            names(&service.root.join("config"), &BTreeSet::new())?;
            for logical in [
                "document.json",
                "upload.json",
                "oex-import.json",
                "publication.json",
                "static-export.json",
            ] {
                if exists(&profile.root, &journal(&service.root, logical)?)? {
                    return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));
                }
            }
            let source_manifest_sha256=digest(&installation_backup::source_bytes(&input.source,"manifest.gcm",17*1024*1024,true)?);
            let system_identifier=session.maintenance_system_identifier()?;
            let pending:Option<Pending>=if exists(&service.root,MARKER)?{Some(json(&service.root,MARKER,4096)?)}else{None};
            let empty=session.maintenance_empty_target()?;
            if pending.is_none() && !empty{return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));}
            if pending.as_ref().is_some_and(|p|p.format!=1 || !uuid(&p.id)){return Err(bad());}
            let id = pending.as_ref().map(|p|p.id.clone()).unwrap_or_else(||Uuid::new_v4().to_string());
            let directory = service.root.join(format!("native-restore-{id}"));
            if pending.is_some(){
                private_directory(&directory)?;
                let intent:Intent=json(&directory,"intent.json",16384)?;
                intent.check(&id,&input.source,&source_manifest_sha256,&system_identifier,&service.root,owner)?;
                if exists(&directory,"stage.json")?{
                    let plan:Plan=json(&directory,"stage.json",65536)?;valid_plan(&plan,&service.root)?;
                    if plan.id!=id || plan.system_identifier!=system_identifier{return Err(bad());}
                    return Ok(Receipt{id,backup_id:plan.backup_id,state:"staged".into()});
                }
            }else{
                new_directory(&directory)?;
                let intent=Intent{format:1,id:id.clone(),source:input.source.clone(),source_manifest_sha256,system_identifier:system_identifier.clone(),target_environment_sha256:file_hash(&service.root,"runtime.env")?.ok_or_else(bad)?,target_owner_sha256:file_hash(&service.root,owner)?.ok_or_else(bad)?};
                write_private_new(&directory,"intent.json",&serde_json::to_vec(&intent).map_err(|_|bad())?,16384)?;
                File::open(&directory).and_then(|v|v.sync_all()).map_err(|_|bad())?;
                let marker=Pending{format:1,id:id.clone()};
                write_private_new(&service.root,MARKER,&serde_json::to_vec(&marker).map_err(|_|bad())?,4096)?;
                File::open(&service.root).and_then(|v|v.sync_all()).map_err(|_|bad())?;
            }
            let scratch = directory.join("scratch");
            if scratch.exists(){private_directory(&scratch)?;}else{new_directory(&scratch)?;}
            let attempt=Uuid::new_v4().to_string();
            let restored = directory.join(format!("restored-{attempt}"));
            let operation=if empty{"restore"}else{"reconcile"};
            write_private_new(&directory,&format!("attempt-{attempt}.json"),&serde_json::to_vec(&serde_json::json!({"format":1,"id":id,"attempt":attempt,"operation":operation})).map_err(|_|bad())?,4096)?;
            // Existing codec authenticates the entire archive before DB/blob writes,
            // refuses original/nonempty DB, then verifies the original inventory.
            session.backup_tool(
                vec![
                    operation.into(),
                    "--key-file".into(),
                    text(&input.key_file)?,
                    "--source".into(),
                    text(&input.source)?,
                    "--destination".into(),
                    text(&restored)?,
                    "--quiesced".into(),
                    "--fresh-destination".into(),
                ],
                "{}".into(),
                "{}".into(),
                budget,
                &scratch,
            )?;
            let configuration = restored.join("configuration");
            authenticated_configuration(&restored, &prepared.kind)?;
            names(
                &configuration,
                &[
                    "native-source.json",
                    "native-runtime.env",
                    owner,
                    "freeze-signing-key.json",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            )?;
            let source: SourceMap = json(&configuration, "native-source.json", 65536)?;
            if source.format != 1
                || source.owner_kind != prepared.kind
                || source.manifest_sha256 != payload.context().manifest_sha256
                || !source.source_root.is_absolute()
                || source.source_root == service.root
                || source
                    .source_root
                    .components()
                    .any(|p| matches!(p, Component::ParentDir))
                || source.journal_names.len() > 4
            {
                return Err(bad());
            }
            for (logical, name) in &source.journal_names {
                if journal(&source.source_root, logical)? != *name {
                    return Err(bad());
                }
            }
            text(&source.source_root)?;
            authenticated_journals(&restored, &source)?;
            let validation = directory.join(format!("validation-{attempt}"));
            new_directory(&validation)?;
            let original_env = Secret(installation_backup::source_bytes(
                &configuration,
                "native-runtime.env",
                8192,
                true,
            )?);
            let original_owner = Secret(installation_backup::source_bytes(
                &configuration,
                owner,
                4096,
                true,
            )?);
            write_private_new(&validation, "runtime.env", &original_env.0, 8192)?;
            write_private_new(&validation, owner, &original_owner.0, 4096)?;
            File::open(&validation).and_then(|v|v.sync_all()).map_err(|_|bad())?;
            let original = LifecycleService::bound_existing(validation)?;
            let original_prepared = original.native_prepared()?.ok_or_else(bad)?;
            if original_prepared.kind != prepared.kind {
                return Err(bad());
            }
            let original_fields =
                Fields(original.native_start_environment(&payload, original_prepared.ports)?);
            session.maintenance_owner(&source.owner_kind, &original_fields.0)?;
            let target_fields = Fields(service.native_start_environment(&payload, prepared.ports)?);
            let environment = rebound_environment(&original_env.0, &target_fields.0)?;
            let mut record: Value = serde_json::from_slice(&original_owner.0).map_err(|_| bad())?;
            record["ports"] = serde_json::to_value(prepared.ports).map_err(|_| bad())?;
            record["manifestSha256"] = Value::String(payload.context().manifest_sha256);
            record["environmentSha256"] = Value::String(digest(&environment.0));
            let activation = directory.join(format!("activation-{attempt}"));
            new_directory(&activation)?;
            let mut files = vec![
                staged_file(
                    &activation,
                    &service.root,
                    false,
                    "runtime.env",
                    "runtime.env",
                    &environment.0,
                )?,
                staged_file(
                    &activation,
                    &service.root,
                    false,
                    owner,
                    owner,
                    &serde_json::to_vec(&record).map_err(|_| bad())?,
                )?,
                staged_file(
                    &activation,
                    &service.root,
                    false,
                    "config/freeze-signing-key.json",
                    "freeze-signing-key.json",
                    &installation_backup::source_bytes(
                        &configuration,
                        "freeze-signing-key.json",
                        65536,
                        true,
                    )?,
                )?,
            ];
            let native_journals = restored.join("deployment/native-authoring");
            if !source.journal_names.is_empty() {
                names(
                    &native_journals,
                    &source.journal_names.keys().cloned().collect(),
                )?;
            }
            for logical in source.journal_names.keys() {
                let bytes =
                    installation_backup::source_bytes(&native_journals, logical, 2_200_000, true)?;
                // A restored external output is historical feedback, not proof of
                // an existing file or an Internet deployment at this destination.
                let bytes = rebound_journal(logical, &bytes, &original_fields)?;
                files.push(staged_file(
                    &activation,
                    &profile.root,
                    true,
                    &journal(&service.root, logical)?,
                    logical,
                    &bytes,
                )?);
            }
            let manifest = installation_backup::source_bytes(
                &restored,
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            let manifest_value: Value = serde_json::from_slice(&manifest).map_err(|_| bad())?;
            let backup_id = manifest_value["id"]
                .as_str()
                .filter(|v| uuid(v))
                .ok_or_else(bad)?
                .to_owned();
            let plan = Plan {
                format: 1,
                id: id.clone(),
                backup_id: backup_id.clone(),
                manifest_sha256: digest(&manifest),
                payload_sha256: payload.context().manifest_sha256,
                system_identifier: session.maintenance_system_identifier()?,
                kind: source.owner_kind,
                ports: prepared.ports,
                files,
                attempt,
            };
            File::open(&activation).and_then(|v|v.sync_all()).map_err(|_|bad())?;
            valid_plan(&plan, &service.root)?;
            let rebound = LifecycleService::bound_existing(activation)?;
            let rebound_fields =
                Fields(rebound.native_start_environment(&payload, prepared.ports)?);
            rebound_identity(&plan.kind, &original_fields, &rebound_fields)?;
            observe(session, &directory, &plan, budget, &scratch)?;
            write_json(&directory, "stage.json", &plan)?;
            let receipt = Receipt {
                id,
                backup_id,
                state: "staged".into(),
            };
            write_json(&directory, "result.json", &receipt)?;
            File::open(&directory).and_then(|v|v.sync_all()).map_err(|_|bad())?;
            Ok(receipt)
        })
    }
    /// Explicit activation only. Reapplying the same plan accepts already
    /// installed exact bytes, while conflicts preserve both versions and refuse.
    /// Pending marker blocks Node startup across partial configuration updates.
    pub fn activate_native_restore(
        &self,
        token: &str,
        id: &str,
        payload: std::sync::Arc<Payload>,
    ) -> Result<Receipt> {
        self.with_current(token, |service| {
            if !uuid(id) {
                return Err(bad());
            }
            let profile = self.profile.as_ref().ok_or_else(bad)?;
            let _profile = profile_lock(profile)?;
            let guard = service.lock()?;
            signed_release::trust::lifecycle_writer::admit(&service.root, &Action::Install)?;
            let directory = service.root.join(format!("native-restore-{id}"));
            private_directory(&directory)?;
            let plan: Plan = json(&directory, "stage.json", 65536)?;
            valid_plan(&plan, &service.root)?;
            let restored = directory.join(format!("restored-{}", plan.attempt));
            let activation = directory.join(format!("activation-{}", plan.attempt));
            let validation_path = directory.join(format!("validation-{}", plan.attempt));
            if plan.id != id || plan.payload_sha256 != payload.context().manifest_sha256 {
                return Err(bad());
            }
            if !exists(&service.root, MARKER)? {
                let receipt: Receipt = json(&directory, "activated.json", 4096)?;
                if receipt.id != id
                    || receipt.backup_id != plan.backup_id
                    || receipt.state != "activated"
                {
                    return Err(bad());
                }
                return Ok(receipt);
            }
            let pending: Pending = json(&service.root, MARKER, 4096)?;
            if pending.format != 1 || pending.id != id {
                return Err(bad());
            }
            let manifest = installation_backup::source_bytes(
                &restored,
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )?;
            if digest(&manifest) != plan.manifest_sha256 {
                return Err(bad());
            }
            authenticated_configuration(&restored, &plan.kind)?;
            let source: SourceMap =
                json(&restored.join("configuration"), "native-source.json", 65536)?;
            if source.format != 1
                || source.owner_kind != plan.kind
                || source.manifest_sha256 != plan.payload_sha256
                || source.source_root == service.root
                || !source.source_root.is_absolute()
                || source.journal_names.len() > 4
                || plan.files.len() != 3 + source.journal_names.len()
            {
                return Err(bad());
            }
            for (logical, name) in &source.journal_names {
                if journal(&source.source_root, logical)? != *name {
                    return Err(bad());
                }
            }
            authenticated_journals(&restored, &source)?;
            let owner = owner_name(&plan.kind)?;
            for (name, archived) in [("runtime.env", "native-runtime.env"), (owner, owner)] {
                if file_hash(&validation_path, name)?
                    != file_hash(&restored.join("configuration"), archived)?
                {
                    return Err(bad());
                }
            }
            let validation = LifecycleService::bound_existing(validation_path)?;
            let original = validation.native_prepared()?.ok_or_else(bad)?;
            let original_fields =
                Fields(validation.native_start_environment(&payload, original.ports)?);
            for file in &plan.files {
                if file.profile {
                    if !source.journal_names.contains_key(&file.staged) {
                        return Err(bad());
                    }
                    let bytes = installation_backup::source_bytes(
                        &restored.join("deployment/native-authoring"),
                        &file.staged,
                        2_200_000,
                        true,
                    )?;
                    if digest(&rebound_journal(&file.staged, &bytes, &original_fields)?)
                        != file.sha256
                    {
                        return Err(bad());
                    }
                } else if file.name == "config/freeze-signing-key.json"
                    && file_hash(&restored.join("configuration"), "freeze-signing-key.json")?
                        .as_deref()
                        != Some(file.sha256.as_str())
                {
                    return Err(bad());
                }
            }
            let free =
                fs2::available_space(&service.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?;
            let budget = resource_budget(free, free)?;
            // Complete/repair only the exact staged configuration before creating
            // a PG-only Session. No Node/API starts while the marker exists.
            for (index, file) in plan.files.iter().enumerate().filter(|(_, f)| !f.profile) {
                install_file(&directory, &activation, &service.root, file, index)?;
            }
            let current = Fields(service.native_start_environment(&payload, plan.ports)?);
            rebound_identity(&plan.kind, &original_fields, &current)?;
            let scratch = directory.join("scratch");
            private_directory(&scratch)?;
            let mut session = Session::new_with_guard(service, payload.clone(), plan.ports, guard)?;
            let result = (|| {
                session.start_database_only()?;
                if session.maintenance_system_identifier()? != plan.system_identifier {
                    return Err(bad());
                }
                observe(&mut session, &directory, &plan, budget, &scratch)?;
                session.maintenance_owner(&plan.kind, &original_fields.0)?;
                for (index, file) in plan.files.iter().enumerate().filter(|(_, f)| f.profile) {
                    install_file(&directory, &activation, &profile.root, file, index)?;
                }
                for file in &plan.files {
                    let root = if file.profile {
                        &profile.root
                    } else {
                        &service.root
                    };
                    if file_hash(root, &file.name)?.as_deref() != Some(file.sha256.as_str()) {
                        return Err(bad());
                    }
                }
                Ok(())
            })();
            let stopped = session.stop()?;
            if stopped.postgres_pid.is_some()
                || stopped.node_pid.is_some()
                || stopped.auxiliary_pid.is_some()
            {
                return Err(err("NATIVE_STOP_PENDING"));
            }
            result?;
            let receipt = Receipt {
                id: id.into(),
                backup_id: plan.backup_id,
                state: "activated".into(),
            };
            write_json(&directory, "activated.json", &receipt)?;
            File::open(&directory)
                .and_then(|v| v.sync_all())
                .map_err(|_| bad())?;
            let current_marker: Pending = json(&service.root, MARKER, 4096)?;
            if current_marker.format != 1 || current_marker.id != id {
                return Err(bad());
            }
            fs::remove_file(service.root.join(MARKER)).map_err(|_| bad())?;
            File::open(&service.root)
                .and_then(|v| v.sync_all())
                .map_err(|_| bad())?;
            Ok(receipt)
        })
    }
}
fn observe(
    session: &mut Session,
    directory: &Path,
    plan: &Plan,
    budget: u64,
    scratch: &Path,
) -> Result<()> {
    session.backup_tool(
        vec![
            "check-restored-inventory".into(),
            "--manifest-file".into(),
            text(&directory.join(format!("restored-{}/manifest.json", plan.attempt)))?,
            "--manifest-sha256".into(),
            plan.manifest_sha256.clone(),
            "--snapshot-system-identifier".into(),
            plan.system_identifier.clone(),
            "--quiesced".into(),
        ],
        "{}".into(),
        "{}".into(),
        budget,
        scratch,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("exhibitos-native-restore-{}", Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        fs::canonicalize(p).unwrap()
    }
    fn fields() -> Fields {
        Fields(BTreeMap::from([
            ("WORKSPACE_ID".into(), Uuid::new_v4().to_string()),
            ("PRINCIPAL_ID".into(), Uuid::new_v4().to_string()),
            ("TENANT_ID".into(), Uuid::new_v4().to_string()),
            ("OWNER_UID".into(), unsafe { libc::geteuid() }.to_string()),
        ]))
    }
    #[test]
    fn environment_rebind_preserves_account_fields_and_identity_rejects_conversion() {
        let source=b"ADMIN_SUBJECT=synthetic\nADMIN_PASSWORD=preserved%20password\nPOSTGRES_PASSWORD=old\nDATABASE_URL=old\nEXHIBITOS_PORT=13200\nEXHIBITOS_API_PORT=13201\nNATIVE_RUNTIME_MANIFEST_SHA256=old\nTENANT_ID=preserved\n";
        let target = BTreeMap::from([
            ("POSTGRES_PASSWORD".into(), "new".into()),
            ("DATABASE_URL".into(), "new-url".into()),
            ("EXHIBITOS_PORT".into(), "14200".into()),
            ("EXHIBITOS_API_PORT".into(), "14201".into()),
            ("NATIVE_RUNTIME_MANIFEST_SHA256".into(), "new-pin".into()),
        ]);
        let value = rebound_environment(source, &target).unwrap();
        let text = std::str::from_utf8(&value.0).unwrap();
        assert!(
            text.contains("ADMIN_PASSWORD=preserved%20password\n")
                && text.contains("TENANT_ID=preserved\n")
                && text.contains("DATABASE_URL=new-url\n")
        );
        let old = fields();
        let mut changed = Fields(old.0.clone());
        rebound_identity("personal", &old, &changed).unwrap();
        changed
            .0
            .insert("PRINCIPAL_ID".into(), Uuid::new_v4().to_string());
        assert!(rebound_identity("personal", &old, &changed).is_err());
        assert!(rebound_identity("legacy_account", &old, &changed).is_err());
    }
    #[test]
    fn journal_rebind_keeps_request_bytes_and_refuses_cross_owner_or_arbitrary_names() {
        let f = fields();
        let source = root();
        let target = root();
        let raw=serde_json::to_vec(&serde_json::json!({"format":1,"revision":0,"workspaceId":f.0["WORKSPACE_ID"],"tenantId":f.0["TENANT_ID"],"principalId":f.0["PRINCIPAL_ID"],"intent":null,"jobId":null,"fileName":null})).unwrap();
        assert_eq!(rebound_journal("upload.json", &raw, &f).unwrap(), raw);
        assert!(rebound_journal("oex-import.json", &raw, &f).is_err());
        assert_ne!(journal(&source, "oex-import.json").unwrap(), journal(&source, "upload.json").unwrap());
        assert_ne!(
            journal(&source, "upload.json").unwrap(),
            journal(&target, "upload.json").unwrap()
        );
        assert!(journal(&target, "../upload.json").is_err());
        let mut other = Fields(f.0.clone());
        other
            .0
            .insert("WORKSPACE_ID".into(), Uuid::new_v4().to_string());
        assert!(rebound_journal("upload.json", &raw, &other).is_err());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(target).unwrap();
    }
    #[test]
    fn activation_replays_partial_configuration_and_preserves_later_user_changes() {
        let root = root();
        let job = root.join("job");
        new_directory(&job).unwrap();
        new_directory(&job.join("activation")).unwrap();
        write_private_new(&root, "runtime.env", b"original", 4096).unwrap();
        let file = staged_file(
            &job.join("activation"),
            &root,
            false,
            "runtime.env",
            "runtime.env",
            b"restored",
        )
        .unwrap();
        install_file(&job, &job.join("activation"), &root, &file, 0).unwrap();
        install_file(&job, &job.join("activation"), &root, &file, 0).unwrap();
        assert_eq!(fs::read(job.join("previous-0.bin")).unwrap(), b"original");
        fs::write(root.join("runtime.env"), b"user changed candidate").unwrap();
        assert!(install_file(&job, &job.join("activation"), &root, &file, 0).is_err());
        assert_eq!(
            fs::read(root.join("runtime.env")).unwrap(),
            b"user changed candidate"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pre_stage_retry_uses_same_intent_and_refuses_changed_archive_cluster_or_target() {
        let target = root();
        let source = root();
        let id = Uuid::new_v4().to_string();
        write_private_new(&target, "runtime.env", b"synthetic target env", 4096).unwrap();
        write_private_new(&target, "native-personal.json", b"synthetic owner", 4096).unwrap();
        let intent = Intent {
            format: 1,
            id: id.clone(),
            source: source.clone(),
            source_manifest_sha256: "a".repeat(64),
            system_identifier: "123".into(),
            target_environment_sha256: digest(b"synthetic target env"),
            target_owner_sha256: digest(b"synthetic owner"),
        };
        // The key path/value is deliberately not frozen: an explicitly chosen
        // corrected key still has to pass archive authentication on the retry.
        intent
            .check(
                &id,
                &source,
                &"a".repeat(64),
                "123",
                &target,
                "native-personal.json",
            )
            .unwrap();
        assert!(
            intent
                .check(
                    &id,
                    &source,
                    &"b".repeat(64),
                    "123",
                    &target,
                    "native-personal.json"
                )
                .is_err()
        );
        assert!(
            intent
                .check(
                    &id,
                    &source,
                    &"a".repeat(64),
                    "456",
                    &target,
                    "native-personal.json"
                )
                .is_err()
        );
        fs::write(target.join("runtime.env"), b"user changed candidate").unwrap();
        assert!(
            intent
                .check(
                    &id,
                    &source,
                    &"a".repeat(64),
                    "123",
                    &target,
                    "native-personal.json"
                )
                .is_err()
        );
        assert_eq!(
            fs::read(target.join("runtime.env")).unwrap(),
            b"user changed candidate"
        );
        fs::remove_dir_all(target).unwrap();
        fs::remove_dir_all(source).unwrap();
    }
    #[test]
    fn activation_plan_cannot_choose_arbitrary_paths_or_change_owner_kind() {
        let root = root();
        let owner = "native-personal.json";
        let files = [
            ("runtime.env", "runtime.env"),
            (owner, owner),
            ("config/freeze-signing-key.json", "freeze-signing-key.json"),
        ]
        .into_iter()
        .map(|(name, staged)| ActivationFile {
            profile: false,
            name: name.into(),
            staged: staged.into(),
            sha256: "a".repeat(64),
            bytes: 1,
            previous: None,
        })
        .collect();
        let mut plan = Plan {
            format: 1,
            id: Uuid::new_v4().to_string(),
            backup_id: Uuid::new_v4().to_string(),
            manifest_sha256: "a".repeat(64),
            payload_sha256: "a".repeat(64),
            system_identifier: "1".into(),
            attempt: Uuid::new_v4().to_string(),
            kind: "personal".into(),
            ports: Ports::default(),
            files,
        };
        valid_plan(&plan, &root).unwrap();
        plan.files[0].name = "../runtime.env".into();
        assert!(valid_plan(&plan, &root).is_err());
        plan.files[0].name = "runtime.env".into();
        plan.kind = "legacy_account".into();
        assert!(valid_plan(&plan, &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[path = "native_configuration_plan.rs"]
mod configuration_plan;
pub(crate) use configuration_plan::{ReboundConfigurationPlan, StagedNativeConfiguration, PublishedConfigurationSnapshot, plan_rebound_configuration, open_beforeimages};
