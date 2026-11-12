// SPDX-License-Identifier: Apache-2.0
//! One durable, revision-checked import intent per selected personal workspace.
//! No source path, asset bytes, password, cookie or service token is stored here.
use super::*;
pub(super) const LIMIT: u64 = 128 * 1024;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_derivation: Option<ModelDerivation>,
    #[serde(default, deserialize_with = "ModelPreparation::deserialize_present", skip_serializing_if = "Option::is_none")]
    pub model_preparation: Option<ModelPreparation>,
    #[serde(
        default,
        deserialize_with = "CoarsePreparation::deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub coarse_preparation: Option<CoarsePreparation>,
    #[serde(
        default,
        deserialize_with = "ProductImagePreparation::deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub product_image_preparation: Option<ProductImagePreparation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hayes_original: Option<HayesOriginal>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductImagePreparation {
    version: u32,
    profile: String,
    artwork_revision: u32,
}
impl ProductImagePreparation {
    fn deserialize_present<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> std::result::Result<Option<Self>, D::Error> {
        Self::deserialize(decoder).map(Some)
    }

    fn valid(&self, intent: &Intent) -> bool {
        self.version == 1
            && match self.profile.as_str() {
                "product-image-pair-2048-512-v1" => intent.mime == "image/jpeg",
                "ordinary-image-preparation-v1" => matches!(intent.mime.as_str(), "image/jpeg" | "image/png"),
                _ => false,
            }
            && (1..i32::MAX as u32).contains(&self.artwork_revision)
            && intent.bytes >= 4
            && intent.scale_meters == 1.0
            && intent.derived.is_none()
            && intent.model_derivation.is_none()
            && intent.model_preparation.is_none()
            && intent.coarse_preparation.is_none()
            && intent.hayes_original.is_none()
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HayesOriginal {
    pub artwork_revision: u32,
    pub display_reference_sha256: String,
}
impl HayesOriginal {
    fn valid(&self, intent: &Intent) -> bool {
        (1..=i32::MAX as u32).contains(&self.artwork_revision)
            && hash_valid(&self.display_reference_sha256)
            && intent.mime == "model/gltf-binary"
            && intent.bytes == 8_383_096
            && intent.sha256 == "e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6"
            && intent.scale_meters == 1.0
            && intent.derived.is_none()
            && intent.model_derivation.is_none()
            && intent.coarse_preparation.is_none()
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoarsePreparation {
    pub version: u32,
    pub profile: String,
    pub approval_id: String,
    pub artwork_revision: u32,
}
impl CoarsePreparation {
    fn deserialize_present<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> std::result::Result<Option<Self>, D::Error> {
        Self::deserialize(decoder).map(Some)
    }

    fn valid(&self, intent: &Intent) -> bool {
        let approval = Uuid::parse_str(&self.approval_id).is_ok_and(|id| {
            id.to_string().eq_ignore_ascii_case(&self.approval_id)
                && (1..=8).contains(&id.get_version_num())
                && id.get_variant() == uuid::Variant::RFC4122
        });
        self.version == 1
            && self.profile == "viewer-png-box-512-v1"
            && approval
            && (1..=i32::MAX as u32).contains(&self.artwork_revision)
            && intent.mime == "image/jpeg"
            && intent.scale_meters == 1.0
            && intent.model_derivation.is_none()
            && intent
                .derived
                .as_ref()
                .is_some_and(|derived| derived.recipe.colour_conversion.is_some())
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDerivation {
    pub version: u32,
    pub profile_id: String,
    pub settings_sha256: String,
    pub tool_version: String,
    #[serde(rename = "operationID")]
    pub operation_id: String,
    pub generation: String,
    pub artwork_revision: u32,
}
impl ModelDerivation {
    fn valid(&self, intent: &Intent) -> bool {
        self.version == 1
            && self.profile_id == "exhibitos-near-texture-pair-v1"
            && self.settings_sha256 == "da9039b67435883255f723fd3b37b0fc395ba2acce3ab26706aeaa17d90f73bf"
            && self.tool_version == "public-near-texture-v1"
            && self.operation_id == intent.idempotency_key
            && uuid(&self.operation_id)
            && uuid(&self.generation)
            && self.artwork_revision > 0
            && intent.mime == "model/gltf-binary"
            && intent.derived.is_none()
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelPreparation {
    pub version: u32,
    pub profile_id: String,
    #[serde(rename = "operationID")]
    pub operation_id: String,
    pub generation: String,
    pub artwork_revision: u32,
}
impl ModelPreparation {
    fn is_large(&self) -> bool {
        self.profile_id == "embedded-model-compact-large-20k-512-v2"
    }

    fn deserialize_present<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> std::result::Result<Option<Self>, D::Error> {
        Self::deserialize(decoder).map(Some)
    }

    fn valid(&self, intent: &Intent) -> bool {
        let generation = self.generation.as_bytes();
        self.version == 1
            && (self.profile_id == "embedded-model-compact-20k-512-v1"
                || (self.is_large() && (28..=64 * 1024 * 1024).contains(&intent.bytes)))
            && self.operation_id == intent.idempotency_key
            && generation.len() <= 128
            && generation.first().is_some_and(u8::is_ascii_alphanumeric)
            && generation.iter().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
            && (1..=i32::MAX as u32).contains(&self.artwork_revision)
            && intent.mime == "model/gltf-binary"
            && intent.derived.is_none()
            && intent.model_derivation.is_none()
            && intent.coarse_preparation.is_none()
            && intent.hayes_original.is_none()
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Derived {
    pub mime: String,
    pub bytes: u64,
    pub sha256: String,
    pub recipe: DerivedRecipe,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour_conversion: Option<ColourConversion>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColourConversion {
    pub version: u32,
    pub algorithm: String,
    pub source_icc_sha256: String,
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
            && recipe.colour_conversion.as_ref().is_none_or(|v|
                v.version == 1 && v.algorithm == "icc-to-srgb-jpeg-rgb8-v1"
                    && hash_valid(&v.source_icc_sha256) && recipe.colour_meaning == "srgb")
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
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactVariantTransfer {
    artwork_id: String,
    request: CompactVariantRequest,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactVariantRequest {
    revision: u32,
    asset_id: String,
    sha256: String,
    bytes: u64,
    idempotency_key: String,
    generation: String,
}
impl CompactVariantTransfer {
    fn valid(&self) -> bool {
        let request = &self.request;
        uuid(&self.artwork_id) && uuid(&request.asset_id) && uuid(&request.idempotency_key)
            && uuid(&request.generation) && hash_valid(&request.sha256)
            && (1..i32::MAX as u32).contains(&request.revision)
            && (28..=32 * 1024 * 1024).contains(&request.bytes)
    }
}
/// Local continuation identity only; a job ID never proves a ready candidate.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GpuVariantTransfer {
    pub version: u32,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub artwork_id: String,
    pub request: GpuVariantRequest,
    #[serde(default, deserialize_with = "deserialize_present_gpu_job", skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GpuVariantRequest {
    pub revision: u32,
    pub asset_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub idempotency_key: String,
    pub generation: String,
    pub profile: String,
}
fn deserialize_present_gpu_job<'de, D: serde::Deserializer<'de>>(decoder: D)
    -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(decoder).map(Some)
}
impl GpuVariantTransfer {
    fn deserialize_present<'de, D: serde::Deserializer<'de>>(decoder: D)
        -> std::result::Result<Option<Self>, D::Error> {
        Self::deserialize(decoder).map(Some)
    }
    fn valid(&self, outer: &Transfer) -> bool {
        let token = |value: &str| !value.is_empty() && value.len() <= 128
            && value.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
        let request = &self.request;
        self.version == 1 && self.workspace_id == outer.workspace_id
            && self.tenant_id == outer.tenant_id && self.principal_id == outer.principal_id
            && uuid(&self.artwork_id) && uuid(&request.asset_id)
            && self.job_id.as_ref().is_none_or(|id| uuid(id))
            && (1..=i32::MAX as u32).contains(&request.revision)
            && (28..=32 * 1024 * 1024).contains(&request.bytes) && hash_valid(&request.sha256)
            && token(&request.idempotency_key) && token(&request.generation)
            && request.profile == "embedded-model-basecolor-uastc-512-mips-v1"
            && outer.intent.is_none() && outer.job_id.is_none() && outer.file_name.is_none()
            && outer.compact_variant.is_none()
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_variant: Option<CompactVariantTransfer>,
    #[serde(default, deserialize_with = "GpuVariantTransfer::deserialize_present", skip_serializing_if = "Option::is_none")]
    pub gpu_variant: Option<GpuVariantTransfer>,
}
#[cfg(all(target_os = "macos", debug_assertions))]
impl Transfer {
    pub(super) fn matches_retained_product_image(&self, proof: &super::native_maintenance::RetainedProductImage) -> bool {
        let Some(intent) = &self.intent else { return false };
        let Some(preparation) = &intent.product_image_preparation else { return false };
        self.gpu_variant.is_none() && self.compact_variant.is_none()
            && self.job_id.as_deref() == Some(proof.job_id.as_str())
            && uuid(&proof.replacement_request_id)
            && hash_valid(&proof.receipt_sha256)
            && hash_valid(&proof.transfer_sha256)
            && intent.artwork_id == proof.artwork_id
            && intent.idempotency_key == proof.upload_idempotency_key
            && intent.sha256 == proof.source_sha256
            && intent.bytes == proof.source_bytes
            && preparation.profile == "product-image-pair-2048-512-v1"
            && preparation.artwork_revision == proof.artwork_revision
    }
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
        || (s.intent.is_none() && s.compact_variant.is_none() && (s.job_id.is_some() || s.file_name.is_some()))
        || s.gpu_variant.as_ref().is_some_and(|gpu| !gpu.valid(s))
        || s.compact_variant.as_ref().is_some_and(|compact|
            s.intent.is_some() || s.file_name.is_some() || !compact.valid())
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    if let Some(i) = &s.intent
        && (!uuid(&i.artwork_id)
            || !uuid(&i.idempotency_key)
            || !match (i.mime.as_str(), &i.derived) {
                ("model/gltf-binary" | "image/png", None) => true,
                ("image/jpeg", Some(derived)) => derived.valid(),
                ("image/jpeg", None) => i.product_image_preparation.is_some(),
                _ => false,
            }
            || !hash_valid(&i.sha256)
            || i.model_derivation.as_ref().is_some_and(|model| !model.valid(i))
            || i.model_preparation.as_ref().is_some_and(|model| !model.valid(i))
            || i.coarse_preparation
                .as_ref()
                .is_some_and(|coarse| !coarse.valid(i))
            || i.product_image_preparation
                .as_ref()
                .is_some_and(|image| !image.valid(i))
            || i.hayes_original.as_ref().is_some_and(|hayes| !hayes.valid(i))
            || i.bytes == 0
            || (i.bytes > 32 * 1024 * 1024
                && !i.model_preparation.as_ref().is_some_and(ModelPreparation::is_large))
            || !i.scale_meters.is_finite()
            || i.scale_meters <= 0.0
            || i.scale_meters > 1_000_000.0
            || !i.rights.is_object())
    {
        return Err(err("NATIVE_UPLOAD_STATE_INVALID"));
    }
    Ok(())
}
fn upload_transition_matches(current: &Transfer, next: &Transfer) -> bool {
    if current.revision != next.revision { return false; }
    let empty = |state: &Transfer| state.intent.is_none() && state.job_id.is_none()
        && state.file_name.is_none() && state.compact_variant.is_none();
    match (&current.gpu_variant, &next.gpu_variant) {
        (None, None) => true,
        (None, Some(gpu)) => empty(current) && gpu.job_id.is_none(),
        (Some(_), None) => empty(next),
        (Some(prior), Some(after)) => {
            let mut bound = prior.clone();
            if bound.job_id.is_none() { bound.job_id = after.job_id.clone(); }
            bound == *after
        }
    }
}

impl InstallationController {
    pub(super) fn upload_unlocked(&self, service: &LifecycleService) -> Result<(String, Transfer)> {
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
            compact_variant: None,
            gpu_variant: None,
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
            if !upload_transition_matches(&current, &state) {
                return Err(err("NATIVE_UPLOAD_STATE_CHANGED"));
            }
            let compact_matches = match (&current.compact_variant, &state.compact_variant) {
                (Some(prior), Some(next)) => prior == next
                    && (current.job_id.is_none() || current.job_id == state.job_id),
                (None, Some(_)) => current.intent.is_none() && state.job_id.is_none(),
                (Some(_), None) => state.intent.is_none() && state.job_id.is_none(),
                (None, None) => true,
            };
            if !compact_matches {
                return Err(err("NATIVE_UPLOAD_STATE_CHANGED"));
            }
            let product_image_bound = [&current.intent, &state.intent].into_iter().any(|intent| {
                intent.as_ref().is_some_and(|intent| intent.product_image_preparation.is_some())
            });
            if let Some(next) = &state.intent
                && (product_image_bound || next.coarse_preparation.is_some() || next.hayes_original.is_some()
                    || current.intent.as_ref().is_some_and(|prior|
                        prior.coarse_preparation.is_some() || prior.hayes_original.is_some()))
            {
                let prior = current
                    .intent
                    .as_ref()
                    .filter(|prior| prior.idempotency_key == next.idempotency_key);
                // Persist a new intent before recording its first server job.
                // Retrying then keeps both the exact intent and any bound job.
                let matches = match prior {
                    None => state.job_id.is_none() && (!product_image_bound || current.intent.is_none()),
                    Some(prior) => prior == next
                        && (current.job_id.is_none() || current.job_id == state.job_id),
                };
                if !matches {
                    return Err(err("NATIVE_UPLOAD_STATE_CHANGED"));
                }
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
mod model_derivation_tests {
    use super::*;

    #[test]
    fn compact_variant_round_trips_without_an_upload_intent() {
        let mut value = serde_json::json!({
            "format":1,"revision":1,"workspaceId":Uuid::new_v4().to_string(),
            "tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
            "intent":null,"jobId":Uuid::new_v4().to_string(),"fileName":null,
            "compactVariant":{"artworkId":Uuid::new_v4().to_string(),"request":{
                "revision":1,"assetId":Uuid::new_v4().to_string(),"sha256":"a".repeat(64),
                "bytes":4096,"idempotencyKey":Uuid::new_v4().to_string(),"generation":Uuid::new_v4().to_string()}}
        });
        let state: Transfer = serde_json::from_value(value.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), value);

        let mut mixed = value.clone();
        mixed["fileName"] = serde_json::json!("file.glb");
        assert!(validate(&serde_json::from_value(mixed).unwrap()).is_err());

        let mut corrupt = value.clone();
        corrupt["compactVariant"]["request"]["sha256"] = serde_json::json!("invalid");
        assert!(validate(&serde_json::from_value(corrupt).unwrap()).is_err());

        value.as_object_mut().unwrap().remove("compactVariant");
        assert!(validate(&serde_json::from_value(value).unwrap()).is_err());
    }

    #[test]
    fn durable_required_model_intent_is_separate_and_restart_safe() {
        let operation = Uuid::new_v4().to_string();
        let value = serde_json::json!({
            "format":1,"revision":1,"workspaceId":Uuid::new_v4().to_string(),
            "tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
            "jobId":null,"fileName":"synthetic.glb",
            "intent":{"artworkId":Uuid::new_v4().to_string(),"idempotencyKey":operation,
                "mime":"model/gltf-binary","sha256":"a".repeat(64),"bytes":1000,"scaleMeters":1,"rights":{},
                "modelPreparation":{"version":1,"profileId":"embedded-model-compact-20k-512-v1",
                    "operationID":operation,"generation":"durable-generation.1","artworkRevision":1}}
        });
        let state: Transfer = serde_json::from_value(value.clone()).unwrap();
        validate(&state).unwrap();
        let reopened: Transfer = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        validate(&reopened).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), serde_json::to_value(&reopened).unwrap());
        for (field, bad) in [("operationID", serde_json::json!("other")),
            ("generation", serde_json::json!("\nother")), ("artworkRevision", serde_json::json!(0))] {
            let mut changed = value.clone();
            changed["intent"]["modelPreparation"][field] = bad;
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        }
        let mut large = value.clone();
        large["intent"]["bytes"] = serde_json::json!(58_293_704);
        assert!(validate(&serde_json::from_value(large.clone()).unwrap()).is_err());
        large["intent"]["modelPreparation"]["profileId"] =
            serde_json::json!("embedded-model-compact-large-20k-512-v2");
        assert!(validate(&serde_json::from_value(large.clone()).unwrap()).is_ok());
        large["intent"]["bytes"] = serde_json::json!(64 * 1024 * 1024 + 1);
        assert!(validate(&serde_json::from_value(large).unwrap()).is_err());
        let mut null = value;
        null["intent"]["modelPreparation"] = Value::Null;
        assert!(serde_json::from_value::<Transfer>(null).is_err());
    }

    #[test]
    fn durable_optional_model_binding_keeps_legacy_and_jpeg_closed() {
        let operation = Uuid::new_v4().to_string();
        let mut value = serde_json::json!({
            "format":1,"revision":1,"workspaceId":Uuid::new_v4().to_string(),
            "tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
            "jobId":null,"fileName":"synthetic.glb",
            "intent":{"artworkId":Uuid::new_v4().to_string(),"idempotencyKey":operation,
                "mime":"model/gltf-binary","sha256":"a".repeat(64),"bytes":1000,"scaleMeters":1,"rights":{}}
        });
        let legacy: Transfer = serde_json::from_value(value.clone()).unwrap();
        validate(&legacy).unwrap();
        assert!(serde_json::to_value(&legacy).unwrap()["intent"].get("modelDerivation").is_none());
        value["intent"]["modelDerivation"] = serde_json::json!({
            "version":1,"profileId":"exhibitos-near-texture-pair-v1",
            "settingsSha256":"da9039b67435883255f723fd3b37b0fc395ba2acce3ab26706aeaa17d90f73bf",
            "toolVersion":"public-near-texture-v1","operationID":operation,
            "generation":Uuid::new_v4().to_string(),"artworkRevision":1
        });
        let state: Transfer = serde_json::from_value(value.clone()).unwrap();
        validate(&state).unwrap();
        let reopened: Transfer = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        validate(&reopened).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), serde_json::to_value(&reopened).unwrap());
        for (field, bad) in [
            ("operationID", serde_json::json!(Uuid::new_v4().to_string())),
            ("generation", serde_json::json!("unbound")),
            ("artworkRevision", serde_json::json!(0)),
            ("settingsSha256", serde_json::json!("b".repeat(64))),
            ("toolVersion", serde_json::json!("private-MacOutput")),
        ] {
            let mut changed = value.clone();
            changed["intent"]["modelDerivation"][field] = bad;
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        }
        let mut unknown = value.clone();
        unknown["intent"]["modelDerivation"]["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Transfer>(unknown).is_err());
        for mime in ["image/png", "image/jpeg"] {
            let mut changed = value.clone();
            changed["intent"]["mime"] = serde_json::json!(mime);
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"format":1,"revision":0,"workspaceId":Uuid::new_v4().to_string(),"tenantId":Uuid::new_v4().to_string(),"principalId":Uuid::new_v4().to_string(),
            "intent":{"artworkId":Uuid::new_v4().to_string(),"idempotencyKey":Uuid::new_v4().to_string(),"mime":"image/png","sha256":"a".repeat(64),"bytes":100,"scaleMeters":1.0,"rights":{}},"jobId":null,"fileName":"synthetic.png"})
    }
    fn derived() -> Value {
        let canonical = br#"{"colourMeaning":"srgb-assumed","role":"baseColor","sampling":"nearest-centre","version":1}"#;
        serde_json::json!({"mime":"image/png","bytes":200,"sha256":"b".repeat(64),"recipe":{"version":1,"role":"baseColor","sampling":"nearest-centre","colourMeaning":"srgb-assumed","recipeSha256":digest(canonical),"settingsSha256":"c".repeat(64),"sourcePixelsSha256":"d".repeat(64),"outputPixelsSha256":"e".repeat(64),"toolVersion":"ExhibitOSManager.ImageIO/1/macOS-27.0.1"}})
    }

    fn icc_derived() -> Value {
        let canonical = br#"{"colourMeaning":"srgb","role":"baseColor","sampling":"nearest-centre","version":1}"#;
        let mut value = derived();
        value["recipe"]["colourMeaning"] = serde_json::json!("srgb");
        value["recipe"]["recipeSha256"] = serde_json::json!(digest(canonical));
        value["recipe"]["colourConversion"] = serde_json::json!({
            "version": 1,
            "algorithm": "icc-to-srgb-jpeg-rgb8-v1",
            "sourceIccSha256": "f".repeat(64)
        });
        value
    }

    fn coarse_fixture() -> Value {
        let mut input = fixture();
        input["fileName"] = serde_json::json!("synthetic.jpg");
        input["intent"]["mime"] = serde_json::json!("image/jpeg");
        input["intent"]["derived"] = icc_derived();
        input["intent"]["coarsePreparation"] = serde_json::json!({
            "version": 1,
            "profile": "viewer-png-box-512-v1",
            "approvalId": Uuid::new_v4().to_string(),
            "artworkRevision": 1
        });
        input
    }

    fn product_image_fixture() -> Value {
        let mut input = fixture();
        input["fileName"] = serde_json::json!("synthetic.jpg");
        input["intent"]["mime"] = serde_json::json!("image/jpeg");
        input["intent"]["productImagePreparation"] = serde_json::json!({
            "version": 1,
            "profile": "product-image-pair-2048-512-v1",
            "artworkRevision": 1
        });
        input
    }

    #[test]
    fn product_image_intent_only_accepts_an_original_jpeg() {
        let input = product_image_fixture();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), input);
        for (pointer, value) in [
            ("/intent/productImagePreparation/version", serde_json::json!(2)),
            ("/intent/productImagePreparation/profile", serde_json::json!("other")),
            ("/intent/productImagePreparation/artworkRevision", serde_json::json!(0)),
            ("/intent/productImagePreparation/artworkRevision", serde_json::json!(2_147_483_647)),
            ("/intent/mime", serde_json::json!("image/png")),
            ("/intent/bytes", serde_json::json!(3)),
            ("/intent/bytes", serde_json::json!(32 * 1024 * 1024 + 1)),
            ("/intent/scaleMeters", serde_json::json!(2.0)),
        ] {
            let mut changed = input.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err(), "{pointer}");
        }
        let mut mixed = coarse_fixture();
        mixed["intent"]["productImagePreparation"] = input["intent"]["productImagePreparation"].clone();
        assert!(validate(&serde_json::from_value(mixed).unwrap()).is_err());
        let mut changed = input.clone();
        changed["intent"]["productImagePreparation"]["approvalId"] = serde_json::json!(Uuid::new_v4());
        assert!(serde_json::from_value::<Transfer>(changed).is_err());
        let mut changed = input;
        changed["intent"]["productImagePreparation"] = Value::Null;
        assert!(serde_json::from_value::<Transfer>(changed).is_err());
    }

    #[test]
    fn coarse_intent_is_closed_and_requires_an_icc_jpeg() {
        let input = coarse_fixture();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), input);
        for (pointer, value) in [
            ("/intent/coarsePreparation/version", serde_json::json!(2)),
            ("/intent/coarsePreparation/profile", serde_json::json!("other")),
            ("/intent/coarsePreparation/approvalId", serde_json::json!("invalid")),
            ("/intent/coarsePreparation/approvalId", serde_json::json!(Uuid::nil())),
            ("/intent/coarsePreparation/artworkRevision", serde_json::json!(0)),
            ("/intent/coarsePreparation/artworkRevision", serde_json::json!(2_147_483_648_u32)),
            ("/intent/mime", serde_json::json!("image/png")),
            ("/intent/derived", derived()),
            ("/intent/scaleMeters", serde_json::json!(2.0)),
        ] {
            let mut changed = input.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate(&serde_json::from_value(changed).unwrap()).is_err(),
                "{pointer}"
            );
        }
        for invalid in [Value::Null, serde_json::json!({"version": 1})] {
            let mut changed = input.clone();
            changed["intent"]["coarsePreparation"] = invalid;
            assert!(serde_json::from_value::<Transfer>(changed).is_err());
        }
        let mut changed = input;
        changed["intent"]["coarsePreparation"]["assetId"] = serde_json::json!(Uuid::new_v4());
        assert!(serde_json::from_value::<Transfer>(changed).is_err());
    }

    #[test]
    fn coarse_intent_and_job_binding_survive_controller_restart() {
        assert_image_intent_survives_restart(coarse_fixture(), "coarsePreparation");
    }

    #[test]
    fn product_image_intent_and_job_binding_survive_controller_restart() {
        assert_image_intent_survives_restart(product_image_fixture(), "productImagePreparation");
    }

    #[test]
    fn fresh_png_and_job_binding_survive_controller_restart() {
        let mut input = product_image_fixture();
        input["fileName"] = serde_json::json!("synthetic.png");
        input["intent"]["mime"] = serde_json::json!("image/png");
        input["intent"]["productImagePreparation"]["profile"] =
            serde_json::json!("ordinary-image-preparation-v1");
        assert_image_intent_survives_restart(input, "productImagePreparation");
    }

    fn assert_image_intent_survives_restart(input: Value, preparation_field: &str) {
        struct OwnedFixture(PathBuf);
        impl Drop for OwnedFixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("exhibitos-image-upload-{}", Uuid::new_v4()));
        new_directory(&parent).unwrap();
        let owned_fixture = OwnedFixture(parent);
        let profile = owned_fixture.0.join("profile");
        let mut state: Transfer = serde_json::from_value(input).unwrap();
        let controller = InstallationController::new(profile.clone(), None).unwrap();
        let token = controller.context().unwrap().selection_token;
        controller
            .with_current(&token, |service| {
                let owner = serde_json::json!({
                    "format": 1, "workspaceId": state.workspace_id, "tenantId": state.tenant_id,
                    "principalId": state.principal_id, "ownerUid": unsafe { libc::geteuid() },
                    "ports": {"web": 13200, "api": 13201, "database": 15432},
                    "manifestSha256": "a".repeat(64), "environmentSha256": "b".repeat(64)
                });
                write_private_new(
                    &service.root,
                    "native-personal.json",
                    &serde_json::to_vec(&owner).unwrap(),
                    4096,
                )
            })
            .unwrap();
        let mut prebound = state.clone();
        prebound.job_id = Some(Uuid::new_v4().to_string());
        assert_eq!(
            controller.save_native_upload(&token, prebound).err().unwrap().code,
            "NATIVE_UPLOAD_STATE_CHANGED"
        );
        state = controller.save_native_upload(&token, state).unwrap();
        let intent_before_job = serde_json::to_value(&state).unwrap()["intent"].clone();
        state.job_id = Some(Uuid::new_v4().to_string());
        state = controller.save_native_upload(&token, state).unwrap();
        let persisted = serde_json::to_value(&state).unwrap();
        assert_eq!(persisted["intent"], intent_before_job);
        drop(controller);

        let reopened = InstallationController::open_existing_managed(profile).unwrap();
        let token = reopened.context().unwrap().selection_token;
        let resumed = reopened.native_upload(&token).unwrap();
        assert_eq!(serde_json::to_value(&resumed).unwrap(), persisted);
        let mut changed = persisted.clone();
        changed["intent"][preparation_field]["artworkRevision"] = serde_json::json!(2);
        assert_eq!(
            reopened
                .save_native_upload(&token, serde_json::from_value(changed).unwrap())
                .err().unwrap().code,
            "NATIVE_UPLOAD_STATE_CHANGED"
        );
        let mut changed = resumed.clone();
        changed.intent.as_mut().unwrap().idempotency_key = Uuid::new_v4().to_string();
        assert_eq!(
            reopened.save_native_upload(&token, changed).err().unwrap().code,
            "NATIVE_UPLOAD_STATE_CHANGED"
        );
        if preparation_field == "productImagePreparation" {
            let mut changed = resumed.clone();
            changed.intent = serde_json::from_value::<Transfer>(fixture()).unwrap().intent;
            changed.job_id = None;
            assert_eq!(
                reopened.save_native_upload(&token, changed).err().unwrap().code,
                "NATIVE_UPLOAD_STATE_CHANGED"
            );
        }
        let mut changed = resumed.clone();
        changed.job_id = Some(Uuid::new_v4().to_string());
        assert_eq!(
            reopened.save_native_upload(&token, changed).err().unwrap().code,
            "NATIVE_UPLOAD_STATE_CHANGED"
        );
        assert_eq!(
            serde_json::to_value(reopened.native_upload(&token).unwrap()).unwrap(),
            persisted
        );
        let saved = reopened.save_native_upload(&token, resumed).unwrap();
        assert_eq!(serde_json::to_value(saved).unwrap()["intent"], intent_before_job);
    }

    #[test]
    fn legacy_original_intent_has_no_new_field_and_retains_validation() {
        let input = fixture();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), input);
    }
    #[test]
    fn icc_recipe_is_optional_closed_and_roundtrips_with_original_intent() {
        let mut input = fixture();
        input["intent"]["mime"] = serde_json::json!("image/jpeg");
        input["intent"]["derived"] = derived();
        let old: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&old).unwrap();
        assert_eq!(serde_json::to_value(&old).unwrap(), input);
        input["intent"]["derived"] = icc_derived();
        let state: Transfer = serde_json::from_value(input.clone()).unwrap();
        validate(&state).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), input);
        for (field, bad) in [("version", serde_json::json!(2)), ("algorithm", serde_json::json!("unknown")), ("sourceIccSha256", serde_json::json!("F".repeat(64)))] {
            let mut changed = input.clone();
            changed["intent"]["derived"]["recipe"]["colourConversion"][field] = bad;
            assert!(validate(&serde_json::from_value(changed).unwrap()).is_err());
        }
        let mut unknown = input.clone();
        unknown["intent"]["derived"]["recipe"]["colourConversion"]["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Transfer>(unknown).is_err());
        let mut wrong_meaning = input.clone();
        wrong_meaning["intent"]["derived"]["recipe"]["colourMeaning"] = serde_json::json!("srgb-assumed");
        wrong_meaning["intent"]["derived"]["recipe"]["recipeSha256"] = derived()["recipe"]["recipeSha256"].clone();
        assert!(validate(&serde_json::from_value(wrong_meaning).unwrap()).is_err());
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

#[cfg(test)]
mod gpu_variant_tests {
    use super::*;
    fn state() -> Transfer {
        let workspace = Uuid::new_v4().to_string();
        let tenant = Uuid::new_v4().to_string();
        let principal = Uuid::new_v4().to_string();
        serde_json::from_value(serde_json::json!({
            "format":1,"revision":7,"workspaceId":workspace,"tenantId":tenant,"principalId":principal,
            "intent":null,"jobId":null,"fileName":null,
            "gpuVariant":{"version":1,"workspaceId":workspace,"tenantId":tenant,"principalId":principal,
                "artworkId":Uuid::new_v4().to_string(),"request":{"revision":2,
                "assetId":Uuid::new_v4().to_string(),"sha256":"a".repeat(64),"bytes":1829056,
                "idempotencyKey":"same-create-operation","generation":"same-original-generation",
                "profile":"embedded-model-basecolor-uastc-512-mips-v1"}}
        })).unwrap()
    }
    #[test]
    fn gpu_pending_reopens_exact_original_request_without_minting_ready_authority() {
        let pending = state(); validate(&pending).unwrap();
        let bytes = serde_json::to_vec(&pending).unwrap();
        let reopened: Transfer = serde_json::from_slice(&bytes).unwrap();
        validate(&reopened).unwrap();
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
        assert!(reopened.gpu_variant.as_ref().unwrap().job_id.is_none());
        assert_eq!(reopened.gpu_variant.as_ref().unwrap().request.generation, "same-original-generation");
        let mut legacy = serde_json::to_value(&pending).unwrap();
        legacy.as_object_mut().unwrap().remove("gpuVariant");
        let old: Transfer = serde_json::from_value(legacy.clone()).unwrap(); validate(&old).unwrap();
        assert_eq!(serde_json::to_value(old).unwrap(), legacy);
    }
    #[test]
    fn gpu_pending_rejects_foreign_scope_open_fields_nulls_and_mixed_writers() {
        let good = serde_json::to_value(state()).unwrap();
        for field in ["workspaceId", "tenantId", "principalId"] {
            let mut wrong = good.clone(); wrong["gpuVariant"][field] = Uuid::new_v4().to_string().into();
            assert!(validate(&serde_json::from_value(wrong).unwrap()).is_err());
        }
        let mut mixed = good.clone(); mixed["fileName"] = "source.glb".into();
        assert!(validate(&serde_json::from_value(mixed).unwrap()).is_err());
        let mut wrong=good.clone();wrong["gpuVariant"]=Value::Null;
        assert!(serde_json::from_value::<Transfer>(wrong).is_err());
        let mut wrong=good.clone();wrong["gpuVariant"]["jobId"]=Value::Null;
        assert!(serde_json::from_value::<Transfer>(wrong).is_err());
        let mut wrong=good.clone();wrong["gpuVariant"]["request"]["path"]="/untrusted".into();
        assert!(serde_json::from_value::<Transfer>(wrong).is_err());
        let mut wrong=good;wrong["gpuVariant"]["request"]["profile"]="different".into();
        assert!(validate(&serde_json::from_value(wrong).unwrap()).is_err());
    }
    #[test]
    fn gpu_cas_binds_one_job_and_refuses_replacement_or_unknown_nonce_changes() {
        let prior = state();
        let mut bound = prior.clone();
        bound.gpu_variant.as_mut().unwrap().job_id=Some(Uuid::new_v4().to_string());
        assert!(upload_transition_matches(&prior, &bound));
        assert!(upload_transition_matches(&bound, &bound));
        let mut stale=bound.clone();stale.revision-=1;
        assert!(!upload_transition_matches(&bound, &stale));
        let mut replacement=bound.clone();replacement.gpu_variant.as_mut().unwrap().request.generation="new-nonce".into();
        assert!(!upload_transition_matches(&bound, &replacement));
        let mut replacement=bound.clone();replacement.gpu_variant.as_mut().unwrap().job_id=Some(Uuid::new_v4().to_string());
        assert!(!upload_transition_matches(&bound, &replacement));
        let mut unbound=bound.clone();unbound.gpu_variant.as_mut().unwrap().job_id=None;
        assert!(!upload_transition_matches(&bound, &unbound));
        let mut cleared=bound.clone();cleared.gpu_variant=None;
        assert!(upload_transition_matches(&bound, &cleared));
        assert!(upload_transition_matches(&cleared, &prior));
        assert!(!upload_transition_matches(&cleared, &bound));
    }
}
