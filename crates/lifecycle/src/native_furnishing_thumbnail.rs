// SPDX-License-Identifier: Apache-2.0
//! One private pending registered-furnishing thumbnail mutation per workspace.
//! This journal retains a replay intent; it never grants render/source authority.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

pub(crate) const LIMIT: u64 = 800 * 1024;
const PNG_LIMIT: usize = 512 * 1024;
const PROFILE: &str = "workspace-furnishing-thumbnail-v1";
const INVALID: &str = "NATIVE_FURNISHING_THUMBNAIL_INVALID";
const CHANGED: &str = "NATIVE_FURNISHING_THUMBNAIL_STATE_CHANGED";
const SCOPE: &str = "NATIVE_FURNISHING_THUMBNAIL_SCOPE_CHANGED";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Pending {
    Capture { furnishing_id: String, furnishing_version: u32, request_id: String, capture: Value, png_base64: String },
    Selection { furnishing_id: String, furnishing_version: u32, request_id: String, candidate: Value, expected_selection_version: u32 },
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub format: u32,
    pub revision: u64,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    #[serde(deserialize_with = "required_pending")]
    pub pending: Option<Pending>,
}
fn required_pending<'de, D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Option<Pending>, D::Error> {
    Option::<Pending>::deserialize(decoder)
}
fn invalid() -> LifecycleError { err(INVALID) }
fn closed(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)))
}
fn integer(value: &Value, minimum: u64, maximum: u64) -> bool {
    value.as_u64().is_some_and(|number| (minimum..=maximum).contains(&number))
}
fn id(value: &Value) -> bool { value.as_str().is_some_and(uuid) }
fn hash(value: &Value) -> bool { value.as_str().is_some_and(hash_valid) }
fn png_metadata(value: &Value) -> bool {
    closed(value, &["mime", "width", "height", "sha256", "bytes"])
        && value["mime"] == "image/png" && value["width"] == 256 && value["height"] == 256
        && hash(&value["sha256"]) && integer(&value["bytes"], 33, PNG_LIMIT as u64)
}
fn render_request(value: &Value, state: &State, furnishing: &str) -> bool {
    closed(value, &["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "furnishingId",
        "furnishingVersion", "modelSha256", "modelBytes", "recipeId", "toolId"])
        && value["version"] == 1 && id(&value["epoch"]) && id(&value["requestId"])
        && integer(&value["sequence"], 1, i64::MAX as u64)
        && value["tenantId"] == state.tenant_id && value["principalId"] == state.principal_id
        && value["furnishingId"] == furnishing && value["furnishingVersion"] == 1
        && hash(&value["modelSha256"]) && integer(&value["modelBytes"], 28, 32 * 1024 * 1024)
        && value["recipeId"] == "furnishing-glb-card-256-v1"
        && value["toolId"] == "exhibitos-three-0.186.1-thumbnail-v1"
}
fn source_metadata(value: &Value, furnishing: &str) -> bool {
    closed(value, &["furnishingId", "furnishingVersion", "modelSha256", "modelBytes", "descriptorSha256", "rightsSha256"])
        && value["furnishingId"] == furnishing && value["furnishingVersion"] == 1
        && hash(&value["modelSha256"]) && integer(&value["modelBytes"], 28, 32 * 1024 * 1024)
        && hash(&value["descriptorSha256"]) && hash(&value["rightsSha256"])
}
fn candidate_metadata(value: &Value, state: &State, furnishing: &str) -> bool {
    let source = &value["source"]; let receipt = &value["receipt"]; let request = &receipt["renderRequest"];
    closed(value, &["candidateId", "state", "source", "thumbnail", "receipt", "receiptSha256"])
        && id(&value["candidateId"]) && value["state"] == "ready" && hash(&value["receiptSha256"])
        && source_metadata(source, furnishing) && png_metadata(&value["thumbnail"])
        && closed(receipt, &["version", "purpose", "profile", "workspaceId", "candidateId", "requestId", "source", "renderRequest", "png"])
        && receipt["version"] == 1 && receipt["purpose"] == "registered-furnishing-thumbnail" && receipt["profile"] == PROFILE
        && receipt["workspaceId"] == state.workspace_id && receipt["candidateId"] == value["candidateId"]
        && receipt["source"] == *source && receipt["png"] == value["thumbnail"]
        && render_request(request, state, furnishing) && receipt["requestId"] == request["requestId"]
        && request["modelSha256"] == source["modelSha256"] && request["modelBytes"] == source["modelBytes"]
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 { crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb88320 } else { crc >> 1 }; }
    }
    crc ^ u32::MAX
}
// Match the existing generated-thumbnail canonical chunk policy. No decoder,
// derivative, pixels-derived-from-model assertion, or metadata rewrite occurs.
fn canonical_png(bytes: &[u8]) -> bool {
    if !(33..=PNG_LIMIT).contains(&bytes.len()) || bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") { return false; }
    let u32_at = |offset: usize| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (mut offset, mut chunks, mut header, mut srgb, mut idat, mut ended) = (8usize, 0, false, false, false, false);
    while offset < bytes.len() {
        chunks += 1;
        if chunks > 256 || bytes.len() - offset < 12 { return false; }
        let length = u32_at(offset) as usize;
        if length > bytes.len() - offset - 12 { return false; }
        let end = offset + length + 12;
        if crc32(&bytes[offset + 4..end - 4]) != u32_at(end - 4) { return false; }
        match &bytes[offset + 4..offset + 8] {
            b"IHDR" if !header && offset == 8 && length == 13 && u32_at(offset + 8) == 256 && u32_at(offset + 12) == 256
                && bytes[offset + 16] == 8 && matches!(bytes[offset + 17], 2 | 6)
                && bytes[offset + 18..offset + 21] == [0, 0, 0] => { header = true; }
            b"sRGB" if header && !srgb && !idat && length == 1 && bytes[offset + 8] <= 3 => { srgb = true; }
            b"IDAT" if header && !ended => { idat = true; }
            b"IEND" if header && idat && !ended && length == 0 && end == bytes.len() => { ended = true; }
            _ => return false,
        }
        offset = end;
    }
    ended
}
pub(crate) fn validate(state: &State) -> Result<()> {
    if state.format != 1 || ![&state.workspace_id, &state.tenant_id, &state.principal_id].into_iter().all(|value| uuid(value)) {
        return Err(invalid());
    }
    match &state.pending {
        None => (),
        Some(Pending::Capture { furnishing_id, furnishing_version, request_id, capture, png_base64 }) => {
            if !uuid(furnishing_id) || *furnishing_version != 1 || !uuid(request_id)
                || !closed(capture, &["workspaceId", "nativeRevision", "editVersion", "baseETag", "request", "png"])
                || capture["workspaceId"] != state.workspace_id
                || !integer(&capture["nativeRevision"], 0, i64::MAX as u64) || !integer(&capture["editVersion"], 0, i64::MAX as u64)
                || !capture["baseETag"].as_str().is_some_and(|v| !v.is_empty() && v.len() <= 1024)
                || !render_request(&capture["request"], state, furnishing_id) || capture["request"]["requestId"] != *request_id
                || !png_metadata(&capture["png"]) || png_base64.len() > 699_052 { return Err(invalid()); }
            let bytes = STANDARD.decode(png_base64).map_err(|_| invalid())?;
            if STANDARD.encode(&bytes) != *png_base64 || capture["png"]["bytes"].as_u64() != Some(bytes.len() as u64)
                || capture["png"]["sha256"].as_str() != Some(digest(&bytes).as_str()) || !canonical_png(&bytes) { return Err(invalid()); }
        }
        Some(Pending::Selection { furnishing_id, furnishing_version, request_id, candidate, expected_selection_version })
            if !uuid(furnishing_id) || *furnishing_version != 1 || !uuid(request_id) || *expected_selection_version > 2_147_483_646
                || !candidate_metadata(candidate, state, furnishing_id) => return Err(invalid()),
        Some(Pending::Selection { .. }) => (),
    }
    if serde_json::to_vec(state).map_err(|_| invalid())?.len() as u64 > LIMIT { return Err(invalid()); }
    Ok(())
}
fn advance(current: &State, mut next: State) -> Result<State> {
    validate(&next)?;
    if (next.workspace_id.as_str(), next.tenant_id.as_str(), next.principal_id.as_str())
        != (current.workspace_id.as_str(), current.tenant_id.as_str(), current.principal_id.as_str()) { return Err(err(SCOPE)); }
    // An acknowledgement can be lost after durable publication. Re-reading or
    // repeating exactly that pending intent may return the current revision.
    if next.pending.is_some() && next.pending == current.pending && next.revision <= current.revision { return Ok(current.clone()); }
    if next.revision != current.revision || (current.pending.is_some() && next.pending.is_some()) { return Err(err(CHANGED)); }
    next.revision = current.revision.checked_add(1).ok_or_else(invalid)?;
    Ok(next)
}
fn read_existing(profile: &Path, name: &str, empty: &State) -> Result<State> {
    let saved = match fs::symlink_metadata(profile.join(name)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => empty.clone(),
        Err(_) => return Err(err("STATE_UNAVAILABLE")),
        Ok(_) => serde_json::from_slice(&installation_backup::source_bytes(profile, name, LIMIT, true)?).map_err(|_| invalid())?,
    };
    validate(&saved)?;
    if (saved.workspace_id.as_str(), saved.tenant_id.as_str(), saved.principal_id.as_str())
        != (empty.workspace_id.as_str(), empty.tenant_id.as_str(), empty.principal_id.as_str()) { return Err(err(SCOPE)); }
    Ok(saved)
}
impl InstallationController {
    pub(super) fn furnishing_thumbnail_unlocked(&self, service: &LifecycleService) -> Result<(String, State)> {
        let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let name = format!("native-furnishing-thumbnail-{}.json", digest(service.root.as_os_str().as_encoded_bytes()));
        let empty = State { format: 1, revision: 0, workspace_id, tenant_id, principal_id, pending: None };
        let saved = read_existing(&profile.root, &name, &empty)?;
        Ok((name, saved))
    }
    pub fn native_furnishing_thumbnail(&self, token: &str) -> Result<State> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; self.ensure_active_creation_unlocked(service)?;
            Ok(self.furnishing_thumbnail_unlocked(service)?.1)
        })
    }
    pub fn save_native_furnishing_thumbnail(&self, token: &str, state: State) -> Result<State> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?; self.ensure_active_creation_unlocked(service)?;
            let (name, current) = self.furnishing_thumbnail_unlocked(service)?;
            let next = advance(&current, state)?;
            if next == current { return Ok(current); }
            write_json(&profile.root, &name, &next)?;
            File::open(&profile.root).and_then(|file| file.sync_all()).map_err(|_| err("STATE_UNAVAILABLE"))?;
            Ok(next)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State { format: 1, revision: 0, workspace_id: "10000000-0000-4000-8000-000000000001".into(),
            tenant_id: "10000000-0000-4000-8000-000000000002".into(),
            principal_id: "10000000-0000-4000-8000-000000000003".into(), pending: None }
    }
    fn request() -> Value {
        serde_json::json!({"version":1,"epoch":"10000000-0000-4000-8000-000000000004","sequence":1,
            "tenantId":state().tenant_id,"principalId":state().principal_id,"requestId":"10000000-0000-4000-8000-000000000005",
            "furnishingId":"10000000-0000-4000-8000-000000000006","furnishingVersion":1,"modelSha256":"a".repeat(64),
            "modelBytes":28,"recipeId":"furnishing-glb-card-256-v1","toolId":"exhibitos-three-0.186.1-thumbnail-v1"})
    }
    fn selection() -> Pending {
        let request = request();
        let source = serde_json::json!({"furnishingId":request["furnishingId"],"furnishingVersion":1,
            "modelSha256":request["modelSha256"],"modelBytes":request["modelBytes"],"descriptorSha256":"b".repeat(64),"rightsSha256":"c".repeat(64)});
        let png = serde_json::json!({"mime":"image/png","width":256,"height":256,"sha256":"d".repeat(64),"bytes":33});
        let candidate = serde_json::json!({"candidateId":"10000000-0000-8000-8000-000000000007","state":"ready","source":source,"thumbnail":png,
            "receiptSha256":"e".repeat(64),"receipt":{"version":1,"purpose":"registered-furnishing-thumbnail","profile":PROFILE,
                "workspaceId":state().workspace_id,"candidateId":"10000000-0000-8000-8000-000000000007",
                "requestId":request["requestId"],"source":source,"renderRequest":request,"png":png}});
        Pending::Selection { furnishing_id: "10000000-0000-4000-8000-000000000006".into(), furnishing_version: 1,
            request_id: "10000000-0000-4000-8000-000000000008".into(), candidate, expected_selection_version: 0 }
    }
    fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        output.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = output.len(); output.extend_from_slice(kind); output.extend_from_slice(data);
        output.extend_from_slice(&crc32(&output[start..]).to_be_bytes());
    }
    fn capture() -> Pending {
        // Synthetic chunk-policy fixture only; not rendered pixels or source authority.
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new(); header.extend_from_slice(&256u32.to_be_bytes()); header.extend_from_slice(&256u32.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &header); chunk(&mut png, b"IDAT", &[0]); chunk(&mut png, b"IEND", &[]);
        let capture = serde_json::json!({"workspaceId":state().workspace_id,"nativeRevision":9,"editVersion":8,
            "baseETag":format!("\"studio-r1-{}\"", "a".repeat(64)),"request":request(),
            "png":{"mime":"image/png","width":256,"height":256,"bytes":png.len(),"sha256":digest(&png)}});
        Pending::Capture { furnishing_id: "10000000-0000-4000-8000-000000000006".into(), furnishing_version: 1,
            request_id: "10000000-0000-4000-8000-000000000005".into(), capture, png_base64: STANDARD.encode(png) }
    }
    #[test]
    fn thumbnail_journal_keeps_selection_nonce_separate_from_render_nonce_and_candidate_hash() {
        let mut value = state(); value.pending = Some(selection()); validate(&value).unwrap();
        let encoded = serde_json::to_vec(&value).unwrap();
        let reopened: State = serde_json::from_slice(&encoded).unwrap(); assert!(reopened == value);
        if let Some(Pending::Selection { request_id, candidate, .. }) = &reopened.pending {
            assert_ne!(request_id.as_str(), candidate["receipt"]["requestId"].as_str().unwrap());
            assert_eq!(candidate["receiptSha256"], "e".repeat(64));
        } else { panic!("selection intent lost"); }
    }
    #[test]
    fn thumbnail_journal_cas_refuses_replacement_clear_and_resurrection_but_replays_exact_intent() {
        let empty = state(); let mut proposed = empty.clone(); proposed.pending = Some(selection());
        let saved = advance(&empty, proposed.clone()).unwrap(); assert_eq!(saved.revision, 1);
        assert!(advance(&saved, proposed).unwrap() == saved);
        let mut replacement = saved.clone();
        if let Some(Pending::Selection { request_id, .. }) = &mut replacement.pending { *request_id = Uuid::new_v4().to_string(); }
        assert!(advance(&saved, replacement).is_err());
        let mut clear = saved.clone(); clear.pending = None; clear.revision = 0; assert!(advance(&saved, clear.clone()).is_err());
        clear.revision = 1; let cleared = advance(&saved, clear).unwrap(); assert_eq!(cleared.revision, 2);
        assert!(advance(&cleared, saved).is_err());
        let mut foreign = empty.clone(); foreign.workspace_id = Uuid::new_v4().to_string();
        assert_eq!(advance(&empty, foreign).err().unwrap().code, SCOPE);
    }
    #[test]
    fn thumbnail_journal_capture_checks_exact_png_metadata_bytes_scope_and_no_inline_copy() {
        let mut value = state(); value.pending = Some(capture()); validate(&value).unwrap();
        let original = value.clone();
        if let Some(Pending::Capture { png_base64, .. }) = &mut value.pending { png_base64.push('='); }
        assert!(validate(&value).is_err()); value = original.clone();
        if let Some(Pending::Capture { capture, .. }) = &mut value.pending { capture["png"]["base64"] = "not allowed".into(); }
        assert!(validate(&value).is_err()); value = original;
        if let Some(Pending::Capture { capture, .. }) = &mut value.pending { capture["request"]["tenantId"] = Uuid::new_v4().to_string().into(); }
        assert!(validate(&value).is_err());
    }
    #[test]
    fn thumbnail_journal_closed_shape_and_original_receipt_binding_reject_untrusted_metadata() {
        let mut value = state(); value.pending = Some(selection());
        if let Some(Pending::Selection { candidate, .. }) = &mut value.pending { candidate["receipt"]["source"]["modelSha256"] = "f".repeat(64).into(); }
        assert!(validate(&value).is_err());
        let mut raw = serde_json::to_value(state()).unwrap(); raw.as_object_mut().unwrap().remove("pending");
        assert!(serde_json::from_value::<State>(raw).is_err());
        let mut raw = serde_json::to_value(state()).unwrap(); raw["token"] = "forbidden".into();
        assert!(serde_json::from_value::<State>(raw).is_err());
    }
    #[test]
    fn thumbnail_journal_reopens_private_bytes_and_missing_read_does_not_materialize() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("exhibitos-thumbnail-journal-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap(); let name = "synthetic-thumbnail.json";
        let empty = state(); assert!(read_existing(&root, name, &empty).unwrap() == empty);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        let mut pending = empty.clone(); pending.pending = Some(selection());
        let saved = advance(&empty, pending).unwrap(); write_json(&root, name, &saved).unwrap();
        let before = fs::read(root.join(name)).unwrap(); let reopened = read_existing(&root, name, &empty).unwrap();
        assert!(reopened == saved); assert_eq!(fs::read(root.join(name)).unwrap(), before);
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_existing(&root, name, &empty).is_err()); assert_eq!(fs::read(root.join(name)).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
}
