// SPDX-License-Identifier: Apache-2.0
//! Fresh native candidate restoration and explicit, replayable activation.
//! Archive crypto, database restore and inventory checks remain Platform-owned.
use super::native_backup::{Secret, SourceMap, resource_budget, text};
use super::native_oex_import::{WORKSPACE_COMPLETION_ARCHIVE_NAME, workspace_completion, workspace_completion_name};
use super::*;
use crate::{native_installation::Ports, native_payload::Payload, native_runtime::{FurnishingWriterIdentity, Session}};
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

/// The ordered fields are also the canonical proof on the owner channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RestoreProof {
    restore_id: String,
    backup_id: String,
    authenticated_manifest_sha256: String,
    source_database_system_id: String,
    source_database_oid: String,
    target_database_system_id: String,
    target_database_oid: String,
    provenance_row_count: u64,
    provenance_table_sha256: String,
}

impl RestoreProof {
    pub(crate) fn matches(&self, id: &str, backup: &str, manifest: &str, target: &str) -> bool {
        let system = |value: &str| value.parse::<u64>().is_ok_and(|n| n != 0 && n.to_string() == value);
        let oid = |value: &str| value.parse::<u32>().is_ok_and(|n| n != 0 && n.to_string() == value);
        self.restore_id == id && uuid(id) && self.backup_id == backup && uuid(backup)
            && self.authenticated_manifest_sha256 == manifest && hash_valid(manifest)
            && self.target_database_system_id == target && system(target)
            && system(&self.source_database_system_id) && self.source_database_system_id != target
            && oid(&self.source_database_oid) && oid(&self.target_database_oid)
            && self.provenance_row_count <= 9_007_199_254_740_991
            && hash_valid(&self.provenance_table_sha256)
    }

    fn matches_manifest(&self, manifest: &Value) -> Result<()> {
        let source = &manifest["sourceIdentity"];
        let mut rows = manifest["inventory"]["tables"].as_array().ok_or_else(bad)?
            .iter().filter(|entry| entry["name"] == "native_oex_exhibition_provenance");
        let table = rows.next().ok_or_else(bad)?;
        if rows.next().is_some()
            || manifest["id"].as_str() != Some(self.backup_id.as_str())
            || source["systemIdentifier"].as_str() != Some(self.source_database_system_id.as_str())
            || source["databaseOid"].as_str() != Some(self.source_database_oid.as_str())
            || table["rowCount"].as_u64() != Some(self.provenance_row_count)
            || table["sha256"].as_str() != Some(self.provenance_table_sha256.as_str())
        {
            return Err(bad());
        }
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    furnishing_target: Option<FurnishingWriterIdentity>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restore_proof: Option<RestoreProof>,
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

pub(crate) fn pending_furnishing_target(
    root: &Path, expected_id: Option<&str>, physical: FurnishingWriterIdentity,
) -> Result<Option<String>> {
    if !exists(root, MARKER)? { return Ok(None); }
    let pending: Pending = json(root, MARKER, 4096)?;
    if pending.format != 1 || !uuid(&pending.id) || expected_id.is_some_and(|id| id != pending.id) {
        return Err(bad());
    }
    let directory = root.join(format!("native-restore-{}", pending.id));
    private_directory(&directory)?;
    let intent: Intent = json(&directory, "intent.json", 16384)?;
    if intent.format != 1 || intent.id != pending.id || intent.furnishing_target != Some(physical)
        || !intent.system_identifier.parse::<u64>().is_ok_and(|id| id != 0 && id.to_string() == intent.system_identifier)
    {
        return Err(bad());
    }
    Ok(Some(intent.system_identifier))
}
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
        WORKSPACE_COMPLETION_ARCHIVE_NAME => return Ok(workspace_completion_name(source)),
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
    if logical == WORKSPACE_COMPLETION_ARCHIVE_NAME {
        let identity = |name| fields.0.get(name).filter(|id| uuid(id)).map(String::as_str).ok_or_else(bad);
        let expected = workspace_completion(identity("WORKSPACE_ID")?, identity("TENANT_ID")?, identity("PRINCIPAL_ID")?);
        let marker: Value = serde_json::from_slice(bytes).map_err(|_| bad())?;
        if bytes.len() > 4096 || marker != expected { return Err(bad()); }
        return Ok(bytes.into());
    }
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
fn validate_source(source: &SourceMap, owner: &str, payload: &str, target: &Path) -> Result<bool> {
    let imported = source.journal_names.contains_key(WORKSPACE_COMPLETION_ARCHIVE_NAME);
    if source.format != 1 || source.owner_kind != owner || source.manifest_sha256 != payload
        || !source.source_root.is_absolute() || source.source_root == target
        || source.source_root.components().any(|p| matches!(p, Component::ParentDir))
        || source.journal_names.len() > if imported { 5 } else { 4 }
        || (imported && owner != "personal")
    {
        return Err(bad());
    }
    for (logical, name) in &source.journal_names {
        if logical == "oex-import.json" || journal(&source.source_root, logical)? != *name {
            return Err(bad());
        }
    }
    text(&source.source_root)?;
    Ok(imported)
}

fn restore_entry<'a>(profile: &Path, target: &Path, registry: &'a mut Registry) -> Result<&'a mut Entry> {
    registry.installations.iter_mut()
        .find(|entry| entry.id == registry.active_id && root(profile, entry) == target)
        .ok_or_else(|| err("INSTALLATION_SELECTION_CHANGED"))
}

