// SPDX-License-Identifier: Apache-2.0
//! Explicit app-closed original-service restoration and restored-candidate completion.
use super::*;
use crate::signed_release::trust::Store;
const LIMIT: usize = 16384;
const MAX_RECORDS: usize = 384;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RollbackAction {
    RestoreOriginal,
    CompleteRestored,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollbackInput {
    pub action: RollbackAction,
    pub operation_id: String,
    pub generation: u64,
    pub maintenance_image: String,
    pub key_file: Option<PathBuf>,
    pub archive: Option<PathBuf>,
    pub port: Option<u16>,
    pub preserve_data: bool,
    pub close_app_and_rollback: bool,
    pub external_writers_quiesced: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollbackContext {
    pub installation_id: String,
    pub operation_id: String,
    pub generation: u64,
    pub stage: crate::update::Stage,
    pub backup_id: String,
    pub source_image: String,
    pub source_schema: String,
    pub backup_manifest: String,
    pub source_inventory: String,
    pub can_restore_original: bool,
    pub can_complete_restored: bool,
    pub selection_reconciliation_required: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    id: String,
    installation_id: String,
    scope: String,
    registry_sha256: String,
    context: RollbackContext,
    input: RollbackInput,
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
    profile.join("update-rollback-handoffs")
}
fn persist<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| err("UPDATE_HANDOFF_INVALID"))?;
    write_private_new(root, name, &bytes, LIMIT)?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("UPDATE_HANDOFF_UNCERTAIN"))
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
pub(crate) fn validate(v: &RollbackInput) -> Result<()> {
    if !v.preserve_data || !v.close_app_and_rollback || !v.external_writers_quiesced {
        return Err(err("UPDATE_HANDOFF_CONSENT_REQUIRED"));
    }
    if v.generation == 0
        || v.generation > 9_007_199_254_740_991
        || v.operation_id.is_empty()
        || v.operation_id.len() > 128
        || !v
            .operation_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        || !v
            .maintenance_image
            .strip_prefix("sha256:")
            .is_some_and(|h| {
                h.len() == 64
                    && h.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
    {
        return Err(err("UPDATE_ROLLBACK_INPUT_INVALID"));
    }
    match v.action {
        RollbackAction::RestoreOriginal => {
            if v.port.is_none_or(|p| p < 1024)
                || [&v.key_file, &v.archive].iter().any(|p| {
                    p.as_ref().is_none_or(|p| {
                        !p.is_absolute()
                            || p.to_str()
                                .is_none_or(|s| s.len() > 4096 || s.contains('\0'))
                    })
                })
            {
                return Err(err("UPDATE_ROLLBACK_INPUT_INVALID"));
            }
        }
        RollbackAction::CompleteRestored => {
            if v.key_file.is_some() || v.archive.is_some() || v.port.is_some() {
                return Err(err("UPDATE_ROLLBACK_INPUT_INVALID"));
            }
        }
    }
    Ok(())
}
fn allowed(context: &RollbackContext, input: &RollbackInput) -> bool {
    context.operation_id == input.operation_id
        && context.generation == input.generation
        && match input.action {
            RollbackAction::RestoreOriginal => context.can_restore_original,
            RollbackAction::CompleteRestored => context.can_complete_restored,
        }
}
pub(crate) fn prepare(
    profile: &Path,
    installation: &str,
    scope: &str,
    registry: &[u8],
    input: RollbackInput,
) -> Result<Receipt> {
    validate(&input)?;
    let context =
        crate::signed_release::trust::inspect_rollback_context(profile, scope, installation)?
            .ok_or_else(|| err("UPDATE_HANDOFF_AUTHORITY_CHANGED"))?;
    if !allowed(&context, &input) {
        return Err(err("UPDATE_HANDOFF_AUTHORITY_CHANGED"));
    }
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
        context,
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
                || !["rolled-back", "failed"].contains(&outcome.state.as_str())
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
    #[cfg(not(unix))]
    {
        let _ = (profile, id);
        return Err(err("UPDATE_TRUST_PLATFORM_UNVERIFIED"));
    }
    #[cfg(unix)]
    {
        if !installations::uuid(id) {
            return Err(err("UPDATE_HANDOFF_INVALID"));
        }
        installations::private_directory(profile)?;
        let root = directory(profile);
        installations::private_directory(&root)?;
        let name = format!("{id}.request.json");
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
            Ok(lock) => OperationGuard(lock),
            Err(e) => {
                receipt.code = Some(e.code.clone());
                persist(&root, &format!("{id}.outcome.json"), &receipt)?;
                return Err(e);
            }
        };
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
            let context = store
                .rollback_handoff_context(&request.installation_id)?
                .ok_or_else(|| err("UPDATE_HANDOFF_AUTHORITY_CHANGED"))?;
            if context != request.context || !allowed(&context, &request.input) {
                return Err(err("UPDATE_HANDOFF_AUTHORITY_CHANGED"));
            }
            Ok(())
        })();
        // Existing rollback methods acquire their own profile/runtime guards.
        // Keep the exclusive Store/anchor, but release this validation guard first.
        drop(operation);
        let result = result.and_then(|()| store.execute_rollback_handoff(&request.input));
        match &result {
            Ok(()) => receipt.state = "rolled-back".into(),
            Err(e) => receipt.code = Some(e.code.clone()),
        }
        persist(&root, &format!("{id}.outcome.json"), &receipt)?;
        result
    }
}
