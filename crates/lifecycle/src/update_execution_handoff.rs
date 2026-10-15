// SPDX-License-Identifier: Apache-2.0
//! App-closed handoff for the registered Prepared apply operation; saved references never replace fresh qualification.
use super::*;
pub use crate::execution_reference::ReferenceSummary;
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
            || !(request.scope == "default" || request.scope == request.installation_id)
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
            let scope = if entry.kind == "default" {
                "default"
            } else {
                &entry.id
            };
            if registry.active_id != entry.id
                || scope != request.scope
                || digest(&original) != request.registry_sha256
            {
                return Err(err("UPDATE_HANDOFF_SELECTION_CHANGED"));
            }
            let (reference, reference_bytes) =
                crate::execution_reference::load(profile, &request.input.reference_id)?;
            if digest(&reference_bytes) != request.reference_sha256
                || reference.operation_id != request.input.operation_id
                || reference.generation != request.input.generation
            {
                return Err(err("UPDATE_REFERENCE_STALE"));
            }
            crate::execution_reference::apply(
                profile,
                &reference,
                &mut store,
                request.input.external_writers_quiesced,
            )
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