fn promote_restored_entry(profile: &Path, target: &Path, mut registry: Registry, previous: &[u8]) -> Result<()> {
    let entry = restore_entry(profile, target, &mut registry)?;
    match entry.kind.as_str() {
        "oex-import" => return Ok(()),
        "recovery" => entry.kind = "oex-import".into(),
        _ => return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY")),
    }
    save(profile, &registry, Some(previous))?;
    let (mut installed, _) = load(profile)?.ok_or_else(bad)?;
    if restore_entry(profile, target, &mut installed)?.kind != "oex-import" { return Err(bad()); }
    Ok(())
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
        || plan.files.len() > 8
        || plan.restore_proof.as_ref().is_some_and(|proof|
            plan.kind != "personal" || !proof.matches(&plan.id, &plan.backup_id, &plan.manifest_sha256, &plan.system_identifier))
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
            self.ensure_active_creation_unlocked(service)?;
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
                WORKSPACE_COMPLETION_ARCHIVE_NAME,
            ] {
                if exists(&profile.root, &journal(&service.root, logical)?)? {
                    return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));
                }
            }
            let source_manifest = installation_backup::source_bytes(&input.source, "manifest.gcm", 17*1024*1024, true)?;
            let (source_manifest_bytes, source_manifest_pin) =
                crate::hosted_runtime_file::bounded(&input.source.join("manifest.gcm"), 17*1024*1024)?;
            if source_manifest_bytes != source_manifest { return Err(bad()); }
            let source_manifest_sha256 = digest(&source_manifest);
            drop(source_manifest_bytes);
            drop(source_manifest);
            let system_identifier=session.maintenance_system_identifier()?;
            let furnishing_target = session.furnishing_restore_target_identity()?;
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
                let intent=Intent{format:1,id:id.clone(),source:input.source.clone(),source_manifest_sha256,system_identifier:system_identifier.clone(),target_environment_sha256:file_hash(&service.root,"runtime.env")?.ok_or_else(bad)?,target_owner_sha256:file_hash(&service.root,owner)?.ok_or_else(bad)?,furnishing_target};
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
            let codec = session.backup_tool(
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
            source_manifest_pin.check()?;
            let manifest = installation_backup::source_bytes(&restored, "manifest.json", 16*1024*1024, true)?;
            let (manifest_bytes, manifest_pin) =
                crate::hosted_runtime_file::bounded(&restored.join("manifest.json"), 16*1024*1024)?;
            if manifest_bytes != manifest { return Err(bad()); }
            let manifest_value: Value = serde_json::from_slice(&manifest).map_err(|_| bad())?;
            let backup_id = manifest_value["id"].as_str().filter(|value| uuid(value)).ok_or_else(bad)?.to_owned();
            let manifest_sha256 = digest(&manifest);
            let codec_authenticated = check_codec_restore_output(&codec, &backup_id, &manifest_sha256)?;
            drop(manifest_bytes);
            drop(manifest);
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
            if validate_source(&source, &prepared.kind, &payload.context().manifest_sha256, &service.root)? {
                let (mut registry, _) = load(&profile.root)?.ok_or_else(bad)?;
                if restore_entry(&profile.root, &service.root, &mut registry)?.kind != "recovery" {
                    return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));
                }
            }
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
            let mut plan = Plan {
                format: 1,
                id: id.clone(),
                backup_id: backup_id.clone(),
                manifest_sha256,
                payload_sha256: payload.context().manifest_sha256,
                system_identifier: session.maintenance_system_identifier()?,
                kind: source.owner_kind,
                ports: prepared.ports,
                files,
                attempt,
                restore_proof: None,
            };
            File::open(&activation).and_then(|v|v.sync_all()).map_err(|_|bad())?;
            valid_plan(&plan, &service.root)?;
            let rebound = LifecycleService::bound_existing(activation)?;
            let rebound_fields =
                Fields(rebound.native_start_environment(&payload, prepared.ports)?);
            rebound_identity(&plan.kind, &original_fields, &rebound_fields)?;
            plan.restore_proof = observe(session, &directory, &plan, &manifest_value, budget, &scratch)?;
            if plan.restore_proof.is_some() && !codec_authenticated { return Err(bad()); }
            source_manifest_pin.check()?;
            manifest_pin.check()?;
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
            self.ensure_active_creation_unlocked(service)?;
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
            let completion = plan.files.iter().find(|f| f.profile && f.staged == WORKSPACE_COMPLETION_ARCHIVE_NAME);
            if !exists(&service.root, MARKER)? {
                let receipt: Receipt = json(&directory, "activated.json", 4096)?;
                if receipt.id != id
                    || receipt.backup_id != plan.backup_id
                    || receipt.state != "activated"
                {
                    return Err(bad());
                }
                if let Some(file) = completion {
                    let (mut registry, _) = load(&profile.root)?.ok_or_else(bad)?;
                    if restore_entry(&profile.root, &service.root, &mut registry)?.kind != "oex-import"
                        || file_hash(&profile.root, &file.name)?.as_deref() != Some(file.sha256.as_str())
                    { return Err(bad()); }
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
            let manifest_value: Value = serde_json::from_slice(&manifest).map_err(|_| bad())?;
            authenticated_configuration(&restored, &plan.kind)?;
            let source: SourceMap =
                json(&restored.join("configuration"), "native-source.json", 65536)?;
            let imported = validate_source(&source, &plan.kind, &plan.payload_sha256, &service.root)?;
            if imported != completion.is_some() || plan.files.len() != 3 + source.journal_names.len() { return Err(bad()); }
            let (mut registry, previous) = load(&profile.root)?.ok_or_else(bad)?;
            let entry = restore_entry(&profile.root, &service.root, &mut registry)?;
            if imported && !matches!(entry.kind.as_str(), "recovery" | "oex-import") {
                return Err(err("NATIVE_RESTORE_TARGET_NOT_EMPTY"));
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
                session.start_restore_activation(id)?;
                if session.maintenance_system_identifier()? != plan.system_identifier {
                    return Err(bad());
                }
                if observe(&mut session, &directory, &plan, &manifest_value, budget, &scratch)? != plan.restore_proof {
                    return Err(bad());
                }
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
            let stopped = if result.is_ok() {
                match session.stop_with_furnishing_activation_fences(id) {
                    Ok(stopped) => stopped,
                    Err(error) => { let _ = session.stop(); return Err(error); }
                }
            } else {
                session.stop()?
            };
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
            if imported {
                // The profile registry is authoritative for ordinary startup.
                // Keep the runtime fence across marker installation and promotion.
                promote_restored_entry(&profile.root, &service.root, registry, &previous)?;
            }
            session.record_furnishing_activation(
                id, &receipt.backup_id, &plan.manifest_sha256, &plan.system_identifier,
                plan.restore_proof,
            )?;
            fs::remove_file(service.root.join(MARKER)).map_err(|_| bad())?;
            File::open(&service.root)
                .and_then(|v| v.sync_all())
                .map_err(|_| bad())?;
            Ok(receipt)
        })
    }
}
pub(crate) fn verify_ordinary_activation(
    root: &Path, id: &str, backup_id: &str, manifest_sha256: &str, system_identifier: &str, payload_sha256: &str,
    restore_proof: Option<&RestoreProof>,
) -> Result<()> {
    if !uuid(id) || exists(root, MARKER)? { return Err(bad()); }
    let directory = root.join(format!("native-restore-{id}"));
    private_directory(&directory)?;
    let plan: Plan = json(&directory, "stage.json", 65536)?;
    valid_plan(&plan, root)?;
    let receipt: Receipt = json(&directory, "activated.json", 4096)?;
    if plan.id != id || receipt.id != id || receipt.state != "activated"
        || plan.backup_id != backup_id || receipt.backup_id != backup_id
        || plan.manifest_sha256 != manifest_sha256 || plan.system_identifier != system_identifier
        || plan.payload_sha256 != payload_sha256 || plan.kind != "personal"
        || plan.restore_proof.as_ref() != restore_proof
    {
        return Err(bad());
    }
    if let Some(proof) = restore_proof {
        let restored = directory.join(format!("restored-{}", plan.attempt));
        let manifest = installation_backup::source_bytes(&restored, "manifest.json", 16 * 1024 * 1024, true)?;
        if digest(&manifest) != manifest_sha256 { return Err(bad()); }
        proof.matches_manifest(&serde_json::from_slice(&manifest).map_err(|_| bad())?)?;
    }
    Ok(())
}

