// SPDX-License-Identifier: Apache-2.0
//! One durable, revision-checked import intent per selected personal workspace.
//! No source path, asset bytes, password, cookie or service token is stored here.
use super::*;
const LIMIT: u64 = 128 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent {
    pub artwork_id: String,
    pub idempotency_key: String,
    pub mime: String,
    pub sha256: String,
    pub bytes: u64,
    pub scale_meters: f64,
    pub rights: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<Derived>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Derived {
    pub mime: String,
    pub bytes: u64,
    pub sha256: String,
    pub recipe: DerivedRecipe,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedRecipe {
    pub version: u32,
    pub role: String,
    pub sampling: String,
    pub colour_meaning: String,
    pub recipe_sha256: String,
    pub settings_sha256: String,
    pub source_pixels_sha256: String,
    pub output_pixels_sha256: String,
    pub tool_version: String,
}
impl Derived {
    fn valid(&self) -> bool {
        let recipe = &self.recipe;
        let canonical = std::collections::BTreeMap::from([
            ("version", serde_json::json!(1)),
            ("role", serde_json::json!("baseColor")),
            ("sampling", serde_json::json!("nearest-centre")),
            ("colourMeaning", serde_json::json!(recipe.colour_meaning)),
        ]);
        self.mime == "image/png"
            && self.bytes > 0
            && self.bytes <= 8_388_608
            && hash_valid(&self.sha256)
            && recipe.version == 1
            && recipe.role == "baseColor"
            && recipe.sampling == "nearest-centre"
            && matches!(recipe.colour_meaning.as_str(), "srgb" | "srgb-assumed")
            && [
                &recipe.recipe_sha256,
                &recipe.settings_sha256,
                &recipe.source_pixels_sha256,
                &recipe.output_pixels_sha256,
            ]
            .into_iter()
            .all(|v| hash_valid(v))
            && serde_json::to_vec(&canonical)
                .is_ok_and(|bytes| digest(&bytes) == recipe.recipe_sha256)
            && !recipe.tool_version.is_empty()
            && recipe.tool_version.len() <= 128
            && recipe.tool_version.bytes().all(|b| (32..=126).contains(&b))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transfer {
    pub format: u32,
    pub revision: u64,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub intent: Option<Intent>,
    pub job_id: Option<String>,
    pub file_name: Option<String>,
}
pub(crate) fn validate(s: &Transfer) -> Result<()> {
    if s.format != 1
        || !uuid(&s.workspace_id)
        || !uuid(&s.tenant_id)
        || !uuid(&s.principal_id)
        || s.job_id.as_ref().is_some_and(|id| !uuid(id))
        || s.file_name.as_ref().is_some_and(|name| {
            name.is_empty()
                || name.len() > 1024
                || name.chars().any(char::is_control)
                || name.contains('/')
                || name.contains('\\')
        })
        || (s.intent.is_none() && (s.job_id.is_some() || s.file_name.is_some()))
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    if let Some(i) = &s.intent
        && (!uuid(&i.artwork_id)
            || !uuid(&i.idempotency_key)
            || !match (i.mime.as_str(), &i.derived) {
                ("model/gltf-binary" | "image/png", None) => true,
                ("image/jpeg", Some(derived)) => derived.valid(),
                _ => false,
            }
            || !hash_valid(&i.sha256)
            || i.bytes == 0
            || i.bytes > 32 * 1024 * 1024
            || !i.scale_meters.is_finite()
            || i.scale_meters <= 0.0
            || i.scale_meters > 1_000_000.0
            || !i.rights.is_object())
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    Ok(())
}
impl InstallationController {
    fn upload_unlocked(&self, service: &LifecycleService) -> Result<(String, Transfer)> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
        let (workspace_id, tenant_id, principal_id) = service.personal_identity()?;
        let file = format!(
            "native-upload-{}.json",
            digest(service.root.as_os_str().as_encoded_bytes())
        );
        let default = Transfer {
            format: 1,
            revision: 0,
            workspace_id,
            tenant_id,
            principal_id,
            intent: None,
            job_id: None,
            file_name: None,
        };
        let saved = match fs::symlink_metadata(profile.root.join(&file)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default.clone(),
            Ok(_) => serde_json::from_slice::<Transfer>(&installation_backup::source_bytes(
                &profile.root,
                &file,
                LIMIT,
                true,
            )?)
            .map_err(|_| err("NATIVE_UPLOAD_STATE_INVALID"))?,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        validate(&saved)?;
        if saved.workspace_id != default.workspace_id
            || saved.tenant_id != default.tenant_id
            || saved.principal_id != default.principal_id
        {
            return Err(err("NATIVE_UPLOAD_SCOPE_CHANGED"));
        }
        Ok((file, saved))
    }
    pub fn native_upload(&self, token: &str) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            Ok(self.upload_unlocked(service)?.1)
        })
    }
    pub fn save_native_upload(&self, token: &str, mut state: Transfer) -> Result<Transfer> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            let _lock = profile_lock(profile)?;
            let (file, current) = self.upload_unlocked(service)?;
            validate(&state)?;
            if state.workspace_id != current.workspace_id
                || state.tenant_id != current.tenant_id
                || state.principal_id != current.principal_id
            {
                return Err(err("NATIVE_UPLOAD_SCOPE_CHANGED"));
            }
            if state.revision != current.revision {
                return Err(err("NATIVE_UPLOAD_STATE_CHANGED"));
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| err("NATIVE_UPLOAD_STATE_INVALID"))?;
            let bytes = serde_json::to_vec(&state).map_err(|_| err("STATE_INVALID"))?;
            if bytes.len() as u64 > LIMIT {
                return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
            }
            write_json(&profile.root, &file, &state)?;
            File::open(&profile.root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
            Ok(state)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"format":1,"revision":0,"workspaceId":Uuid::new_v4().to_string(),"tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
            "intent":{"artworkId":Uuid::new_v4().to_string(),"idempotencyKey":Uuid::new_v4().to_string(),"mime":"image/png","sha256":"a".repeat(64),"bytes":100,"scaleMeters":1,"rights":{}},"jobId":null,"fileName":"synthetic.png"})
    }
    fn derived() -> Value {
        let canonical = br#"{"colourMeaning":"srgb-assumed","role":"baseColor","sampling":"nearest-centre","version":1}"#;
        serde_json::json!({"mime":"image/png","bytes":200,"sha256":"b".repeat(64),"recipe":{"version":1,"role":"baseColor","sampling":"nearest-centre","colourMeaning":"srgb-assumed","recipeSha256":digest(canonical),"settingsSha256":"c".repeat(64),"sourcePixelsSha256":"d".repeat(64),"outputPixelsSha256":"e".repeat(64),"toolVersion":"ExhibitOSManager.ImageIO/1/macOS-27.0.1"}})
    }
    #[test]
    fn legacy_original_intent_has_no_new_field_and_retains_validation() {
        let input = fixture();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), input);
    }
    #[test]
    fn jpeg_intent_binds_distinct_original_and_derived_hashes() {
        let mut input = fixture();
        input["intent"]["mime"] = serde_json::json!("image/jpeg");
        input["intent"]["derived"] = derived();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), input);
        for (path, value) in [
            ("mime", serde_json::json!("image/png")),
            ("derived", Value::Null),
        ] {
            let mut bad = input.clone();
            bad["intent"][path] = value;
            assert!(validate(&serde_json::from_value(bad).unwrap()).is_err());
        }
    }
    #[test]
    fn derived_recipe_is_closed_bounded_and_not_an_authority_marker() {
        let mut input = fixture();
        input["intent"]["mime"] = serde_json::json!("image/jpeg");
        input["intent"]["derived"] = derived();
        for (pointer, value) in [
            ("/intent/derived/bytes", serde_json::json!(8_388_609)),
            (
                "/intent/derived/recipe/recipeSha256",
                serde_json::json!("f".repeat(64)),
            ),
            (
                "/intent/derived/recipe/toolVersion",
                serde_json::json!("bad\nversion"),
            ),
            ("/intent/derived/recipe/role", serde_json::json!("normal")),
            ("/intent/derived/sha256", serde_json::json!("not-a-hash")),
        ] {
            let mut bad = input.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate(&serde_json::from_value(bad).unwrap()).is_err(),
                "{pointer}"
            );
        }
        input["intent"]["derived"]["recipe"]["serverCertified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Transfer>(input).is_err());
    }
}