fn observe(
    session: &mut Session,
    directory: &Path,
    plan: &Plan,
    manifest: &Value,
    budget: u64,
    scratch: &Path,
) -> Result<Option<RestoreProof>> {
    session.verify_furnishing_restore_target(&plan.id)?;
    let result = session.backup_tool(
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
    if plan.kind != "personal" || session.maintenance_payload().workspace_furnishing_catalog()?.map(|catalog| catalog.version()) != Some(40) {
        return Ok(None);
    }
    restore_proof_from_observation(plan, manifest, &result)
}

fn restore_proof_from_observation(plan: &Plan, manifest: &Value, result: &Value) -> Result<Option<RestoreProof>> {
    let Some(observation) = result.get("restoredProvenanceInventory") else { return Ok(None); };
    if plan.kind != "personal"
        || result["operation"] != "restored-inventory-matched"
        || result["backupId"].as_str() != Some(plan.backup_id.as_str())
        || result["authenticatedManifestSha256"].as_str() != Some(plan.manifest_sha256.as_str())
        || result["currentInventoryVerified"] != true
    {
        return Err(bad());
    }
    let mut fields = observation.as_object().ok_or_else(bad)?.clone();
    for (key, value) in [("restoreId", &plan.id), ("backupId", &plan.backup_id),
        ("authenticatedManifestSha256", &plan.manifest_sha256)]
    {
        if fields.insert(key.into(), Value::String(value.clone())).is_some() { return Err(bad()); }
    }
    let proof: RestoreProof = serde_json::from_value(Value::Object(fields)).map_err(|_| bad())?;
    if !proof.matches(&plan.id, &plan.backup_id, &plan.manifest_sha256, &plan.system_identifier) {
        return Err(bad());
    }
    proof.matches_manifest(manifest)?;
    Ok(Some(proof))
}

fn check_codec_restore_output(codec: &Value, backup_id: &str, manifest_sha256: &str) -> Result<bool> {
    match (codec.get("backupId"), codec.get("authenticatedManifestSha256")) {
        (None, None) => Ok(false),
        (Some(id), Some(sha)) if codec["operation"] == "restored-and-verified"
            && id.as_str() == Some(backup_id) && sha.as_str() == Some(manifest_sha256) => Ok(true),
        _ => Err(bad()),
    }
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
    fn restored_provenance_requires_the_observed_target_and_authenticated_manifest() {
        let plan = Plan {
            format: 1, id: Uuid::new_v4().to_string(), backup_id: Uuid::new_v4().to_string(),
            manifest_sha256: "a".repeat(64), payload_sha256: "b".repeat(64),
            system_identifier: "220".into(), kind: "personal".into(), ports: Ports::default(),
            files: vec![], attempt: Uuid::new_v4().to_string(), restore_proof: None,
        };
        let manifest = serde_json::json!({
            "id": plan.backup_id, "sourceIdentity": {"systemIdentifier":"110","databaseOid":"16000"},
            "inventory":{"tables":[{"name":"native_oex_exhibition_provenance","rowCount":1,"sha256":"c".repeat(64)}]}
        });
        let result = serde_json::json!({
            "operation":"restored-inventory-matched","backupId":plan.backup_id,
            "authenticatedManifestSha256":plan.manifest_sha256,"currentInventoryVerified":true,
            "restoredProvenanceInventory": {
                "sourceDatabaseSystemId":"110","sourceDatabaseOid":"16000",
                "targetDatabaseSystemId":"220","targetDatabaseOid":"16002",
                "provenanceRowCount":1,"provenanceTableSha256":"c".repeat(64)
            }
        });
        assert!(restore_proof_from_observation(&plan,&manifest,&result).unwrap().is_some());
        for (key, value) in [
            ("sourceDatabaseOid", "16001".into()), ("targetDatabaseSystemId", "110".into()),
            ("targetDatabaseOid", "016002".into()), ("provenanceTableSha256", "d".repeat(64).into()),
            ("provenanceRowCount", 2.into()), ("extra", true.into()),
        ] {
            let mut invalid = result.clone();
            invalid["restoredProvenanceInventory"][key] = value;
            assert!(restore_proof_from_observation(&plan,&manifest,&invalid).is_err(), "{key}");
        }
        let mut wrong_manifest = manifest;
        wrong_manifest["inventory"]["tables"][0]["rowCount"] = 2.into();
        assert!(restore_proof_from_observation(&plan,&wrong_manifest,&result).is_err());
        assert!(restore_proof_from_observation(&plan,&wrong_manifest,&serde_json::json!({})).unwrap().is_none());
    }

    #[test]
    fn restore_proof_requires_the_manifest_bytes_authenticated_by_the_codec() {
        let id = Uuid::new_v4().to_string();
        let raw = b"{\"authenticated\":true}\n";
        let sha = digest(raw);
        let mut codec = serde_json::json!({
            "operation":"restored-and-verified","backupId":id,"authenticatedManifestSha256":sha
        });
        assert!(check_codec_restore_output(&codec, &id, &sha).unwrap());
        assert!(check_codec_restore_output(&codec, &id, &digest(b"{\"authenticated\":false}\n")).is_err());
        assert!(check_codec_restore_output(&codec, &Uuid::new_v4().to_string(), &sha).is_err());
        codec.as_object_mut().unwrap().remove("backupId");
        assert!(check_codec_restore_output(&codec, &id, &sha).is_err());
        assert!(!check_codec_restore_output(&serde_json::json!({"operation":"restored-and-verified"}), &id, &sha).unwrap());
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
    fn settled_import_marker_rebinds_only_exact_source_path_payload_and_owner() {
        let source = root();
        let target = root();
        let f = fields();
        let logical = WORKSPACE_COMPLETION_ARCHIVE_NAME;
        let source_name = journal(&source, logical).unwrap();
        let target_name = journal(&target, logical).unwrap();
        assert_ne!(source_name, target_name);
        let payload = "a".repeat(64);
        let mut map = SourceMap { format: 1, source_root: source.clone(), owner_kind: "personal".into(),
            manifest_sha256: payload.clone(), journal_names: BTreeMap::new() };
        assert!(!validate_source(&map, "personal", &payload, &target).unwrap());
        map.journal_names.insert(logical.into(), target_name);
        assert!(validate_source(&map, "personal", &payload, &target).is_err());
        map.journal_names.insert(logical.into(), source_name);
        assert!(validate_source(&map, "personal", &payload, &target).unwrap());
        assert!(validate_source(&map, "personal", &payload, &source).is_err());
        assert!(validate_source(&map, "personal", &"b".repeat(64), &target).is_err());

        let mut marker = workspace_completion(&f.0["WORKSPACE_ID"], &f.0["TENANT_ID"], &f.0["PRINCIPAL_ID"]);
        let raw = serde_json::to_vec(&marker).unwrap();
        assert_eq!(rebound_journal(logical, &raw, &f).unwrap(), raw);
        marker["principalId"] = Value::String(Uuid::new_v4().to_string());
        assert!(rebound_journal(logical, &serde_json::to_vec(&marker).unwrap(), &f).is_err());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn settled_import_registry_promotion_is_exact_and_replayable() {
        let profile = root();
        let original = Entry { id: Uuid::new_v4().to_string(), kind: "default".into(), created_at: now() };
        let candidate = Entry { id: Uuid::new_v4().to_string(), kind: "recovery".into(), created_at: now() };
        let target = super::root(&profile, &candidate);
        let wrong = super::root(&profile, &original);
        let registry = Registry { format: 1, active_id: candidate.id.clone(), installations: vec![original, candidate] };
        save(&profile, &registry, None).unwrap();
        let (registry, before) = load(&profile).unwrap().unwrap();
        assert!(promote_restored_entry(&profile, &wrong, registry, &before).is_err());
        let (registry, before) = load(&profile).unwrap().unwrap();
        promote_restored_entry(&profile, &target, registry, &before).unwrap();
        let (mut registry, after) = load(&profile).unwrap().unwrap();
        assert_eq!(registry.installations[0].kind, "default");
        assert_eq!(restore_entry(&profile, &target, &mut registry).unwrap().kind, "oex-import");
        promote_restored_entry(&profile, &target, registry, &after).unwrap();
        assert_eq!(load(&profile).unwrap().unwrap().1, after);
        fs::remove_dir_all(profile).unwrap();
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
    fn ordinary_furnishing_pending_restore_target_requires_exact_intent_and_physical_binding() {
        let target = root();
        let id = Uuid::new_v4().to_string();
        let physical = serde_json::from_value(serde_json::json!({
            "rootDevice": 1, "rootInode": 2, "lockDevice": 1, "lockInode": 3,
        })).unwrap();
        let replacement = serde_json::from_value(serde_json::json!({
            "rootDevice": 1, "rootInode": 2, "lockDevice": 1, "lockInode": 4,
        })).unwrap();
        assert!(pending_furnishing_target(&target, None, physical).unwrap().is_none());
        let directory = target.join(format!("native-restore-{id}"));
        new_directory(&directory).unwrap();
        write_json(&target, MARKER, &Pending { format: 1, id: id.clone() }).unwrap();
        assert!(pending_furnishing_target(&target, Some(&id), physical).is_err());
        let mut intent = Intent {
            format: 1, id: id.clone(), source: target.join("archive"),
            source_manifest_sha256: "a".repeat(64), system_identifier: "123".into(),
            target_environment_sha256: "b".repeat(64), target_owner_sha256: "c".repeat(64),
            furnishing_target: None,
        };
        write_json(&directory, "intent.json", &intent).unwrap();
        assert!(pending_furnishing_target(&target, Some(&id), physical).is_err());
        intent.furnishing_target = Some(physical);
        write_json(&directory, "intent.json", &intent).unwrap();
        assert_eq!(pending_furnishing_target(&target, None, physical).unwrap().as_deref(), Some("123"));
        assert_eq!(pending_furnishing_target(&target, Some(&id), physical).unwrap().as_deref(), Some("123"));
        assert!(pending_furnishing_target(&target, Some(&id), replacement).is_err());
        assert!(pending_furnishing_target(&target, Some(&Uuid::new_v4().to_string()), physical).is_err());
        fs::remove_dir_all(target).unwrap();
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
            furnishing_target: None,
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
            restore_proof: None,
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
