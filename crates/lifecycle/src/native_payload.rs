// SPDX-License-Identifier: Apache-2.0
//! App-contained macOS runtime admission. No installation, process launch or
//! credential read occurs here. UI mode cannot bypass payload verification.
use super::*;
use std::collections::BTreeSet;

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
#[allow(dead_code)] // Local candidate preflight. Worker activation remains gated.
#[path = "native_payload_hayes.rs"]
pub(crate) mod hayes;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    mode: String,
    platform: String,
    node_version: String,
    postgres_version: String,
    platform_version: String,
    protocol_version: String,
    source_commit: String,
    #[serde(rename = "producerSHA256")]
    producer_sha256: String,
    entry: Entries,
    files: Vec<Item>,
    redistribution_qualified: bool,
    official_signed: bool,
    remaining: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entries {
    node: String,
    script: String,
    postgres: String,
    initdb: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    path: String,
    bytes: Option<u64>,
    sha256: Option<String>,
    executable: Option<bool>,
    link: Option<String>,
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
pub(crate) enum StaticRefreshRuntimeFile {
    Regular { path: String, bytes: u64, executable: bool },
    Link { path: String, target: String },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FreezeBudgetDescriptor {
    schema_version: String,
    version: String,
    core_digest: String,
    files: Vec<FreezeBudgetFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FreezeBudgetFile {
    path: String,
    bytes: u64,
    sha256: String,
    mime: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub state: String,
    pub platform_version: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub redistribution_qualified: bool,
    pub official_signed: bool,
}
pub struct Payload {
    root: PathBuf,
    manifest: Manifest,
    manifest_sha256: String,
    workspace_v5_validation: bool,
    workspace_import: Option<(WorkspaceImport, bool)>,
}
const STATIC_DETAIL_MARKER: &str = "platform/scripts/native-detail-static-export-capability-v1.json";
const STATIC_DETAIL_MARKER_SHA256: &str = "9663e0fb346c78ebbd1b9e883bbd2400a1b968368d101102f0e0285c8e7deaef";
const STATIC_DETAIL_MODULES: &[&str] = &[
    "platform/apps/api/dist/native-detail-static-export.js",
    "platform/apps/api/dist/native-detail-http.js",
    "platform/apps/api/dist/native-detail-context.js",
    "platform/scripts/native-detail-capability.mjs",
    "platform/scripts/native-detail-pipe.mjs",
    "platform/scripts/native-detail-control.mjs",
    "platform/scripts/native-detail-export-runner.mjs",
    "platform/apps/web/src/static-json.mjs",
];

fn static_detail_inventory_matches(files: &[Item]) -> bool {
    let regular = |path| {
        files.iter().find(|item| {
            item.path == path && item.link.is_none() && item.executable == Some(false)
        })
    };
    regular(STATIC_DETAIL_MARKER).is_some_and(|marker| {
        marker.bytes == Some(177) && marker.sha256.as_deref() == Some(STATIC_DETAIL_MARKER_SHA256)
    }) && STATIC_DETAIL_MODULES.iter().all(|path| regular(*path).is_some())
}

#[cfg(test)]
#[test]
fn static_detail_requires_every_regular_nonexecutable_dependency() {
    let file = |path: &str| Item {
        path: path.into(),
        bytes: Some(1),
        sha256: Some("a".repeat(64)),
        executable: Some(false),
        link: None,
    };
    let mut files: Vec<_> = STATIC_DETAIL_MODULES.iter().map(|path| file(path)).collect();
    assert!(!static_detail_inventory_matches(&files));
    files.push(Item {
        bytes: Some(177),
        sha256: Some(STATIC_DETAIL_MARKER_SHA256.into()),
        ..file(STATIC_DETAIL_MARKER)
    });
    assert!(static_detail_inventory_matches(&files));
    for index in 0..STATIC_DETAIL_MODULES.len() {
        let dependency = files.remove(index);
        assert!(!static_detail_inventory_matches(&files));
        files.insert(index, dependency);
        files[index].link = Some("substitute".into());
        assert!(!static_detail_inventory_matches(&files));
        files[index].link = None;
        files[index].executable = Some(true);
        assert!(!static_detail_inventory_matches(&files));
        files[index].executable = Some(false);
    }
    assert!(static_detail_inventory_matches(&files));
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const WORKSPACE_V5_TRANSITION: (&str, &str) = (
    "40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415",
    "cfe1faba63dc6d41e2451ad8bf8870d197a48e1b6d234850bdbad16c23083b59",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const WORKSPACE_WALL_PREVIEW_TRANSITION: (&str, &str) = (
    WORKSPACE_V5_TRANSITION.1,
    "6be2d5969462e4961e9ecbd3b6417936f2f075de1fbc3626d1b4f19cde7e65ec",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const WORKSPACE_V5_SAVED_TRANSITIONS: &[(&str, &str)] =
    &[WORKSPACE_V5_TRANSITION, WORKSPACE_WALL_PREVIEW_TRANSITION,
        CATALOG40_FRAME_TRANSITION, CATALOG40_TANGENT_TRANSITION];

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V5_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[
    (
        "742412c152037c20a4c32e5ccce8535be46822aa",
        "d0dac02c2b8cc5344a1aef48042d64fdd5f6d61a26851ae6c27cee03bcef7364",
        WORKSPACE_V5_TRANSITION.0,
    ),
    (
        "af7f9a8f8ff2cc259e015c88680d202cc339ee47",
        "d0dac02c2b8cc5344a1aef48042d64fdd5f6d61a26851ae6c27cee03bcef7364",
        WORKSPACE_V5_TRANSITION.1,
    ),
    (
        "f4f6625110df63258a13407cbe45334e4372ab5e",
        "f57122ec4bfcbd8c943c101d8b8bba3a40a5152e1cd292d85416cfb633d2f7d0",
        WORKSPACE_WALL_PREVIEW_TRANSITION.1,
    ),
];
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V5_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-material-capability-v5.json",
    700,
    "8c5f7f44fb7b0a898611977064f95d4988eeff4945093c0df351f7b6baaf217a",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn workspace_v5_validation_matches(manifest: &Manifest, manifest_hash: &str, selected: bool) -> bool {
    let (path, bytes, sha) = WORKSPACE_V5_CAPABILITY;
    manifest.mode == "local-validation" && !manifest.redistribution_qualified && !manifest.official_signed
        && (!selected || manifest_hash != WORKSPACE_WALL_PREVIEW_TRANSITION.1)
        && WORKSPACE_V5_VALIDATION_PROVENANCE.contains(&(
            manifest.source_commit.as_str(), manifest.producer_sha256.as_str(), manifest_hash,
        ))
        && manifest.files.iter().any(|item| {
            item.path == path && item.link.is_none() && item.executable == Some(false)
                && item.bytes == Some(bytes) && item.sha256.as_deref() == Some(sha)
        })
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceImport {
    V6,
    V7,
}

pub(crate) const WORKSPACE_WALL_SCHEMA_NAME: &str = "035_workspace_material_wall_import.sql";
pub(crate) const WORKSPACE_WALL_SCHEMA_SHA256: &str =
    "c347183b3e58805c46d1dde7ae6e2defc37762af52f585846357181b9635a544";

const WORKSPACE_FURNISHING_CAPABILITY: &str = "platform/scripts/native-workspace-furnishing-capability-v1.json";
const WORKSPACE_FURNISHING_WRITER_DIRECTORY: &str = "platform/database/native-workspace-furnishing-writer/";
const WORKSPACE_FURNISHING_WRITER_SCHEMA: &str =
    "platform/database/native-workspace-furnishing-writer/037_workspace_furnishing_writer_generation.sql";
// An independently verified full payload tuple is required before native use.
const WORKSPACE_FURNISHING_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];
// The fourth field must name the independently reviewed writer capability.
// The exact manifest binds its bytes and the complete SQL inventory.
const WORKSPACE_FURNISHING_WRITER_VALIDATION_PROVENANCE: &[(&str, &str, &str, &str)] = &[];
const WORKSPACE_FURNISHING_PUBLICATION_DIRECTORY: &str =
    "platform/database/native-workspace-furnishing-publication/";
const WORKSPACE_FURNISHING_PUBLICATION_SCHEMA: &str =
    "platform/database/native-workspace-furnishing-publication/038_workspace_furnishing_publication.sql";
const WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES: u64 = 1032;
const WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256: &str =
    "cbee8e1f2a7af5daa1aec498d014ad25934b116aa5d578966b23efef2e110a98";
const WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY: &str =
    "platform/scripts/native-workspace-furnishing-publication-capability-v1.json";
const WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_BYTES: u64 = 165;
const WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_SHA256: &str =
    "968dec2c1d4312b3e8a50194fa13d961c3bfb47ec4df17800f94e353a682daf3";
const WORKSPACE_FURNISHING_PUBLICATION_WRITER_CAPABILITY: &str =
    "platform/scripts/native-workspace-furnishing-writer-capability-v1.json";
// Admit only a separately qualified 038 payload. Keep the table empty until delivery.
const WORKSPACE_FURNISHING_PUBLICATION_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];
const WORKSPACE_MATERIAL_PUBLICATION_DIRECTORY: &str =
    "platform/database/native-workspace-material-publication/";
const WORKSPACE_MATERIAL_PUBLICATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-workspace-material-publication/039_workspace_material_publication.sql",
    1011,
    "6e2984911b585f0ceba0c64273d3535a505272c310a2f2f550f29f2a8ff90b32",
);
const WORKSPACE_MATERIAL_PUBLICATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-material-publication-capability-v1.json",
    163,
    "c93117acc9cc397cb978bcb5d4bbc5732699af983f2f3c0413e9b4205455339c",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_MATERIAL_PUBLICATION_VALIDATION_PROVENANCE: &[(&str, &str, &str)] =
    if CATALOG39_UPGRADE_TARGET.is_some() { &[CATALOG39_UPGRADE_SOURCE] } else { &[] };
#[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
const WORKSPACE_MATERIAL_PUBLICATION_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];
const WORKSPACE_MATERIAL_MULTIMAP_DIRECTORY: &str =
    "platform/database/native-workspace-material-multimap/";
const WORKSPACE_MATERIAL_MULTIMAP_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-workspace-material-multimap/040_workspace_material_multimap.sql",
    5544,
    "db01d1910ba01a0a2e223829b92f9fbc0e4ce0240c21d3a8ae6b70963a84ef21",
);
const WORKSPACE_MATERIAL_MULTIMAP_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-material-multimap-capability-v1.json",
    160,
    "14203083f3e9e735bba46a5dcc436c3f797aea168e5c91f428f28c134c1b3f84",
);
const WORKSPACE_FRAME_PREPARATION_DIRECTORY: &str =
    "platform/database/native-workspace-frame-preparation/";
const WORKSPACE_FRAME_PREPARATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-workspace-frame-preparation/041_workspace_frame_preparation.sql",
    4199,
    "c5a030186557dc4774aa0d2a677be7f432c1010e2ee1c1a34345f69ac7e2723d",
);
const WORKSPACE_FRAME_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-frame-preparation-capability-v1.json",
    348,
    "4af9cebfa05eff33bb7e77eca01038fd7dc23ea6c582479fecc4fab00a518152",
);
// Independent local validation for fresh and same-catalog reopen only.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_FRAME_PREPARATION_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[(
    "eca43ed3f6ae52ab3bb59eb70b537feb60d3a9b9",
    "b4685bf257eb44fe0636e0ae0851c3598df811ffdf4ce7083cc991b981f12bab",
    "1c1f3cbddae02e2c0a1053da99362f0fc110558aa5b5bd4f00cded2eec948a63",
)];
#[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
const WORKSPACE_FRAME_PREPARATION_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];
const HAYES_ORIGINAL_DIRECTORY: &str = "platform/database/native-hayes-original/";
const HAYES_ORIGINAL_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-hayes-original/042_hayes_original_attempt.sql",
    3617,
    "074de4171572039f2f400d49d07aff8a3ec83ec222c1684cbc5452c6cc8b3ece",
);
const HAYES_ORIGINAL_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-hayes-original-capability-v1.json",
    288,
    "89bcf960b415e5601cdf345be7c763975744f3d6e45f6772a90d8d1a0f991762",
);
const HAYES_DISPOSITION_DIRECTORY: &str = "platform/database/native-hayes-disposition/";
const HAYES_DISPOSITION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-hayes-disposition/043_hayes_output_stage.sql",
    11581,
    "d526e19e77161534c21fbe0f3a09d23651b52d6375889bcd680cc6770fed9e39",
);
const HAYES_DISPOSITION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-hayes-disposition-capability-v1.json",
    287,
    "5815c26a4439aa868f1e7ff012a6aa3b79ecbf35e3824e35e6af7a525b69f098",
);
const HAYES_RELEASE_DIRECTORY: &str = "platform/database/native-hayes-release/";
const HAYES_RELEASE_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-hayes-release/044_hayes_native_release.sql",
    4726,
    "83a4f49ea7e2a114494166c590a179eb40f19a7379b887871f81c307a0b038ca",
);
const HAYES_RELEASE_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-hayes-release-capability-v1.json",
    285,
    "b21403370f5f9088207d0d9a5aa4a74c7b2eb33e8692d0d61b617e40f08947c0",
);
const MODEL_PREPARATION_DIRECTORY: &str = "platform/database/native-model-preparation/";
const MODEL_PREPARATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-model-preparation/045_required_model_preparation.sql",
    2485,
    "ebd546db086b5488374abae719f34f0c18de95c9a6eb74acd69e2cea6f6430d8",
);
const MODEL_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-model-preparation-capability-v1.json",
    352,
    "19beda5c00b9427270dd08c800955076daf5ebc1699ab61881e41fa4c48a3370",
);
const LARGE_MODEL_PREPARATION_DIRECTORY: &str = "platform/database/native-large-model-preparation/";
const LARGE_MODEL_PREPARATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-large-model-preparation/046_large_model_preparation.sql",
    1406,
    "88d2840c9f948be4573e1a8682aa67c4225c087643273afb3b74b9cac8b558b4",
);
const LARGE_MODEL_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-large-model-preparation-capability-v2.json",
    279,
    "80fd3739748f7d28cedfaa7c37e7ea1aec1a39be8662a8f81b4a35e429cceba6",
);
const LARGE_MODEL_RECOVERY_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-large-model-recovery-capability-v1.json",
    207,
    "9745703701bfbbd2f8257acfba8e7c2929b6aa4dea2e8ea2041882f6ba101435",
);
const FRESH_IMAGE_PREPARATION_DIRECTORY: &str = "platform/database/native-fresh-image-preparation/";
const FRESH_IMAGE_PREPARATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-fresh-image-preparation/047_fresh_image_preparation.sql",
    1357,
    "682a7bf2f07a3b43cb8c398758c37568723c85abc372be1f374a59a29f474bf7",
);
const FRESH_IMAGE_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-fresh-image-preparation-capability-v1.json",
    269,
    "92f9752cf464f7e75839c57f40d81586e1a6a2b452b06b5d5bee1868211c45ac",
);
const INFO_STAND_PREPARATION_DIRECTORY: &str = "platform/database/native-workspace-info-stand-preparation/";
const INFO_STAND_PREPARATION_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-workspace-info-stand-preparation/048_workspace_info_stand_preparation.sql",
    1961,
    "5f41eba8cff108ee545e7454ca5ef3757087335c7650c95660ef1022d6698ca6",
);
const INFO_STAND_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-info-stand-preparation-capability-v1.json",
    231,
    "36a0dfacb8a71463737f723ab7f6c260e686d5a13fe6878a826b54817f3b71c1",
);
const GPU_MODEL_DIRECTORY: &str = "platform/database/native-gpu-model-preparation/";
pub(crate) const GPU_MODEL_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-gpu-model-preparation/049_gpu_model_preparation.sql",
    2103,
    "d525cb7f8d5040330aa7ccc3afd5d8a7878fd8612eae80a6613517510e546790",
);
const GPU_MODEL_CAPABILITY_PATH: &str = "platform/scripts/native-gpu-model-preparation-capability-v1.json";
// Exact assembled candidates, including the retained historical recipe build.
// These pins do not establish transition, encoding, preview or release.
const GPU_MODEL_CAPABILITY: Option<(&str, u64, &str)> = Some((
    GPU_MODEL_CAPABILITY_PATH, 264,
    "58e3bae5525b063f6a992ca3784701647ab2bef8d6d7ed0489131288ab7670a7",
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const GPU_MODEL_LEGACY_TARGET: RuntimeTuple = (
    "daf4fe857051ad4d08a009ddf25cf95ab8e76227",
    "09e8c2212145423f277c86e2f16584ebb58bf4d5253a9f94bd7761f094c77634",
    "3ee986e7d6601240db6b16f311235bb3a3d43cd2fef00688a740a6c9e91cd377",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const GPU_MODEL_WRAPPING_TARGET: RuntimeTuple = (
    "ac4053e0cedc303219e65da3072327987d33518a",
    "09e8c2212145423f277c86e2f16584ebb58bf4d5253a9f94bd7761f094c77634",
    "0b56a5e6701f2f4f6eccf08ba89d552dea9fac9a28bc3513b92a09418cb51b8e",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const GPU_MODEL_TARGETS: &[RuntimeTuple] = &[GPU_MODEL_LEGACY_TARGET, GPU_MODEL_WRAPPING_TARGET];
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const GPU_MODEL_HELPER_FILES: Option<&[(&str, u64, &str)]> = Some(&[
    ("platform/scripts/native-gpu-model-preparation-bootstrap.mjs", 5119, "fc1d1444b7578ff55486b3b32edc111f45eb174eaef11efbf7c5ffd2860337e3"),
    ("platform/scripts/local-runtime.mjs", 97862, "f9f68391893a04ad2a6ce92dedcf222df1b0ab4c12514cda04779e60f627216a"),
    ("platform/scripts/native-gpu-model-runtime.mjs", 2892, "e6e361faa28b35c4d9ed40fcf3053726ff6160cd236ce084c538aa2d14040a02"),
    ("platform/scripts/native-oex-normal-catalog.mjs", 51484, "6e7424891a1e13cc4094e2eb034412d93d7d3ba2b8e022a3556b3279f6f6a52a"),
    ("platform/scripts/native-owner-channel.mjs", 8667, "f677be4f02be73b411e7107bcc30396ef58fe869d4353ce072d4b877d0134e80"),
    ("platform/scripts/native-workspace-furnishing-admission.mjs", 8457, "35e3ee1e27fbd3cb647b993b9e3a0bb4d0ec1e907091e6a7051d513f728bad76"),
    ("platform/scripts/native-workspace-furnishing-bootstrap.mjs", 30545, "8e66e0cdd2e88158769dab749d13c3314fcf3b2f1886bb7c84cdd8f5ac070f08"),
    ("platform/scripts/native-workspace-furnishing-lease.mjs", 4792, "9d6ad474b604156c278e459a4955813541957222bf0e1dd928faf0e8d24c72b4"),
    ("platform/scripts/native-workspace-furnishing-ordinary-assertion.mjs", 7989, "06c0784148d2e75204d9b7ed7d05cd72ddbc8d0510d9668ffb50400f7d67e29e"),
]);

pub(crate) fn gpu_model_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return GPU_MODEL_TARGETS.contains(&tuple) && GPU_MODEL_CAPABILITY.is_some()
        && GPU_MODEL_HELPER_FILES.is_some();
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn gpu_model_carried_receipt_payload(tuple: (&str, &str, &str)) -> Option<&'static str> {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return gpu_model_host_matches(tuple).then_some(CATALOG45_CARRIED_RESTORE_BASE.2);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; None }
}

const FURNISHING_THUMBNAIL_DIRECTORY: &str = "platform/database/native-workspace-furnishing-thumbnail/";
pub(crate) const FURNISHING_THUMBNAIL_SCHEMA: (&str, u64, &str) = (
    "platform/database/native-workspace-furnishing-thumbnail/050_workspace_furnishing_thumbnail.sql",
    5974,
    "8461cad231b80c109b1aaedcef6e1a4d599143313faf00f366f52e79780219fc",
);
const FURNISHING_THUMBNAIL_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-furnishing-thumbnail-capability-v1.json",
    273,
    "3c3e6a6cea3f0d83c55996cab5a4e98d48d92cce28ea090fe6b969a7764014f5",
);
// One assembled, inventory-verified local candidate. This full tuple does not
// establish a successful schema transition, runtime startup, or release.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const FURNISHING_THUMBNAIL_TARGETS: &[RuntimeTuple] = &[(
    "55e8ad6c861eab44326ee8db60db2692e3f7baf7",
    "02a2e7eb9630eedad593edc04e6365ab263ee9f637d1aff994ba7dc093c356c5",
    "2c29a8ffe2a56395dd1cdc1b0acd93e4a1623e8aac8b0e10aca30369e60b4626",
)];

pub(crate) fn furnishing_thumbnail_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return FURNISHING_THUMBNAIL_TARGETS.contains(&tuple)
        && tuple.0 == "55e8ad6c861eab44326ee8db60db2692e3f7baf7"
        && tuple.1 == "02a2e7eb9630eedad593edc04e6365ab263ee9f637d1aff994ba7dc093c356c5";
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn furnishing_thumbnail_carried_receipt_payload(tuple: (&str, &str, &str)) -> Option<&'static str> {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return furnishing_thumbnail_host_matches(tuple).then_some(CATALOG45_CARRIED_RESTORE_BASE.2);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; None }
}

const HAYES_FRAME_PREPARATION_CAPABILITY: (&str, u64, &str) = (
    WORKSPACE_FRAME_PREPARATION_CAPABILITY.0,
    WORKSPACE_FRAME_PREPARATION_CAPABILITY.1,
    "c1fe667c80a3b36ddb124fddb5ce427131bae3018c47ecfce7238be556955f92",
);

// The private actual-host probe uses one reviewed local package. Ordinary
// Manager binaries have no admitted 042 host tuple.
pub(crate) fn hayes_bootstrap_validation_tuple() -> Option<(&'static str, &'static str, &'static str)> {
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return Some((
        "2ebddaab6c68f07f8f0da5338f43993524830589",
        "032fc0e28994987ed6fcb8f2adbc01e3e349b0e7e076b5c570d47c75c7a45048",
        "1eb6203060fd0ac598eda8c64e49b513b028c17debc3675f5d2e5f646fc5f98f",
    ));
    #[cfg(not(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    None
}

const HAYES_SESSION_PROBE_SCRIPT: &str = "platform/scripts/native-hayes-session-probe.mjs";
const HAYES_DISPOSITION_PROBE_SCRIPT: &str = "platform/scripts/native-hayes-disposition-probe.mjs";
const HAYES_RELEASE_PROBE_SCRIPT: &str = "platform/scripts/native-hayes-release-probe.mjs";

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) struct HayesSessionProbePin {
    pub(crate) host: (&'static str, &'static str, &'static str),
    script_bytes: u64,
    script_sha256: &'static str,
}

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const HAYES_SESSION_PROBE_PIN: Option<HayesSessionProbePin> = Some(HayesSessionProbePin {
    host: (
        "2da5c0448bfb102c8039038b72c5f61b084b484e",
        "1d46b4eb7bfd5b774aebad364a3131b09c3c42427dd633bb6622434b21bb60a3",
        "4a8422b0d5ee20fb1d67ddcf3aaec942470c9fb26f1239c96274e5ff98f3e498",
    ),
    script_bytes: 7307,
    script_sha256: "cf761453a0567ec1a9e2d0be8e336bec757ac8af323a47541a73080f3aa8f7f8",
});

// Admit a 043 probe only after the exact receiver, host manifest and probe
// script have been packaged and independently reviewed together.
#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const HAYES_DISPOSITION_PROBE_PIN: Option<HayesSessionProbePin> = Some(HayesSessionProbePin {
    host: (
        "b0d0bb53e4c04db401ab3a8d2d82443dbd150247",
        "0a25e52d222218615cafbc44a50cd623b5cd235ae7574c19647606b509a2ce80",
        "82c0109c2c104dcd1362f1d737fc3fabc6d6f59b93830efe168f34704eab7d93",
    ),
    script_bytes: 7622,
    script_sha256: "1de14b2af1e31b733132df57d1a7c919396c2850be993611292d610e5816140a",
});

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const HAYES_RELEASE_PROBE_PIN: Option<HayesSessionProbePin> = Some(HayesSessionProbePin {
    host: (
        "71f116bdac0e0bb0dd327659fd4407048fa100e4",
        "c3912c3251a22f32292a7317e0ad1b980aedb5318a0008aab439947db95a89a2",
        "14bf2a6175cef712c2d5c48679ae8f0d01d894158854061c943353559f61462f",
    ),
    script_bytes: 8196,
    script_sha256: "89a77022b0d88aae45e1f0256d2b15f95d7221308d899faaeee57a144b559384",
});

pub(crate) fn hayes_validation_host_matches(tuple: (&str, &str, &str)) -> bool {
    hayes_bootstrap_validation_tuple() == Some(tuple) || hayes_session_probe_matches(tuple)
}

fn hayes_session_probe_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return HAYES_SESSION_PROBE_PIN.as_ref().is_some_and(|pin| pin.host == tuple);
    #[cfg(not(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn hayes_disposition_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return HAYES_DISPOSITION_PROBE_PIN.as_ref().is_some_and(|pin| pin.host == tuple);
    #[cfg(not(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn hayes_release_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return HAYES_RELEASE_PROBE_PIN.as_ref().is_some_and(|pin| pin.host == tuple);
    #[cfg(not(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

// Keep this exact physical transition local to the debug validation helper.
#[cfg(debug_assertions)]
pub(crate) const CATALOG40_FRAME_SOURCE: (&str, &str, &str) = (
    "bb003b24492c0ee2de6c402dd555ab3e72b6c809",
    "89d47502358275e4ccbba4ff05740752021c074b8ef7c35eca98a73f2720dd8c",
    "df6efc23ea2d9aab790fc25fab9833db9f5b48b3cb0c5417a1bd7c5ef609ac2a",
);
#[cfg(debug_assertions)]
pub(crate) const CATALOG40_FRAME_TARGET: (&str, &str, &str) = (
    "7a70a7c742fc8e7eeeaf75f8429e23682c68c43c",
    "d0f03a339b7e65934a27f0a51210c28205b6dfbb98769d9deca10fa22cc05380",
    "c42e9657015695601ffb0c4ea493ae9ae6df9c954fce032db3212a1b20d309ee",
);
#[cfg(debug_assertions)]
pub(crate) const CATALOG40_TANGENT_TARGET: (&str, &str, &str) = (
    "dcde70254b4bd5f93487ee16c7147db1f97debc1",
    "afe2d4e6cc881733a218b2bc3354e026b5492adbd3a7dc0acf13e6bb66f9c178",
    "2645f013b8dcef5bb33f138dc0fac9789bbb508306cca722112f222a7476e871",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG40_FRAME_TRANSITION: (&str, &str) =
    (CATALOG40_FRAME_SOURCE.2, CATALOG40_FRAME_TARGET.2);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG40_TANGENT_TRANSITION: (&str, &str) =
    (CATALOG40_FRAME_TARGET.2, CATALOG40_TANGENT_TARGET.2);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
type RuntimeTuple = (&'static str, &'static str, &'static str);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG39_UPGRADE_SOURCE: RuntimeTuple = (
    "66d70f701a76d043f06ef4ce0933aad3669f8230",
    "927324495d06cba996cb4e718fbc09b386f646b4fd91e155af01ba18e6bf4ac0",
    "caf4d4d25245cfddbae8db2b80595250b3f08baceb715c66855a46537a88ef63",
);
// Reviewed local validation build with the occupied-upgrade writer.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG39_UPGRADE_TARGET: Option<RuntimeTuple> = Some((
    "71985bef3d9ac13b417cc7f9a9ccc7ffc6315f17",
    "afe2d4e6cc881733a218b2bc3354e026b5492adbd3a7dc0acf13e6bb66f9c178",
    "bdfc8202d851f9f8983440b9475da3fc3ddc6ee1f0bf4cf2c0d4bf1995dde338",
));

// Independently reviewed catalog 040 runtime with service backup support.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_EXB_TARGET: Option<RuntimeTuple> = Some((
    "394ca8bdb6b9ef9e4d5f298557f975c01ac36563",
    "afe2d4e6cc881733a218b2bc3354e026b5492adbd3a7dc0acf13e6bb66f9c178",
    "ebbf0aeabf013f5071151580495c9c7e5ff6812cbd995769b5e347e01ae92c0b",
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_RESTORE_PROVENANCE_TARGET: Option<RuntimeTuple> = Some((
    "e639f84fb3bb70d16eb209ce0d6dd1042ec92beb",
    "afe2d4e6cc881733a218b2bc3354e026b5492adbd3a7dc0acf13e6bb66f9c178",
    "7f76b19894fe3a9824a640cf1856ba7c828d5bd3201ede0da5d1c53a501bd1e7",
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_FRAME_RECORD_TARGET: Option<RuntimeTuple> = Some((
    "ae52da56ea7cd7d2d266056bcf0b9e146fd8ef29",
    "b97c87c4f0017db1caaed97d012ee7e5c0c60ebd6f4cad136b57657e3de18725",
    "d31372305fd54f8d42cec1ebc76bf0ec4d47cf36cb4ece658bca08195687748e",
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_ARTWORK_REPLACEMENT_TARGET: Option<RuntimeTuple> = Some((
    "660acf0920716242cf22a8e2822b22ec5a0b0764",
    "ee5a94b613031e2cea918dead21c885c42fce4d585a03e55a2f3799af4e6991c",
    "73a491c071ae5c6f10c786793733968549ab400333569d8427530b6e81606c9b",
));
// Fresh ordinary validation of the private Wheat grant with detail export disabled.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_WHEAT_GRANT: Option<RuntimeTuple> = Some((
    "3e1c83e8939889c552606198edc5363a020c6d50",
    "153558a719595549071e9bf6dd06950a0bfba9549c463601c1c6ac0ff51cc3ba",
    "ab297cadff8a0348c402f937fc95851315e595e8b93dea2b43c2b40cfd22b851",
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_WHEAT_DETAIL: Option<RuntimeTuple> = Some((
    "7aba217f521f30c5587febfc4ad910989f050649",
    "5433a44ab03bedbf9dc300f11f8304396169707417ea7460ea581c2f648f8230",
    "074719930770137f954302e1d27f0035b4d25f6489fa4e61391d96f6d25d90f1",
));
#[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
const CATALOG40_WHEAT_DETAIL: Option<(&str, &str, &str)> = None;

// The ordinary package is admitted only after its sealed inventory is reviewed.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const HAYES_ORIGINAL_RELEASE_MANIFEST: &str =
    "c0565112569fc1b1b95f8ada72b2b7cacabc8ea3e24b58a61dc9aa138c8bb406";

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG44_WHEAT_TARGET: Option<RuntimeTuple> = Some((
    "e4702f67a8eb2f8c9fd3d344c344f463a70e39e0",
    "2627954c7ac6c083bed01470db1e3ccec94e857ea2858babca1b4ed1e740adad",
    HAYES_ORIGINAL_RELEASE_MANIFEST,
));
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG44_HAYES_AUTO_TARGET: RuntimeTuple = (
    "f93f5113759760e7ddf0db8a3041aecbd797e538",
    "2627954c7ac6c083bed01470db1e3ccec94e857ea2858babca1b4ed1e740adad",
    "af9c141f071d84ba4e9397798759e1cb13b2cde4d350f2fcf0a4b852ce3a09b5",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG44_BC96_FRAME_LINEAGE_TARGET: RuntimeTuple = (
    "05be49accd64f9f0fd79c7cbf268938c441e61c9",
    "2627954c7ac6c083bed01470db1e3ccec94e857ea2858babca1b4ed1e740adad",
    "aa7534f2165c5d8a28a3e54dc5da4c8e1f026c16e806f28df52d35598d84bf27",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_BC96_REQUIRED_MODEL_TARGET: RuntimeTuple = (
    "0f743bda2a055f1025ea11ba64f17933cf28ccec",
    "f166f37f9323ea557bfc229feaa7b0f0e7b19472908f21ef3b5862578aedee25",
    "af4e787a5fa5c714d60bf24cba23a1b2e7015946d1f4bea9a5777a3af3c8b2d0",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_COMPACT_AND_CANDIDATE_TARGET: RuntimeTuple = (
    "ba88d3534b72ce882a24969e7aa79b79307fb697",
    "f166f37f9323ea557bfc229feaa7b0f0e7b19472908f21ef3b5862578aedee25",
    "a7cab3643e4b6554c70ef4c1fda7bfb2ce5234273dead44711be9f72df9ff34c",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_CURRENT_ORIGINAL_TARGET: RuntimeTuple = (
    "9297ba5b94a385a108fc4d9f356a7cc4b915a529",
    "f166f37f9323ea557bfc229feaa7b0f0e7b19472908f21ef3b5862578aedee25",
    "3ef0cf579846cfd682e63cf1bf89c8735c042b46a711569b4ec6d6c10870c76c",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET: RuntimeTuple = (
    "3db5e972e2cd3a46be58723c99d74fe8b16a03c5",
    "29cdc6fd07e1c5d4e5fda06a2c37270d7023024d76a7cc3d31596af9f1c7a921",
    "9c5da125d1789e93bdeb9446060b452fcb33b1bb2d95645c909d60c1ff30d18d",
);

const CATALOG45_RECOVERY_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-interrupted-recovery-capability-v1.json",
    137,
    "75dd973c5e497ad93df9a0563470de11b0831c91ccccc23fa9c3442d0897d9fc",
);
const CATALOG45_SELECTED_RECOVERY_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-interrupted-recovery-capability-v2.json",
    137,
    "8a50c1df0fb773cf0ec0252792c341a9c4f7a7e575d2068974649323d935d637",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_RECOVERY_TARGET: RuntimeTuple = (
    "fc152aca21da393c0214b3e028edcb2b9e58e9de",
    "ecdcc0f612240daa231d74842c4b1a181493da03a4a18c04a6e9b1bd7a9ab37e",
    "7c3efefb9dc90786da5efdcb1025d5953e0900de333830f3775f138c1dbc21f3",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_SELECTED_RECOVERY_TARGET: RuntimeTuple = (
    "749e624539005db2b0036416b83d308a169dd2d0",
    "84088f217c42dde5cb2d52d4d357879fcf4f32ea710ae5cd0e0fc52d861077b8",
    "62200d7ccb47e7ede5d1910a65e0087ac171c595d76ca14df6a19290382e4288",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET: RuntimeTuple = (
    "09b778996f83c3d0f6a242a7f638c162cc2d89e8",
    "84088f217c42dde5cb2d52d4d357879fcf4f32ea710ae5cd0e0fc52d861077b8",
    "003a98c579e69e1b38a7cb96971f0872f15b8afd169d392a96ac94fe87be9f92",
);

const CATALOG45_RETAINED_IMAGE_RECOVERY_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-retained-image-recovery-capability-v1.json",
    140,
    "670640e465b4fb8d74d18bb8dfdc503d89e43db54bd66d9a7de5eab8379b9857",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET: RuntimeTuple = (
    "fa7f54139b127a803de63bf3ede9daff2d6ff94c",
    "e173cb50d5017725c067104bfefc82c685838ddc2f2ba8b45860798029186d46",
    "f15d2530a298c3e9507e4719fa3ba65f26aff8cb9b40d9f646f1043976d0a75d",
);
const CATALOG45_STARTUP_CANCEL_RECOVERY_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-startup-cancel-recovery-capability-v1.json",
    140,
    "e1653f03f9e07da7c1f540a69da0008666aa56f9a30a401c62f8903973e56ac2",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET: RuntimeTuple = (
    "a0029e68b679a4c487756da64e2846eaaff20bb5",
    "041039e134b8d099516f8a4ee08916ec2e18c7f2e84847b2e6639140eef5457b",
    "f738f33c777de39629b6a0ab2dd3a477b45d078b738189e173d00d2cd7ab652a",
);

const CATALOG45_PRE_READY_PRESERVATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-pre-ready-preservation-capability-v1.json",
    139,
    "c597e580c1863c6a6efbb2dcb683046aa57b1a9207e578d0828977a630f38df9",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_PRE_READY_PRESERVATION_TARGET: RuntimeTuple = (
    "3414e771111b238f08d18a2c4775bb8477a51a63",
    "79a9efc8e6550bb0dd998bef02c9273cf4603c9349ffce0bb7aae05cd7928037",
    "baf33be8865ee1c558874702ca8a772db90a9c59c3ab28759e7fc6ae8d608c86",
);

const CATALOG45_PRE_READY_HAYES_PRESERVATION_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-catalog-045-pre-ready-hayes-preservation-capability-v1.json",
    145,
    "68eb708a876ec949bb26e0789b569790a4cefeb69e233c4c1e7f3186c5d3d262",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET: RuntimeTuple = (
    "3575da477b91c0300a2781e9272fd372a385cc5a",
    "a6cab25d569db1ad4cb3f1c3cc29d9019c2573241e7465441e923ca2fc74a721",
    "5cb2431e561c21e1463f78faf938a5cfc3807c9dd690a545f3474769c1c7f4a9",
);

pub(crate) fn catalog45_recovery_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_RECOVERY_TARGET == tuple || CATALOG45_SELECTED_RECOVERY_TARGET == tuple
        || CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET == tuple
        || CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET == tuple
        || CATALOG45_PRE_READY_PRESERVATION_TARGET == tuple
        || CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_DEFAULT_FRAME: RuntimeTuple = (
    "df34e11e5335e170c66f5b160a51b92b530e42d1",
    "29cdc6fd07e1c5d4e5fda06a2c37270d7023024d76a7cc3d31596af9f1c7a921",
    "a70e062ea7f1d0aa799e4dd924cfe6043bde06ffe11960e2967e60c0ab8285be",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_PREPARED_SIGN_AUDIO: RuntimeTuple = (
    "3b0adc9d70ddd143c3afa884d9647b679dc1061d",
    "83d230e2cab53d7c4472b3640de01f5937088559f9dfaff41b4db32df58b16be",
    "cbf4214bbf755216e8173190ce98ee32fb3f3708bd9c6a25e394eb3ce30e0a0a",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_PLACED_EXIT_DOORS: RuntimeTuple = (
    "5b855dde3ed645a7d29e75fbf498bfff9bc7e9ec",
    "83d230e2cab53d7c4472b3640de01f5937088559f9dfaff41b4db32df58b16be",
    "1245b842efb55525ebf0c307f984391431552ce19d6f59874dbcd8435b8e39d2",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_DEFAULT_SEATS: RuntimeTuple = (
    "170f7b39c335cac4d906d4a1116cc567dea78509",
    "a6cab25d569db1ad4cb3f1c3cc29d9019c2573241e7465441e923ca2fc74a721",
    "ca92c7530f594462e459bdf16ff9cd64cc1dd1ffc0d68ab4b13c31e7ea3a7630",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_BACKED_BENCH: RuntimeTuple = (
    "9f938be182e0c30066d7675457a0306bb89048ed",
    "0a0669e95010371c2dc379285930d166d4bac30506d88f89326b4264ebd0a385",
    "f8d5933a2c2e361154dbcc9cb4a332144ca9217e5edb363fdd5056ee70aed702",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_BOTTOM_CATALOG: RuntimeTuple = (
    "1022863c07e6673ad0acf29225ecedc6d48871dd",
    "0a0669e95010371c2dc379285930d166d4bac30506d88f89326b4264ebd0a385",
    "4a0bbccc8ef04c6734407fd448f2b68e6b25a6a7d5f5b0afc33878be5aa29d4d",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_PRODUCT_IMAGE: RuntimeTuple = (
    "d33714b74d2b12ad1bc089bd8180ad0a49974894",
    "29cdc6fd07e1c5d4e5fda06a2c37270d7023024d76a7cc3d31596af9f1c7a921",
    "157f538603a9557c30dfbb58ffc04755db9e174817aaeeb13353ce90e6dd84a2",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_RENDERED_SCULPTURE_THUMBNAIL: RuntimeTuple = (
    "d20776d173ba22df971236648ac8c34a7cdd662c",
    "c8f2613ffe79d1aa1baf223201ec89b5fe38941d9b960a1317fded652b9df531",
    "f21b1378b39d11fd6ea8f9c89636912f233f884af176e2deb9856dea3ae77b53",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX: RuntimeTuple = (
    "ca53a3b0aeb4f1ce7cf3d9a4040d8d09336cad51",
    "93f6ceebc80298d946df23624e0d7b4f755bbe9d6764771e6ecc95f60e951ee2",
    "0efbf042ad20b7cd8963751e5cfe5f343ead93d6ced6164e8bbd08566e378c99",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_SAVED_HEAD_MODULAR_PORTAL: RuntimeTuple = (
    "3bcf73ed523c465376b213b0649185621d10e216",
    "93f6ceebc80298d946df23624e0d7b4f755bbe9d6764771e6ecc95f60e951ee2",
    "375672ab366c52a8a177e2815fda65382708aba8126461a903268534918b43fe",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE: RuntimeTuple = (
    "17488095a3851ac76386c40dfdab114212fd3981",
    "93f6ceebc80298d946df23624e0d7b4f755bbe9d6764771e6ecc95f60e951ee2",
    "31b12d9f6fac4019f39c2a9128ced09b7ee918c49ffb119beb544e5e40831ac0",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS: RuntimeTuple = (
    "fe4491994abf75f6e8872108b3fe19d6e8aa0db4",
    "66b63cfecc4c48f77d4b04a10ee9df495de64072371817b3c8eee57710ba1ca6",
    "4fbd168027090f38776bd75990e27ed336db6c65d15c4af3f17bf428d140440b",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_NATIVE_PUBLICATION_CAPACITY: RuntimeTuple = (
    "493223193a6db1b7941835d96a6d8f3a39084863",
    "93f6ceebc80298d946df23624e0d7b4f755bbe9d6764771e6ecc95f60e951ee2",
    "1209d5d95417d1e29f637d8ce327e78cf38ed1b32eabcb5ab6fd3e2a62a10203",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_NATIVE_PUBLICATION_TARGETS: &[RuntimeTuple] = &[
    CATALOG45_NATIVE_PUBLICATION_CAPACITY,
    (
        "08776739a3c65709dbd998c4150c06a7b2401c4e",
        "93f6ceebc80298d946df23624e0d7b4f755bbe9d6764771e6ecc95f60e951ee2",
        "22d253f8928d7dee5e2d01e8ce65e20cd24f10b7bfa0251b1eb8371aca8e8fc9",
    ),
    CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS,
    CATALOG45_PREPARED_SIGN_AUDIO,
];

fn catalog45_native_publication_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_NATIVE_PUBLICATION_TARGETS.contains(&tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_MODEL_ADOPTION_VALIDATION: (&str, &str, &str) = (
    "5a087c69dd4459b4d189f3bfa4eed66ddcda9b06",
    "cb424ea0d4b3c579ab85313e0311e074fc119d94acc96fcc512932138ce1d701",
    "0fd5149ea964713218d87b09b41b7d9aa05576ce0b5f94c9d520ac2221e36ff0",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_MODEL_ADOPTION_CARRIED_TARGET: RuntimeTuple = (
    "c45a91c92a661dbc67aedac1b45f8b4781c065c0",
    "bc4d0b5c6e228b988077846b2fe92d2715e2f2918bc47cc5ff43b20eb67ed2d4",
    "6ada7bd42642333218ae623010532065c8cae2fa66a0cd70e3f226ec37dcda18",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET: RuntimeTuple = (
    "8c1e7d076fc8496387cb37a7ae59ef49015a665b",
    "bc4d0b5c6e228b988077846b2fe92d2715e2f2918bc47cc5ff43b20eb67ed2d4",
    "49f05729bfe39c7e2a228888c6541f5e22e08057f1e07bf04a66055a2016e861",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET: RuntimeTuple = (
    "781b1de5005a44ff94d04f0e35fa20cebef38169",
    "bc4d0b5c6e228b988077846b2fe92d2715e2f2918bc47cc5ff43b20eb67ed2d4",
    "e41df146d7097fd6964564de3d2f82b4bb766dba5377ec7f938896955fea39b0",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET: RuntimeTuple = (
    "77bf7d1435a9ee248eeb19c164b6062db5fbaa5d",
    "bc4d0b5c6e228b988077846b2fe92d2715e2f2918bc47cc5ff43b20eb67ed2d4",
    "3e05e41286c517d5304299bdd987b8e5b6fe08ab8396dee3804feeb0f0c4f670",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET: RuntimeTuple = (
    "b0c02301c39b85a75ca576e3dca56274ccd92c28",
    "bc4d0b5c6e228b988077846b2fe92d2715e2f2918bc47cc5ff43b20eb67ed2d4",
    "eac64ed106bdac283c1aa0c6c3dee5541c98919ed4a67bd9e5509b21496b5bce",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_PORTAL_HAYES_CARD_TARGET: RuntimeTuple = (
    "9abaa59286687fe62fd059dd3bf244bdb89d2294",
    "1691cc459858f5b4dff4bfc47635ec78868dcdb593ceb7d60ab0e7a115b704f5",
    "86faf157a49ffa7cd07495a064e19efd2f25e5b2bb5a1944432bae04872305e3",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_SCENE_AUTHORING_TARGET: RuntimeTuple = (
    "e65c1a46d04604c6f3cac836d80f3b6f6426f68b",
    "1691cc459858f5b4dff4bfc47635ec78868dcdb593ceb7d60ab0e7a115b704f5",
    "d6f8eb6a9abca42685de3b8d2671a63ab12795a45387129d76e4343874aa985f",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_THUMBNAIL_FRESHNESS_TARGET: RuntimeTuple = (
    "b09ab6b48a9a308ec113ce35bed5efa7805fdf07",
    "1691cc459858f5b4dff4bfc47635ec78868dcdb593ceb7d60ab0e7a115b704f5",
    "bc4dc0736a07774550107f12cbab729d91d2156d6ec8828c321dcf0677555c7d",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_COMPLETE_STATIC_TARGET: RuntimeTuple = (
    "ff3df6e26d56031a036301da5d3022386222bc7a",
    "ec8f7f1e73a4e48bfe90a529103d9205ef5324e04d5e6b1142b4748f3539f8cd",
    "21de5a4cdc00d01f51f4ae0746c8abc08bc062e71c2d73778f706d2f4e8d6088",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_FIXTURE_AUTHORING_TARGET: RuntimeTuple = (
    "48155ad7a0b8b2ff0d3967a30b525316be92aae5",
    "ec8f7f1e73a4e48bfe90a529103d9205ef5324e04d5e6b1142b4748f3539f8cd",
    "5b3aadfe5332bdaf084741f2b9a20b0e9c6733c44be36cec7f4d1ddb8ba338f2",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_DEFAULT_1_7_TARGET: RuntimeTuple = (
    "0de658cd2ce28e766ad4d9a05a912eb43c082691",
    "7b6890fae09bc3d40963a16d8bab2a040cc431023a7952ff2bb4f429da83543c",
    "1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_DEFAULT_1_11_TARGET: RuntimeTuple = (
    "f24631da70f2c072816f12445ac7af49c8e032d3",
    "e662b20c775c2852521dca75cdcea1d631d4aa11d1cd157db272848d09afceaf",
    "4b8a50b3b9debcafdc757048dd98a1a8997feaf894ead81543739d7bd1b85b84",
);

// Exact current carried profile source and its single clean read-route successor.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_SELECTED_PNG_READ_SOURCE: RuntimeTuple = (
    "5ed3aa9166d5f3858a86f8d97f1cf1ca9e3cc2ca",
    "6b133c8f1372b8d886d08c894b43a17d8300fc7577db4b430874d8568557bbec",
    "ac2248e1184ad4514f386ab6ee11b6ddd8bf2ca71792f4ec9eb9242e189724d7",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_SELECTED_PNG_READ_TARGET: RuntimeTuple = (
    "7fa626c674056a361b37a59ff10fc351b80222c6",
    "e662b20c775c2852521dca75cdcea1d631d4aa11d1cd157db272848d09afceaf",
    "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238",
);

// Exact additive current-approved-image producer; no schema, restore or recovery expansion.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET: RuntimeTuple = (
    "a888494bb9ebb9299b6adc14eebad314e4153573",
    "e662b20c775c2852521dca75cdcea1d631d4aa11d1cd157db272848d09afceaf",
    "0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988",
);

// These normal runtime families intentionally contain neither model-adoption
// marker. Their ordinary start/clean carried-receipt authority is independent
// of an adoption operation channel.
fn catalog45_markerless_ordinary_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return [CATALOG45_DEFAULT_1_11_TARGET, CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET].contains(&tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

fn catalog45_model_adoption_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
        CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_DEFAULT_1_7_TARGET, CATALOG45_DEFAULT_1_11_TARGET, CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET].contains(&tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn ordinary_fresh_catalog45_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_DEFAULT_FRAME == tuple || CATALOG45_PRODUCT_IMAGE == tuple
        || CATALOG45_RENDERED_SCULPTURE_THUMBNAIL == tuple
        || CATALOG45_SAVED_HEAD_MODULAR_PORTAL == tuple
        || CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE == tuple
        || CATALOG45_PLACED_EXIT_DOORS == tuple
        || CATALOG45_DEFAULT_SEATS == tuple
        || CATALOG45_BACKED_BENCH == tuple
        || CATALOG45_BOTTOM_CATALOG == tuple
        || catalog45_native_publication_host_matches(tuple) || catalog45_model_adoption_host_matches(tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn catalog45_same_version_restore_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_PLACED_EXIT_DOORS == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_CARRIED_RESTORE_BASE: (&str, &str, &str) = (
    "ded4a9a77c052650849770fc7a754c517334ee74",
    "0a0669e95010371c2dc379285930d166d4bac30506d88f89326b4264ebd0a385",
    "d0209c6ac9980a1507e53fc7a5eaa2b6b254bb60b6a8a1feb2f72d98e48d2874",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG45_CARRIED_RESTORE_HOSTS: [(&str, &str, &str); 3] = [
    CATALOG45_CARRIED_RESTORE_BASE,
    (
        "3dfa5c1b430973c59d86a50796bfd5377f2dbde6",
        "0a0669e95010371c2dc379285930d166d4bac30506d88f89326b4264ebd0a385",
        "1bfd19cd28329a63e2f81527de380e65c3417f1242cdfa243b92cf87f0315fb4",
    ),
    (
        "2a0e3eb89641967ad2cc18d85e6b1747b505fba1",
        "0a0669e95010371c2dc379285930d166d4bac30506d88f89326b4264ebd0a385",
        "62e3b790a013d9f6895eb4236db17d538ca1996f9bb031d249a675ea9de9be34",
    ),
];

pub(crate) fn catalog45_carried_restore_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_CARRIED_RESTORE_HOSTS.contains(&tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn catalog45_carried_restore_payload(payload: &Payload) -> bool {
    let context = payload.context();
    catalog45_carried_restore_host_matches((&context.source_commit, payload.producer_sha256(), &context.manifest_sha256))
}

/// The existing activation receipt remains pinned to the original payload
/// after the separately authorized clean adoption handoff.
pub(crate) fn catalog45_adoption_carried_receipt_payload(tuple: (&str, &str, &str)) -> Option<&'static str> {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return [CATALOG45_MODEL_ADOPTION_CARRIED_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_DEFAULT_1_7_TARGET, CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET]
        .contains(&tuple).then_some(CATALOG45_CARRIED_RESTORE_BASE.2);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; None }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn catalog45_adoption_transition(pair: (&str, &str)) -> Option<(RuntimeTuple, RuntimeTuple)> {
    [
        (CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET),
        (CATALOG45_CURRENT_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET),
        (CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET),
        (CATALOG45_CURRENT_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET),
        (CATALOG45_MODEL_ADOPTION_CARRIED_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET),
        (CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET),
        (CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET),
        (CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET),
        (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET),
        (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_SCENE_AUTHORING_TARGET),
        (CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET),
        (CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET),
        (CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET),
        (CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET),
        (CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_DEFAULT_1_7_TARGET),
        (CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET),
        (CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET),
    ].into_iter().find(|(source, target)| pair == (source.2, target.2))
}

#[cfg(feature = "workspace-v5-owner-validation")]
pub(crate) fn catalog45_adoption_handoff(pair: (&str, &str)) -> bool {
    #[cfg(all(debug_assertions, target_os = "macos"))]
    return catalog45_adoption_transition(pair).is_some();
    #[cfg(not(all(debug_assertions, target_os = "macos")))]
    { let _ = pair; false }
}

pub(crate) fn catalog45_carried_restore_source(payload: &Payload) -> bool {
    let context = payload.context();
    catalog45_same_version_restore_host_matches((&context.source_commit, payload.producer_sha256(), &context.manifest_sha256))
}

pub(crate) fn native_restore_provenance_host_matches(catalog: u32, tuple: (&str, &str, &str)) -> bool {
    catalog == 40 || (catalog == 45 && catalog45_same_version_restore_host_matches(tuple))
}

/// Existing activated restore receipt for the exact backed-bench transition.
pub(crate) fn catalog45_retained_restore_payload(tuple: (&str, &str, &str)) -> Option<&'static str> {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return (tuple == CATALOG45_BACKED_BENCH).then_some(CATALOG45_PLACED_EXIT_DOORS.2);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; None }
}

pub(crate) fn catalog45_restored_backed_handoff(pair: (&str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return pair == (CATALOG45_PLACED_EXIT_DOORS.2, CATALOG45_BACKED_BENCH.2);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = pair; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG46_LARGE_MODEL: RuntimeTuple = (
    "c3937794193b2262f0ad2d5dcf30cdb838e424c9",
    "682a92a91bf39a71dd308646cea57796eeb3be6c0d6f457a02fc5c431d4106f5",
    "d9ccc2d90985981279b5e3d49835cd5675a91589aabedc838d100c63e95da8fa",
);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG46_LARGE_MODEL_RECOVERY: RuntimeTuple = (
    "ab17d52e4145a063e02a5adb883b2f7c32615ce9",
    "110678e6b231c67f9b522c4f0ab1100c95bdef4fa20a4ef1224e487c8f5b98e6",
    "31d6f02f26aab8ce736d41ccf4148139587f9880336471afd4beaca995dfbc7d",
);

pub(crate) fn large_model_recovery_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG46_LARGE_MODEL_RECOVERY == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn large_model_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG46_LARGE_MODEL == tuple || large_model_recovery_host_matches(tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const CATALOG47_FRESH_IMAGE_RECOVERY: RuntimeTuple = (
    "11f6a7ef0b297b5f57cb262870829708c1fe9904",
    "489e5c418e1757a8c9202cfde3efa3a1bda55b16b01a075e48cfba893b6ce643",
    "257ea9d3d0ef1064494da5c68c2d9b9dd2430ac9f7882ae85e3399c49420ac96",
);

pub(crate) fn fresh_image_recovery_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG47_FRESH_IMAGE_RECOVERY == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

// Only the reviewed finalizer receipt supplies these build-time values. A
// missing receipt leaves ordinary release catalog 047 closed.
pub(crate) fn fresh_image_release_manifest_matches(manifest: &str) -> bool {
    cfg!(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))
        && option_env!("EXHIBITOS_FRESH_IMAGE_RELEASE_MANIFEST_SHA256") == Some(manifest)
        && hash_valid(manifest)
        && option_env!("EXHIBITOS_FRESH_IMAGE_RELEASE_PRODUCER_SHA256").is_some_and(hash_valid)
}

fn fresh_image_release_host_matches(tuple: (&str, &str, &str)) -> bool {
    fresh_image_release_manifest_matches(tuple.2)
        && tuple.0 == "7ad04db38db884ffc7dfd32d803aaf5bf08b0aa5"
        && option_env!("EXHIBITOS_FRESH_IMAGE_RELEASE_PRODUCER_SHA256") == Some(tuple.1)
}

pub(crate) fn fresh_image_host_matches(tuple: (&str, &str, &str)) -> bool {
    fresh_image_recovery_host_matches(tuple) || fresh_image_release_host_matches(tuple)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG48_INFO_STAND_PREPARATION: RuntimeTuple = (
    "9611961c75c80980babcec8fe9e7c0267800fcc0",
    "392bde543dd12306061a65c1a60b1696def07407e0019faaf22e3060a4fc0e10",
    "cfc795c35628e15a9f91b71dcca0587023a0b249b2984a2615973b72bc6ed32f",
);

pub(crate) fn info_stand_preparation_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG48_INFO_STAND_PREPARATION == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn required_model_preparation_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG45_BC96_REQUIRED_MODEL_TARGET == tuple
        || CATALOG45_COMPACT_AND_CANDIDATE_TARGET == tuple
        || CATALOG45_CURRENT_ORIGINAL_TARGET == tuple
        || CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET == tuple
        || CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET == tuple
        || CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX == tuple
        || catalog45_recovery_host_matches(tuple);
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

pub(crate) fn ordinary_hayes_release_host_matches(tuple: (&str, &str, &str)) -> bool {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    return CATALOG44_WHEAT_TARGET == Some(tuple) || CATALOG44_HAYES_AUTO_TARGET == tuple
        || CATALOG44_BC96_FRAME_LINEAGE_TARGET == tuple;
    #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
    { let _ = tuple; false }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Catalog39Upgrade(RuntimeTuple);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Catalog40Upgrade {
    source: RuntimeTuple,
    target: RuntimeTuple,
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn saved_workspace_transition_supported(pair: (&str, &str)) -> bool {
    WORKSPACE_V5_SAVED_TRANSITIONS.contains(&pair) || FurnishingTransition::from_manifests(pair).is_some()
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn retained_product_image_transition(pair: (&str, &str)) -> bool {
    pair == (CATALOG45_SELECTED_RECOVERY_TARGET.2, CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET.2)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn retained_product_image_recovery_transition(pair: (&str, &str)) -> bool {
    pair == (CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET.2, CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET.2)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn catalog45_native_publication_transition(pair: (&str, &str)) -> Option<(RuntimeTuple, RuntimeTuple)> {
    [
        (CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET, CATALOG45_NATIVE_PUBLICATION_CAPACITY),
        (CATALOG45_NATIVE_PUBLICATION_CAPACITY, CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS),
        (CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS, CATALOG45_PREPARED_SIGN_AUDIO),
    ].into_iter().find(|(source, target)| pair == (source.2, target.2))
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn catalog45_clean_exit_handoff(pair: (&str, &str)) -> bool {
    pair == (CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET.2, CATALOG45_PLACED_EXIT_DOORS.2)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Catalog45StartupRecovery {
    Cancelled,
    Unqualified,
    PreReadyPreservation,
    PreReadyHayesPreservation,
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
impl Catalog45StartupRecovery {
    pub(crate) fn preserves_pre_ready(self) -> bool {
        matches!(self, Self::PreReadyPreservation | Self::PreReadyHayesPreservation)
    }

    pub(crate) fn requires_hayes_proof(self) -> bool {
        matches!(self, Self::Cancelled | Self::Unqualified | Self::PreReadyHayesPreservation)
    }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn catalog45_startup_recovery(pair: (&str, &str)) -> Option<Catalog45StartupRecovery> {
    let current = CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET.2;
    match pair {
        (source, target) if source == CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET.2 && target == current =>
            Some(Catalog45StartupRecovery::Cancelled),
        (source, target) if source == current && target == current => Some(Catalog45StartupRecovery::Unqualified),
        (source, target) if source == CATALOG45_PREPARED_SIGN_AUDIO.2 && target == CATALOG45_PRE_READY_PRESERVATION_TARGET.2 =>
            Some(Catalog45StartupRecovery::PreReadyPreservation),
        (source, target) if source == CATALOG45_PREPARED_SIGN_AUDIO.2 && target == CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET.2 =>
            Some(Catalog45StartupRecovery::PreReadyHayesPreservation),
        _ => None,
    }
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn saved_workspace_transition_target(manifest: &str) -> bool {
    WORKSPACE_V5_SAVED_TRANSITIONS.iter().any(|(_, target)| *target == manifest)
        || CATALOG39_UPGRADE_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG40_EXB_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG40_RESTORE_PROVENANCE_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG40_FRAME_RECORD_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG40_ARTWORK_REPLACEMENT_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG40_WHEAT_DETAIL.is_some_and(|target| target.2 == manifest)
        || CATALOG44_WHEAT_TARGET.is_some_and(|target| target.2 == manifest)
        || CATALOG44_HAYES_AUTO_TARGET.2 == manifest
        || CATALOG44_BC96_FRAME_LINEAGE_TARGET.2 == manifest
        || CATALOG45_BC96_REQUIRED_MODEL_TARGET.2 == manifest
        || CATALOG45_COMPACT_AND_CANDIDATE_TARGET.2 == manifest
        || CATALOG45_CURRENT_ORIGINAL_TARGET.2 == manifest
        || CATALOG45_MODEL_ADOPTION_CARRIED_TARGET.2 == manifest
        || CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET.2 == manifest
        || CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET.2 == manifest
        || CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET.2 == manifest
        || CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2 == manifest
        || CATALOG45_PORTAL_HAYES_CARD_TARGET.2 == manifest
        || CATALOG45_SCENE_AUTHORING_TARGET.2 == manifest
        || CATALOG45_THUMBNAIL_FRESHNESS_TARGET.2 == manifest
        || CATALOG45_COMPLETE_STATIC_TARGET.2 == manifest
        || CATALOG45_FIXTURE_AUTHORING_TARGET.2 == manifest
        || CATALOG45_DEFAULT_1_7_TARGET.2 == manifest
        || CATALOG45_SELECTED_PNG_READ_TARGET.2 == manifest
        || CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET.2 == manifest
        || CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET.2 == manifest
        || CATALOG45_RECOVERY_TARGET.2 == manifest
        || CATALOG45_SELECTED_RECOVERY_TARGET.2 == manifest
        || CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET.2 == manifest
        || CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET.2 == manifest
        || CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET.2 == manifest
        || CATALOG45_NATIVE_PUBLICATION_CAPACITY.2 == manifest
        || CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS.2 == manifest
        || CATALOG45_PREPARED_SIGN_AUDIO.2 == manifest
        || CATALOG45_PRE_READY_PRESERVATION_TARGET.2 == manifest
        || CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET.2 == manifest
        || CATALOG45_PLACED_EXIT_DOORS.2 == manifest
        || CATALOG45_BACKED_BENCH.2 == manifest
        || CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX.2 == manifest
        || GPU_MODEL_TARGETS.iter().any(|target| target.2 == manifest && gpu_model_host_matches(*target))
        || FURNISHING_THUMBNAIL_TARGETS.iter().any(|target| target.2 == manifest && furnishing_thumbnail_host_matches(*target))
        || CATALOG40_RETAINED_OEX_CLEANUP.2 == manifest
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FurnishingTransition {
    White,
    Tangent,
    Upgrade39(Catalog39Upgrade),
    Upgrade40(Catalog40Upgrade),
    Upgrade44 { source: RuntimeTuple, target: RuntimeTuple },
    Upgrade49 { source: RuntimeTuple, target: RuntimeTuple },
    Upgrade50 { source: RuntimeTuple, target: RuntimeTuple },
    Recover45 { source: RuntimeTuple, target: RuntimeTuple },
    SameCatalog { source: RuntimeTuple, target: RuntimeTuple, catalog: WorkspaceFurnishingCatalog },
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
impl FurnishingTransition {
    pub(crate) fn startup_recovery(self) -> Option<Catalog45StartupRecovery> {
        catalog45_startup_recovery((self.source().2, self.target().2))
    }

    pub(crate) fn retains_catalog45_restore(self) -> bool {
        matches!(self, Self::SameCatalog { source, target, catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045 }
            if source == CATALOG45_PLACED_EXIT_DOORS && target == CATALOG45_BACKED_BENCH)
    }

    pub(crate) fn retains_catalog45_adoption_carried_restore(self) -> bool {
        matches!(self, Self::SameCatalog { source, target, catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045 }
            if [CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET].contains(&source)
                && catalog45_adoption_transition((source.2, target.2)) == Some((source, target)))
    }

    pub(crate) fn allows_catalog45_adoption_without_restore(self) -> bool {
        matches!(self, Self::SameCatalog { source, target, catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045 }
            if (source == CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET && target == CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET)
                || (source == CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET && target == CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET)
                || (source == CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET && target == CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET)
                || (source == CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET && target == CATALOG45_PORTAL_HAYES_CARD_TARGET)
                || ([CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET].contains(&source) && target == CATALOG45_SCENE_AUTHORING_TARGET)
                || (source == CATALOG45_SCENE_AUTHORING_TARGET && target == CATALOG45_THUMBNAIL_FRESHNESS_TARGET)
                || (source == CATALOG45_THUMBNAIL_FRESHNESS_TARGET && target == CATALOG45_COMPLETE_STATIC_TARGET)
                || (source == CATALOG45_COMPLETE_STATIC_TARGET && target == CATALOG45_FIXTURE_AUTHORING_TARGET)
                || (source == CATALOG45_FIXTURE_AUTHORING_TARGET && target == CATALOG45_DEFAULT_1_7_TARGET))
    }

    pub(crate) fn from_manifests(pair: (&str, &str)) -> Option<Self> {
        if gpu_model_wrapping_transition(pair) {
            return Some(Self::SameCatalog {
                source: GPU_MODEL_LEGACY_TARGET,
                target: GPU_MODEL_WRAPPING_TARGET,
                catalog: WorkspaceFurnishingCatalog::GpuModelPreparation049,
            });
        }
        if pair.0 == CATALOG45_DEFAULT_1_7_TARGET.2
            && let Some(&target) = GPU_MODEL_TARGETS.iter().find(|target|
                target.2 == pair.1 && gpu_model_host_matches(**target))
        {
            return Some(Self::Upgrade49 { source: CATALOG45_DEFAULT_1_7_TARGET, target });
        }
        if pair.0 == CATALOG45_DEFAULT_1_7_TARGET.2
            && let Some(&target) = FURNISHING_THUMBNAIL_TARGETS.iter().find(|target|
                target.2 == pair.1 && furnishing_thumbnail_host_matches(**target))
        {
            return Some(Self::Upgrade50 { source: CATALOG45_DEFAULT_1_7_TARGET, target });
        }
        if let Some((source, target)) = catalog45_adoption_transition(pair) {
            return Some(Self::SameCatalog {
                source,
                target,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        for (source, target) in [
            (CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET, CATALOG45_RECOVERY_TARGET),
            (CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET, CATALOG45_SELECTED_RECOVERY_TARGET),
            (CATALOG45_SELECTED_RECOVERY_TARGET, CATALOG45_SELECTED_RECOVERY_TARGET),
        ] {
            if pair == (source.2, target.2) {
                return Some(Self::Recover45 { source, target });
            }
        }
        if retained_product_image_recovery_transition(pair) {
            return Some(Self::Recover45 {
                source: CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET,
                target: CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET,
            });
        }
        if let Some(startup) = catalog45_startup_recovery(pair) {
            let (source, target) = match startup {
                Catalog45StartupRecovery::Cancelled => (CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET, CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET),
                Catalog45StartupRecovery::Unqualified => (CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET, CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET),
                Catalog45StartupRecovery::PreReadyPreservation => (CATALOG45_PREPARED_SIGN_AUDIO, CATALOG45_PRE_READY_PRESERVATION_TARGET),
                Catalog45StartupRecovery::PreReadyHayesPreservation => (CATALOG45_PREPARED_SIGN_AUDIO, CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET),
            };
            return Some(Self::Recover45 { source, target });
        }
        if let Some(transition) = Self::same_catalog_from_manifests(
            pair, Some(CATALOG40_DRAFT2_OEX_SELECTED), Some(CATALOG40_RETAINED_OEX_CLEANUP),
        ) {
            return Some(transition);
        }
        if pair == (CATALOG44_BC96_FRAME_LINEAGE_TARGET.2, CATALOG45_BC96_REQUIRED_MODEL_TARGET.2) {
            return Some(Self::Upgrade44 {
                source: CATALOG44_BC96_FRAME_LINEAGE_TARGET,
                target: CATALOG45_BC96_REQUIRED_MODEL_TARGET,
            });
        }
        if pair == (CATALOG44_HAYES_AUTO_TARGET.2, CATALOG45_COMPACT_AND_CANDIDATE_TARGET.2) {
            return Some(Self::Upgrade44 {
                source: CATALOG44_HAYES_AUTO_TARGET,
                target: CATALOG45_COMPACT_AND_CANDIDATE_TARGET,
            });
        }
        if pair == (CATALOG45_BC96_REQUIRED_MODEL_TARGET.2, CATALOG45_COMPACT_AND_CANDIDATE_TARGET.2) {
            return Some(Self::SameCatalog {
                source: CATALOG45_BC96_REQUIRED_MODEL_TARGET,
                target: CATALOG45_COMPACT_AND_CANDIDATE_TARGET,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if pair == (CATALOG45_COMPACT_AND_CANDIDATE_TARGET.2, CATALOG45_CURRENT_ORIGINAL_TARGET.2) {
            return Some(Self::SameCatalog {
                source: CATALOG45_COMPACT_AND_CANDIDATE_TARGET,
                target: CATALOG45_CURRENT_ORIGINAL_TARGET,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if pair == (CATALOG45_CURRENT_ORIGINAL_TARGET.2, CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET.2) {
            return Some(Self::SameCatalog {
                source: CATALOG45_CURRENT_ORIGINAL_TARGET,
                target: CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if retained_product_image_transition(pair) {
            return Some(Self::SameCatalog {
                source: CATALOG45_SELECTED_RECOVERY_TARGET,
                target: CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if let Some((source, target)) = catalog45_native_publication_transition(pair) {
            return Some(Self::SameCatalog {
                source,
                target,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if catalog45_clean_exit_handoff(pair) {
            return Some(Self::SameCatalog {
                source: CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET,
                target: CATALOG45_PLACED_EXIT_DOORS,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if catalog45_restored_backed_handoff(pair) {
            return Some(Self::SameCatalog {
                source: CATALOG45_PLACED_EXIT_DOORS,
                target: CATALOG45_BACKED_BENCH,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if pair == (CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE.2, CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS.2) {
            return Some(Self::SameCatalog {
                source: CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE,
                target: CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if pair == (CATALOG45_RENDERED_SCULPTURE_THUMBNAIL.2, CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX.2) {
            return Some(Self::SameCatalog {
                source: CATALOG45_RENDERED_SCULPTURE_THUMBNAIL,
                target: CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX,
                catalog: WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            });
        }
        if let Some((source, target)) = CATALOG40_WHEAT_DETAIL.zip(CATALOG44_WHEAT_TARGET)
            && pair == (source.2, target.2)
        {
            return Some(Self::Upgrade40(Catalog40Upgrade { source, target }));
        }
        if let Some(source) = CATALOG40_ARTWORK_REPLACEMENT_TARGET
            && pair == (source.2, CATALOG44_BC96_FRAME_LINEAGE_TARGET.2)
        {
            return Some(Self::Upgrade40(Catalog40Upgrade {
                source, target: CATALOG44_BC96_FRAME_LINEAGE_TARGET,
            }));
        }
        if let Some(source) = CATALOG44_WHEAT_TARGET
            && pair == (source.2, CATALOG44_HAYES_AUTO_TARGET.2)
        {
            return Some(Self::SameCatalog {
                source,
                target: CATALOG44_HAYES_AUTO_TARGET,
                catalog: WorkspaceFurnishingCatalog::HayesRelease044,
            });
        }
        match pair {
            CATALOG40_FRAME_TRANSITION => Some(Self::White),
            CATALOG40_TANGENT_TRANSITION => Some(Self::Tangent),
            _ => Self::upgrade_from_manifests(pair, CATALOG39_UPGRADE_TARGET)
                .or_else(|| Self::same_catalog_from_manifests(pair, CATALOG39_UPGRADE_TARGET, CATALOG40_EXB_TARGET))
                .or_else(|| Self::same_catalog_from_manifests(pair, CATALOG40_EXB_TARGET, CATALOG40_RESTORE_PROVENANCE_TARGET))
                .or_else(|| Self::same_catalog_from_manifests(pair, CATALOG40_RESTORE_PROVENANCE_TARGET, CATALOG40_FRAME_RECORD_TARGET))
                .or_else(|| Self::same_catalog_from_manifests(pair, CATALOG40_FRAME_RECORD_TARGET, CATALOG40_ARTWORK_REPLACEMENT_TARGET))
                .or_else(|| Self::same_catalog_from_manifests(pair, CATALOG40_WHEAT_GRANT, CATALOG40_WHEAT_DETAIL)),
        }
    }

    fn upgrade_from_manifests(pair: (&str, &str), target: Option<RuntimeTuple>) -> Option<Self> {
        let target = target?;
        (pair == (CATALOG39_UPGRADE_SOURCE.2, target.2)).then_some(Self::Upgrade39(Catalog39Upgrade(target)))
    }

    fn same_catalog_from_manifests(pair: (&str, &str), source: Option<RuntimeTuple>, target: Option<RuntimeTuple>) -> Option<Self> {
        let source = source?;
        let target = target?;
        (pair == (source.2, target.2)).then_some(Self::SameCatalog {
            source, target, catalog: WorkspaceFurnishingCatalog::MaterialMultimap040,
        })
    }

    #[cfg(test)]
    pub(crate) fn reviewed_catalog44_to_45() -> Self {
        Self::from_manifests((CATALOG44_BC96_FRAME_LINEAGE_TARGET.2, CATALOG45_BC96_REQUIRED_MODEL_TARGET.2))
            .expect("exact directed catalog 44 to 45 transition")
    }

    #[cfg(test)]
    pub(crate) fn reviewed_catalog40_to_44() -> Self {
        let source = CATALOG40_WHEAT_DETAIL.expect("reviewed catalog 40 source");
        let target = CATALOG44_WHEAT_TARGET.expect("reviewed ordinary catalog 44 target");
        assert!(Self::from_manifests((target.2, source.2)).is_none());
        assert!(Self::from_manifests((source.2, HAYES_RELEASE_PROBE_PIN.as_ref().unwrap().host.2)).is_none());
        Self::from_manifests((source.2, target.2)).expect("exact directed catalog 40 to 44 transition")
    }

    #[cfg(test)]
    pub(crate) fn test_upgrade() -> Self {
        Self::Upgrade39(Catalog39Upgrade((
            "aabb0000aabb0000aabb0000aabb0000aabb0000",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )))
    }

    #[cfg(test)]
    pub(crate) fn upgrades_catalog(self) -> bool {
        matches!(self, Self::Upgrade39(_) | Self::Upgrade40(_) | Self::Upgrade44 { .. } | Self::Upgrade49 { .. } | Self::Upgrade50 { .. })
    }

    pub(crate) fn source_catalog(self) -> WorkspaceFurnishingCatalog {
        match self {
            Self::Upgrade39(_) => WorkspaceFurnishingCatalog::MaterialPublication039,
            Self::Upgrade44 { .. } => WorkspaceFurnishingCatalog::HayesRelease044,
            Self::Upgrade49 { .. } | Self::Upgrade50 { .. } => WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            Self::Recover45 { .. } => WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            Self::SameCatalog { catalog, .. } => catalog,
            _ => WorkspaceFurnishingCatalog::MaterialMultimap040,
        }
    }

    pub(crate) fn target_catalog(self) -> WorkspaceFurnishingCatalog {
        match self {
            Self::Upgrade40(_) => WorkspaceFurnishingCatalog::HayesRelease044,
            Self::Upgrade44 { .. } => WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            Self::Upgrade49 { .. } => WorkspaceFurnishingCatalog::GpuModelPreparation049,
            Self::Upgrade50 { .. } => WorkspaceFurnishingCatalog::FurnishingThumbnail050,
            Self::Recover45 { .. } => WorkspaceFurnishingCatalog::RequiredModelPreparation045,
            Self::SameCatalog { catalog, .. } => catalog,
            _ => WorkspaceFurnishingCatalog::MaterialMultimap040,
        }
    }

    pub(crate) fn source(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::White => CATALOG40_FRAME_SOURCE,
            Self::Tangent => CATALOG40_FRAME_TARGET,
            Self::Upgrade39(_) => CATALOG39_UPGRADE_SOURCE,
            Self::Upgrade40(step) => step.source,
            Self::Upgrade44 { source, .. } | Self::Upgrade49 { source, .. } | Self::Upgrade50 { source, .. } => source,
            Self::Recover45 { source, .. } => source,
            Self::SameCatalog { source, .. } => source,
        }
    }

    pub(crate) fn target(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::White => CATALOG40_FRAME_TARGET,
            Self::Tangent => CATALOG40_TANGENT_TARGET,
            Self::Upgrade39(target) => target.0,
            Self::Upgrade40(step) => step.target,
            Self::Upgrade44 { target, .. } | Self::Upgrade49 { target, .. } | Self::Upgrade50 { target, .. } => target,
            Self::Recover45 { target, .. } => target,
            Self::SameCatalog { target, .. } => target,
        }
    }
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn gpu_model_wrapping_transition(pair: (&str, &str)) -> bool {
    pair == (GPU_MODEL_LEGACY_TARGET.2, GPU_MODEL_WRAPPING_TARGET.2)
        && gpu_model_host_matches(GPU_MODEL_LEGACY_TARGET)
        && gpu_model_host_matches(GPU_MODEL_WRAPPING_TARGET)
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_DRAFT2_OEX_SELECTED: RuntimeTuple = (
    "9297ba5b94a385a108fc4d9f356a7cc4b915a529",
    "f166f37f9323ea557bfc229feaa7b0f0e7b19472908f21ef3b5862578aedee25",
    "d8daa29b32a9fd27d932047e055286f5a78522d8d7b0cec5eb0f7abddfe706c2",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const CATALOG40_RETAINED_OEX_CLEANUP: RuntimeTuple = (
    "cff5a7eb4db9d71542c900689e644173a80d1160",
    "f166f37f9323ea557bfc229feaa7b0f0e7b19472908f21ef3b5862578aedee25",
    "ebd9295fdc72f21c128d0fc35a1e17ea7e1fb229386549608fc44e6997d4a292",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const BUNDLED_FURNISHINGS_VALIDATION: RuntimeTuple = (
    "ee26d6480e675f02427bde172fff47d354990125",
    "c8f2613ffe79d1aa1baf223201ec89b5fe38941d9b960a1317fded652b9df531",
    "41bda4a70ab5fd98be5d8f1fbbfa996a13acf2c3c00b81b9787b9992b11665ca",
);
#[cfg(debug_assertions)]
const WORKSPACE_MATERIAL_MULTIMAP_VALIDATION_PROVENANCE: &[(&str, &str, &str)] =
    &[CATALOG40_FRAME_SOURCE, CATALOG40_FRAME_TARGET, CATALOG40_TANGENT_TARGET];
#[cfg(not(debug_assertions))]
const WORKSPACE_MATERIAL_MULTIMAP_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];



#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceFurnishingCatalog {
    Schema036,
    Writer037,
    Publication038,
    MaterialPublication039,
    MaterialMultimap040,
    FramePreparation041,
    HayesOriginal042,
    HayesDisposition043,
    HayesRelease044,
    RequiredModelPreparation045,
    LargeModelPreparation046,
    FreshImagePreparation047,
    InfoStandPreparation048,
    GpuModelPreparation049,
    FurnishingThumbnail050,
}

impl WorkspaceFurnishingCatalog {
    pub(crate) fn version(self) -> u32 {
        match self {
            Self::Schema036 => 36,
            Self::Writer037 => 37,
            Self::Publication038 => 38,
            Self::MaterialPublication039 => 39,
            Self::MaterialMultimap040 => 40,
            Self::FramePreparation041 => 41,
            Self::HayesOriginal042 => 42,
            Self::HayesDisposition043 => 43,
            Self::HayesRelease044 => 44,
            Self::RequiredModelPreparation045 => 45,
            Self::LargeModelPreparation046 => 46,
            Self::FreshImagePreparation047 => 47,
            Self::InfoStandPreparation048 => 48,
            Self::GpuModelPreparation049 => 49,
            Self::FurnishingThumbnail050 => 50,
        }
    }

    pub(crate) fn uses_writer_lease(self) -> bool {
        matches!(self, Self::Writer037 | Self::Publication038 | Self::MaterialPublication039
            | Self::MaterialMultimap040 | Self::FramePreparation041 | Self::HayesOriginal042
            | Self::HayesDisposition043 | Self::HayesRelease044 | Self::RequiredModelPreparation045
            | Self::LargeModelPreparation046 | Self::FreshImagePreparation047 | Self::InfoStandPreparation048
            | Self::GpuModelPreparation049 | Self::FurnishingThumbnail050)
    }
}

const WORKSPACE_IMPORT_FORMAT_HINT: &str = "platform/apps/api/dist/oex-workspace-format-hint.js";
const WORKSPACE_IMPORT_PREFLIGHT: &str = "platform/apps/api/dist/oex-owned-workspace-preflight.js";
const WORKSPACE_IMPORT_MATERIALIZER: &str = "platform/apps/api/dist/oex-owned-workspace-chunk-materializer.js";
const WORKSPACE_IMPORT_PACKED: &str = "platform/apps/api/dist/oex-owned-workspace-packed-v2.js";
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V6_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-oex-import-capability-v6.json",
    775,
    "1d5177c8c4ddb9dec26ed7724178376ae4ca615e06e64573b132b7c7abfa25e8",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V7_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-oex-import-capability-v7.json",
    552,
    "7229cba9fbf98301960f44d39927c8d528ed55c3eb5e5fbdf203341261f342d5",
);
const WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-furnishing-selected-oex-v1.json",
    209,
    "2792fac7aba1e053a9a43849e2899ad27e447be9fc58114f316a8a4c9d23d0ce",
);
const WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-furnishing-selected-oex-039-v1.json",
    209,
    "4bded824d2c2eaab8be1f165903ed79b08f5182bb873065c9934226e135e6225",
);
const WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-furnishing-selected-oex-040-v1.json",
    209,
    "08eddd08e0a17921a0cc4ac733258b70580e400ced004a582dac3d057a22a979",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_WALL_SCHEMA_PATH: &str =
    "platform/database/native-oex-workspace-import-wall/035_workspace_material_wall_import.sql";

// Admit only an independently verified local runtime's full source, producer
// and manifest tuple. Source conformance alone does not populate this list.
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V6_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[];
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V7_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[(
    "d823896613e27c49d7a3aaeee570a527b2142350",
    "809ae0bd70b5702b325e0607324d53423474b453242e72a222a22fc08e970f34",
    "aad7ddf920dc64b02b916166e829d9638fca11b585cd0cb9157bc49a0f09e0d9",
)];

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn workspace_import_validation_matches(
    manifest: &Manifest,
    manifest_sha256: &str,
    version: WorkspaceImport,
    approved: &[(&str, &str, &str)],
) -> bool {
    manifest.mode == "local-validation" && !manifest.redistribution_qualified && !manifest.official_signed
        && approved.contains(&(manifest.source_commit.as_str(), manifest.producer_sha256.as_str(), manifest_sha256))
        && workspace_import_files_match(manifest, version, None)
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn workspace_import_files_match(
    manifest: &Manifest,
    version: WorkspaceImport,
    selected_marker: Option<(&str, u64, &str)>,
) -> bool {
    let regular_file = |path: &str| {
        manifest.files.iter().find(|file| file.path == path && file.link.is_none()
            && file.executable == Some(false))
    };
    let exact_file = |(path, bytes, hash): (&str, u64, &str)| {
        regular_file(path).is_some_and(|file| file.bytes == Some(bytes)
            && file.sha256.as_deref() == Some(hash))
    };
    exact_file(WORKSPACE_V6_CAPABILITY)
        && [WORKSPACE_IMPORT_FORMAT_HINT, WORKSPACE_IMPORT_MATERIALIZER, WORKSPACE_IMPORT_PREFLIGHT, WORKSPACE_IMPORT_PACKED]
            .into_iter().all(|path| regular_file(path).is_some())
        && match version {
            WorkspaceImport::V6 => selected_marker.is_none() && !manifest.files.iter().any(|file| {
                file.path == WORKSPACE_V7_CAPABILITY.0
                    || file.path.rsplit('/').next() == Some(WORKSPACE_WALL_SCHEMA_NAME)
            }),
            WorkspaceImport::V7 => exact_file(WORKSPACE_V7_CAPABILITY)
                && exact_file((WORKSPACE_WALL_SCHEMA_PATH, 3720, WORKSPACE_WALL_SCHEMA_SHA256))
                && selected_marker.is_none_or(exact_file),
        }
}

const NORMAL_OEX_CAPABILITY_PATH: &str = "platform/scripts/native-normal-oex-capability-v1.json";
const NORMAL_OEX_CAPABILITY_SHA256: &str = "f7573901d3884338eca4b262ad68b1b8379dfa84a9dd891fc44a174c5c453e57";
const NORMAL_OEX_CAPABILITY: &[u8] = br#"{"format":1,"kind":"exhibitos-native-normal-oex","protocol":"normal-oex-v1","ownerChannelVersion":2,"schemaPath":"platform/database/native-oex-normal/026_oex_file_profile.sql","schemaSHA256":"9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112","archiveFormat":"1.0.0-draft.9","maxArchiveBytes":536870912,"oneShot":true}"#;
// No official selected-OEX release provenance has yet been approved. Each
// future entry must bind reviewed source, producer and complete manifest hash.
const NORMAL_OEX_RELEASE_PROVENANCE: &[(&str, &str, &str)] = &[];
// Separate 027 release admission; empty until the producer and full payload
// are independently qualified. Capability presence alone grants nothing.
const NORMAL_OEX_027_RELEASE_PROVENANCE: &[(&str, &str, &str)] = &[];
// Development validation is separate from official release admission. Add only
// an independently checked complete local payload tuple; no caller/env override.
// Release builds never use this list, even if copied alongside a local payload.
const NORMAL_OEX_027_LOCAL_VALIDATION_PROVENANCE: &[(&str, &str, &str)] = &[
    // Independently checked managed-core supplier and complete PNG payload;
    // non-frontend inventory is byte-identical to the retained 661 packet.
    ("d630fcc2c3c079284c3febfe723cd08624ddb920",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "1ed982ce1701a7f1ef12b2c7d1fde065e648f5787e9b999bc60ef7d6a67b49be"),
    // DPR-corrected PNG payload; independently qualified with unchanged
    // non-frontend inventory and the same managed-core supplier.
    ("d9caa058175c684fad8e2070373497cc735cc8a8",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "1af44d49d98c097ef2c44549b0b83a4ab103c894fe39af1012a2edba29fae2dc"),
    // Final reviewed frontend integration also hides the editing light guide;
    // non-frontend inventory and the managed-core supplier remain unchanged.
    ("26b191c90ccee7045f234a058f6c30801b623035",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "115be25154dc989c9bfe4c0b5a94ed23b7ed9930afc9695cc8198480bc0b5dfe"),
    // Reviewed transient inspection-camera frontend; non-web inventory and
    // managed-core supplier are unchanged from the retained 115be packet.
    ("a99897a09b6c5b09a785cd063e6c60634c203364",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "8dcfd78c3d6b4a99f1a6e63440c44a4bc7c8e50324adab7b11e769b70b72266c"),
    // Exact loader-stage validation packet: complete inventory independently
    // checked, with non-web resources unchanged from the retained 8dc packet.
    // Renderer qualification and owning-app execution remain separate.
    ("a2ced9b583f24c36768d0160acde47a86c68bf2b",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "76f58e45e632154752bb513b7d20042e04308757bfc7a87eee195aed7b3a3992"),
    // Existing local loader stage callback; complete inventory independently
    // checked with non-web resources unchanged from the retained 76f58 packet.
    // Native observation remains separate from source/package qualification.
    ("7833eb206f45327b6c83741d553cb6c34c33ffee",
     "15021a235469473596f496c277543b17d243044fc3a31fdd37ad2d036d9bdd65",
     "7f287fb38599adb560496a6d78f5e9c975434ccc4e9247a483ac008791c208aa"),
    // Independently checked local ABI3 packet with the same managed-core
    // supplier and bounded Spec 11 preflight. Release lists stay separate.
    ("7d7a4829674e04ffa7c51ad2d59eddac7e59d1eb",
     "461aaeab80c9250a20ad4cbbf40a0352d0f3ac597cbe94cfcab9750c70c526d9",
     "85aaa2fef1458ccf68a44eaadbffd7d919a03311f7896ee6091543afd45475cd"),
    // Exact local dual-entry packet with independently verified inventory.
    ("735c7724c611441fcc1f17b3212c9f3dc270dc71",
     "3e1676fd354b4dd7bfa43fdf3b995062e810e4c91b2c4c34e647e76d5fddca8d",
     "10bb2250789b5b42ddb8c0ef5ef28c0634c7a31c7b7837f292380de1ee5867d3"),
    // The independently verified ICC receipt delta retains the exact ABI3 bytes.
    ("a393f38a9d0b48b1ce683db0cadaffb978c276cc",
     "3e1676fd354b4dd7bfa43fdf3b995062e810e4c91b2c4c34e647e76d5fddca8d",
     "253de8963ec9f0e0a2884a27d8e756b453cf61899e38c31571d9993a7d6ecff4"),
];
const NORMAL_OEX_027_PATH: &str = "platform/database/native-oex-normal/027_oex_exhibition_provenance.sql";
const NORMAL_OEX_027_SHA256: &str = "09bd82b3195e2f54545f6af45173bbab569effac95553a50f573745dab96f2c7";
const NORMAL_OEX_027_CAPABILITY_PATH: &str = "platform/scripts/native-normal-oex-027-capability-v3.json";
const NORMAL_OEX_027_CAPABILITY_SHA256: &str = "f9eecec7b17a7f010a94bcc8711326a44261c9713249b01231cc62b86b18eb6d";


// Explicit material-catalogue format admission; legacy capability bytes stay frozen.
const NORMAL_OEX_028_CAPABILITY_PATH:&str="platform/scripts/native-normal-oex-028-capability-v4.json";
const NORMAL_OEX_028_CAPABILITY_SHA256:&str="9ffbff5c770be6c70e5bbffd4183006003ed73dabb807e18f496a19af94a58b4";
const NORMAL_OEX_028_CAPABILITY:&[u8]=br#"{"format":1,"kind":"exhibitos-native-normal-oex-028","protocol":"normal-oex-028-v4","ownerChannelVersion":3,"baseSchemaPath":"platform/database/native-oex-normal/026_oex_file_profile.sql","baseSchemaSHA256":"9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112","provenanceSchemaPath":"platform/database/native-oex-normal/027_oex_exhibition_provenance.sql","provenanceSchemaSHA256":"09bd82b3195e2f54545f6af45173bbab569effac95553a50f573745dab96f2c7","schemaPath":"platform/database/native-oex-normal/028_oex_material_catalogue.sql","schemaSHA256":"1f88a8ed07a3814954d11d58ebe9b627f399a39f918e8eb838d94a5df4f5b9af","archiveFormats":["1.0.0-draft.9","1.0.0-draft.10"],"formatHintScript":"platform/apps/api/dist/oex-selected-format.js","maxArchiveBytes":536870912,"oneShot":true}"#;
const NORMAL_OEX_028_SCHEMA_PATH:&str="platform/database/native-oex-normal/028_oex_material_catalogue.sql";
pub(crate) const NORMAL_OEX_028_SCHEMA_SHA256:&str="1f88a8ed07a3814954d11d58ebe9b627f399a39f918e8eb838d94a5df4f5b9af";
const NORMAL_OEX_028_RELEASE_PROVENANCE:&[(&str,&str,&str)]=&[];
const NORMAL_OEX_028_LOCAL_VALIDATION_PROVENANCE:&[(&str,&str,&str)]=&[
    // Complete inventory independently checked; old Node/PostgreSQL/static
    // assets retained. Explicit formats and additive028 remain local-only.
    ("61bd336b3333ab917bc2588a63ff99cb56261fad",
     "1739c79934910d21a40d67509dda16fa2a86c6a45c3d78ef5dfda52214fed1f2",
     "4f4b584c98edfb54ce82da76bec53bad4e6b72e9c8389b0385e524d81db15a23"),
    ("7a2150bc4112044b313236ea081e5fa151a60ff8",
     "1739c79934910d21a40d67509dda16fa2a86c6a45c3d78ef5dfda52214fed1f2",
     "83d711e787ba01938739f2af95f83f578784dc6105db887799ac8355068f8483"),
    ("c40b79664defdfc3619a2237bbea4323e8d52216",
     "1739c79934910d21a40d67509dda16fa2a86c6a45c3d78ef5dfda52214fed1f2",
     "a2a83a968a2fab269cd43ba0c0f054dfe86ce1033135bd304964726c3676ea24"),
];
const NORMAL_OEX_029_FLOOR_CAPABILITY_PATH:&str="platform/scripts/native-normal-oex-029-floor-capability-v4.json";
const NORMAL_OEX_029_FLOOR_CAPABILITY_SHA256:&str="b8fef3df267e7cae39b5f6bb169d3ceb0efdad9050e615b7e8b33e640c29b649";
const NORMAL_OEX_029_FLOOR_CAPABILITY:&[u8]=br#"{"format":1,"kind":"exhibitos-native-normal-oex-floor-supplement","protocol":"normal-oex-028-v4","ownerChannelVersion":3,"parentCapabilityPath":"platform/scripts/native-normal-oex-028-capability-v4.json","parentCapabilitySHA256":"9ffbff5c770be6c70e5bbffd4183006003ed73dabb807e18f496a19af94a58b4","schemaPath":"platform/database/native-oex-normal/029_oex_quiet_oak_floor.sql","schemaSHA256":"d09e11450cdaa4e64f0b977cc17b1e468543314e6ce733571bc61b6b20deeb8a","packId":"quiet-oak-herringbone","packVersion":1,"freshOnly":true}"#;
const NORMAL_OEX_029_FLOOR_SCHEMA_PATH:&str="platform/database/native-oex-normal/029_oex_quiet_oak_floor.sql";
pub(crate) const NORMAL_OEX_029_FLOOR_SCHEMA_SHA256:&str="d09e11450cdaa4e64f0b977cc17b1e468543314e6ce733571bc61b6b20deeb8a";
// The frozen additional schema remains local-only. Historical028 tuples never
// opt into a schema upgrade merely because a newer payload exists.
const NORMAL_OEX_029_FLOOR_LOCAL_VALIDATION_PROVENANCE:&[(&str,&str,&str)]=&[
    ("b5b586b4f3e4757a12896083210513a5fa446cdc",
     "6b7753dbe3885bed579700011cddc738aa9bf9a5e8a246e57b46bf473969321e",
     "b751e43ed1968cd53aea2a9495a395d193b4d02ec48cd9e16ebae6806b4599b9"),
];
const NORMAL_OEX_030_CONCRETE_CAPABILITY_PATH: &str = "platform/scripts/native-normal-oex-030-concrete-capability-v4.json";
const NORMAL_OEX_030_CONCRETE_CAPABILITY_SHA256: &str = "3efa68edbbe474d5f873a09d6316d48909a53dc8239526e44ff4b99077125da0";
const NORMAL_OEX_030_CONCRETE_CAPABILITY_BYTES: u64 = 575;
const NORMAL_OEX_030_CONCRETE_SCHEMA_PATH: &str = "platform/database/native-oex-normal/030_oex_concrete_surface.sql";
pub(crate) const NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256: &str = "8c42cecb6a90c61eab4c72c4af81af42801db6bac60a567b61e69e7a31e22848";
const NORMAL_OEX_030_CONCRETE_LOCAL_VALIDATION_PROVENANCE: (&str, &str, &str) = (
    "4dc05911b32e2e627213e515a51b75aed406f395",
    "a256cc405532140d2e135a228c106619bedaa76caafd1898d8607db63b1cf9aa",
    "c9907e0733363fac0fa05848976913b5704d28ea8a7cb6bfcef343eb42af7031",
);

fn normal_oex_030_concrete_inventory_matches(manifest: &Manifest) -> bool {
    [
        (NORMAL_OEX_030_CONCRETE_CAPABILITY_PATH, NORMAL_OEX_030_CONCRETE_CAPABILITY_BYTES, NORMAL_OEX_030_CONCRETE_CAPABILITY_SHA256),
        (NORMAL_OEX_030_CONCRETE_SCHEMA_PATH, 519, NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256),
    ].iter().all(|&(path, bytes, sha)| manifest.files.iter().any(|item|
        item.path == path && item.link.is_none() && item.executable == Some(false)
            && item.bytes == Some(bytes) && item.sha256.as_deref() == Some(sha)))
}

fn normal_oex_028_inventory_matches(manifest:&Manifest)->bool {
    let regular=|name:&str|manifest.files.iter().find(|f|f.path==name&&f.link.is_none()&&f.executable==Some(false));
    normal_oex_inventory_matches(manifest,NORMAL_OEX_CAPABILITY)&&normal_oex_027_inventory_matches(manifest)
        && regular(NORMAL_OEX_028_CAPABILITY_PATH).is_some_and(|f|f.bytes==Some(NORMAL_OEX_028_CAPABILITY.len() as u64)&&f.sha256.as_deref()==Some(NORMAL_OEX_028_CAPABILITY_SHA256))
        && regular(NORMAL_OEX_028_SCHEMA_PATH).is_some_and(|f|f.bytes.is_some_and(|n|n>0)&&f.sha256.as_deref()==Some(NORMAL_OEX_028_SCHEMA_SHA256))
        && regular("platform/apps/api/dist/oex-selected-format.js").is_some_and(|f|f.bytes.is_some_and(|n|n>0)&&f.sha256.as_deref().is_some_and(hash_valid))
}
fn normal_oex_029_floor_inventory_matches(manifest:&Manifest)->bool {
    let regular=|name:&str|manifest.files.iter().find(|f|f.path==name&&f.link.is_none()&&f.executable==Some(false));
    normal_oex_028_inventory_matches(manifest)
        && regular(NORMAL_OEX_029_FLOOR_CAPABILITY_PATH).is_some_and(|f|f.bytes==Some(NORMAL_OEX_029_FLOOR_CAPABILITY.len() as u64)&&f.sha256.as_deref()==Some(NORMAL_OEX_029_FLOOR_CAPABILITY_SHA256))
        && regular(NORMAL_OEX_029_FLOOR_SCHEMA_PATH).is_some_and(|f|f.bytes==Some(566)&&f.sha256.as_deref()==Some(NORMAL_OEX_029_FLOOR_SCHEMA_SHA256))
}

fn normal_oex_release_declaration_matches(manifest: &Manifest, manifest_hash: &str, bytes: &[u8], approved: &[(&str, &str, &str)]) -> bool {
    if manifest.mode != "release" || !manifest.redistribution_qualified || !manifest.official_signed
        || !approved.iter().any(|(source, producer, hash)| *source == manifest.source_commit.as_str()
            && *producer == manifest.producer_sha256.as_str() && *hash == manifest_hash) {
        return false;
    }
    normal_oex_inventory_matches(manifest, bytes)
}

fn normal_oex_inventory_matches(manifest: &Manifest, bytes: &[u8]) -> bool {
    if bytes != NORMAL_OEX_CAPABILITY || digest(bytes) != NORMAL_OEX_CAPABILITY_SHA256 { return false; }
    let regular = |name: &str| manifest.files.iter().find(|f| f.path == name && f.link.is_none() && f.executable == Some(false));
    let Some(capability) = regular(NORMAL_OEX_CAPABILITY_PATH) else { return false };
    if capability.bytes != Some(NORMAL_OEX_CAPABILITY.len() as u64) || capability.sha256.as_deref() != Some(NORMAL_OEX_CAPABILITY_SHA256) { return false; }
    let Some(schema) = regular("platform/database/native-oex-normal/026_oex_file_profile.sql") else { return false };
    if schema.bytes != Some(1538) || schema.sha256.as_deref() != Some("9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112") { return false; }
    ["platform/scripts/native-oex-normal-catalog.mjs", "platform/apps/api/dist/native-fresh-oex-owner.js",
     "platform/apps/api/dist/native-selected-studio-put.js", "platform/apps/api/dist/oex-owned-chunk-materializer.js",
     "platform/apps/api/dist/oex-owned-packed-worker-v2.js"].iter().all(|name| regular(name).is_some())
}

fn normal_oex_027_declaration_matches(manifest: &Manifest, manifest_hash: &str, bytes: &[u8], approved: &[(&str, &str, &str)]) -> bool {
    normal_oex_release_declaration_matches(manifest, manifest_hash, bytes, approved)
        && normal_oex_027_inventory_matches(manifest)
}

fn normal_oex_027_inventory_matches(manifest: &Manifest) -> bool {
    let regular = |name: &str| manifest.files.iter().find(|f| f.path == name && f.link.is_none() && f.executable == Some(false));
    regular(NORMAL_OEX_027_PATH).is_some_and(|f| f.bytes == Some(2611) && f.sha256.as_deref() == Some(NORMAL_OEX_027_SHA256))
        && regular(NORMAL_OEX_027_CAPABILITY_PATH).is_some_and(|f| f.bytes == Some(516) && f.sha256.as_deref() == Some(NORMAL_OEX_027_CAPABILITY_SHA256))
        && regular("platform/apps/api/dist/native-oex-provenance.js").is_some_and(|f| f.bytes.is_some_and(|n| n > 0) && f.sha256.as_deref().is_some_and(hash_valid))
        && regular("platform/apps/api/dist/native-oex-catalog-proof.js").is_some_and(|f| f.bytes.is_some_and(|n| n > 0) && f.sha256.as_deref().is_some_and(hash_valid))
        && ["platform/apps/api/dist/oex-owned-preflight.js", "platform/apps/api/dist/oex-size-preflight.js"].iter().all(|name|
            regular(name).is_some_and(|f| f.bytes.is_some_and(|n| n > 0) && f.sha256.as_deref().is_some_and(hash_valid)))
}

fn normal_oex_027_local_validation_matches(manifest: &Manifest, manifest_hash: &str, bytes: &[u8], approved: &[(&str, &str, &str)]) -> bool {
    cfg!(all(debug_assertions, target_os = "macos"))
        && manifest.mode == "local-validation" && !manifest.redistribution_qualified && !manifest.official_signed
        && approved.iter().any(|(source, producer, hash)| *source == manifest.source_commit.as_str()
            && *producer == manifest.producer_sha256.as_str() && *hash == manifest_hash)
        && normal_oex_inventory_matches(manifest, bytes) && normal_oex_027_inventory_matches(manifest)
}

#[cfg(test)]
mod normal_oex_release_declaration_tests {
    use super::*;

    fn fixture() -> Manifest {
        let mut files = vec![
            serde_json::json!({"path":NORMAL_OEX_CAPABILITY_PATH,"bytes":NORMAL_OEX_CAPABILITY.len(),"sha256":NORMAL_OEX_CAPABILITY_SHA256,"executable":false}),
            serde_json::json!({"path":"platform/database/native-oex-normal/026_oex_file_profile.sql","bytes":1538,"sha256":"9e2a204ef9f24132f25ca2247415604c65f3f301c04e70a6a2a2facbc9122112","executable":false}),
        ];
        for path in ["platform/scripts/native-oex-normal-catalog.mjs", "platform/apps/api/dist/native-fresh-oex-owner.js",
            "platform/apps/api/dist/native-selected-studio-put.js", "platform/apps/api/dist/oex-owned-chunk-materializer.js",
            "platform/apps/api/dist/oex-owned-packed-worker-v2.js"] {
            files.push(serde_json::json!({"path":path,"bytes":1,"sha256":"a".repeat(64),"executable":false}));
        }
        serde_json::from_value(serde_json::json!({"format":"exhibitos-native-runtime-v1","mode":"release","platform":"darwin-arm64",
            "nodeVersion":"24.21.0","postgresVersion":"18.6","platformVersion":"0.1.0","protocolVersion":"1",
            "sourceCommit":"a".repeat(40),"producerSHA256":"b".repeat(64),
            "entry":{"node":"bin/node","script":"platform/scripts/local-runtime.mjs","postgres":"postgres/bin/postgres","initdb":"postgres/bin/initdb"},
            "files":files,"redistributionQualified":true,"officialSigned":true,"remaining":"synthetic policy test"})).unwrap()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn workspace_v5_validation_requires_exact_local_tuple_and_regular_marker() {
        for &(source, producer, hash) in WORKSPACE_V5_VALIDATION_PROVENANCE {
            let (path, bytes, sha) = WORKSPACE_V5_CAPABILITY;
            let mut manifest = fixture();
            manifest.source_commit = source.into();
            manifest.producer_sha256 = producer.into();
            assert!(!workspace_v5_validation_matches(&manifest, hash, false));
            manifest.mode = "local-validation".into();
            manifest.redistribution_qualified = false;
            manifest.official_signed = false;
            assert!(!workspace_v5_validation_matches(&manifest, hash, false));
            manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(sha.into()), executable: Some(false), link: None });
            assert!(workspace_v5_validation_matches(&manifest, hash, false));
            assert_eq!(workspace_v5_validation_matches(&manifest, hash, true), hash != WORKSPACE_WALL_PREVIEW_TRANSITION.1);
            assert!(!workspace_v5_validation_matches(&manifest, &"0".repeat(64), false));
            manifest.files.last_mut().unwrap().link = Some("other".into());
            assert!(!workspace_v5_validation_matches(&manifest, hash, false));
        }
    }

    fn local_fixture() -> Manifest {
        let mut m = fixture();
        m.mode = "local-validation".into(); m.redistribution_qualified = false; m.official_signed = false;
        for (path, bytes, sha) in [(NORMAL_OEX_027_PATH,2611,NORMAL_OEX_027_SHA256),
            (NORMAL_OEX_027_CAPABILITY_PATH,516,NORMAL_OEX_027_CAPABILITY_SHA256),
            ("platform/apps/api/dist/native-oex-provenance.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/native-oex-catalog-proof.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/oex-owned-preflight.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/oex-size-preflight.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")] {
            m.files.push(serde_json::from_value(serde_json::json!({"path":path,"bytes":bytes,"sha256":sha,"executable":false})).unwrap());
        }
        m
    }

    fn furnishing_ledger_file(path: String) -> Item {
        Item { path, bytes: Some(1), sha256: Some("b".repeat(64)), executable: Some(false), link: None }
    }

    fn furnishing_ledger_fixture() -> Payload {
        let mut payload = Payload { root: PathBuf::new(), manifest: local_fixture(), manifest_sha256: "a".repeat(64), workspace_v5_validation: false, workspace_import: None };
        for index in (1..=36).rev().filter(|index| !matches!(*index, 26 | 27)) {
            let dir = match index {
                1..=25 => "migrations",
                26..=32 => "native-oex-normal",
                33..=34 => "native-oex-workspace-import",
                35 => "native-oex-workspace-import-wall",
                _ => "native-workspace-furnishing",
            };
            payload.manifest.files.push(furnishing_ledger_file(format!("platform/database/{dir}/{index:03}_fixture.sql")));
        }
        payload
    }

    fn thumbnail_ledger_fixture() -> Payload {
        let mut payload = furnishing_ledger_fixture();
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_WRITER_SCHEMA.into()));
        for (path, bytes, hash) in [
            (WORKSPACE_FURNISHING_PUBLICATION_SCHEMA, WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES,
                WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256),
            WORKSPACE_MATERIAL_PUBLICATION_SCHEMA, WORKSPACE_MATERIAL_MULTIMAP_SCHEMA,
            WORKSPACE_FRAME_PREPARATION_SCHEMA, HAYES_ORIGINAL_SCHEMA, HAYES_DISPOSITION_SCHEMA,
            HAYES_RELEASE_SCHEMA, MODEL_PREPARATION_SCHEMA, FURNISHING_THUMBNAIL_SCHEMA,
        ] {
            payload.manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()),
                executable: Some(false), link: None });
        }
        payload
    }

    #[test]
    fn furnishing_thumbnail_050_is_parent45_plus_one_exact_migration() {
        let payload = thumbnail_ledger_fixture();
        let catalog = WorkspaceFurnishingCatalog::FurnishingThumbnail050;
        assert_eq!(catalog.version(), 50);
        assert!(catalog.uses_writer_lease());
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last().unwrap(), format!("050_workspace_furnishing_thumbnail.sql|{}", FURNISHING_THUMBNAIL_SCHEMA.2));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045).is_err());
    }

    #[test]
    fn furnishing_thumbnail_050_rejects_missing_parent_or_changed_migration() {
        let mut payload = thumbnail_ledger_fixture();
        payload.manifest.files.last_mut().unwrap().sha256 = Some("e".repeat(64));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FurnishingThumbnail050).is_err());
        payload.manifest.files.last_mut().unwrap().sha256 = Some(FURNISHING_THUMBNAIL_SCHEMA.2.into());
        payload.manifest.files.retain(|file| !file.path.ends_with("001_fixture.sql"));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FurnishingThumbnail050).is_err());
    }

    #[test]
    fn furnishing_thumbnail_050_rejects_046_047_048_and_unified_migration_mix() {
        for (path, bytes, hash) in [LARGE_MODEL_PREPARATION_SCHEMA, FRESH_IMAGE_PREPARATION_SCHEMA,
            INFO_STAND_PREPARATION_SCHEMA, ("platform/database/native-unified-light/049_light.sql", 1, "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")] {
            let mut payload = thumbnail_ledger_fixture();
            payload.manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()), executable: Some(false), link: None });
            assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FurnishingThumbnail050).is_err());
        }
    }

    #[test]
    fn furnishing_thumbnail_source_and_marker_do_not_admit_an_unpinned_manifest() {
        let tuple = ("55e8ad6c861eab44326ee8db60db2692e3f7baf7",
            "02a2e7eb9630eedad593edc04e6365ab263ee9f637d1aff994ba7dc093c356c5",
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        assert!(!furnishing_thumbnail_host_matches(tuple));
        assert!(furnishing_thumbnail_carried_receipt_payload(tuple).is_none());
        let mut payload = thumbnail_ledger_fixture();
        payload.manifest.source_commit = tuple.0.into(); payload.manifest.producer_sha256 = tuple.1.into();
        payload.manifest_sha256 = tuple.2.into();
        let (path, bytes, hash) = FURNISHING_THUMBNAIL_CAPABILITY;
        payload.manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()), executable: Some(false), link: None });
        assert!(payload.workspace_furnishing_catalog().is_err());
    }

    fn gpu_ledger_fixture() -> Payload {
        let mut payload = furnishing_ledger_fixture();
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_WRITER_SCHEMA.into()));
        for (path, bytes, hash) in [
            (WORKSPACE_FURNISHING_PUBLICATION_SCHEMA, WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES,
                WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256),
            WORKSPACE_MATERIAL_PUBLICATION_SCHEMA, WORKSPACE_MATERIAL_MULTIMAP_SCHEMA,
            WORKSPACE_FRAME_PREPARATION_SCHEMA, HAYES_ORIGINAL_SCHEMA, HAYES_DISPOSITION_SCHEMA,
            HAYES_RELEASE_SCHEMA, MODEL_PREPARATION_SCHEMA, GPU_MODEL_SCHEMA,
        ] {
            payload.manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()),
                executable: Some(false), link: None });
        }
        payload
    }

    #[test]
    fn gpu_model_049_is_parent45_plus_one_exact_migration() {
        let payload = gpu_ledger_fixture();
        let catalog = WorkspaceFurnishingCatalog::GpuModelPreparation049;
        assert_eq!(catalog.version(), 49);
        assert!(catalog.uses_writer_lease());
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last().unwrap(), format!("049_gpu_model_preparation.sql|{}", GPU_MODEL_SCHEMA.2));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045).is_err());
    }

    #[test]
    fn gpu_model_049_rejects_missing_parent_or_changed_migration() {
        let mut payload = gpu_ledger_fixture();
        payload.manifest.files.last_mut().unwrap().sha256 = Some("e".repeat(64));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::GpuModelPreparation049).is_err());
        payload.manifest.files.last_mut().unwrap().sha256 = Some(GPU_MODEL_SCHEMA.2.into());
        payload.manifest.files.retain(|file| !file.path.ends_with("001_fixture.sql"));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::GpuModelPreparation049).is_err());
    }

    #[test]
    fn gpu_model_049_rejects_046_047_048_and_unified_migration_mix() {
        for (path, bytes, hash) in [LARGE_MODEL_PREPARATION_SCHEMA, FRESH_IMAGE_PREPARATION_SCHEMA,
            INFO_STAND_PREPARATION_SCHEMA, FURNISHING_THUMBNAIL_SCHEMA, ("platform/database/native-unified-light/049_light.sql", 1, "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")] {
            let mut payload = gpu_ledger_fixture();
            payload.manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()), executable: Some(false), link: None });
            assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::GpuModelPreparation049).is_err());
        }
    }

    #[test]
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    fn gpu_model_runtime_admission_requires_the_exact_assembled_tuple() {
        assert_eq!(GPU_MODEL_TARGETS, &[GPU_MODEL_LEGACY_TARGET, GPU_MODEL_WRAPPING_TARGET]);
        let source = CATALOG45_DEFAULT_1_7_TARGET;
        for &actual in GPU_MODEL_TARGETS {
            assert!(gpu_model_host_matches(actual));
            for wrong in [(source.0, actual.1, actual.2), (actual.0, source.1, actual.2),
                (actual.0, actual.1, source.2), FURNISHING_THUMBNAIL_TARGETS[0]] {
                assert!(!gpu_model_host_matches(wrong));
            }
        }
        assert_eq!(GPU_MODEL_CAPABILITY.unwrap().0, GPU_MODEL_CAPABILITY_PATH);
        assert!(!GPU_MODEL_HELPER_FILES.unwrap().is_empty());
        let source = CATALOG45_DEFAULT_1_7_TARGET;
        assert!(!gpu_model_host_matches(source));
        assert!(gpu_model_carried_receipt_payload(source).is_none());
        assert!(FurnishingTransition::from_manifests((source.2, source.2)).is_none());
        let payload = gpu_ledger_fixture();
        assert!(payload.workspace_furnishing_catalog().is_err());
        assert!(payload.gpu_model_upgrade_script().is_err());
        assert!(payload.native_gpu_preview_resources().is_err());
    }

    #[test]
    fn workspace_furnishing_draft_payload_stays_closed_and_ledger_requires_all_rows() {
        let mut payload = furnishing_ledger_fixture();
        assert!(!payload.workspace_furnishing_validation().unwrap());
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_CAPABILITY.into()));
        assert_eq!(payload.workspace_furnishing_validation().unwrap_err().code, "NATIVE_RUNTIME_UNQUALIFIED");
        let ledger = payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::Schema036).unwrap();
        assert_eq!(ledger.lines().count(), 36);
        assert!(ledger.starts_with("001_fixture.sql|"));
        assert!(ledger.lines().last().unwrap().starts_with("036_fixture.sql|"));
        payload.manifest.files.push(furnishing_ledger_file("platform/database/native-workspace-furnishing/036_other.sql".into()));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::Schema036).is_err());
        payload.manifest.files.pop();
        payload.manifest.files.retain(|file| !file.path.ends_with("001_fixture.sql"));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::Schema036).is_err());
    }

    #[test]
    fn workspace_furnishing_writer_ledger_requires_exact_037_path_and_all_36_prior_rows() {
        let mut payload = furnishing_ledger_fixture();
        let writer = WorkspaceFurnishingCatalog::Writer037;
        assert!(payload.workspace_furnishing_ledger(writer).is_err());
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_WRITER_SCHEMA.into()));
        assert_eq!(payload.workspace_furnishing_validation().unwrap_err().code, "NATIVE_RUNTIME_UNQUALIFIED");
        let ledger = payload.workspace_furnishing_ledger(writer).unwrap();
        assert_eq!(ledger.lines().count(), 37);
        assert_eq!(ledger.lines().last().unwrap(), format!("037_workspace_furnishing_writer_generation.sql|{}", "b".repeat(64)));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::Schema036).is_err());
        payload.manifest.files.push(furnishing_ledger_file(format!("{WORKSPACE_FURNISHING_WRITER_DIRECTORY}037_other.sql")));
        assert!(payload.workspace_furnishing_ledger(writer).is_err());
        payload.manifest.files.pop();
        payload.manifest.files.retain(|file| !file.path.ends_with("035_fixture.sql"));
        assert!(payload.workspace_furnishing_ledger(writer).is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn workspace_furnishing_writer_needs_separate_exact_tuple_and_regular_marker() {
        const WRITER_FIXTURE_MARKER: &str = "platform/scripts/fixture-writer-capability.json";
        let mut payload = furnishing_ledger_fixture();
        let source = payload.manifest.source_commit.clone();
        let producer = payload.manifest.producer_sha256.clone();
        let manifest = payload.manifest_sha256.clone();
        let legacy_pin = [(source.as_str(), producer.as_str(), manifest.as_str())];
        let writer_pin = [(source.as_str(), producer.as_str(), manifest.as_str(), WRITER_FIXTURE_MARKER)];
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_CAPABILITY.into()));
        assert_eq!(payload.workspace_furnishing_catalog_for(&legacy_pin, &[], &[], &[], &[], &[]).unwrap(), Some(WorkspaceFurnishingCatalog::Schema036));
        assert!(payload.workspace_furnishing_catalog_for(&legacy_pin, &writer_pin, &[], &[], &[], &[]).is_err());
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_WRITER_SCHEMA.into()));
        assert!(payload.workspace_furnishing_catalog_for(&legacy_pin, &[], &[], &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog_for(&legacy_pin, &writer_pin, &[], &[], &[], &[]).is_err());
        payload.manifest.files.push(furnishing_ledger_file(WRITER_FIXTURE_MARKER.into()));
        assert_eq!(payload.workspace_furnishing_catalog_for(&legacy_pin, &writer_pin, &[], &[], &[], &[]).unwrap(), Some(WorkspaceFurnishingCatalog::Writer037));
        payload.manifest.producer_sha256.replace_range(0..1, "e");
        assert!(payload.workspace_furnishing_catalog_for(&legacy_pin, &writer_pin, &[], &[], &[], &[]).is_err());
        payload.manifest.producer_sha256 = producer.clone();
        payload.manifest.files.last_mut().unwrap().link = Some("other".into());
        assert!(payload.workspace_furnishing_catalog_for(&legacy_pin, &writer_pin, &[], &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog().is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn workspace_furnishing_publication_requires_its_own_exact_pin_marker_and_ledger() {
        let mut payload = furnishing_ledger_fixture();
        let source = payload.manifest.source_commit.clone();
        let producer = payload.manifest.producer_sha256.clone();
        let manifest = payload.manifest_sha256.clone();
        let publication_pin = [(source.as_str(), producer.as_str(), manifest.as_str())];
        let writer_pin = [(source.as_str(), producer.as_str(), manifest.as_str(),
            WORKSPACE_FURNISHING_PUBLICATION_WRITER_CAPABILITY)];
        let catalog = WorkspaceFurnishingCatalog::Publication038;
        for path in [WORKSPACE_FURNISHING_CAPABILITY, WORKSPACE_FURNISHING_PUBLICATION_WRITER_CAPABILITY,
            WORKSPACE_FURNISHING_WRITER_SCHEMA] {
            payload.manifest.files.push(furnishing_ledger_file(path.into()));
        }
        assert_eq!(payload.workspace_furnishing_catalog_for(&[], &writer_pin, &[], &[], &[], &[]).unwrap(),
            Some(WorkspaceFurnishingCatalog::Writer037));
        let mut marker = furnishing_ledger_file(WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY.into());
        marker.bytes = Some(WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_BYTES);
        marker.sha256 = Some(WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_SHA256.into());
        payload.manifest.files.push(marker);
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &publication_pin, &[], &[], &[]).is_err());

        let mut sql = furnishing_ledger_file(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA.into());
        sql.bytes = Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES);
        sql.sha256 = Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256.into());
        payload.manifest.files.push(sql);
        assert!(payload.workspace_furnishing_catalog().is_err());
        assert!(payload.workspace_furnishing_catalog_for(&[], &writer_pin, &[], &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog_for(&[], &writer_pin, &publication_pin, &[], &[], &[]).is_err());
        assert_eq!(payload.workspace_furnishing_catalog_for(&[], &[], &publication_pin, &[], &[], &[]).unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 38);
        assert_eq!(ledger.lines().last().unwrap(),
            format!("038_workspace_furnishing_publication.sql|{WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256}"));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::Writer037).is_err());

        payload.manifest.files.push(furnishing_ledger_file(format!("{WORKSPACE_FURNISHING_PUBLICATION_DIRECTORY}038_extra.sql")));
        assert!(payload.workspace_furnishing_ledger(catalog).is_err());
        payload.manifest.files.pop();
        payload.manifest.files.last_mut().unwrap().bytes = Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES + 1);
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &publication_pin, &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_ledger(catalog).is_err());
        payload.manifest.files.last_mut().unwrap().bytes = Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES);
        payload.manifest.files.last_mut().unwrap().sha256 = Some("f".repeat(64));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &publication_pin, &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_ledger(catalog).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the reviewed 040 and ordinary 044 runtime directories"]
    fn opens_reviewed_catalog40_to_44_packages_without_starting_a_profile() {
        let source = std::env::var_os("EXHIBITOS_TEST_CATALOG40_RUNTIME").unwrap();
        let target = std::env::var_os("EXHIBITOS_TEST_ORDINARY_CATALOG44_RUNTIME").unwrap();
        let source = Payload::open_saved_workspace_transition(Path::new(&source)).unwrap();
        let target = Payload::open_saved_workspace_transition(Path::new(&target)).unwrap();
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::MaterialMultimap040));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::HayesRelease044));
        let source_ledger = source.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::MaterialMultimap040).unwrap();
        assert_eq!(target.catalog40_upgrade_source_ledger().unwrap(), source_ledger);
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(target.hayes_session_probe_script().unwrap().is_none());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn workspace_furnishing_material_catalog_requires_exact_ancestry_and_pin() {
        use WorkspaceFurnishingCatalog::{FramePreparation041, MaterialMultimap040, MaterialPublication039, Publication038};
        let mut payload = furnishing_ledger_fixture();
        let source = payload.manifest.source_commit.clone();
        let producer = payload.manifest.producer_sha256.clone();
        let manifest = payload.manifest_sha256.clone();
        let pin = [(source.as_str(), producer.as_str(), manifest.as_str())];
        for path in [WORKSPACE_FURNISHING_CAPABILITY, WORKSPACE_FURNISHING_PUBLICATION_WRITER_CAPABILITY,
            WORKSPACE_FURNISHING_WRITER_SCHEMA] {
            payload.manifest.files.push(furnishing_ledger_file(path.into()));
        }
        for (path, bytes, sha256) in [
            (WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY, WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_BYTES,
                WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_SHA256),
            (WORKSPACE_FURNISHING_PUBLICATION_SCHEMA, WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES,
                WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256),
            WORKSPACE_MATERIAL_PUBLICATION_CAPABILITY,
            WORKSPACE_MATERIAL_PUBLICATION_SCHEMA,
        ] {
            payload.manifest.files.push(Item {
                path: path.into(), bytes: Some(bytes), sha256: Some(sha256.into()), executable: Some(false), link: None,
            });
        }
        assert_eq!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &pin, &[], &[]).unwrap(), Some(MaterialPublication039));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &pin, &[], &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &pin, &pin, &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog().is_err());
        let ledger = payload.workspace_furnishing_ledger(MaterialPublication039).unwrap();
        assert_eq!(ledger.lines().count(), 39);
        assert_eq!(ledger.lines().last().unwrap(), format!("039_workspace_material_publication.sql|{}", WORKSPACE_MATERIAL_PUBLICATION_SCHEMA.2));
        assert!(payload.workspace_furnishing_ledger(Publication038).is_err());
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY.0.into()));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &pin, &[], &[]).is_err());
        payload.manifest.files.pop();
        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY.0.into()));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &pin, &[], &[]).is_err());
        payload.manifest.files.pop();

        for (path, bytes, sha256) in [WORKSPACE_MATERIAL_MULTIMAP_CAPABILITY, WORKSPACE_MATERIAL_MULTIMAP_SCHEMA] {
            payload.manifest.files.push(Item {
                path: path.into(), bytes: Some(bytes), sha256: Some(sha256.into()), executable: Some(false), link: None,
            });
        }
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &pin, &[], &[]).is_err());
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &pin, &pin, &[]).is_err());
        assert_eq!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &pin, &[]).unwrap(), Some(MaterialMultimap040));
        let ledger = payload.workspace_furnishing_ledger(MaterialMultimap040).unwrap();
        assert_eq!(ledger.lines().count(), 40);
        assert_eq!(ledger.lines().last().unwrap(), format!("040_workspace_material_multimap.sql|{}", WORKSPACE_MATERIAL_MULTIMAP_SCHEMA.2));
        assert!(payload.workspace_furnishing_ledger(MaterialPublication039).is_err());

        payload.manifest.files.push(furnishing_ledger_file(WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY.0.into()));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &pin, &[]).is_err());
        payload.manifest.files.pop();

        for (path, bytes, sha256) in [WORKSPACE_FRAME_PREPARATION_CAPABILITY, WORKSPACE_FRAME_PREPARATION_SCHEMA] {
            payload.manifest.files.push(Item {
                path: path.into(), bytes: Some(bytes), sha256: Some(sha256.into()), executable: Some(false), link: None,
            });
        }
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &pin, &[]).is_err());
        assert_eq!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &[], &pin).unwrap(), Some(FramePreparation041));
        let ledger = payload.workspace_furnishing_ledger(FramePreparation041).unwrap();
        assert_eq!(ledger.lines().count(), 41);
        assert_eq!(ledger.lines().last().unwrap(), format!("041_workspace_frame_preparation.sql|{}", WORKSPACE_FRAME_PREPARATION_SCHEMA.2));
        assert!(payload.workspace_furnishing_ledger(MaterialMultimap040).is_err());

        let parent_inventory = payload.manifest.files.len();
        for (path, bytes, sha256) in [HAYES_ORIGINAL_CAPABILITY, HAYES_ORIGINAL_SCHEMA,
            HAYES_DISPOSITION_CAPABILITY, HAYES_DISPOSITION_SCHEMA] {
            payload.manifest.files.push(Item {
                path: path.into(), bytes: Some(bytes), sha256: Some(sha256.into()), executable: Some(false), link: None,
            });
        }
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &[], &pin).is_err());
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesOriginal042).is_err());
        let ledger = payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesDisposition043).unwrap();
        assert_eq!(ledger.lines().count(), 43);
        assert_eq!(ledger.lines().last().unwrap(), format!("043_hayes_output_stage.sql|{}", HAYES_DISPOSITION_SCHEMA.2));
        payload.manifest.files.last_mut().unwrap().bytes = Some(HAYES_DISPOSITION_SCHEMA.1 - 1);
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesDisposition043).is_err());
        payload.manifest.files.last_mut().unwrap().bytes = Some(HAYES_DISPOSITION_SCHEMA.1);
        for (path, bytes, sha256) in [HAYES_RELEASE_CAPABILITY, HAYES_RELEASE_SCHEMA] {
            payload.manifest.files.push(Item {
                path: path.into(), bytes: Some(bytes), sha256: Some(sha256.into()), executable: Some(false), link: None,
            });
        }
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &[], &pin).is_err());
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesDisposition043).is_err());
        let ledger = payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesRelease044).unwrap();
        assert_eq!(ledger.lines().count(), 44);
        assert_eq!(ledger.lines().last().unwrap(), format!("044_hayes_native_release.sql|{}", HAYES_RELEASE_SCHEMA.2));
        payload.manifest.files.last_mut().unwrap().sha256 = Some("f".repeat(64));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesRelease044).is_err());
        payload.manifest.files.truncate(parent_inventory);

        payload.manifest.files.last_mut().unwrap().sha256 = Some("f".repeat(64));
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &[], &pin).is_err());
        assert!(payload.workspace_furnishing_ledger(FramePreparation041).is_err());
        payload.manifest.files.truncate(payload.manifest.files.len() - 2);

        payload.manifest.files.retain(|file| file.path != WORKSPACE_MATERIAL_PUBLICATION_SCHEMA.0);
        assert!(payload.workspace_furnishing_catalog_for(&[], &[], &[], &[], &pin, &[]).is_err());
        assert!(payload.workspace_furnishing_ledger(MaterialMultimap040).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn catalog40_exb_refresh_requires_an_exact_directed_same_catalog_pair() {
        let source = CATALOG39_UPGRADE_TARGET.unwrap();
        let target = (
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222222222222222222222222222",
            "3333333333333333333333333333333333333333333333333333333333333333",
        );
        let transition = FurnishingTransition::same_catalog_from_manifests((source.2, target.2), Some(source), Some(target)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert_eq!(transition.source_catalog(), WorkspaceFurnishingCatalog::MaterialMultimap040);
        assert!(!transition.upgrades_catalog());
        assert!(FurnishingTransition::same_catalog_from_manifests((target.2, source.2), Some(source), Some(target)).is_none());
        assert!(FurnishingTransition::same_catalog_from_manifests((CATALOG40_TANGENT_TARGET.2, target.2), Some(source), Some(target)).is_none());
        assert!(FurnishingTransition::same_catalog_from_manifests((source.2, target.2), Some(source), None).is_none());
        let reviewed = CATALOG40_EXB_TARGET.unwrap();
        assert!(saved_workspace_transition_target(reviewed.2));
        assert_eq!(FurnishingTransition::from_manifests((source.2, reviewed.2)).unwrap().target(), reviewed);
        assert!(FurnishingTransition::from_manifests((reviewed.2, source.2)).is_none());
        let restore = CATALOG40_RESTORE_PROVENANCE_TARGET.unwrap();
        assert!(saved_workspace_transition_target(restore.2));
        assert_eq!(FurnishingTransition::from_manifests((reviewed.2, restore.2)).unwrap().source(), reviewed);
        assert!(FurnishingTransition::from_manifests((restore.2, reviewed.2)).is_none());
        assert!(FurnishingTransition::from_manifests((source.2, restore.2)).is_none());
        let frames = CATALOG40_FRAME_RECORD_TARGET.unwrap();
        let refresh = FurnishingTransition::from_manifests((restore.2, frames.2)).unwrap();
        assert_eq!(refresh.target(), frames);
        assert_eq!(refresh.source_catalog(), WorkspaceFurnishingCatalog::MaterialMultimap040);
        assert!(saved_workspace_transition_target(frames.2));
        assert!(FurnishingTransition::from_manifests((frames.2, restore.2)).is_none());
        assert!(FurnishingTransition::from_manifests((reviewed.2, frames.2)).is_none());
        let frame_preparation = WORKSPACE_FRAME_PREPARATION_VALIDATION_PROVENANCE[0];
        assert!(FurnishingTransition::from_manifests((frame_preparation.2, frames.2)).is_none());
        assert!(FurnishingTransition::from_manifests((frames.2, frame_preparation.2)).is_none());
        let replacement = CATALOG40_ARTWORK_REPLACEMENT_TARGET.unwrap();
        let transition = FurnishingTransition::from_manifests((frames.2, replacement.2)).unwrap();
        assert_eq!(transition.source(), frames);
        assert_eq!(transition.target(), replacement);
        assert!(!transition.upgrades_catalog());
        assert!(saved_workspace_transition_target(replacement.2));
        assert!(FurnishingTransition::from_manifests((restore.2, replacement.2)).is_none());
        assert!(FurnishingTransition::from_manifests((replacement.2, frames.2)).is_none());
        let grant = CATALOG40_WHEAT_GRANT.unwrap();
        let detail = CATALOG40_WHEAT_DETAIL.unwrap();
        let transition = FurnishingTransition::from_manifests((grant.2, detail.2)).unwrap();
        assert_eq!(transition.source(), grant);
        assert_eq!(transition.target(), detail);
        assert!(!transition.upgrades_catalog());
        assert!(saved_workspace_transition_target(detail.2));
        assert!(FurnishingTransition::from_manifests((detail.2, grant.2)).is_none());
        assert!(FurnishingTransition::from_manifests((replacement.2, detail.2)).is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires both exact sealed ordinary 044 packages"]
    fn opens_reviewed_hayes_auto_packages_without_starting_a_profile() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_ORDINARY_CATALOG44_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_AUTO_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::HayesRelease044;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        let source_context = source.context();
        let target_context = target.context();
        let pair = (source_context.manifest_sha256.as_str(), target_context.manifest_sha256.as_str());
        let transition = FurnishingTransition::from_manifests(pair).unwrap();
        assert_eq!(transition.source_catalog(), catalog);
        assert_eq!(transition.target_catalog(), catalog);
        assert!(!transition.upgrades_catalog());
        assert!(FurnishingTransition::from_manifests((pair.1, pair.0)).is_none());
        assert!(FurnishingTransition::from_manifests((CATALOG40_WHEAT_DETAIL.unwrap().2, pair.1)).is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact bc96 040 and registered-frame 044 packages"]
    fn opens_reviewed_bc96_frame_lineage_packages_without_starting_a_profile() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BC96_CATALOG40_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_FRAME_LINEAGE_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let from = WorkspaceFurnishingCatalog::MaterialMultimap040;
        let to = WorkspaceFurnishingCatalog::HayesRelease044;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(from));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(to));
        assert_eq!(source.workspace_furnishing_ledger(from).unwrap(), target.catalog40_upgrade_source_ledger().unwrap());
        let source_context = source.context();
        let target_context = target.context();
        let pair = (source_context.manifest_sha256.as_str(), target_context.manifest_sha256.as_str());
        let transition = FurnishingTransition::from_manifests(pair).unwrap();
        assert_eq!(transition.source_catalog(), from);
        assert_eq!(transition.target_catalog(), to);
        assert!(transition.upgrades_catalog());
        assert!(FurnishingTransition::from_manifests((pair.1, pair.0)).is_none());
        assert!(FurnishingTransition::from_manifests((CATALOG40_WHEAT_DETAIL.unwrap().2, pair.1)).is_none());
        assert!(FurnishingTransition::from_manifests((pair.0, CATALOG44_HAYES_AUTO_TARGET.2)).is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed bc96 044 and required-model 045 packages"]
    fn opens_reviewed_bc96_required_model_packages_without_starting_a_profile() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BC96_CATALOG44_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BC96_CATALOG45_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let from = WorkspaceFurnishingCatalog::HayesRelease044;
        let to = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(from));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(to));
        let source_ledger = source.workspace_furnishing_ledger(from).unwrap();
        assert_eq!(source_ledger, target.catalog44_upgrade_source_ledger().unwrap());
        assert_eq!(source_ledger.lines().count(), 44);
        assert_eq!(target.workspace_furnishing_ledger(to).unwrap().lines().count(), 45);
        assert!(!target.ordinary_hayes_release());

        let reopened = Payload::open_workspace_v5_validation(&target_root, false).unwrap();
        assert_eq!(reopened.context().manifest_sha256, target.context().manifest_sha256);
        assert!(!reopened.workspace_v5_selected_input());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());

        let source_context = source.context();
        let target_context = target.context();
        let pair = (source_context.manifest_sha256.as_str(), target_context.manifest_sha256.as_str());
        assert_eq!(FurnishingTransition::from_manifests(pair), Some(FurnishingTransition::reviewed_catalog44_to_45()));
        assert!(saved_workspace_transition_target(pair.1));
        assert!(FurnishingTransition::from_manifests((pair.1, pair.0)).is_none());
        assert!(FurnishingTransition::from_manifests((CATALOG44_HAYES_AUTO_TARGET.2, pair.1)).is_none());
        assert!(FurnishingTransition::from_manifests((pair.0, CATALOG44_HAYES_AUTO_TARGET.2)).is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed Hayes 044 and both catalog 045 packages"]
    fn opens_reviewed_compact_and_candidate_packages_without_starting_a_profile() {
        let hayes_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_CATALOG44_RUNTIME").unwrap());
        let bc96_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BC96_CATALOG45_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_COMPACT_CATALOG45_RUNTIME").unwrap());
        let hayes = Payload::open_saved_workspace_transition(&hayes_root).unwrap();
        let bc96 = Payload::open_saved_workspace_transition(&bc96_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let old = WorkspaceFurnishingCatalog::HayesRelease044;
        let current = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(hayes.workspace_furnishing_catalog().unwrap(), Some(old));
        assert_eq!(bc96.workspace_furnishing_catalog().unwrap(), Some(current));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(current));
        assert_eq!(hayes.workspace_furnishing_ledger(old).unwrap(), target.catalog44_upgrade_source_ledger().unwrap());
        assert_eq!(bc96.workspace_furnishing_ledger(current).unwrap(), target.workspace_furnishing_ledger(current).unwrap());
        let manifest = target.context().manifest_sha256;
        assert_eq!(manifest, CATALOG45_COMPACT_AND_CANDIDATE_TARGET.2);
        assert!(!target.ordinary_hayes_release());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(bc96.static_detail_export_resources(true).is_err());
        assert!(saved_workspace_transition_target(&manifest));
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
        assert_eq!(Payload::open_workspace_v5_validation(&target_root, false).unwrap().context().manifest_sha256, manifest);

        for (source, upgrade, catalog) in [(hayes, true, old), (bc96, false, current)] {
            let source_manifest = source.context().manifest_sha256;
            let step = FurnishingTransition::from_manifests((&source_manifest, &manifest)).unwrap();
            assert_eq!(step.upgrades_catalog(), upgrade);
            assert_eq!(step.source_catalog(), catalog);
            assert_eq!(step.target_catalog(), current);
            assert!(FurnishingTransition::from_manifests((&manifest, &source_manifest)).is_none());
        }
        assert!(FurnishingTransition::from_manifests((CATALOG44_BC96_FRAME_LINEAGE_TARGET.2, &manifest)).is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed combined and current-original 045 packages"]
    fn opens_reviewed_current_original_runtime_for_retained_catalog45() {
        let old_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_COMPACT_CATALOG45_RUNTIME").unwrap());
        let new_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CURRENT_ORIGINAL_CATALOG45_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&old_root).unwrap();
        let target = Payload::open_workspace_v5_validation(&new_root, false).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        let old = source.context().manifest_sha256;
        let new = target.context().manifest_sha256;
        let step = FurnishingTransition::from_manifests((&old, &new)).unwrap();
        assert!(!step.upgrades_catalog());
        assert_eq!(step.source_catalog(), catalog);
        assert_eq!(step.target_catalog(), catalog);
        assert!(saved_workspace_transition_target(&new));
        assert!(FurnishingTransition::from_manifests((&new, &old)).is_none());
        assert!(source.static_detail_export_resources(true).is_ok());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(Payload::open_workspace_v5_validation(&new_root, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed current-original and image-fit Wheat packages"]
    fn opens_reviewed_product_image_fit_wheat_transition() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CURRENT_ORIGINAL_CATALOG45_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PRODUCT_IMAGE_FIT_WHEAT_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        let old = source.context().manifest_sha256;
        let new = target.context().manifest_sha256;
        let step = FurnishingTransition::from_manifests((&old, &new)).unwrap();
        assert!(!step.upgrades_catalog());
        assert_eq!(step.source(), CATALOG45_CURRENT_ORIGINAL_TARGET);
        assert_eq!(step.target(), CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET);
        assert!(saved_workspace_transition_target(&new));
        assert!(FurnishingTransition::from_manifests((&new, &old)).is_none());
        assert!(FurnishingTransition::from_manifests((CATALOG45_PRODUCT_IMAGE.2, &new)).is_none());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert_eq!(Payload::open_workspace_v5_validation(&target_root, false).unwrap().context().manifest_sha256, new);
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact isolated large-model 046 package"]
    fn opens_reviewed_ordinary_large_model_catalog46() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_LARGE_MODEL_CATALOG46_RUNTIME").unwrap());
        let mut payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::LargeModelPreparation046;
        assert!(payload.ordinary_fresh_furnishing());
        assert!(!payload.fresh_image_recovery_enabled());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last(), Some(concat!(
            "046_large_model_preparation.sql|",
            "88d2840c9f948be4573e1a8682aa67c4225c087643273afb3b74b9cac8b558b4"
        )));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FreshImagePreparation047).is_err());
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!payload.normal_oex_selected_input().unwrap());
        payload.manifest_sha256 = "f".repeat(64);
        assert!(payload.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact isolated large-model 046 recovery package"]
    fn opens_reviewed_large_model_catalog46_recovery() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_LARGE_MODEL_RECOVERY_RUNTIME").unwrap());
        let mut payload = Payload::open(&root).unwrap();
        assert!(payload.large_model_recovery_validation());
        assert!(!payload.fresh_image_recovery_enabled());
        let catalog = WorkspaceFurnishingCatalog::LargeModelPreparation046;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 46);
        payload.manifest.files.retain(|file| file.path != LARGE_MODEL_RECOVERY_CAPABILITY.0);
        assert!(payload.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed catalog 045 sculpture thumbnail validation package"]
    fn opens_reviewed_rendered_sculpture_thumbnail_package() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_RENDERED_SCULPTURE_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.ordinary_fresh_furnishing());
        assert!(!payload.workspace_v5_selected_input());
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!saved_workspace_transition_target(&payload.context().manifest_sha256));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed ordinary saved-head and modular portal package"]
    fn opens_reviewed_saved_head_modular_portal_package() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_SAVED_HEAD_MODULAR_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.ordinary_fresh_furnishing());
        assert!(!payload.workspace_v5_selected_input());
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!saved_workspace_transition_target(&payload.context().manifest_sha256));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires both exact sealed Catalog 045 thumbnail packages"]
    fn admits_only_forward_saved_thumbnail_oex_transition() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_THUMBNAIL_OEX_SOURCE").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_THUMBNAIL_OEX_TARGET").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let from = source.context().manifest_sha256;
        let to = target.context().manifest_sha256;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(),
                   target.workspace_furnishing_ledger(catalog).unwrap());
        assert_eq!(FurnishingTransition::from_manifests((&from, &to)), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_RENDERED_SCULPTURE_THUMBNAIL,
            target: CATALOG45_RENDERED_SCULPTURE_THUMBNAIL_OEX,
            catalog,
        }));
        assert_eq!(FurnishingTransition::from_manifests((&to, &from)), None);
        assert_eq!(FurnishingTransition::from_manifests((&from, CATALOG45_SAVED_HEAD_MODULAR_PORTAL.2)), None);
        assert!(!saved_workspace_transition_target(&from));
        assert!(saved_workspace_transition_target(&to));
        assert!(!target.ordinary_fresh_furnishing());
        assert!(Payload::open_workspace_v5_validation(&target_root, false).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed catalog 040 package with bundled original furnishings"]
    fn opens_reviewed_bundled_furnishings_validation_package() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BUNDLED_FURNISHINGS_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::MaterialMultimap040;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 40);
        assert!(!payload.workspace_v5_selected_input());
        assert!(payload.retained_furnishing_catalog().is_none());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed selected recovery and combined catalog 045 packages"]
    fn opens_reviewed_catalog45_retained_product_image_transition() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RECOVERY_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RETAINED_PRODUCT_IMAGE_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let old = source.context().manifest_sha256;
        let new = target.context().manifest_sha256;
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert_eq!(FurnishingTransition::from_manifests((&old, &new)), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_SELECTED_RECOVERY_TARGET,
            target: CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET,
            catalog,
        }));
        for pair in [(&*new, &*old), (&*new, &*new), (CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET.2, &*new)] {
            assert!(!retained_product_image_transition(pair));
            assert!(!saved_workspace_transition_supported(pair));
        }
        assert!(!retained_product_image_transition((&old, &old)));
        assert!(source.catalog45_selected_recovery_validation());
        assert!(!target.catalog45_recovery_validation());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, false).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact f738 source and 160 MiB capacity payloads"]
    fn opens_reviewed_catalog45_native_publication_transition() {
        let old = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_STARTUP_CANCEL_RUNTIME").unwrap());
        let new = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_PUBLICATION_CAP_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&old).unwrap();
        let target = Payload::open_saved_workspace_transition(&new).unwrap();
        let source_manifest = source.manifest_sha256.as_str();
        let target_manifest = target.manifest_sha256.as_str();
        let pair = (source_manifest, target_manifest);
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(FurnishingTransition::from_manifests(pair), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET,
            target: CATALOG45_NATIVE_PUBLICATION_CAPACITY,
            catalog,
        }));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert!(saved_workspace_transition_target(target_manifest));
        assert!(!target.catalog45_recovery_validation());
        assert!(Payload::open_workspace_v5_validation(&new, false).is_ok());
        for rejected in [
            (target_manifest, source_manifest),
            (target_manifest, target_manifest),
            (source_manifest, CATALOG45_NATIVE_PUBLICATION_TARGETS[1].2),
            (CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET.2, target_manifest),
        ] {
            assert!(!saved_workspace_transition_supported(rejected));
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact reviewed catalog 045 carried restore runtime"]
    fn opens_reviewed_catalog45_carried_restore_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_CARRIED_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let context = payload.context();
        assert!(catalog45_carried_restore_payload(&payload));
        assert!(!payload.ordinary_fresh_furnishing());
        assert!(!catalog45_carried_restore_source(&payload));
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert!(payload.workspace_furnishing_ledger(catalog).is_ok());
        assert!(!catalog45_carried_restore_host_matches((
            &context.source_commit, payload.producer_sha256(),
            "ea2570531e6a988d1333f4f8a0898f972f75da24919a32e47c75fff638bdbf67",
        )));
        let other = CATALOG45_CARRIED_RESTORE_HOSTS.iter()
            .find(|host| host.0 != context.source_commit).unwrap();
        assert!(!catalog45_carried_restore_host_matches((
            &context.source_commit, payload.producer_sha256(), other.2,
        )));
        assert!(!catalog45_carried_restore_host_matches((
            &context.source_commit, "unreviewed-producer", &context.manifest_sha256,
        )));
        assert!(Payload::open_workspace_v5_validation(&root, false).is_err());
        if context.manifest_sha256 == CATALOG45_CARRIED_RESTORE_BASE.2 {
            assert!(payload.static_detail_export_resources(true).is_ok());
        } else {
            assert_eq!(payload.static_detail_export_resources(true).unwrap_err().code, "NATIVE_STATIC_DETAIL_UNAVAILABLE");
        }
        assert_eq!(payload.static_detail_export_resources(false).unwrap_err().code, "NATIVE_STATIC_DETAIL_UNAVAILABLE");
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local ordinary Catalog 045 model-adoption runtime"]
    fn opens_exact_native_model_adoption_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert!(payload.workspace_furnishing_ledger(catalog).is_ok());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        let (source, producer, manifest) = CATALOG45_MODEL_ADOPTION_VALIDATION;
        assert!(!catalog45_model_adoption_host_matches((source, producer, "unreviewed-manifest")));
        assert!(!catalog45_model_adoption_host_matches((source, "unreviewed-producer", manifest)));
        assert!(!catalog45_model_adoption_host_matches(("unreviewed-source", producer, manifest)));
        assert!(!catalog45_same_version_restore_host_matches((source, producer, manifest)));
        assert!(!catalog45_carried_restore_host_matches((source, producer, manifest)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn adoption_successor_allows_only_the_exact_source_and_carried_handoffs() {
        let successor = CATALOG45_MODEL_ADOPTION_CARRIED_TARGET;
        for (source, carried) in [
            (CATALOG45_CARRIED_RESTORE_BASE, true),
            (CATALOG45_CURRENT_ORIGINAL_TARGET, false),
        ] {
            let transition = FurnishingTransition::from_manifests((source.2, successor.2)).unwrap();
            assert_eq!(transition.source(), source);
            assert_eq!(transition.target(), successor);
            assert_eq!(transition.retains_catalog45_adoption_carried_restore(), carried);
            assert!(FurnishingTransition::from_manifests((successor.2, source.2)).is_none());
        }
        for unrelated in [CATALOG45_BC96_REQUIRED_MODEL_TARGET, CATALOG45_MODEL_ADOPTION_VALIDATION, successor] {
            assert!(FurnishingTransition::from_manifests((unrelated.2, successor.2)).is_none());
        }
        assert_eq!(catalog45_adoption_carried_receipt_payload(successor), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        assert!(!catalog45_carried_restore_host_matches(successor));
        assert!(!catalog45_same_version_restore_host_matches(successor));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local ordinary Catalog 045 original-adoption runtime"]
    fn opens_exact_native_model_adoption_original_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_ORIGINAL_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET;
        assert!(payload.native_model_adoption_original_target());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
        assert!(!catalog45_same_version_restore_host_matches(tuple));
        assert!(!catalog45_carried_restore_host_matches(tuple));
        for (source, carried) in [(CATALOG45_CARRIED_RESTORE_BASE, true),
            (CATALOG45_CURRENT_ORIGINAL_TARGET, false), (CATALOG45_MODEL_ADOPTION_CARRIED_TARGET, true)]
        {
            let transition = FurnishingTransition::from_manifests((source.2, tuple.2)).unwrap();
            assert_eq!(transition.source(), source);
            assert_eq!(transition.target(), tuple);
            assert_eq!(transition.retains_catalog45_adoption_carried_restore(), carried);
            assert!(FurnishingTransition::from_manifests((tuple.2, source.2)).is_none());
        }
        assert_eq!(catalog45_adoption_carried_receipt_payload(tuple), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn terminal_adoption_handoff_only_admits_the_current_exact_predecessor() {
        let source = CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET;
        let target = CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for predecessor in [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_CURRENT_ORIGINAL_TARGET, target] {
            assert!(FurnishingTransition::from_manifests((predecessor.2, target.2)).is_none());
        }
        assert!(FurnishingTransition::from_manifests((target.2, source.2)).is_none());
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local combined terminal-disposition runtime"]
    fn opens_exact_native_model_adoption_terminal_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_TERMINAL_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn rendered_thumbnail_adoption_handoff_only_admits_the_current_exact_predecessor() {
        let source = CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET;
        let target = CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for predecessor in [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_CURRENT_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, target] {
            assert!(FurnishingTransition::from_manifests((predecessor.2, target.2)).is_none());
        }
        assert!(FurnishingTransition::from_manifests((target.2, source.2)).is_none());
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local rendered artwork thumbnail runtime"]
    fn opens_exact_native_rendered_artwork_thumbnail_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_RENDERED_ARTWORK_THUMBNAIL_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn canonical_thumbnail_adoption_handoff_only_admits_the_current_exact_predecessor() {
        let source = CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET;
        let target = CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for predecessor in [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_CURRENT_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, target] {
            assert!(FurnishingTransition::from_manifests((predecessor.2, target.2)).is_none());
        }
        assert!(FurnishingTransition::from_manifests((target.2, source.2)).is_none());
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn portal_hayes_handoff_only_admits_the_current_exact_predecessor() {
        let source = CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET;
        let target = CATALOG45_PORTAL_HAYES_CARD_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        assert!(saved_workspace_transition_target(target.2));
        for predecessor in [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_CURRENT_ORIGINAL_TARGET,
            CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET,
            CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, target] {
            assert!(FurnishingTransition::from_manifests((predecessor.2, target.2)).is_none());
        }
        assert!(FurnishingTransition::from_manifests((target.2, source.2)).is_none());
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
        assert!(!catalog45_model_adoption_host_matches((target.0, source.1, target.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn scene_handoff_only_admits_current_exact_predecessors() {
        let target = CATALOG45_SCENE_AUTHORING_TARGET;
        for source in [CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET] {
            let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
            assert_eq!(transition.source(), source);
            assert_eq!(transition.target(), target);
            assert!(transition.retains_catalog45_adoption_carried_restore());
            assert!(transition.allows_catalog45_adoption_without_restore());
            assert!(FurnishingTransition::from_manifests((target.2, source.2)).is_none());
        }
        for source in [CATALOG45_MODEL_ADOPTION_VALIDATION, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_CURRENT_ORIGINAL_TARGET,
            CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET,
            CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, target] {
            assert!(FurnishingTransition::from_manifests((source.2, target.2)).is_none());
        }
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        assert!(saved_workspace_transition_target(target.2));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
        assert!(!catalog45_model_adoption_host_matches((target.0, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.1, target.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn complete_static_handoff_only_admits_current_exact_predecessor() {
        let source = CATALOG45_THUMBNAIL_FRESHNESS_TARGET;
        let target = CATALOG45_COMPLETE_STATIC_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert!(catalog45_model_adoption_host_matches(target));
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for pair in [(target.2, source.2), (target.2, target.2), (source.2, source.2),
                     (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        assert!(!catalog45_model_adoption_host_matches((source.0, target.1, target.2)));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced Server complete static runtime assembly copy"]
    fn opens_exact_complete_static_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_COMPLETE_STATIC_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_COMPLETE_STATIC_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn selected_png_read_admits_only_exact_clean_carried_source_successor() {
        let source = CATALOG45_SELECTED_PNG_READ_SOURCE;
        let target = CATALOG45_SELECTED_PNG_READ_TARGET;
        assert!(catalog45_markerless_ordinary_host_matches(target));
        assert!(catalog45_markerless_ordinary_host_matches(CATALOG45_DEFAULT_1_11_TARGET));
        assert!(!catalog45_markerless_ordinary_host_matches(source));
        assert!(!catalog45_markerless_ordinary_host_matches((source.0, target.1, target.2)));
        assert!(!catalog45_markerless_ordinary_host_matches((target.0, source.1, target.2)));
        assert!(!catalog45_markerless_ordinary_host_matches((target.0, target.1, source.2)));
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(!transition.allows_catalog45_adoption_without_restore());
        assert!(transition.startup_recovery().is_none());
        for tuple in [source, target] {
            assert!(catalog45_model_adoption_host_matches(tuple));
            assert_eq!(catalog45_adoption_carried_receipt_payload(tuple), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
            assert!(!catalog45_carried_restore_host_matches(tuple));
            assert!(!catalog45_same_version_restore_host_matches(tuple));
            assert!(!catalog45_recovery_host_matches(tuple));
            assert!(!gpu_model_host_matches(tuple));
            assert!(!large_model_host_matches(tuple));
            assert!(!furnishing_thumbnail_host_matches(tuple));
        }
        for pair in [(target.2, source.2), (target.2, target.2), (source.2, source.2),
                     (CATALOG45_DEFAULT_1_7_TARGET.2, target.2), (CATALOG45_DEFAULT_1_11_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        for mixed in [(source.0, target.1, target.2), (target.0, source.1, target.2), (target.0, target.1, source.2)] {
            assert!(!catalog45_model_adoption_host_matches(mixed));
        }
        assert!(saved_workspace_transition_target(target.2));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn current_image_thumbnail_admits_only_exact_clean_source_successor() {
        let source = CATALOG45_SELECTED_PNG_READ_TARGET;
        let target = CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET;
        assert!(catalog45_markerless_ordinary_host_matches(target));
        assert!(ordinary_fresh_catalog45_host_matches(target));
        assert!(catalog45_model_adoption_host_matches(target));
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source); assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(!transition.allows_catalog45_adoption_without_restore());
        assert!(transition.startup_recovery().is_none());
        for mixed in [(source.0, target.1, target.2), (target.0, "unqualified", target.2), (target.0, target.1, source.2)] {
            assert!(!catalog45_model_adoption_host_matches(mixed));
            assert!(!catalog45_markerless_ordinary_host_matches(mixed));
        }
        for pair in [(target.2, source.2), (target.2, target.2), (CATALOG45_SELECTED_PNG_READ_SOURCE.2, target.2),
            (CATALOG45_DEFAULT_1_11_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        assert!(!catalog45_carried_restore_host_matches(target));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_recovery_host_matches(target));
        assert!(!gpu_model_host_matches(target)); assert!(!large_model_host_matches(target));
        assert!(!furnishing_thumbnail_host_matches(target));
        assert!(saved_workspace_transition_target(target.2));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the separately qualified immutable current-image thumbnail runtime"]
    fn current_image_thumbnail_opens_exact_payload_without_profile() {
        let root = std::env::var_os("EXHIBITOS_TEST_CURRENT_IMAGE_THUMBNAIL_RUNTIME").expect("qualified runtime path");
        let payload = Payload::open_workspace_v5_validation(Path::new(&root), false).unwrap();
        let context = payload.context();
        assert_eq!((context.source_commit.as_str(), payload.producer_sha256(), context.manifest_sha256.as_str()), CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.native_model_adoption_root().unwrap().is_none());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(Payload::open_saved_workspace_transition(Path::new(&root)).is_ok());
        assert!(!payload.large_model_recovery_validation()); assert!(!payload.fresh_image_recovery_enabled());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the separately qualified immutable PR675 runtime"]
    fn opens_exact_selected_png_read_target_payload() {
        let root = std::env::var_os("EXHIBITOS_TEST_SELECTED_PNG_READ_RUNTIME").expect("qualified runtime path");
        let payload = Payload::open_workspace_v5_validation(Path::new(&root), false).unwrap();
        let context = payload.context();
        assert_eq!((context.source_commit.as_str(), payload.producer_sha256(), context.manifest_sha256.as_str()), CATALOG45_SELECTED_PNG_READ_TARGET);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.native_model_adoption_root().unwrap().is_none());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(!payload.large_model_recovery_validation());
        assert!(!payload.fresh_image_recovery_enabled());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn default_1_11_is_exact_ordinary_catalog45_without_transition_or_recovery() {
        let target = CATALOG45_DEFAULT_1_11_TARGET;
        assert!(ordinary_fresh_catalog45_host_matches(target));
        assert!(catalog45_model_adoption_host_matches(target));
        let old = CATALOG45_DEFAULT_1_7_TARGET;
        for mixed in [(old.0, target.1, target.2), (target.0, old.1, target.2),
                      (target.0, target.1, old.2)] {
            assert!(!ordinary_fresh_catalog45_host_matches(mixed));
        }
        assert!(!catalog45_recovery_host_matches(target));
        assert!(!large_model_host_matches(target));
        assert!(!gpu_model_host_matches(target));
        assert!(!furnishing_thumbnail_host_matches(target));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(catalog45_adoption_carried_receipt_payload(target).is_none());
        for pair in [(old.2, target.2), (target.2, old.2), (target.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn default_1_7_handoff_only_admits_current_exact_predecessor() {
        let source = CATALOG45_FIXTURE_AUTHORING_TARGET;
        let target = CATALOG45_DEFAULT_1_7_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert!(catalog45_model_adoption_host_matches(target));
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for pair in [(target.2, source.2), (target.2, target.2), (source.2, source.2),
                     (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2, target.2),
                     (CATALOG45_SCENE_AUTHORING_TARGET.2, target.2),
                     (CATALOG45_THUMBNAIL_FRESHNESS_TARGET.2, target.2),
                     (CATALOG45_COMPLETE_STATIC_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        assert!(!catalog45_model_adoption_host_matches((source.0, target.1, target.2)));
        assert!(!catalog45_model_adoption_host_matches((target.0, source.1, target.2)));
        assert!(!catalog45_model_adoption_host_matches((target.0, target.1, source.2)));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced Server default1.7 runtime assembly copy"]
    fn opens_exact_default_1_7_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_DEFAULT_1_7_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_DEFAULT_1_7_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn fixture_authoring_handoff_only_admits_current_exact_predecessor() {
        let source = CATALOG45_COMPLETE_STATIC_TARGET;
        let target = CATALOG45_FIXTURE_AUTHORING_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert!(catalog45_model_adoption_host_matches(target));
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for pair in [(target.2, source.2), (target.2, target.2), (source.2, source.2),
                     (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2, target.2),
                     (CATALOG45_SCENE_AUTHORING_TARGET.2, target.2),
                     (CATALOG45_THUMBNAIL_FRESHNESS_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        assert!(!catalog45_model_adoption_host_matches((source.0, target.1, target.2)));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced Server fixture authoring runtime assembly copy"]
    fn opens_exact_fixture_authoring_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_FIXTURE_AUTHORING_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_FIXTURE_AUTHORING_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    fn thumbnail_freshness_handoff_only_admits_current_exact_predecessor() {
        let source = CATALOG45_SCENE_AUTHORING_TARGET;
        let target = CATALOG45_THUMBNAIL_FRESHNESS_TARGET;
        let transition = FurnishingTransition::from_manifests((source.2, target.2)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert!(transition.retains_catalog45_adoption_carried_restore());
        assert!(transition.allows_catalog45_adoption_without_restore());
        assert!(catalog45_model_adoption_host_matches(target));
        assert_eq!(catalog45_adoption_carried_receipt_payload(target), Some(CATALOG45_CARRIED_RESTORE_BASE.2));
        for pair in [(target.2, source.2), (target.2, target.2), (source.2, source.2),
                     (CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET.2, target.2)] {
            assert!(FurnishingTransition::from_manifests(pair).is_none());
        }
        assert!(!catalog45_model_adoption_host_matches((source.0, target.1, target.2)));
        assert!(!catalog45_same_version_restore_host_matches(target));
        assert!(!catalog45_carried_restore_host_matches(target));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced Server thumbnail freshness runtime assembly copy"]
    fn opens_exact_thumbnail_freshness_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_THUMBNAIL_FRESHNESS_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_THUMBNAIL_FRESHNESS_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced Server scene authoring runtime assembly copy"]
    fn opens_exact_native_scene_authoring_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_SCENE_AUTHORING_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_SCENE_AUTHORING_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }


    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact produced portal plus Hayes card runtime assembly copy"]
    fn opens_exact_native_portal_hayes_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_PORTAL_HAYES_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_PORTAL_HAYES_CARD_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local canonical model plus thumbnail runtime"]
    fn opens_exact_native_canonical_model_thumbnail_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_CANONICAL_MODEL_THUMBNAIL_RUNTIME").unwrap());
        let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert!(payload.native_model_adoption_original_target());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local ordinary Catalog 045 carried-target adoption runtime"]
    fn opens_exact_native_model_adoption_carried_target_payload() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_CARRIED_RUNTIME").unwrap());
        let predecessor = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_CARRIED_PREDECESSOR").unwrap());
        let source = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_MODEL_ADOPTION_SOURCE_PREDECESSOR").unwrap());
        let payload = Payload::open_saved_workspace_transition(&root).unwrap();
        for (path, pin) in [(predecessor, CATALOG45_CARRIED_RESTORE_BASE), (source, CATALOG45_CURRENT_ORIGINAL_TARGET)] {
            let existing = Payload::open_saved_workspace_transition(&path).unwrap();
            assert_eq!(existing.context().manifest_sha256, pin.2);
            let transition = FurnishingTransition::from_manifests((pin.2, &payload.context().manifest_sha256)).unwrap();
            assert_eq!(transition.source(), pin);
        }
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert!(payload.workspace_furnishing_ledger(catalog).is_ok());
        assert_eq!(payload.native_model_adoption_root().unwrap(), Some(root.as_path()));
        let (source, producer, manifest) = CATALOG45_MODEL_ADOPTION_CARRIED_TARGET;
        assert_eq!((payload.context().source_commit.as_str(), payload.producer_sha256(),
            payload.context().manifest_sha256.as_str()), (source, producer, manifest));
        assert_eq!(catalog45_adoption_carried_receipt_payload((source, producer, "other")), None);
        assert_eq!(catalog45_adoption_carried_receipt_payload((source, "other", manifest)), None);
        assert_eq!(catalog45_adoption_carried_receipt_payload(("other", producer, manifest)), None);
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact publication capacity, authored-seat and museum-caption payloads"]
    fn opens_reviewed_catalog45_native_publication_payloads() {
        for (tuple, variable) in CATALOG45_NATIVE_PUBLICATION_TARGETS.iter().zip([
            "EXHIBITOS_TEST_NATIVE_PUBLICATION_CAP_RUNTIME",
            "EXHIBITOS_TEST_NATIVE_PUBLICATION_SEAT_RUNTIME",
            "EXHIBITOS_TEST_MUSEUM_CAPTIONS_RUNTIME",
        ]) {
            let root = PathBuf::from(std::env::var_os(variable).unwrap());
            let payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
            assert_eq!(payload.manifest_sha256, tuple.2);
            assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
            assert!(payload.ordinary_fresh_furnishing());
            assert!(!payload.catalog45_recovery_validation());
            assert!(payload.retained_furnishing_catalog().is_none());
            assert!(payload.static_detail_export_resources(false).is_ok());
            assert!(payload.static_detail_export_resources(true).is_ok());
            assert!(Payload::open_workspace_v5_validation(&root, true).is_err());
            assert!(!catalog45_native_publication_host_matches((tuple.0, tuple.1, CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET.2)));
        }
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact packaged default-seat runtime"]
    fn opens_catalog45_default_seats_for_ordinary_startup_only() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_DEFAULT_SEAT_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let pin = CATALOG45_DEFAULT_SEATS;
        let old = CATALOG45_PLACED_EXIT_DOORS;
        assert_eq!(payload.manifest_sha256, pin.2);
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.static_export_resources(true).is_ok());
        assert!(!ordinary_fresh_catalog45_host_matches((pin.0, old.1, pin.2)));
        assert!(!ordinary_fresh_catalog45_host_matches((pin.0, pin.1, old.2)));
        assert!(!catalog45_same_version_restore_host_matches(pin));
        assert!(!catalog45_recovery_host_matches(pin));
        assert!(Payload::open_saved_workspace_transition(&root).is_err());
        assert!(!saved_workspace_transition_supported((old.2, pin.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact backed-bench and catalogue 1.5 payload"]
    fn opens_catalog45_backed_bench_for_ordinary_startup_and_retained_restore_transition() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BACKED_BENCH_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let pin = CATALOG45_BACKED_BENCH;
        assert_eq!(payload.manifest_sha256, pin.2);
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.static_export_resources(true).is_ok());
        assert!(!ordinary_fresh_catalog45_host_matches((pin.0, pin.1, CATALOG45_DEFAULT_SEATS.2)));
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(!catalog45_same_version_restore_host_matches(pin));
        assert!(!catalog45_recovery_host_matches(pin));
        assert!(Payload::open_saved_workspace_transition(&root).is_ok());
        assert!(saved_workspace_transition_supported((CATALOG45_PLACED_EXIT_DOORS.2, pin.2)));
        assert!(!saved_workspace_transition_supported((pin.2, CATALOG45_PLACED_EXIT_DOORS.2)));
        assert!(!saved_workspace_transition_supported((CATALOG45_DEFAULT_SEATS.2, pin.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact bottom-catalog viewport payload"]
    fn opens_catalog45_bottom_catalog_for_ordinary_startup_only() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_BOTTOM_CATALOG_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let pin = CATALOG45_BOTTOM_CATALOG;
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(payload.manifest_sha256, pin.2);
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.static_export_resources(true).is_ok());
        assert!(!ordinary_fresh_catalog45_host_matches((pin.0, pin.1, CATALOG45_BACKED_BENCH.2)));
        assert!(!catalog45_same_version_restore_host_matches(pin));
        assert!(!catalog45_recovery_host_matches(pin));
        assert!(Payload::open_saved_workspace_transition(&root).is_err());
        assert!(!saved_workspace_transition_supported((CATALOG45_BACKED_BENCH.2, pin.2)));
        assert!(!saved_workspace_transition_supported((CATALOG45_PLACED_EXIT_DOORS.2, pin.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact museum-caption and prepared-sign/audio runtime payloads"]
    fn opens_reviewed_prepared_sign_audio_catalog45_pair() {
        let old = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_MUSEUM_CAPTIONS_RUNTIME").unwrap());
        let new = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PREPARED_SIGN_AUDIO_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&old).unwrap();
        let target = Payload::open_saved_workspace_transition(&new).unwrap();
        let pair = (source.manifest_sha256.as_str(), target.manifest_sha256.as_str());
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(catalog45_native_publication_transition(pair), Some((
            CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS, CATALOG45_PREPARED_SIGN_AUDIO,
        )));
        assert!(!saved_workspace_transition_supported((pair.1, pair.0)));
        assert!(target.ordinary_fresh_furnishing());
        assert_eq!(target.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert!(target.static_detail_export_resources(false).is_ok());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(Payload::open_workspace_v5_validation(&new, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact prepared-sign and pre-ready preservation runtime payloads"]
    fn opens_reviewed_catalog45_pre_ready_preservation_pair() {
        let source = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PREPARED_SIGN_AUDIO_RUNTIME").unwrap());
        let target = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PRE_READY_PRESERVATION_RUNTIME").unwrap());
        let old = Payload::open_saved_workspace_transition(&source).unwrap();
        let new = Payload::open_saved_workspace_transition(&target).unwrap();
        let pair = (old.manifest_sha256.as_str(), new.manifest_sha256.as_str());
        let transition = FurnishingTransition::from_manifests(pair).unwrap();
        assert_eq!(transition.startup_recovery(), Some(Catalog45StartupRecovery::PreReadyPreservation));
        assert!(matches!(transition, FurnishingTransition::Recover45 { .. }));
        assert!(!saved_workspace_transition_supported((pair.1, pair.0)));
        assert!(!saved_workspace_transition_supported((pair.0, pair.0)));
        assert!(new.catalog45_recovery_validation());
        assert!(!new.ordinary_fresh_furnishing());
        assert!(new.static_detail_export_resources(true).is_err());
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(old.workspace_furnishing_ledger(catalog).unwrap(), new.workspace_furnishing_ledger(catalog).unwrap());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact prepared-sign and pre-ready Hayes preservation runtime payloads"]
    fn opens_reviewed_catalog45_pre_ready_hayes_preservation_pair() {
        let source = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PREPARED_SIGN_AUDIO_RUNTIME").unwrap());
        let target = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PRE_READY_HAYES_PRESERVATION_RUNTIME").unwrap());
        let old = Payload::open_saved_workspace_transition(&source).unwrap();
        let new = Payload::open_saved_workspace_transition(&target).unwrap();
        let pair = (old.manifest_sha256.as_str(), new.manifest_sha256.as_str());
        let transition = FurnishingTransition::from_manifests(pair).unwrap();
        assert_eq!(transition.startup_recovery(), Some(Catalog45StartupRecovery::PreReadyHayesPreservation));
        assert!(!saved_workspace_transition_supported((pair.1, pair.0)));
        assert!(!saved_workspace_transition_supported((pair.1, pair.1)));
        assert!(!saved_workspace_transition_supported((CATALOG45_PRE_READY_PRESERVATION_TARGET.2, pair.1)));
        assert!(new.catalog45_recovery_validation());
        assert!(!new.ordinary_fresh_furnishing());
        assert!(new.static_detail_export_resources(false).is_err());
        assert!(new.static_detail_export_resources(true).is_err());
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(old.workspace_furnishing_ledger(catalog).unwrap(), new.workspace_furnishing_ledger(catalog).unwrap());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact isolated placed exit-door runtime payload"]
    fn opens_reviewed_placed_exit_door_runtime() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_EXIT_DOOR_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        assert_eq!(payload.manifest_sha256, CATALOG45_PLACED_EXIT_DOORS.2);
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
        assert!(payload.ordinary_fresh_furnishing());
        assert!(payload.static_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(true).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
        assert!(!saved_workspace_transition_supported((CATALOG45_PREPARED_SIGN_AUDIO.2, CATALOG45_PLACED_EXIT_DOORS.2)));
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact clean Hayes and placed exit-door runtime payloads"]
    fn opens_exact_clean_exit_handoff() {
        let source = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_PRE_READY_HAYES_PRESERVATION_RUNTIME").unwrap());
        let target = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_EXIT_DOOR_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source).unwrap();
        let target = Payload::open_saved_workspace_transition(&target).unwrap();
        let pair = (source.manifest_sha256.as_str(), target.manifest_sha256.as_str());
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(FurnishingTransition::from_manifests(pair), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET,
            target: CATALOG45_PLACED_EXIT_DOORS,
            catalog,
        }));
        assert!(!saved_workspace_transition_supported((pair.1, pair.0)));
        assert!(!source.ordinary_fresh_furnishing());
        assert!(target.ordinary_fresh_furnishing());
        assert!(source.static_detail_export_resources(true).is_err());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact combined catalog 045 and its sealed retained-image recovery successor"]
    fn opens_reviewed_catalog45_retained_image_recovery() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RETAINED_PRODUCT_IMAGE_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RETAINED_IMAGE_RECOVERY_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let mut target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let old = source.context().manifest_sha256;
        let new = target.context().manifest_sha256;
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert!(retained_product_image_recovery_transition((&old, &new)));
        assert!(!retained_product_image_transition((&old, &new)));
        for pair in [(&*new, &*old), (&*new, &*new), (CATALOG45_SELECTED_RECOVERY_TARGET.2, &*new)] {
            assert!(!saved_workspace_transition_supported(pair));
        }
        assert!(!source.catalog45_recovery_validation());
        assert!(target.catalog45_recovery_validation());
        assert!(!target.catalog45_selected_recovery_validation());
        assert!(target.static_detail_export_resources(true).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, false).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
        target.manifest.files.retain(|file| file.path != CATALOG45_RETAINED_IMAGE_RECOVERY_CAPABILITY.0);
        assert!(target.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires exact catalog 045 recovery and startup cancellation successor packages"]
    fn opens_reviewed_catalog45_startup_cancellation_recovery() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RETAINED_IMAGE_RECOVERY_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_STARTUP_CANCEL_RECOVERY_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&source_root).unwrap();
        let mut target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let old = source.context().manifest_sha256;
        let new = target.context().manifest_sha256;
        let transition = FurnishingTransition::from_manifests((&old, &new)).unwrap();
        assert!(matches!(transition, FurnishingTransition::Recover45 { .. }));
        assert_eq!(transition.startup_recovery(), Some(Catalog45StartupRecovery::Cancelled));
        let same = FurnishingTransition::from_manifests((&new, &new)).unwrap();
        assert_eq!(same.startup_recovery(), Some(Catalog45StartupRecovery::Unqualified));
        assert!(!retained_product_image_recovery_transition((&old, &new)));
        assert!(!saved_workspace_transition_supported((&new, &old)));
        assert!(saved_workspace_transition_target(&new));
        assert!(target.catalog45_recovery_validation() && !target.catalog45_selected_recovery_validation());
        assert!(!target.ordinary_fresh_furnishing());
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert!(Payload::open_workspace_v5_validation(&target_root, false).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
        target.manifest.files.retain(|file| file.path != CATALOG45_STARTUP_CANCEL_RECOVERY_CAPABILITY.0);
        assert!(target.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact local catalog 045 source and recovery packages"]
    fn opens_reviewed_catalog45_recovery_transition() {
        let source_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_SOURCE_RUNTIME").unwrap());
        let target_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_CATALOG45_RECOVERY_RUNTIME").unwrap());
        let source = Payload::open_workspace_v5_validation(&source_root, false).unwrap();
        let mut target = Payload::open_saved_workspace_transition(&target_root).unwrap();
        let source_manifest = source.context().manifest_sha256;
        let target_manifest = target.context().manifest_sha256;
        let transition = FurnishingTransition::from_manifests((&source_manifest, &target_manifest)).unwrap();
        assert!(matches!(transition, FurnishingTransition::Recover45 { .. }));
        assert!(FurnishingTransition::from_manifests((&target_manifest, &source_manifest)).is_none());
        let same_payload = FurnishingTransition::from_manifests((&target_manifest, &target_manifest));
        if target.catalog45_selected_recovery_validation() {
            let settled_recovery = same_payload.unwrap();
            assert_eq!(settled_recovery.source(), settled_recovery.target());
            assert!(matches!(settled_recovery, FurnishingTransition::Recover45 { .. }));
        } else {
            assert!(same_payload.is_none());
        }
        assert!(!source.catalog45_recovery_validation());
        assert!(target.catalog45_recovery_validation());
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        assert_eq!(source.workspace_furnishing_ledger(catalog).unwrap(), target.workspace_furnishing_ledger(catalog).unwrap());
        assert!(Payload::open_workspace_v5_validation(&target_root, false).is_ok());
        assert!(Payload::open_workspace_v5_validation(&target_root, true).is_err());
        target.manifest.files.retain(|file| file.path != CATALOG45_RECOVERY_CAPABILITY.0
            && file.path != CATALOG45_SELECTED_RECOVERY_CAPABILITY.0);
        assert!(target.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed isolated fresh-image 047 package"]
    fn opens_reviewed_ordinary_fresh_image_catalog47() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_FRESH_IMAGE_CATALOG47_RUNTIME").unwrap());
        let mut payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::FreshImagePreparation047;
        let manifest = payload.context().manifest_sha256;
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last(), Some(concat!(
            "047_fresh_image_preparation.sql|",
            "682a7bf2f07a3b43cb8c398758c37568723c85abc372be1f374a59a29f474bf7"
        )));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045).is_err());
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!saved_workspace_transition_target(&manifest));
        assert!(!workspace_v5_validation_matches(&payload.manifest, &manifest, true));
        assert!(!payload.normal_oex_selected_input().unwrap());
        assert!(payload.static_detail_export_resources(false).is_err());
        payload.manifest_sha256 = "f".repeat(64);
        assert!(payload.workspace_furnishing_catalog().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed default frame 045 package"]
    fn opens_reviewed_default_frame_runtime_for_ordinary_catalog45() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_DEFAULT_FRAME_CATALOG45_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let manifest = payload.context().manifest_sha256;
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        assert_eq!(payload.workspace_furnishing_ledger(catalog).unwrap().lines().count(), 45);
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!saved_workspace_transition_target(&manifest));
        assert!(!workspace_v5_validation_matches(&payload.manifest, &manifest, true));
        assert!(!payload.normal_oex_selected_input().unwrap());
        assert!(payload.static_export_resources(false).is_ok());
        assert!(payload.static_detail_export_resources(false).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact assembled ordinary GPU049 runtime; read-only opener only"]
    fn opens_actual_gpu_model049_payload_without_starting_a_workspace() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_GPU_MODEL049_RUNTIME")
            .expect("explicit exact GPU049 runtime path required"));
        let mut payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = GPU_MODEL_WRAPPING_TARGET;
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        let catalog = WorkspaceFurnishingCatalog::GpuModelPreparation049;
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last().unwrap(), format!("049_gpu_model_preparation.sql|{}", GPU_MODEL_SCHEMA.2));
        assert_eq!(payload.catalog49_upgrade_source_ledger().unwrap().lines().count(), 45);
        assert_eq!(payload.gpu_model_upgrade_script().unwrap(), root.join("platform/scripts/local-runtime.mjs"));
        let resources = payload.native_gpu_preview_resources().unwrap();
        assert_eq!(resources["payloadManifestSha256"], tuple.2);
        assert_eq!(resources["format"], 1);
        let paths = resources["paths"].as_array().unwrap();
        assert!((5..=260).contains(&paths.len()));
        for fixed in GPU_PREVIEW_FIXED {
            assert!(paths.iter().any(|path| path.as_str() == Some(fixed)));
        }
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!payload.workspace_v5_selected_input());
        assert!(payload.native_model_adoption_root().unwrap().is_none());
        let step = FurnishingTransition::from_manifests((CATALOG45_DEFAULT_1_7_TARGET.2, tuple.2)).unwrap();
        assert!(matches!(step, FurnishingTransition::Upgrade49 { .. }));
        assert_eq!(step.source(), CATALOG45_DEFAULT_1_7_TARGET);
        assert_eq!(step.target(), tuple);
        assert!(FurnishingTransition::from_manifests((tuple.2, CATALOG45_DEFAULT_1_7_TARGET.2)).is_none());
        payload.manifest_sha256 = "e".repeat(64);
        assert!(payload.workspace_furnishing_catalog().is_err());
        assert!(payload.gpu_model_upgrade_script().is_err());
        assert!(payload.native_gpu_preview_resources().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact assembled furnishing-thumbnail050 runtime; read-only opener only"]
    fn opens_actual_furnishing_thumbnail050_payload_without_starting_a_workspace() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_FURNISHING_THUMBNAIL050_RUNTIME")
            .expect("explicit exact thumbnail050 runtime path required"));
        let mut payload = Payload::open_workspace_v5_validation(&root, false).unwrap();
        let tuple = FURNISHING_THUMBNAIL_TARGETS[0];
        assert_eq!(FURNISHING_THUMBNAIL_TARGETS.len(), 1);
        assert_eq!(payload.context().source_commit, tuple.0);
        assert_eq!(payload.producer_sha256(), tuple.1);
        assert_eq!(payload.context().manifest_sha256, tuple.2);
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::FurnishingThumbnail050));
        let ledger = payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FurnishingThumbnail050).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert_eq!(ledger.lines().last().unwrap(), format!("050_workspace_furnishing_thumbnail.sql|{}", FURNISHING_THUMBNAIL_SCHEMA.2));
        assert_eq!(payload.catalog50_upgrade_source_ledger().unwrap().lines().count(), 45);
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045).is_err());
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(!payload.workspace_v5_selected_input());
        assert!(payload.native_model_adoption_root().unwrap().is_none());
        assert!(payload.manifest.files.iter().all(|file| {
            !file.path.starts_with(LARGE_MODEL_PREPARATION_DIRECTORY)
                && !file.path.starts_with(FRESH_IMAGE_PREPARATION_DIRECTORY)
                && !file.path.starts_with(INFO_STAND_PREPARATION_DIRECTORY)
                && file.path != "platform/scripts/native-model-adoption-capability-v1.json"
                && file.path != "platform/scripts/native-model-adoption-carried-target-capability-v1.json"
                && !(file.path.starts_with("platform/database/")
                    && file.path.ends_with(".sql")
                    && file.path.rsplit('/').next().and_then(|name| name.split_once('_'))
                        .and_then(|(number, _)| number.parse::<u32>().ok()).is_some_and(|number| (46..=49).contains(&number)))
        }));
        assert_eq!(payload.furnishing_thumbnail_upgrade_script().unwrap(), root.join("platform/scripts/local-runtime.mjs"));
        let step = FurnishingTransition::from_manifests((CATALOG45_DEFAULT_1_7_TARGET.2, tuple.2)).unwrap();
        assert!(matches!(step, FurnishingTransition::Upgrade50 { .. }));
        assert_eq!(step.source(), CATALOG45_DEFAULT_1_7_TARGET);
        assert_eq!(step.target(), tuple);
        assert_eq!(step.source_catalog(), WorkspaceFurnishingCatalog::RequiredModelPreparation045);
        assert_eq!(step.target_catalog(), WorkspaceFurnishingCatalog::FurnishingThumbnail050);
        assert!(saved_workspace_transition_target(tuple.2));
        assert!(FurnishingTransition::from_manifests((tuple.2, CATALOG45_DEFAULT_1_7_TARGET.2)).is_none());
        payload.manifest_sha256 = "e".repeat(64);
        assert!(payload.workspace_furnishing_catalog().is_err());
        assert!(payload.furnishing_thumbnail_upgrade_script().is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact 045 plus 048 info-stand package"]
    fn opens_reviewed_info_stand_preparation_runtime() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_INFO_STAND_PREPARATION_RUNTIME").unwrap());
        let payload = Payload::open(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::InfoStandPreparation048;
        assert_eq!(payload.manifest_sha256, CATALOG48_INFO_STAND_PREPARATION.2);
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 46);
        assert!(ledger.lines().last().unwrap().starts_with("048_workspace_info_stand_preparation.sql|"));
        assert!(payload.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045).is_err());
        assert!(!saved_workspace_transition_target(&payload.manifest_sha256));
        assert!(payload.retained_furnishing_catalog().is_none());
        assert!(payload.static_detail_export_resources(true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact original and corrected museum captions and publication capacity payloads"]
    fn opens_reviewed_museum_captions_runtime_for_ordinary_catalog45() {
        let original = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_MUSEUM_CAPTIONS_SOURCE_RUNTIME").unwrap());
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_MUSEUM_CAPTIONS_RUNTIME").unwrap());
        let source = Payload::open_saved_workspace_transition(&original).unwrap();
        let payload = Payload::open_saved_workspace_transition(&root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::RequiredModelPreparation045;
        let old = source.manifest_sha256.as_str();
        let new = payload.manifest_sha256.as_str();
        assert_eq!(old, CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE.2);
        assert_eq!(new, CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS.2);
        assert!(payload.ordinary_fresh_furnishing());
        assert_eq!(payload.workspace_furnishing_catalog().unwrap(), Some(catalog));
        let ledger = payload.workspace_furnishing_ledger(catalog).unwrap();
        assert_eq!(ledger.lines().count(), 45);
        assert_eq!(ledger, source.workspace_furnishing_ledger(catalog).unwrap());
        assert_eq!(FurnishingTransition::from_manifests((old, new)), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE,
            target: CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS,
            catalog,
        }));
        assert!(saved_workspace_transition_supported((old, new)));
        for pair in [(new, old), (old, old), (new, new), (CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET.2, new)] {
            assert!(!saved_workspace_transition_supported(pair));
        }
        assert!(!saved_workspace_transition_target(old));
        assert!(saved_workspace_transition_target(new));
        assert!(!workspace_v5_validation_matches(&payload.manifest, new, true));
        assert!(!payload.normal_oex_selected_input().unwrap());

        let publication_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_NATIVE_PUBLICATION_CAP_RUNTIME").unwrap());
        let publication = Payload::open_saved_workspace_transition(&publication_root).unwrap();
        assert_eq!(publication.manifest_sha256, CATALOG45_NATIVE_PUBLICATION_CAPACITY.2);
        assert_eq!(ledger, publication.workspace_furnishing_ledger(catalog).unwrap());
        assert_eq!(FurnishingTransition::from_manifests((publication.manifest_sha256.as_str(), new)), Some(FurnishingTransition::SameCatalog {
            source: CATALOG45_NATIVE_PUBLICATION_CAPACITY,
            target: CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS,
            catalog,
        }));
        assert!(!saved_workspace_transition_supported((new, publication.manifest_sha256.as_str())));
        assert!(payload.static_detail_export_resources(true).is_ok());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed draft.2 OEX selected 040 package"]
    fn opens_reviewed_draft2_oex_package_for_selection_and_reopen() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_DRAFT2_OEX_SELECTED_RUNTIME").unwrap());
        let selected = Payload::open_workspace_import_validation(&root, WorkspaceImport::V7, true).unwrap();
        assert_eq!(selected.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::MaterialMultimap040));
        assert!(matches!(selected.selected_workspace_import_version(), Some(WorkspaceImport::V7)));
        assert!(selected.admits_selected_oex_format("1.0.0-draft.12").unwrap());
        assert!(!selected.admits_selected_oex_format("1.0.0-draft.10").unwrap());

        let reopened = Payload::open_workspace_import_validation(&root, WorkspaceImport::V7, false).unwrap();
        assert_eq!(reopened.workspace_furnishing_catalog().unwrap(), Some(WorkspaceFurnishingCatalog::MaterialMultimap040));
        assert!(!reopened.workspace_import_selected_input());
        assert_eq!(reopened.context().manifest_sha256, CATALOG40_DRAFT2_OEX_SELECTED.2);
        assert_eq!(reopened.context().manifest_sha256, selected.context().manifest_sha256);
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    #[test]
    #[ignore = "requires the exact sealed source and cleanup catalog 040 packages"]
    fn opens_reviewed_retained_oex_cleanup_refresh() {
        let old_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_DRAFT2_OEX_SELECTED_RUNTIME").unwrap());
        let new_root = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_RETAINED_OEX_CLEANUP_RUNTIME").unwrap());
        let old = Payload::open_saved_workspace_transition(&old_root).unwrap();
        let new = Payload::open_saved_workspace_transition(&new_root).unwrap();
        let catalog = WorkspaceFurnishingCatalog::MaterialMultimap040;
        assert_eq!(old.workspace_furnishing_ledger(catalog).unwrap(), new.workspace_furnishing_ledger(catalog).unwrap());
        assert!(!old.retained_oex_cleanup_hook());
        assert!(new.retained_oex_cleanup_hook());
        let old_hash = old.context().manifest_sha256;
        let new_hash = new.context().manifest_sha256;
        let step = FurnishingTransition::from_manifests((&old_hash, &new_hash)).unwrap();
        assert_eq!(step.source_catalog(), catalog);
        assert_eq!(step.target_catalog(), catalog);
        assert!(!step.upgrades_catalog());
        assert!(saved_workspace_transition_target(&new_hash));
        assert!(FurnishingTransition::from_manifests((&new_hash, &old_hash)).is_none());
        let reopened = Payload::open_workspace_import_validation(&new_root, WorkspaceImport::V7, false).unwrap();
        assert!(reopened.retained_oex_cleanup_hook() && !reopened.workspace_import_selected_input());
        assert!(Payload::open_workspace_import_validation(&new_root, WorkspaceImport::V7, true).is_err());
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    #[test]
    fn workspace_import_validation_distinguishes_v6_v7_and_preserves_reopen() {
        let mut manifest = local_fixture();
        let sha = "a".repeat(64);
        let mut add = |(path, bytes, digest): (&str, u64, &str)| {
            manifest.files.push(serde_json::from_value(serde_json::json!({
                "path": path, "bytes": bytes, "sha256": digest, "executable": false
            })).unwrap());
        };
        add(WORKSPACE_V6_CAPABILITY);
        for path in [WORKSPACE_IMPORT_FORMAT_HINT, WORKSPACE_IMPORT_PREFLIGHT, WORKSPACE_IMPORT_MATERIALIZER, WORKSPACE_IMPORT_PACKED] {
            add((path, 1, &sha));
        }
        let check = |manifest: &Manifest, version| workspace_import_validation_matches(
            manifest, &sha, version, &[(&manifest.source_commit, &manifest.producer_sha256, &sha)],
        );
        assert!(check(&manifest, WorkspaceImport::V6));
        assert!(!check(&manifest, WorkspaceImport::V7));
        for (path, bytes, digest) in [WORKSPACE_V7_CAPABILITY, (WORKSPACE_WALL_SCHEMA_PATH, 3720, WORKSPACE_WALL_SCHEMA_SHA256)] {
            manifest.files.push(serde_json::from_value(serde_json::json!({
                "path": path, "bytes": bytes, "sha256": digest, "executable": false
            })).unwrap());
        }
        assert!(check(&manifest, WorkspaceImport::V7));
        assert!(!check(&manifest, WorkspaceImport::V6));
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY)));
        let (path, bytes, hash) = WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY;
        manifest.files.push(serde_json::from_value(serde_json::json!({
            "path": path, "bytes": bytes, "sha256": hash, "executable": false
        })).unwrap());
        assert!(workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY)));
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY)));
        let selected_marker = manifest.files.last_mut().unwrap();
        selected_marker.bytes = Some(bytes + 1);
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY)));
        manifest.files.pop();
        let (path, bytes, hash) = WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY;
        manifest.files.push(serde_json::from_value(serde_json::json!({
            "path": path, "bytes": bytes, "sha256": hash, "executable": false
        })).unwrap());
        assert!(workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY)));
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY)));
        manifest.files.pop();
        let (path, bytes, hash) = WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY;
        manifest.files.push(Item {
            path: path.into(), bytes: Some(bytes), sha256: Some(hash.into()), executable: Some(false), link: None,
        });
        assert!(workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY)));
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V7, Some(WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY)));
        assert!(!workspace_import_files_match(&manifest, WorkspaceImport::V6, Some(WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY)));
        manifest.files.pop();
        assert!(!workspace_import_validation_matches(&manifest, &sha, WorkspaceImport::V7, &[]));
        for path in [WORKSPACE_V6_CAPABILITY.0, WORKSPACE_V7_CAPABILITY.0, WORKSPACE_WALL_SCHEMA_PATH] {
            let file = manifest.files.iter_mut().find(|file| file.path == path).unwrap();
            let original = file.sha256.replace(sha.clone());
            assert!(!check(&manifest, WorkspaceImport::V7));
            manifest.files.iter_mut().find(|file| file.path == path).unwrap().sha256 = original;
        }
        let [(source, producer, pinned_manifest)] = WORKSPACE_V7_VALIDATION_PROVENANCE else {
            panic!("expected one isolated V7 validation payload");
        };
        manifest.source_commit = source.to_string();
        manifest.producer_sha256 = producer.to_string();
        let admitted = |manifest: &Manifest, hash| workspace_import_validation_matches(
            manifest, hash, WorkspaceImport::V7, WORKSPACE_V7_VALIDATION_PROVENANCE,
        );
        assert!(admitted(&manifest, pinned_manifest));
        assert!(!admitted(&manifest, &sha));
        let original = std::mem::replace(&mut manifest.source_commit, "0".repeat(40));
        assert!(!admitted(&manifest, pinned_manifest));
        manifest.source_commit = original;
        let original = std::mem::replace(&mut manifest.producer_sha256, sha.clone());
        assert!(!admitted(&manifest, pinned_manifest));
        manifest.producer_sha256 = original;
        manifest.mode = "release".into();
        assert!(!admitted(&manifest, pinned_manifest));
        manifest.mode = "local-validation".into();
        assert!(!workspace_import_validation_matches(
            &manifest, pinned_manifest, WorkspaceImport::V6, WORKSPACE_V6_VALIDATION_PROVENANCE,
        ));

        let mut payload = Payload {
            root: PathBuf::new(), manifest, manifest_sha256: pinned_manifest.to_string(),
            workspace_v5_validation: false, workspace_import: Some((WorkspaceImport::V7, false)),
        };
        assert!(payload.workspace_import_version() == Some(WorkspaceImport::V7));
        assert!(!payload.workspace_import_selected_input());
        assert!(!payload.normal_oex_selected_input().unwrap());
        payload.workspace_import = Some((WorkspaceImport::V7, true));
        assert!(payload.workspace_import_selected_input());
    }

    #[test]
    fn material_catalogue_capability_requires_both_frozen_schemas_and_new_inventory() {
        let mut m=local_fixture();
        for (path,bytes,hash) in [(NORMAL_OEX_028_CAPABILITY_PATH,NORMAL_OEX_028_CAPABILITY.len() as u64,NORMAL_OEX_028_CAPABILITY_SHA256),
            (NORMAL_OEX_028_SCHEMA_PATH,100,NORMAL_OEX_028_SCHEMA_SHA256),
            ("platform/apps/api/dist/oex-selected-format.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")] {
            m.files.push(serde_json::from_value(serde_json::json!({"path":path,"bytes":bytes,"sha256":hash,"executable":false})).unwrap());
        }
        assert!(normal_oex_028_inventory_matches(&m));
        assert_eq!(digest(NORMAL_OEX_028_CAPABILITY),NORMAL_OEX_028_CAPABILITY_SHA256);
        for name in [NORMAL_OEX_CAPABILITY_PATH,NORMAL_OEX_027_PATH,NORMAL_OEX_028_SCHEMA_PATH,NORMAL_OEX_028_CAPABILITY_PATH,"platform/apps/api/dist/oex-selected-format.js"] {
            let index=m.files.iter().position(|f|f.path==name).unwrap();let hash=m.files[index].sha256.clone();
            m.files[index].sha256=Some(if name.ends_with("oex-selected-format.js"){"invalid".into()}else{"f".repeat(64)});assert!(!normal_oex_028_inventory_matches(&m));m.files[index].sha256=hash;
        }
    }
    #[test]
    fn quiet_oak_floor_supplement_requires_exact_parent_schema_and_inventory() {
        let mut m=local_fixture();
        for (path,bytes,hash) in [(NORMAL_OEX_028_CAPABILITY_PATH,NORMAL_OEX_028_CAPABILITY.len() as u64,NORMAL_OEX_028_CAPABILITY_SHA256),
            (NORMAL_OEX_028_SCHEMA_PATH,100,NORMAL_OEX_028_SCHEMA_SHA256),
            ("platform/apps/api/dist/oex-selected-format.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            (NORMAL_OEX_029_FLOOR_CAPABILITY_PATH,NORMAL_OEX_029_FLOOR_CAPABILITY.len() as u64,NORMAL_OEX_029_FLOOR_CAPABILITY_SHA256),
            (NORMAL_OEX_029_FLOOR_SCHEMA_PATH,566,NORMAL_OEX_029_FLOOR_SCHEMA_SHA256)] {
            m.files.push(serde_json::from_value(serde_json::json!({"path":path,"bytes":bytes,"sha256":hash,"executable":false})).unwrap());
        }
        assert_eq!(digest(NORMAL_OEX_029_FLOOR_CAPABILITY),NORMAL_OEX_029_FLOOR_CAPABILITY_SHA256);
        assert!(normal_oex_029_floor_inventory_matches(&m));
        for name in [NORMAL_OEX_028_CAPABILITY_PATH,NORMAL_OEX_028_SCHEMA_PATH,NORMAL_OEX_029_FLOOR_CAPABILITY_PATH,NORMAL_OEX_029_FLOOR_SCHEMA_PATH] {
            let index=m.files.iter().position(|f|f.path==name).unwrap();let file=m.files.remove(index);
            assert!(!normal_oex_029_floor_inventory_matches(&m));m.files.insert(index,file);
            let hash=m.files[index].sha256.clone();m.files[index].sha256=Some("f".repeat(64));
            assert!(!normal_oex_029_floor_inventory_matches(&m));m.files[index].sha256=hash;
        }
        let index=m.files.iter().position(|f|f.path==NORMAL_OEX_029_FLOOR_SCHEMA_PATH).unwrap();m.files[index].bytes=Some(565);
        assert!(!normal_oex_029_floor_inventory_matches(&m));
        assert!(normal_oex_028_inventory_matches(&m),"Historical028 inventory checks remain independent of029");
    }
    #[test]
    fn concrete_surface_supplement_binds_exact_local_tuple_and_inventory() {
        let (source, producer, hash) = NORMAL_OEX_030_CONCRETE_LOCAL_VALIDATION_PROVENANCE;
        let mut manifest = local_fixture();
        manifest.source_commit = source.into();
        manifest.producer_sha256 = producer.into();
        let mut payload = Payload { root: PathBuf::new(), manifest, manifest_sha256: hash.into(), workspace_v5_validation: false, workspace_import: None };
        assert!(payload.normal_oex_030_concrete_tuple());
        assert!(!payload.normal_oex_029_floor_tuple());
        assert!(!normal_oex_030_concrete_inventory_matches(&payload.manifest));
        for (path, bytes, digest) in [
            (NORMAL_OEX_030_CONCRETE_CAPABILITY_PATH, 575, NORMAL_OEX_030_CONCRETE_CAPABILITY_SHA256),
            (NORMAL_OEX_030_CONCRETE_SCHEMA_PATH, 519, NORMAL_OEX_030_CONCRETE_SCHEMA_SHA256),
        ] {
            payload.manifest.files.push(serde_json::from_value(serde_json::json!({
                "path": path, "bytes": bytes, "sha256": digest, "executable": false
            })).unwrap());
        }
        assert!(normal_oex_030_concrete_inventory_matches(&payload.manifest));
        payload.manifest.files.last_mut().unwrap().bytes = Some(518);
        assert!(!normal_oex_030_concrete_inventory_matches(&payload.manifest));
        payload.manifest.official_signed = true;
        assert!(!payload.normal_oex_030_concrete_tuple());
        payload.manifest.official_signed = false;
        payload.manifest_sha256 = "f".repeat(64);
        assert!(!payload.normal_oex_030_concrete_tuple());
    }
    #[test]
    fn local_027_accepts_only_exact_development_tuple_and_never_official_release() {
        let mut m = local_fixture(); let hash = "c".repeat(64);
        let source = "a".repeat(40); let producer = "b".repeat(64);
        let approved = [(source.as_str(), producer.as_str(), hash.as_str())];
        assert_eq!(normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved),
            cfg!(all(debug_assertions, target_os="macos")));
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,NORMAL_OEX_027_LOCAL_VALIDATION_PROVENANCE));
        assert!(!normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        assert!(!normal_oex_027_local_validation_matches(&m,&"d".repeat(64),NORMAL_OEX_CAPABILITY,&approved));
        m.source_commit="e".repeat(40);
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        m=local_fixture();m.producer_sha256="f".repeat(64);
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        m=local_fixture();m.mode="release".into();m.official_signed=true;m.redistribution_qualified=true;
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        assert!(!normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,NORMAL_OEX_027_RELEASE_PROVENANCE));
    }

    #[test]
    fn local_027_refuses_mixed_flags_and_changed_capability_or_schema() {
        let hash="c".repeat(64);let source="a".repeat(40);let producer="b".repeat(64);
        let approved=[(source.as_str(),producer.as_str(),hash.as_str())];
        let mut m=local_fixture();m.official_signed=true;
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        m=local_fixture();m.redistribution_qualified=true;
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        m=local_fixture();let mut altered=NORMAL_OEX_CAPABILITY.to_vec();altered.push(b'\n');
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,&altered,&approved));
        m=local_fixture();m.files.iter_mut().find(|f|f.path==NORMAL_OEX_027_PATH).unwrap().sha256=Some("e".repeat(64));
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        m=local_fixture();m.files.retain(|f|f.path!=NORMAL_OEX_027_CAPABILITY_PATH);
        assert!(!normal_oex_027_local_validation_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
    }

    #[test]
    fn provenance_027_needs_distinct_approved_tuple_and_complete_inventory() {
        let mut m=fixture();let hash="c".repeat(64);let source="a".repeat(40);let producer="b".repeat(64);
        let approved=[(source.as_str(),producer.as_str(),hash.as_str())];
        assert!(!normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        for (path,bytes,sha) in [(NORMAL_OEX_027_PATH,2611,NORMAL_OEX_027_SHA256),
            (NORMAL_OEX_027_CAPABILITY_PATH,516,NORMAL_OEX_027_CAPABILITY_SHA256),
            ("platform/apps/api/dist/native-oex-provenance.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/native-oex-catalog-proof.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/oex-owned-preflight.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("platform/apps/api/dist/oex-size-preflight.js",1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")] {
            m.files.push(serde_json::from_value(serde_json::json!({"path":path,"bytes":bytes,"sha256":sha,"executable":false})).unwrap());
        }
        assert!(normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
        assert!(!normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,NORMAL_OEX_027_RELEASE_PROVENANCE));
        m.files.pop();assert!(!normal_oex_027_declaration_matches(&m,&hash,NORMAL_OEX_CAPABILITY,&approved));
    }

    #[test]
    fn current_027_inventory_refuses_previous_private_abi_and_missing_preflight() {
        let mut m=local_fixture();
        assert!(normal_oex_027_inventory_matches(&m));
        let capability=m.files.iter_mut().find(|f|f.path==NORMAL_OEX_027_CAPABILITY_PATH).unwrap();
        capability.path="platform/scripts/native-normal-oex-027-capability-v2.json".into();
        capability.sha256=Some("e7f7bb5c5c12bf7cd901df5d8c1e5a0072fae30f98cdbd905215f311de09033b".into());
        assert!(!normal_oex_027_inventory_matches(&m));
        m=local_fixture();m.files.retain(|f|f.path!="platform/apps/api/dist/oex-owned-preflight.js");
        assert!(!normal_oex_027_inventory_matches(&m));
    }

    #[test]
    fn selected_release_requires_exact_declaration_inventory_and_approved_tuple() {
        let mut m = fixture();
        let source = "a".repeat(40); let producer = "b".repeat(64); let hash = "c".repeat(64);
        let approved = [(source.as_str(), producer.as_str(), hash.as_str())];
        assert_eq!(NORMAL_OEX_CAPABILITY.len(), 333);
        assert!(normal_oex_release_declaration_matches(&m, &hash, NORMAL_OEX_CAPABILITY, &approved));
        assert!(!normal_oex_release_declaration_matches(&m, &hash, NORMAL_OEX_CAPABILITY, NORMAL_OEX_RELEASE_PROVENANCE));
        assert!(!normal_oex_release_declaration_matches(&m, &"d".repeat(64), NORMAL_OEX_CAPABILITY, &approved));
        let mut altered = NORMAL_OEX_CAPABILITY.to_vec(); altered.push(b'\n');
        assert!(!normal_oex_release_declaration_matches(&m, &hash, &altered, &approved));
        m.files[1].sha256 = Some("e".repeat(64));
        assert!(!normal_oex_release_declaration_matches(&m, &hash, NORMAL_OEX_CAPABILITY, &approved));
        m = fixture(); m.files.pop();
        assert!(!normal_oex_release_declaration_matches(&m, &hash, NORMAL_OEX_CAPABILITY, &approved));
        m = fixture(); m.mode = "local-validation".into();
        assert!(!normal_oex_release_declaration_matches(&m, &hash, NORMAL_OEX_CAPABILITY, &approved));
    }
}
/// Optional maintenance capability. Construction requires all fixed resources
/// in the already byte-verified payload; older payloads remain usable for
/// authoring, but cannot silently fall back to host PostgreSQL tools.
pub struct BackupResources {
    pub script: PathBuf,
    pub pg_dump: PathBuf,
    pub pg_restore: PathBuf,
}
fn invalid() -> LifecycleError {
    err("NATIVE_RUNTIME_PAYLOAD_INVALID")
}
fn relative_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 2048
        && !name.contains(['\\', '\0', '\r', '\n'])
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
const GPU_PREVIEW_WEB_PREFIX: &str = "platform/apps/web/dist/";
const GPU_PREVIEW_FIXED: [&str; 4] = [
    "native-viewport.html", "basis/basis_transcoder.js", "basis/basis_transcoder.wasm",
    "basis/exhibitos-basis-worker.js",
];

fn gpu_preview_asset_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("assets/") else { return false; };
    !name.is_empty() && path.len() <= 200 && !name.contains('/') && !name.contains("..")
        && path.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/_.-".contains(&byte))
        && (name.ends_with(".js") || name.ends_with(".css"))
}

/// Enumerate exact paths from the authenticated payload inventory. The caller
/// receives no prefix/wildcard grant, filesystem path, URL or caller authority.
fn gpu_preview_resource_paths(files: &[Item]) -> Result<Vec<String>> {
    let unavailable = || err("NATIVE_GPU_PREVIEW_UNAVAILABLE");
    let mut paths = BTreeSet::new();
    let mut assets = 0usize;
    for item in files {
        let Some(path) = item.path.strip_prefix(GPU_PREVIEW_WEB_PREFIX) else { continue; };
        let fixed = GPU_PREVIEW_FIXED.contains(&path);
        let compiled = path.starts_with("assets/") && (path.ends_with(".js") || path.ends_with(".css"));
        if !fixed && !compiled { continue; }
        if (!fixed && !gpu_preview_asset_path(path)) || item.link.is_some()
            || item.executable != Some(false) || !item.bytes.is_some_and(|bytes| (1..=16 * 1024 * 1024).contains(&bytes))
            || !item.sha256.as_deref().is_some_and(hash_valid) || !paths.insert(path.to_owned())
        { return Err(unavailable()); }
        if compiled { assets += 1; }
        if assets > 256 { return Err(unavailable()); }
    }
    if assets == 0 || !GPU_PREVIEW_FIXED.into_iter().all(|path| paths.contains(path)) {
        return Err(unavailable());
    }
    Ok(paths.into_iter().collect())
}

/// Bound the files actually consumed by Platform's loadFreezeRuntime rather
/// than unrelated authoring assets in the same web directory. Platform still
/// performs its own full descriptor, MIME, hash and 16 MiB validation.
fn freeze_backup_runtime_bytes(manifest: &Manifest, descriptor: &[u8]) -> Result<u64> {
    let value: FreezeBudgetDescriptor = serde_json::from_slice(descriptor).map_err(|_| invalid())?;
    if value.schema_version != "1.0.0-draft.1" || value.version.trim().is_empty() || value.version.len() > 128
        || !hash_valid(&value.core_digest) || value.files.len() < 3 || value.files.len() > 512 {
        return Err(invalid());
    }
    let find = |name: &str| -> Result<&Item> {
        let path = format!("platform/apps/web/dist/{name}");
        manifest.files.iter().find(|v| v.path == path && v.link.is_none() && v.executable == Some(false)).ok_or_else(invalid)
    };
    let mut total = descriptor.len() as u64;
    let sw = find("studio-sw.js")?;
    total = total.checked_add(sw.bytes.ok_or_else(invalid)?).ok_or_else(|| err("NATIVE_BACKUP_RESOURCE_BUDGET"))?;
    let mut seen = BTreeSet::new();
    for file in &value.files {
        if file.path.len() > 240 || !relative_name(&file.path) || matches!(file.path.as_str(), "freeze-runtime.json" | "studio-sw.js")
            || !seen.insert(file.path.as_str()) || file.bytes == 0 || !hash_valid(&file.sha256) || file.mime.is_empty() {
            return Err(invalid());
        }
        let item = find(&file.path)?;
        if item.bytes != Some(file.bytes) || item.sha256.as_deref() != Some(file.sha256.as_str()) { return Err(invalid()); }
        total = total.checked_add(file.bytes).ok_or_else(|| err("NATIVE_BACKUP_RESOURCE_BUDGET"))?;
    }
    if !["index.html", "THIRD_PARTY_NOTICES.txt", "offline-server.mjs"].iter().all(|n| seen.contains(n)) { return Err(invalid()); }
    Ok(total)
}

#[cfg(test)]
mod freeze_backup_budget_tests {
    use super::*;

    #[test]
    fn budget_counts_only_descriptor_bound_runtime_and_rejects_mismatches() {
        let hash = "a".repeat(64);
        let paths = ["index.html", "THIRD_PARTY_NOTICES.txt", "offline-server.mjs"];
        let files: Vec<_> = paths.iter().map(|p| serde_json::json!({"path":p,"bytes":10,"sha256":hash,"mime":"text/plain"})).collect();
        let descriptor = serde_json::json!({"schemaVersion":"1.0.0-draft.1","version":"1","coreDigest":hash,"files":files});
        let mut entries: Vec<_> = paths.iter().map(|p| serde_json::json!({"path":format!("platform/apps/web/dist/{p}"),"bytes":10,"sha256":hash,"executable":false})).collect();
        entries.push(serde_json::json!({"path":"platform/apps/web/dist/studio-sw.js","bytes":5,"sha256":hash,"executable":false}));
        // A large unrelated authoring asset is not loaded by the backup codec.
        entries.push(serde_json::json!({"path":"platform/apps/web/dist/unrelated.png","bytes":40000000,"sha256":hash,"executable":false}));
        let manifest: Manifest = serde_json::from_value(serde_json::json!({"format":"exhibitos-native-runtime-v1","mode":"local-validation","platform":"darwin-arm64","nodeVersion":"24.21.0","postgresVersion":"18.6","platformVersion":"0.1.0","protocolVersion":"1","sourceCommit":"b".repeat(40),"producerSHA256":hash,"entry":{"node":"bin/node","script":"platform/scripts/local-runtime.mjs","postgres":"postgres/bin/postgres","initdb":"postgres/bin/initdb"},"files":entries,"redistributionQualified":false,"officialSigned":false,"remaining":"synthetic"})).unwrap();
        let bytes = serde_json::to_vec(&descriptor).unwrap();
        assert_eq!(freeze_backup_runtime_bytes(&manifest, &bytes).unwrap(), bytes.len() as u64 + 35);
        let mut changed = descriptor.clone();
        changed["files"][0]["bytes"] = serde_json::json!(11);
        assert!(freeze_backup_runtime_bytes(&manifest, &serde_json::to_vec(&changed).unwrap()).is_err());
        let mut duplicate = descriptor.clone();
        let first = duplicate["files"][0].clone();
        duplicate["files"].as_array_mut().unwrap().push(first);
        assert!(freeze_backup_runtime_bytes(&manifest, &serde_json::to_vec(&duplicate).unwrap()).is_err());
        let mut escape = descriptor;
        escape["files"][0]["path"] = serde_json::json!("../outside");
        assert!(freeze_backup_runtime_bytes(&manifest, &serde_json::to_vec(&escape).unwrap()).is_err());
    }
}
fn contained(root: &Path, relative: &str) -> Result<PathBuf> {
    if !relative_name(relative) {
        return Err(invalid());
    }
    let path = fs::canonicalize(root.join(relative)).map_err(|_| invalid())?;
    if !path.starts_with(root) || path == root {
        return Err(invalid());
    }
    Ok(path)
}
fn actual_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    names: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 64 {
        return Err(invalid());
    }
    for entry in fs::read_dir(directory).map_err(|_| invalid())? {
        let path = entry.map_err(|_| invalid())?.path();
        let stat = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if stat.is_dir() && !stat.is_symlink() {
            actual_files(root, &path, depth + 1, names)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?;
            if relative == "manifest.json" {
                continue;
            }
            if names.len() >= 50_000 || !relative_name(relative) {
                return Err(invalid());
            }
            names.insert(relative.into());
        }
    }
    Ok(())
}
// Each admission still reads and hashes every declared regular file. Two
// independent readers overlap filesystem latency without an unbounded task
// queue, retained byte cache, or relaxing the final complete inventory check.
fn verify_manifest_files<const READERS: usize>(root: &Path, items: &[Item]) -> Result<()> {
    if !(1..=2).contains(&READERS) { return Err(invalid()); }
    std::thread::scope(|scope| {
        let mut readers = Vec::with_capacity(READERS);
        for lane in 0..READERS {
            readers.push(std::thread::Builder::new()
                .name("runtime-admission".into())
                .spawn_scoped(scope, move || -> Result<()> {
                    for item in items.iter().skip(lane).step_by(READERS) {
                        let path = contained(root, &item.path)?;
                        let raw = fs::symlink_metadata(root.join(&item.path)).map_err(|_| invalid())?;
                        match (&item.link, item.bytes, &item.sha256, item.executable) {
                            (Some(link), None, None, None) => {
                                if !raw.is_symlink() || link.len() > 2048 {
                                    return Err(invalid());
                                }
                                let expected = fs::canonicalize(
                                    root.join(&item.path)
                                        .parent()
                                        .ok_or_else(invalid)?
                                        .join(link),
                                )
                                .map_err(|_| invalid())?;
                                if expected != path || !expected.starts_with(root) {
                                    return Err(invalid());
                                }
                            }
                            (None, Some(size), Some(hash), Some(executable)) => {
                                if raw.is_symlink() || !raw.is_file() || raw.len() != size || !hash_valid(hash)
                                {
                                    return Err(invalid());
                                }
                                use std::os::unix::fs::PermissionsExt;
                                if (raw.permissions().mode() & 0o111 != 0) != executable {
                                    return Err(invalid());
                                }
                                let mut file = File::open(path).map_err(|_| invalid())?;
                                let mut digest = Sha256::new();
                                let mut block = [0u8; 64 * 1024];
                                loop {
                                    let length = file.read(&mut block).map_err(|_| invalid())?;
                                    if length == 0 {
                                        break;
                                    }
                                    digest.update(&block[..length]);
                                }
                                if format!("{:x}", digest.finalize()) != *hash {
                                    return Err(invalid());
                                }
                            }
                            _ => return Err(invalid()),
                        }
                    }
                    Ok(())
                }).map_err(|_| invalid())?);
        }
        // Join both readers even if one refuses. No worker or open file may
        // outlive admission, and a refusal from either lane refuses the payload.
        let mut accepted = true;
        for reader in readers {
            if !matches!(reader.join(), Ok(Ok(()))) { accepted = false; }
        }
        if accepted { Ok(()) } else { Err(invalid()) }
    })
}

impl Payload {
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute()
            || fs::symlink_metadata(root)
                .map_err(|_| invalid())?
                .is_symlink()
        {
            return Err(invalid());
        }
        let root = fs::canonicalize(root).map_err(|_| invalid())?;
        if fs::symlink_metadata(&root)
            .map_err(|_| invalid())?
            .is_symlink()
        {
            return Err(invalid());
        }
        let path = root.join("manifest.json");
        let mut options = OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let mut file = options.open(path).map_err(|_| invalid())?;
        let stat = file.metadata().map_err(|_| invalid())?;
        if !stat.is_file() || stat.len() > 16 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|_| invalid())?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if manifest.format != "exhibitos-native-runtime-v1"
            || manifest.platform != "darwin-arm64"
            || !cfg!(target_arch = "aarch64")
            || manifest.node_version != "24.21.0"
            || manifest.postgres_version != "18.6"
            || manifest.platform_version != "0.1.0"
            || manifest.protocol_version != "1"
            || manifest.source_commit.len() != 40
            || !manifest
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !hash_valid(&manifest.producer_sha256)
            || manifest.files.is_empty()
            || manifest.files.len() > 50_000
            || manifest.remaining.len() > 4096
        {
            return Err(invalid());
        }
        // A local validation copy is not accepted by a production build.
        if manifest.mode == "local-validation" {
            if !cfg!(debug_assertions)
                || manifest.official_signed
                || manifest.redistribution_qualified
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
        } else if manifest.mode != "release"
            || !manifest.redistribution_qualified
            || !manifest.official_signed
        {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        } else {
            // Flags remain necessary declarations. The running helper and its
            // owning sealed app must independently satisfy the pinned official
            // Developer ID identity before any payload file is admitted.
            crate::native_signed_admission::admit(&root)?;
        }
        if manifest.entry.node != "bin/node"
            || manifest.entry.script != "platform/scripts/local-runtime.mjs"
            || manifest.entry.postgres != "postgres/bin/postgres"
            || manifest.entry.initdb != "postgres/bin/initdb"
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        for item in &manifest.files {
            if !relative_name(&item.path)
                || item.path == "manifest.json"
                || !names.insert(item.path.clone())
            {
                return Err(invalid());
            }
        }
        verify_manifest_files::<2>(&root, &manifest.files)?;
        let mut actual = BTreeSet::new();
        actual_files(&root, &root, 0, &mut actual)?;
        if actual != names {
            return Err(invalid());
        }
        for (name, executable) in [
            (manifest.entry.node.as_str(), true),
            (manifest.entry.script.as_str(), false),
            (manifest.entry.postgres.as_str(), true),
            (manifest.entry.initdb.as_str(), true),
        ] {
            if !manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            root,
            manifest,
            manifest_sha256: digest(&bytes),
            workspace_v5_validation: false, workspace_import: None,
        })
    }
    /// Available only to the fresh selected validation entry. The normal
    /// payload opener cannot create this state, even in a validation build.
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn open_workspace_v5_validation(root: &Path, selected: bool) -> Result<Self> {
        let mut payload = Self::open(root)?;
        if !workspace_v5_validation_matches(&payload.manifest, &payload.manifest_sha256, selected) {
            // Existing-workspace validation never admits selected import or a
            // source-only thumbnail candidate without its exact manifest pin.
            let gpu = gpu_model_host_matches((&payload.manifest.source_commit,
                &payload.manifest.producer_sha256, &payload.manifest_sha256));
            let thumbnail = furnishing_thumbnail_host_matches((&payload.manifest.source_commit,
                &payload.manifest.producer_sha256, &payload.manifest_sha256));
            let expected = Some(if gpu { WorkspaceFurnishingCatalog::GpuModelPreparation049 }
                else if thumbnail { WorkspaceFurnishingCatalog::FurnishingThumbnail050 }
                else { WorkspaceFurnishingCatalog::RequiredModelPreparation045 });
            let publication = catalog45_native_publication_host_matches((
                &payload.manifest.source_commit, &payload.manifest.producer_sha256, &payload.manifest_sha256,
            ));
            if selected || (!gpu && !thumbnail && !publication && !payload.native_model_adoption_original_target()
                && payload.retained_furnishing_catalog() != expected)
                || payload.workspace_furnishing_catalog()? != expected
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
        }
        payload.workspace_v5_validation = selected;
        Ok(payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn open_saved_workspace_transition(root: &Path) -> Result<Self> {
        let payload = Self::open(root)?;
        let context = payload.context();
        if [CATALOG40_DRAFT2_OEX_SELECTED.2, CATALOG40_RETAINED_OEX_CLEANUP.2]
            .contains(&context.manifest_sha256.as_str())
        {
            return if payload.workspace_furnishing_catalog()? == Some(WorkspaceFurnishingCatalog::MaterialMultimap040) {
                Ok(payload)
            } else {
                Err(err("NATIVE_RUNTIME_UNQUALIFIED"))
            };
        }
        let ordinary_catalog45 = [
            CATALOG45_RENDERED_SCULPTURE_THUMBNAIL, CATALOG45_NATIVE_PUBLICATION_CAPACITY,
            CATALOG45_MUSEUM_CAPTION_TRANSITION_SOURCE, CATALOG45_SAVED_HEAD_MUSEUM_CAPTIONS,
            CATALOG45_PREPARED_SIGN_AUDIO, CATALOG45_PLACED_EXIT_DOORS, CATALOG45_BACKED_BENCH,
            CATALOG45_CARRIED_RESTORE_BASE, CATALOG45_MODEL_ADOPTION_CARRIED_TARGET,
            CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_DEFAULT_1_7_TARGET, CATALOG45_DEFAULT_1_11_TARGET, CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET,
        ].contains(&(context.source_commit.as_str(), payload.manifest.producer_sha256.as_str(), context.manifest_sha256.as_str()));
        let retained_catalog = furnishing_thumbnail_host_matches((&context.source_commit,
            payload.producer_sha256(), &context.manifest_sha256))
            .then_some(WorkspaceFurnishingCatalog::FurnishingThumbnail050)
            .or_else(|| gpu_model_host_matches((&context.source_commit, payload.producer_sha256(), &context.manifest_sha256))
                .then_some(WorkspaceFurnishingCatalog::GpuModelPreparation049))
            .or(payload.retained_furnishing_catalog()).or(
            ordinary_catalog45.then_some(WorkspaceFurnishingCatalog::RequiredModelPreparation045),
        );
        if let Some(expected) = retained_catalog {
            return if payload.workspace_furnishing_catalog()? == Some(expected) {
                Ok(payload)
            } else {
                Err(err("NATIVE_RUNTIME_UNQUALIFIED"))
            };
        }
        if let Some(target) = CATALOG39_UPGRADE_TARGET {
            let catalog = if context.manifest_sha256 == CATALOG39_UPGRADE_SOURCE.2 {
                Some(WorkspaceFurnishingCatalog::MaterialPublication039)
            } else if context.manifest_sha256 == target.2
                || CATALOG40_EXB_TARGET.is_some_and(|exb| context.manifest_sha256 == exb.2)
                || CATALOG40_RESTORE_PROVENANCE_TARGET.is_some_and(|restore| context.manifest_sha256 == restore.2)
                || CATALOG40_FRAME_RECORD_TARGET.is_some_and(|frames| context.manifest_sha256 == frames.2)
                || CATALOG40_ARTWORK_REPLACEMENT_TARGET.is_some_and(|replacement| context.manifest_sha256 == replacement.2)
                || CATALOG40_WHEAT_GRANT.is_some_and(|grant| context.manifest_sha256 == grant.2)
                || CATALOG40_WHEAT_DETAIL.is_some_and(|detail| context.manifest_sha256 == detail.2) {
                Some(WorkspaceFurnishingCatalog::MaterialMultimap040)
            } else { None };
            if let Some(expected) = catalog {
                if payload.workspace_furnishing_catalog()? == Some(expected) {
                    return Ok(payload);
                }
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
        }
        if [FurnishingTransition::White, FurnishingTransition::Tangent].iter().any(|pair|
            context.manifest_sha256 == pair.source().2 || context.manifest_sha256 == pair.target().2)
        {
            if payload.workspace_furnishing_catalog()? == Some(WorkspaceFurnishingCatalog::MaterialMultimap040) {
                return Ok(payload);
            }
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        Self::open_workspace_v5_validation(root, false)
    }

    pub(crate) fn ordinary_hayes_release(&self) -> bool {
        ordinary_hayes_release_host_matches((
            &self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256,
        ))
    }

    pub(crate) fn ordinary_fresh_furnishing(&self) -> bool {
        let tuple = (&*self.manifest.source_commit, &*self.manifest.producer_sha256, &*self.manifest_sha256);
        ordinary_fresh_catalog45_host_matches(tuple) || large_model_host_matches(tuple)
            || self.fresh_image_release() || fresh_image_recovery_host_matches(tuple)
            || info_stand_preparation_host_matches(tuple)
    }

    pub(crate) fn fresh_image_release(&self) -> bool {
        self.manifest.mode == "release" && self.manifest.official_signed
            && self.manifest.redistribution_qualified
            && fresh_image_release_host_matches((&self.manifest.source_commit,
                &self.manifest.producer_sha256, &self.manifest_sha256))
    }

    pub(crate) fn large_model_recovery_validation(&self) -> bool {
        large_model_recovery_host_matches((
            &self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256,
        ))
    }

    fn catalog45_retained_image_recovery_validation(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return CATALOG45_RETAINED_IMAGE_RECOVERY_TARGET == (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str(),
        );
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    fn catalog45_startup_cancel_recovery_validation(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return CATALOG45_STARTUP_CANCEL_RECOVERY_TARGET == (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str(),
        );
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    pub(crate) fn catalog45_recovery_validation(&self) -> bool {
        catalog45_recovery_host_matches((
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(),
            self.manifest_sha256.as_str(),
        ))
    }

    pub(crate) fn catalog45_selected_recovery_validation(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return CATALOG45_SELECTED_RECOVERY_TARGET == (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(),
            self.manifest_sha256.as_str(),
        );
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    fn catalog45_pre_ready_preservation_validation(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return CATALOG45_PRE_READY_PRESERVATION_TARGET == (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str(),
        );
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    fn catalog45_pre_ready_hayes_preservation_validation(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return CATALOG45_PRE_READY_HAYES_PRESERVATION_TARGET == (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str(),
        );
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    pub(crate) fn fresh_image_recovery_enabled(&self) -> bool {
        self.fresh_image_release() || fresh_image_recovery_host_matches((
            &self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256,
        ))
    }

    pub(crate) fn retained_furnishing_catalog(&self) -> Option<WorkspaceFurnishingCatalog> {
        if self.ordinary_hayes_release() {
            Some(WorkspaceFurnishingCatalog::HayesRelease044)
        } else if required_model_preparation_host_matches((
            &self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256,
        )) {
            Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045)
        } else {
            None
        }
    }

    pub(crate) fn workspace_v5_selected_input(&self) -> bool {
        self.workspace_v5_validation
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn native_model_adoption_carried_target(&self) -> bool {
        catalog45_adoption_carried_receipt_payload((
            &self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256,
        )).is_some()
    }

    pub(crate) fn native_model_adoption_original_target(&self) -> bool {
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        return [CATALOG45_MODEL_ADOPTION_ORIGINAL_TARGET, CATALOG45_MODEL_ADOPTION_TERMINAL_TARGET, CATALOG45_RENDERED_ARTWORK_THUMBNAIL_TARGET, CATALOG45_CANONICAL_MODEL_THUMBNAIL_TARGET, CATALOG45_PORTAL_HAYES_CARD_TARGET, CATALOG45_SCENE_AUTHORING_TARGET, CATALOG45_THUMBNAIL_FRESHNESS_TARGET, CATALOG45_COMPLETE_STATIC_TARGET, CATALOG45_FIXTURE_AUTHORING_TARGET, CATALOG45_DEFAULT_1_7_TARGET, CATALOG45_DEFAULT_1_11_TARGET, CATALOG45_SELECTED_PNG_READ_SOURCE, CATALOG45_SELECTED_PNG_READ_TARGET, CATALOG45_CURRENT_IMAGE_THUMBNAIL_TARGET]
            .contains(&(&self.manifest.source_commit[..], self.producer_sha256(), &self.manifest_sha256[..]));
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        false
    }

    pub(crate) fn native_model_adoption_root(&self) -> Result<Option<&Path>> {
        let tuple = (
            self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str(),
        );
        let pinned = catalog45_model_adoption_host_matches(tuple) && !catalog45_markerless_ordinary_host_matches(tuple);
        let carried = pinned && catalog45_adoption_carried_receipt_payload(tuple).is_some();
        let declarations: [(&str, &[u8], &str, bool); 2] = [
            (
                "platform/scripts/native-model-adoption-capability-v1.json",
                b"{\"version\":1,\"profile\":\"native-model-adoption-v1\"}\n",
                "1cc2e6bc81fec1ef0aa6b6643dd177feff9480ad690d7b1b75e870b5072ca4a3",
                pinned,
            ),
            (
                "platform/scripts/native-model-adoption-carried-target-capability-v1.json",
                b"{\"version\":1,\"profile\":\"native-model-adoption-carried-target-v1\"}\n",
                "064a5034b9146eacfebcdbbcde56a8cd8b8e7ba28b27e097113ec11c625f3281",
                carried,
            ),
        ];
        for (path, bytes, sha, required) in declarations {
            let marker = self.manifest.files.iter().find(|file| file.path == path);
            if marker.is_some() != required || marker.is_some_and(|file| file.link.is_some()
                || file.executable != Some(false) || file.bytes != Some(bytes.len() as u64)
                || file.sha256.as_deref() != Some(sha))
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
        }
        if !pinned {
            return Ok(None);
        }
        if self.workspace_furnishing_catalog()? != Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045) {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        self.recheck()?;
        for (path, expected, _, required) in declarations {
            if required {
                let (bytes, pin) = crate::hosted_runtime_file::bounded(&self.root.join(path), expected.len() as u64)?;
                if bytes != expected {
                    return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
                }
                pin.check()?;
            }
        }
        Ok(Some(&self.root))
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn furnishing_thumbnail_upgrade_script(&self) -> Result<PathBuf> {
        if !furnishing_thumbnail_host_matches((&self.manifest.source_commit,
                &self.manifest.producer_sha256, &self.manifest_sha256))
            || self.workspace_furnishing_catalog()? != Some(WorkspaceFurnishingCatalog::FurnishingThumbnail050)
        { return Err(err("NATIVE_RUNTIME_UNQUALIFIED")); }
        for (path, hash) in [
            ("platform/scripts/local-runtime.mjs", "93250a38af737b4a2b9affa72567f67bc6e67f624d252f61ed382a93c68f9c73"),
            ("platform/scripts/native-workspace-furnishing-bootstrap.mjs", "aec07582c5ad85a06e1d0d59b040488bea2a9d03ebc675efc9fed7687d165bdc"),
        ] {
            if !self.manifest.files.iter().any(|file| file.path == path && file.link.is_none()
                && file.executable == Some(false) && file.sha256.as_deref() == Some(hash))
            { return Err(err("NATIVE_RUNTIME_UNQUALIFIED")); }
        }
        self.recheck()?;
        Ok(self.script())
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn gpu_model_upgrade_script(&self) -> Result<PathBuf> {
        if !gpu_model_host_matches((&self.manifest.source_commit,
                &self.manifest.producer_sha256, &self.manifest_sha256))
            || self.workspace_furnishing_catalog()? != Some(WorkspaceFurnishingCatalog::GpuModelPreparation049)
        { return Err(err("NATIVE_RUNTIME_UNQUALIFIED")); }
        let pins = GPU_MODEL_HELPER_FILES.ok_or_else(|| err("NATIVE_RUNTIME_UNQUALIFIED"))?;
        if pins.is_empty() { return Err(err("NATIVE_RUNTIME_UNQUALIFIED")); }
        for &(path, bytes, hash) in pins {
            if !self.manifest.files.iter().any(|file| file.path == path && file.link.is_none()
                && file.executable == Some(false) && file.bytes == Some(bytes)
                && file.sha256.as_deref() == Some(hash))
            { return Err(err("NATIVE_RUNTIME_UNQUALIFIED")); }
        }
        self.recheck()?;
        Ok(self.script())
    }

    pub(crate) fn native_gpu_preview_resources(&self) -> Result<serde_json::Value> {
        if !gpu_model_host_matches((&self.manifest.source_commit,
                &self.manifest.producer_sha256, &self.manifest_sha256))
            || self.workspace_furnishing_catalog()? != Some(WorkspaceFurnishingCatalog::GpuModelPreparation049)
        { return Err(err("NATIVE_GPU_PREVIEW_UNAVAILABLE")); }
        self.recheck()?;
        let paths = gpu_preview_resource_paths(&self.manifest.files)?;
        Ok(serde_json::json!({"format":1,"payloadManifestSha256":self.manifest_sha256,"paths":paths}))
    }

    pub(crate) fn workspace_furnishing_validation(&self) -> Result<bool> {
        Ok(self.workspace_furnishing_catalog()?.is_some())
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn retained_oex_cleanup_hook(&self) -> bool {
        self.manifest_sha256 == CATALOG40_RETAINED_OEX_CLEANUP.2
            && self.manifest.files.iter().any(|file|
                file.path == "platform/apps/api/dist/native-oex-cleanup.js"
                    && file.link.is_none() && file.executable == Some(false)
                    && file.sha256.as_deref() == Some("2c9f5f667cbaaf00cbd2e4c6b34940735c47d5cd8ed69b7a50f2ac6283ea7b23"))
    }

    pub(crate) fn workspace_furnishing_catalog(&self) -> Result<Option<WorkspaceFurnishingCatalog>> {
        const PORTAL_MARKER: &str = "platform/scripts/native-workspace-portal-finish-capability-v1.json";
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let portal = Some((
            "0403f9c3df008a95772291ac41dae77db2cd4af4",
            "cf0558967fd6966053c164b336974f3a99b08f90264d41eee622ee637c80e5dd",
            "0bea16fb7d61aa3b0e60921a7f4d8d702025e4857f779fd763b9633f4884b330",
        ));
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let portal: Option<(&str, &str, &str)> = None;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let artwork_replacement = CATALOG40_ARTWORK_REPLACEMENT_TARGET;
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let artwork_replacement: Option<(&str, &str, &str)> = None;
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let wheat_grant = CATALOG40_WHEAT_GRANT;
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let wheat_grant: Option<(&str, &str, &str)> = None;
        let pinned_portal = self.retained_furnishing_catalog().is_some()
            || self.ordinary_fresh_furnishing()
            || catalog45_carried_restore_payload(self)
            || furnishing_thumbnail_host_matches((&self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256))
            || gpu_model_host_matches((&self.manifest.source_commit, &self.manifest.producer_sha256, &self.manifest_sha256))
            || portal.into_iter().chain(artwork_replacement).chain(wheat_grant).chain(CATALOG40_WHEAT_DETAIL).any(|(source, producer, manifest)|
                self.manifest.source_commit == source && self.manifest.producer_sha256 == producer
                    && self.manifest_sha256 == manifest);
        let portal_marker = self.manifest.files.iter().find(|file| file.path == PORTAL_MARKER);
        if (pinned_portal || portal_marker.is_some())
            && !(pinned_portal && portal_marker.is_some_and(|file| file.link.is_none()
                && file.executable == Some(false) && file.bytes == Some(300)
                && file.sha256.as_deref() == Some("6a7e94ccd178ec05d57a120d77de28d7fcc6fb54f2069c18ead5beb52cd9aebc")))
        {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let multimap = WORKSPACE_MATERIAL_MULTIMAP_VALIDATION_PROVENANCE.iter().copied()
            .chain(CATALOG39_UPGRADE_TARGET).chain(CATALOG40_EXB_TARGET)
            .chain(CATALOG40_RESTORE_PROVENANCE_TARGET).chain(CATALOG40_FRAME_RECORD_TARGET)
            .chain(portal).chain(artwork_replacement).chain(wheat_grant).chain(CATALOG40_WHEAT_DETAIL)
            .chain([CATALOG40_DRAFT2_OEX_SELECTED, CATALOG40_RETAINED_OEX_CLEANUP, BUNDLED_FURNISHINGS_VALIDATION])
            .collect::<Vec<_>>();
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let multimap = multimap.as_slice();
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let multimap = WORKSPACE_MATERIAL_MULTIMAP_VALIDATION_PROVENANCE;
        self.workspace_furnishing_catalog_for(WORKSPACE_FURNISHING_VALIDATION_PROVENANCE,
            WORKSPACE_FURNISHING_WRITER_VALIDATION_PROVENANCE,
            WORKSPACE_FURNISHING_PUBLICATION_VALIDATION_PROVENANCE,
            WORKSPACE_MATERIAL_PUBLICATION_VALIDATION_PROVENANCE,
            multimap, WORKSPACE_FRAME_PREPARATION_VALIDATION_PROVENANCE)
    }

    fn workspace_furnishing_catalog_for(&self, legacy: &[(&str, &str, &str)],
                                        writer: &[(&str, &str, &str, &str)],
                                        publication: &[(&str, &str, &str)],
                                        material: &[(&str, &str, &str)],
                                        multimap: &[(&str, &str, &str)],
                                        frame_preparation: &[(&str, &str, &str)]) -> Result<Option<WorkspaceFurnishingCatalog>> {
        let unqualified = || err("NATIVE_RUNTIME_UNQUALIFIED");
        let tuple = (self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str());
        let regular = |path: &str| self.manifest.files.iter().any(|file| file.path == path
            && file.link.is_none() && file.executable == Some(false));
        let marker = self.manifest.files.iter().any(|file| file.path == WORKSPACE_FURNISHING_CAPABILITY);
        let writer_schema = self.manifest.files.iter().any(|file| file.path.starts_with(WORKSPACE_FURNISHING_WRITER_DIRECTORY));
        let writer_pin = writer.iter().find(|&&(source, producer, manifest, _)| (source, producer, manifest) == tuple);
        let publication_declared = self.manifest.files.iter().any(|file|
            file.path == WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY
                || file.path.starts_with(WORKSPACE_FURNISHING_PUBLICATION_DIRECTORY));
        let publication_pinned = publication.contains(&tuple);
        let material_declared = self.manifest.files.iter().any(|file|
            file.path == WORKSPACE_MATERIAL_PUBLICATION_CAPABILITY.0
                || file.path.starts_with(WORKSPACE_MATERIAL_PUBLICATION_DIRECTORY)
                || file.path == WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY.0);
        let material_pinned = material.contains(&tuple);
        let multimap_pinned = multimap.contains(&tuple);
        let release_probe_pinned = hayes_release_host_matches(tuple);
        let large_model_pinned = large_model_host_matches(tuple);
        let large_model_declared = self.manifest.files.iter().any(|file|
            file.path == LARGE_MODEL_PREPARATION_CAPABILITY.0 || file.path.starts_with(LARGE_MODEL_PREPARATION_DIRECTORY));
        let large_recovery_pinned = self.large_model_recovery_validation();
        let large_recovery_declared = self.manifest.files.iter().any(|file|
            file.path == LARGE_MODEL_RECOVERY_CAPABILITY.0);
        if large_model_declared != large_model_pinned || large_recovery_declared != large_recovery_pinned {
            return Err(unqualified());
        }
        let fresh_image_release = self.fresh_image_release();
        let fresh_image_pinned = fresh_image_release || fresh_image_recovery_host_matches(tuple);
        let fresh_image_declared = self.manifest.files.iter().any(|file|
            file.path == FRESH_IMAGE_PREPARATION_CAPABILITY.0 || file.path.starts_with(FRESH_IMAGE_PREPARATION_DIRECTORY));
        if fresh_image_declared != fresh_image_pinned {
            return Err(unqualified());
        }
        let gpu_pinned = gpu_model_host_matches(tuple);
        let gpu_declared = self.manifest.files.iter().any(|file|
            file.path == GPU_MODEL_CAPABILITY_PATH || file.path.starts_with(GPU_MODEL_DIRECTORY));
        if gpu_declared != gpu_pinned { return Err(unqualified()); }
        let thumbnail_pinned = furnishing_thumbnail_host_matches(tuple);
        let thumbnail_declared = self.manifest.files.iter().any(|file|
            file.path == FURNISHING_THUMBNAIL_CAPABILITY.0 || file.path.starts_with(FURNISHING_THUMBNAIL_DIRECTORY));
        if thumbnail_declared != thumbnail_pinned { return Err(unqualified()); }
        let info_stand_pinned = info_stand_preparation_host_matches(tuple);
        let info_stand_declared = self.manifest.files.iter().any(|file|
            file.path == INFO_STAND_PREPARATION_CAPABILITY.0 || file.path.starts_with(INFO_STAND_PREPARATION_DIRECTORY));
        if info_stand_declared != info_stand_pinned {
            return Err(unqualified());
        }
        let model_preparation_pinned = required_model_preparation_host_matches(tuple)
            || self.ordinary_fresh_furnishing() || catalog45_carried_restore_host_matches(tuple) || thumbnail_pinned || gpu_pinned;
        let model_preparation_declared = self.manifest.files.iter().any(|file|
            file.path == MODEL_PREPARATION_CAPABILITY.0 || file.path.starts_with(MODEL_PREPARATION_DIRECTORY));
        if model_preparation_declared != model_preparation_pinned {
            return Err(unqualified());
        }
        let selected_recovery = self.catalog45_selected_recovery_validation();
        let startup_cancel_recovery = self.catalog45_startup_cancel_recovery_validation();
        let retained_recovery = self.catalog45_retained_image_recovery_validation() || startup_cancel_recovery;
        let pre_ready_preservation = self.catalog45_pre_ready_preservation_validation();
        let pre_ready_hayes_preservation = self.catalog45_pre_ready_hayes_preservation_validation();
        for ((path, bytes, sha), expected) in [
            (CATALOG45_RECOVERY_CAPABILITY, self.catalog45_recovery_validation() && !selected_recovery && !retained_recovery && !pre_ready_preservation && !pre_ready_hayes_preservation),
            (CATALOG45_SELECTED_RECOVERY_CAPABILITY, selected_recovery),
            (CATALOG45_RETAINED_IMAGE_RECOVERY_CAPABILITY, retained_recovery),
            (CATALOG45_STARTUP_CANCEL_RECOVERY_CAPABILITY, startup_cancel_recovery),
            (CATALOG45_PRE_READY_PRESERVATION_CAPABILITY, pre_ready_preservation),
            (CATALOG45_PRE_READY_HAYES_PRESERVATION_CAPABILITY, pre_ready_hayes_preservation),
        ] {
            let marker = self.manifest.files.iter().find(|file| file.path == path);
            if marker.is_some() != expected || marker.is_some_and(|file|
                file.link.is_some() || file.executable != Some(false)
                    || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(sha))
            {
                return Err(unqualified());
            }
        }
        let release_pinned = release_probe_pinned || ordinary_hayes_release_host_matches(tuple)
            || model_preparation_pinned;
        let release_declared = self.manifest.files.iter().any(|file|
            file.path == HAYES_RELEASE_CAPABILITY.0 || file.path.starts_with(HAYES_RELEASE_DIRECTORY));
        if release_declared != release_pinned {
            return Err(unqualified());
        }
        let disposition_probe_pinned = hayes_disposition_host_matches(tuple);
        let disposition_pinned = disposition_probe_pinned || release_pinned;
        let disposition_declared = self.manifest.files.iter().any(|file|
            file.path == HAYES_DISPOSITION_CAPABILITY.0 || file.path.starts_with(HAYES_DISPOSITION_DIRECTORY));
        if disposition_declared != disposition_pinned {
            return Err(unqualified());
        }
        let probe_declared = self.manifest.files.iter().any(|file| file.path == HAYES_SESSION_PROBE_SCRIPT);
        let disposition_probe_declared = self.manifest.files.iter().any(|file| file.path == HAYES_DISPOSITION_PROBE_SCRIPT);
        let release_probe_declared = self.manifest.files.iter().any(|file| file.path == HAYES_RELEASE_PROBE_SCRIPT);
        if probe_declared != hayes_session_probe_matches(tuple) || disposition_probe_declared != disposition_probe_pinned
            || release_probe_declared != release_probe_pinned {
            return Err(unqualified());
        }
        let hayes_pinned = hayes_validation_host_matches(tuple) || disposition_pinned;
        let hayes_declared = self.manifest.files.iter().any(|file|
            file.path == HAYES_ORIGINAL_CAPABILITY.0 || file.path.starts_with(HAYES_ORIGINAL_DIRECTORY));
        if hayes_declared != hayes_pinned {
            return Err(unqualified());
        }
        let preparation_pinned = frame_preparation.contains(&tuple) || hayes_pinned;
        let wants_preparation = preparation_pinned || self.manifest.files.iter().any(|file|
            file.path == WORKSPACE_FRAME_PREPARATION_CAPABILITY.0
                || file.path.starts_with(WORKSPACE_FRAME_PREPARATION_DIRECTORY));
        let wants_multimap = wants_preparation || multimap_pinned || self.manifest.files.iter().any(|file|
            file.path == WORKSPACE_MATERIAL_MULTIMAP_CAPABILITY.0
                || file.path.starts_with(WORKSPACE_MATERIAL_MULTIMAP_DIRECTORY)
                || file.path == WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY.0);
        let wants_material = material_declared || material_pinned || wants_multimap;
        if !marker && !writer_schema && writer_pin.is_none()
            && !publication_declared && !publication_pinned && !wants_material
        {
            return Ok(None);
        }
        let local_validation = cfg!(debug_assertions) && self.manifest.mode == "local-validation"
            && !self.manifest.redistribution_qualified && !self.manifest.official_signed;
        if !(local_validation || fresh_image_release) || !regular(WORKSPACE_FURNISHING_CAPABILITY)
        {
            return Err(unqualified());
        }
        if publication_declared || publication_pinned || wants_material {
            let pinned_file = |(path, bytes, hash): (&str, u64, &str)| {
                self.manifest.files.iter().any(|file| file.path == path && file.link.is_none()
                    && file.executable == Some(false) && file.bytes == Some(bytes)
                    && file.sha256.as_deref() == Some(hash))
            };
            let pinned = if wants_preparation {
                preparation_pinned && !multimap_pinned && !material_pinned && !publication_pinned
            } else if wants_multimap {
                multimap_pinned && !material_pinned && !publication_pinned
            } else if wants_material {
                material_pinned && !publication_pinned
            } else {
                publication_pinned
            };
            if !pinned || writer_pin.is_some() || legacy.contains(&tuple)
                || !regular(WORKSPACE_FURNISHING_PUBLICATION_WRITER_CAPABILITY)
                || !regular(WORKSPACE_FURNISHING_WRITER_SCHEMA)
                || !pinned_file((WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY,
                    WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_BYTES, WORKSPACE_FURNISHING_PUBLICATION_CAPABILITY_SHA256))
                || !pinned_file((WORKSPACE_FURNISHING_PUBLICATION_SCHEMA,
                    WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES, WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256))
            {
                return Err(unqualified());
            }
            if wants_material {
                if !pinned_file(WORKSPACE_MATERIAL_PUBLICATION_CAPABILITY)
                    || !pinned_file(WORKSPACE_MATERIAL_PUBLICATION_SCHEMA)
                    || self.manifest.files.iter().any(|file| file.path == WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY.0)
                {
                    return Err(unqualified());
                }
                if wants_multimap {
                    if !pinned_file(WORKSPACE_MATERIAL_MULTIMAP_CAPABILITY)
                        || !pinned_file(WORKSPACE_MATERIAL_MULTIMAP_SCHEMA)
                        || self.manifest.files.iter().any(|file| file.path == WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY.0)
                    {
                        return Err(unqualified());
                    }
                    if wants_preparation {
                        let marker = if hayes_pinned { HAYES_FRAME_PREPARATION_CAPABILITY }
                            else { WORKSPACE_FRAME_PREPARATION_CAPABILITY };
                        if !pinned_file(marker)
                            || !pinned_file(WORKSPACE_FRAME_PREPARATION_SCHEMA)
                            || self.manifest.files.iter().any(|file| file.path == WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY.0)
                        {
                            return Err(unqualified());
                        }
                        if hayes_pinned {
                            if !pinned_file(HAYES_ORIGINAL_CAPABILITY) || !pinned_file(HAYES_ORIGINAL_SCHEMA) {
                                return Err(unqualified());
                            }
                            if disposition_pinned {
                                if !pinned_file(HAYES_DISPOSITION_CAPABILITY) || !pinned_file(HAYES_DISPOSITION_SCHEMA) {
                                    return Err(unqualified());
                                }
                                if release_pinned {
                                    if !pinned_file(HAYES_RELEASE_CAPABILITY) || !pinned_file(HAYES_RELEASE_SCHEMA) {
                                        return Err(unqualified());
                                    }
                                    if model_preparation_pinned {
                                        if !pinned_file(MODEL_PREPARATION_CAPABILITY) || !pinned_file(MODEL_PREPARATION_SCHEMA) {
                                            return Err(unqualified());
                                        }
                                        if large_model_pinned {
                                            if !pinned_file(LARGE_MODEL_PREPARATION_CAPABILITY)
                                                || !pinned_file(LARGE_MODEL_PREPARATION_SCHEMA)
                                                || (large_recovery_pinned && !pinned_file(LARGE_MODEL_RECOVERY_CAPABILITY))
                                            {
                                                return Err(unqualified());
                                            }
                                            return Ok(Some(WorkspaceFurnishingCatalog::LargeModelPreparation046));
                                        }
                                        if fresh_image_pinned {
                                            if !pinned_file(FRESH_IMAGE_PREPARATION_CAPABILITY) || !pinned_file(FRESH_IMAGE_PREPARATION_SCHEMA) {
                                                return Err(unqualified());
                                            }
                                            return Ok(Some(WorkspaceFurnishingCatalog::FreshImagePreparation047));
                                        }
                                        if gpu_pinned {
                                            if !GPU_MODEL_CAPABILITY.is_some_and(pinned_file)
                                                || !pinned_file(GPU_MODEL_SCHEMA)
                                                || ["platform/scripts/native-model-adoption-capability-v1.json",
                                                    "platform/scripts/native-model-adoption-carried-target-capability-v1.json"]
                                                    .into_iter().any(|path| self.manifest.files.iter().any(|file| file.path == path))
                                            { return Err(unqualified()); }
                                            return Ok(Some(WorkspaceFurnishingCatalog::GpuModelPreparation049));
                                        }
                                        if thumbnail_pinned {
                                            if !pinned_file(FURNISHING_THUMBNAIL_CAPABILITY)
                                                || !pinned_file(FURNISHING_THUMBNAIL_SCHEMA)
                                                || ["platform/scripts/native-model-adoption-capability-v1.json",
                                                    "platform/scripts/native-model-adoption-carried-target-capability-v1.json"]
                                                    .into_iter().any(|path| self.manifest.files.iter().any(|file| file.path == path))
                                            { return Err(unqualified()); }
                                            return Ok(Some(WorkspaceFurnishingCatalog::FurnishingThumbnail050));
                                        }
                                        if info_stand_pinned {
                                            if !pinned_file(INFO_STAND_PREPARATION_CAPABILITY) || !pinned_file(INFO_STAND_PREPARATION_SCHEMA) {
                                                return Err(unqualified());
                                            }
                                            return Ok(Some(WorkspaceFurnishingCatalog::InfoStandPreparation048));
                                        }
                                        return Ok(Some(WorkspaceFurnishingCatalog::RequiredModelPreparation045));
                                    }
                                    return Ok(Some(WorkspaceFurnishingCatalog::HayesRelease044));
                                }
                                return Ok(Some(WorkspaceFurnishingCatalog::HayesDisposition043));
                            }
                            return Ok(Some(WorkspaceFurnishingCatalog::HayesOriginal042));
                        }
                        return Ok(Some(WorkspaceFurnishingCatalog::FramePreparation041));
                    }
                    return Ok(Some(WorkspaceFurnishingCatalog::MaterialMultimap040));
                }
                return Ok(Some(WorkspaceFurnishingCatalog::MaterialPublication039));
            }
            return Ok(Some(WorkspaceFurnishingCatalog::Publication038));
        }
        if writer_schema || writer_pin.is_some() {
            let &(_, _, _, writer_marker) = writer_pin.ok_or_else(unqualified)?;
            if writer_marker == WORKSPACE_FURNISHING_CAPABILITY || !regular(writer_marker)
                || !regular(WORKSPACE_FURNISHING_WRITER_SCHEMA)
            {
                return Err(unqualified());
            }
            return Ok(Some(WorkspaceFurnishingCatalog::Writer037));
        }
        if !legacy.contains(&tuple) { return Err(unqualified()); }
        Ok(Some(WorkspaceFurnishingCatalog::Schema036))
    }

    pub(crate) fn workspace_furnishing_ledger(&self, catalog: WorkspaceFurnishingCatalog) -> Result<String> {
        let directories = [
            ("migrations", 1..=25),
            ("native-oex-normal", 26..=32),
            ("native-oex-workspace-import", 33..=34),
            ("native-oex-workspace-import-wall", 35..=35),
            ("native-workspace-furnishing", 36..=36),
        ];
        let mut rows = std::collections::BTreeMap::new();
        for file in &self.manifest.files {
            let Some((directory, name)) = file.path.strip_prefix("platform/database/").and_then(|path| path.split_once('/')) else {
                continue;
            };
            if !name.ends_with(".sql") { continue; }
            if catalog == WorkspaceFurnishingCatalog::GpuModelPreparation049
                && name.split_once('_').and_then(|(n, _)| n.parse::<u32>().ok()).is_some_and(|n| n > 45)
                && file.path != GPU_MODEL_SCHEMA.0
            { return Err(invalid()); }
            if catalog == WorkspaceFurnishingCatalog::FurnishingThumbnail050
                && name.split_once('_').and_then(|(n, _)| n.parse::<u32>().ok()).is_some_and(|n| n > 45)
                && file.path != FURNISHING_THUMBNAIL_SCHEMA.0
            { return Err(invalid()); }
            let number = if file.path.starts_with(WORKSPACE_FURNISHING_WRITER_DIRECTORY) {
                if !catalog.uses_writer_lease() || file.path != WORKSPACE_FURNISHING_WRITER_SCHEMA {
                    return Err(invalid());
                }
                37
            } else if file.path.starts_with(WORKSPACE_FURNISHING_PUBLICATION_DIRECTORY) {
                if catalog.version() < 38
                    || file.path != WORKSPACE_FURNISHING_PUBLICATION_SCHEMA
                    || file.bytes != Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_BYTES)
                    || file.sha256.as_deref() != Some(WORKSPACE_FURNISHING_PUBLICATION_SCHEMA_SHA256)
                {
                    return Err(invalid());
                }
                38
            } else if file.path.starts_with(WORKSPACE_MATERIAL_PUBLICATION_DIRECTORY) {
                let (path, bytes, hash) = WORKSPACE_MATERIAL_PUBLICATION_SCHEMA;
                if catalog.version() < 39
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                39
            } else if file.path.starts_with(WORKSPACE_MATERIAL_MULTIMAP_DIRECTORY) {
                let (path, bytes, hash) = WORKSPACE_MATERIAL_MULTIMAP_SCHEMA;
                if catalog.version() < 40
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                40
            } else if file.path.starts_with(WORKSPACE_FRAME_PREPARATION_DIRECTORY) {
                let (path, bytes, hash) = WORKSPACE_FRAME_PREPARATION_SCHEMA;
                if catalog.version() < 41
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                41
            } else if file.path.starts_with(HAYES_ORIGINAL_DIRECTORY) {
                let (path, bytes, hash) = HAYES_ORIGINAL_SCHEMA;
                if catalog.version() < 42
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                42
            } else if file.path.starts_with(HAYES_DISPOSITION_DIRECTORY) {
                let (path, bytes, hash) = HAYES_DISPOSITION_SCHEMA;
                if catalog.version() < 43
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                43
            } else if file.path.starts_with(HAYES_RELEASE_DIRECTORY) {
                let (path, bytes, hash) = HAYES_RELEASE_SCHEMA;
                if catalog.version() < 44
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                44
            } else if file.path.starts_with(MODEL_PREPARATION_DIRECTORY) {
                let (path, bytes, hash) = MODEL_PREPARATION_SCHEMA;
                if catalog.version() < 45
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                45
            } else if file.path.starts_with(LARGE_MODEL_PREPARATION_DIRECTORY) {
                let (path, bytes, hash) = LARGE_MODEL_PREPARATION_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::LargeModelPreparation046
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                46
            } else if file.path.starts_with(FRESH_IMAGE_PREPARATION_DIRECTORY) {
                let (path, bytes, hash) = FRESH_IMAGE_PREPARATION_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::FreshImagePreparation047
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                47
            } else if file.path.starts_with(GPU_MODEL_DIRECTORY) {
                let (path, bytes, hash) = GPU_MODEL_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::GpuModelPreparation049
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                { return Err(invalid()); }
                49
            } else if file.path.starts_with(FURNISHING_THUMBNAIL_DIRECTORY) {
                let (path, bytes, hash) = FURNISHING_THUMBNAIL_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::FurnishingThumbnail050
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                { return Err(invalid()); }
                50
            } else if file.path.starts_with(INFO_STAND_PREPARATION_DIRECTORY) {
                let (path, bytes, hash) = INFO_STAND_PREPARATION_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::InfoStandPreparation048
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                48
            } else {
                let Some((_, range)) = directories.iter().find(|(expected, _)| *expected == directory) else { continue; };
                let Some(number) = name.split_once('_').and_then(|(number, _)| number.parse::<usize>().ok()) else { return Err(invalid()); };
                if !range.contains(&number) || !name.starts_with(&format!("{number:03}_")) { return Err(invalid()); }
                number
            };
            let hash = file.sha256.as_deref().filter(|hash| hash_valid(hash)).ok_or_else(invalid)?;
            if !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                || file.link.is_some() || file.executable != Some(false)
                || rows.insert(number, format!("{name}|{hash}")).is_some()
            {
                return Err(invalid());
            }
        }
        // These isolated catalogs append one migration to 045.
        let count = match catalog {
            WorkspaceFurnishingCatalog::FreshImagePreparation047 | WorkspaceFurnishingCatalog::InfoStandPreparation048
                | WorkspaceFurnishingCatalog::GpuModelPreparation049 | WorkspaceFurnishingCatalog::FurnishingThumbnail050 => 46,
            _ => catalog.version(),
        };
        if rows.len() != count as usize { return Err(invalid()); }
        Ok(rows.into_values().collect::<Vec<_>>().join("\n"))
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn catalog39_upgrade_source_ledger(&self) -> Result<String> {
        let full = self.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::MaterialMultimap040)?;
        let (source, last) = full.rsplit_once('\n').ok_or_else(invalid)?;
        let (path, _, sha) = WORKSPACE_MATERIAL_MULTIMAP_SCHEMA;
        let name = path.rsplit('/').next().ok_or_else(invalid)?;
        if last != format!("{name}|{sha}") { return Err(invalid()); }
        Ok(source.into())
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn catalog40_upgrade_source_ledger(&self) -> Result<String> {
        let full = self.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::HayesRelease044)?;
        Ok(full.lines().take(40).collect::<Vec<_>>().join("\n"))
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn catalog49_upgrade_source_ledger(&self) -> Result<String> {
        let full = self.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::GpuModelPreparation049)?;
        let (source, last) = full.rsplit_once('\n').ok_or_else(invalid)?;
        let (path, _, sha) = GPU_MODEL_SCHEMA;
        if last != format!("{}|{sha}", path.rsplit('/').next().ok_or_else(invalid)?) { return Err(invalid()); }
        Ok(source.into())
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn catalog50_upgrade_source_ledger(&self) -> Result<String> {
        let full = self.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::FurnishingThumbnail050)?;
        let (source, last) = full.rsplit_once('\n').ok_or_else(invalid)?;
        let (path, _, sha) = FURNISHING_THUMBNAIL_SCHEMA;
        if last != format!("{}|{sha}", path.rsplit('/').next().ok_or_else(invalid)?) { return Err(invalid()); }
        Ok(source.into())
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn catalog44_upgrade_source_ledger(&self) -> Result<String> {
        let full = self.workspace_furnishing_ledger(WorkspaceFurnishingCatalog::RequiredModelPreparation045)?;
        let (source, _) = full.rsplit_once('\n').ok_or_else(invalid)?;
        Ok(source.into())
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn open_workspace_import_validation(root: &Path, version: WorkspaceImport, selected: bool) -> Result<Self> {
        let mut payload = Self::open(root)?;
        let approved = match version {
            WorkspaceImport::V6 => WORKSPACE_V6_VALIDATION_PROVENANCE,
            WorkspaceImport::V7 => WORKSPACE_V7_VALIDATION_PROVENANCE,
        };
        if let Some(catalog) = payload.workspace_furnishing_catalog()? {
            let marker = match catalog {
                WorkspaceFurnishingCatalog::Publication038 => WORKSPACE_FURNISHING_SELECTED_OEX_CAPABILITY,
                WorkspaceFurnishingCatalog::MaterialPublication039 => WORKSPACE_FURNISHING_SELECTED_OEX_039_CAPABILITY,
                WorkspaceFurnishingCatalog::MaterialMultimap040 => WORKSPACE_FURNISHING_SELECTED_OEX_040_CAPABILITY,
                _ => return Err(err("NATIVE_RUNTIME_UNQUALIFIED")),
            };
            if version != WorkspaceImport::V7 || (selected && payload.retained_oex_cleanup_hook())
                || !workspace_import_files_match(&payload.manifest, version, Some(marker))
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
            payload.workspace_furnishing_ledger(catalog)?;
            if !selected {
                return Ok(payload);
            }
        } else if !workspace_import_validation_matches(&payload.manifest, &payload.manifest_sha256, version, approved) {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        payload.workspace_import = Some((version, selected));
        Ok(payload)
    }
    pub(crate) fn workspace_import_version(&self) -> Option<WorkspaceImport> {
        self.workspace_import.map(|(version, _)| version)
    }
    pub(crate) fn selected_workspace_import_version(&self) -> Option<WorkspaceImport> {
        self.workspace_import.and_then(|(version, selected)| selected.then_some(version))
    }
    pub(crate) fn workspace_import_selected_input(&self) -> bool {
        self.selected_workspace_import_version().is_some()
    }
    pub(crate) fn admits_selected_oex_format(&self, format: &str) -> Result<bool> {
        if self.workspace_import_selected_input() {
            self.recheck()?;
            return Ok(format == "1.0.0-draft.12");
        }
        match format {
            "1.0.0-draft.9" => Ok(true),
            "1.0.0-draft.10" => self.normal_oex_028_selected_input(),
            _ => Ok(false),
        }
    }
    /// Fresh byte admission before a migration writes or runs maintenance tools.
    pub(crate) fn recheck(&self) -> Result<()> {
        let current = Self::open(&self.root)?;
        if current.manifest_sha256 != self.manifest_sha256 { return Err(invalid()); }
        Ok(())
    }
    pub fn context(&self) -> Context {
        Context {
            state: "bytes_verified".into(),
            platform_version: self.manifest.platform_version.clone(),
            source_commit: self.manifest.source_commit.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
            redistribution_qualified: self.manifest.redistribution_qualified,
            official_signed: self.manifest.official_signed,
        }
    }
    pub(crate) fn producer_sha256(&self) -> &str {
        &self.manifest.producer_sha256
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(crate) fn static_refresh_inventory(bytes: &[u8], expected: (&str, &str, &str))
        -> Result<Vec<StaticRefreshRuntimeFile>> {
        let reject = || err("NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED");
        if digest(bytes) != expected.2 {
            return Err(reject());
        }
        let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| reject())?;
        if manifest.source_commit != expected.0 || manifest.producer_sha256 != expected.1
            || manifest.files.is_empty() || manifest.files.len() > 50_000 {
            return Err(reject());
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        manifest.files.into_iter().map(|file| {
            if !relative_name(&file.path) || file.path == "manifest.json" || !names.insert(file.path.clone()) {
                return Err(reject());
            }
            match (file.link, file.bytes, file.sha256, file.executable) {
                (None, Some(bytes), Some(hash), Some(executable)) if hash_valid(&hash) => {
                    total = total.checked_add(bytes).filter(|sum| *sum <= 2 * 1024 * 1024 * 1024).ok_or_else(reject)?;
                    Ok(StaticRefreshRuntimeFile::Regular { path: file.path, bytes, executable })
                }
                (Some(target), None, None, None) if !target.is_empty() && target.len() <= 2048
                    && !Path::new(&target).is_absolute() => {
                    Ok(StaticRefreshRuntimeFile::Link { path: file.path, target })
                }
                _ => Err(reject()),
            }
        }).collect()
    }

    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(crate) fn static_refresh_script(&self, path: &str) -> Result<PathBuf> {
        if !self.manifest.files.iter().any(|item| item.path == path && item.link.is_none()
            && item.executable == Some(false) && item.bytes.is_some_and(|len| len > 0)
            && item.sha256.as_deref().is_some_and(hash_valid)) {
            return Err(err("NATIVE_STATIC_REFRESH_RUNTIME_UNQUALIFIED"));
        }
        contained(&self.root, path)
    }

    pub fn node(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.node)
    }
    /// A signed release additionally needs an explicitly approved source,
    /// producer and exact manifest tuple. A capability record alone is never
    /// authority; the allowlist is intentionally empty pending review.
    pub(crate) fn normal_oex_selected_input(&self) -> Result<bool> {
        self.normal_oex_026_selected_input().and_then(|v| if v { Ok(true) } else { self.normal_oex_027_selected_input() })
    }
    fn normal_oex_026_selected_input(&self) -> Result<bool> {
        self.normal_oex_selected_admission(NORMAL_OEX_RELEASE_PROVENANCE, false)
    }
    pub(crate) fn normal_oex_028_selected_input(&self)->Result<bool> {
        if self.workspace_v5_selected_input() {
            self.recheck()?;
            return Ok(true);
        }
        let concrete = self.normal_oex_030_concrete_tuple();
        let floor=self.normal_oex_029_floor_tuple() || concrete;
        let approved=if self.manifest.mode=="release"&&self.manifest.redistribution_qualified&&self.manifest.official_signed {
            NORMAL_OEX_028_RELEASE_PROVENANCE
        } else if cfg!(all(debug_assertions,target_os="macos"))&&self.manifest.mode=="local-validation"&&!self.manifest.redistribution_qualified&&!self.manifest.official_signed {
            NORMAL_OEX_028_LOCAL_VALIDATION_PROVENANCE
        } else {return Ok(false);};
        if !(floor||approved.iter().any(|(source,producer,hash)|*source==self.manifest.source_commit&&*producer==self.manifest.producer_sha256&&*hash==self.manifest_sha256))
            || !normal_oex_028_inventory_matches(&self.manifest)
            || (floor&&!normal_oex_029_floor_inventory_matches(&self.manifest))
            || (concrete && !normal_oex_030_concrete_inventory_matches(&self.manifest)){return Ok(false);}
        self.recheck()?;
        let path=self.root.join(NORMAL_OEX_028_CAPABILITY_PATH);
        if fs::canonicalize(&path).map_err(|_|invalid())?!=path{return Err(invalid());}
        let (bytes,pin)=crate::hosted_runtime_file::bounded(&path,2048)?;
        let accepted=bytes==NORMAL_OEX_028_CAPABILITY&&digest(&bytes)==NORMAL_OEX_028_CAPABILITY_SHA256;
        pin.check()?;
        if !accepted||!floor{return Ok(accepted);}
        let path=self.root.join(NORMAL_OEX_029_FLOOR_CAPABILITY_PATH);
        if fs::canonicalize(&path).map_err(|_|invalid())?!=path{return Err(invalid());}
        let (bytes,pin)=crate::hosted_runtime_file::bounded(&path,2048)?;
        let accepted=bytes==NORMAL_OEX_029_FLOOR_CAPABILITY&&digest(&bytes)==NORMAL_OEX_029_FLOOR_CAPABILITY_SHA256;
        pin.check()?;
        if !accepted || !concrete { return Ok(accepted); }
        let path = self.root.join(NORMAL_OEX_030_CONCRETE_CAPABILITY_PATH);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2048)?;
        let accepted = bytes.len() as u64 == NORMAL_OEX_030_CONCRETE_CAPABILITY_BYTES
            && digest(&bytes) == NORMAL_OEX_030_CONCRETE_CAPABILITY_SHA256;
        pin.check()?;
        Ok(accepted)
    }
    fn normal_oex_029_floor_tuple(&self)->bool {
        cfg!(all(debug_assertions,target_os="macos"))&&self.manifest.mode=="local-validation"
            &&!self.manifest.redistribution_qualified&&!self.manifest.official_signed
            &&NORMAL_OEX_029_FLOOR_LOCAL_VALIDATION_PROVENANCE.iter().any(|(source,producer,hash)|
                *source==self.manifest.source_commit&&*producer==self.manifest.producer_sha256&&*hash==self.manifest_sha256)
    }
    pub(crate) fn normal_oex_029_floor_selected_input(&self)->Result<bool> {
        if !self.normal_oex_029_floor_tuple() && !self.normal_oex_030_concrete_tuple(){return Ok(false);}
        self.normal_oex_028_selected_input()
    }
    /// Only identifies the pinned successor. Admission also requires the
    /// complete 028/029/030 inventory and capability checks.
    pub(crate) fn normal_oex_030_concrete_tuple(&self) -> bool {
        cfg!(all(debug_assertions, target_os = "macos"))
            && self.manifest.mode == "local-validation"
            && !self.manifest.redistribution_qualified && !self.manifest.official_signed
            && (self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str())
                == NORMAL_OEX_030_CONCRETE_LOCAL_VALIDATION_PROVENANCE
    }
    pub(crate) fn oex_format_hint(&self)->Result<(PathBuf,PathBuf,PathBuf)> {
        let script = if self.workspace_import_selected_input() {
            self.recheck()?;
            WORKSPACE_IMPORT_FORMAT_HINT
        } else if self.normal_oex_028_selected_input()? {
            "platform/apps/api/dist/oex-selected-format.js"
        } else {
            return Err(err("NATIVE_OEX_FORMAT_UNAVAILABLE"));
        };
        Ok((self.node(), self.root.join(script), self.root.clone()))
    }
    pub(crate) fn normal_oex_027_selected_input(&self) -> Result<bool> {
        if self.workspace_import_selected_input() { self.recheck()?; return Ok(true); }
        if self.normal_oex_028_selected_input()? {return Ok(true);}
        if self.normal_oex_selected_admission(NORMAL_OEX_027_RELEASE_PROVENANCE, true)? { return Ok(true); }
        // Only the compiled development tuple enters this path. Existing fresh
        // selected-root/UID/owner and stage fences remain in Session/controller.
        if !normal_oex_027_local_validation_matches(&self.manifest, &self.manifest_sha256, NORMAL_OEX_CAPABILITY,
            NORMAL_OEX_027_LOCAL_VALIDATION_PROVENANCE) { return Ok(false); }
        self.recheck()?;
        let path = self.root.join(NORMAL_OEX_CAPABILITY_PATH);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 1024)?;
        let accepted = normal_oex_027_local_validation_matches(&self.manifest, &self.manifest_sha256, &bytes,
            NORMAL_OEX_027_LOCAL_VALIDATION_PROVENANCE);
        pin.check()?;
        Ok(accepted)
    }
    fn normal_oex_selected_admission(&self, approved: &[(&str, &str, &str)], with_027: bool) -> Result<bool> {
        if !approved.iter().any(|(source, producer, hash)| *source == self.manifest.source_commit.as_str()
            && *producer == self.manifest.producer_sha256.as_str() && *hash == self.manifest_sha256.as_str()) { return Ok(false); }
        self.recheck()?;
        let path = self.root.join(NORMAL_OEX_CAPABILITY_PATH);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 1024)?;
        let accepted = if with_027 { normal_oex_027_declaration_matches(&self.manifest, &self.manifest_sha256, &bytes, approved) }
            else { normal_oex_release_declaration_matches(&self.manifest, &self.manifest_sha256, &bytes, approved) };
        pin.check()?;
        Ok(accepted)
    }
    /// Fixed private OEX workers. Their presence alone is not admission or
    /// authority; the selected Session must independently bind the payload.
    pub(crate) fn oex_packed_worker(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = "platform/apps/api/dist/oex-owned-packed-worker.js";
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    pub(crate) fn oex_materializer(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = if self.workspace_import_selected_input() { WORKSPACE_IMPORT_MATERIALIZER } else { "platform/apps/api/dist/oex-owned-chunk-materializer.js" };
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    pub(crate) fn oex_preflight(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = if self.workspace_import_selected_input() { WORKSPACE_IMPORT_PREFLIGHT } else { "platform/apps/api/dist/oex-owned-preflight.js" };
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    /// Fixed codec-only resources, rechecked against this admitted manifest and
    /// held across authentication. No arbitrary module or executable selection.
    pub(crate) fn oex_packed_worker_v2(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = if self.workspace_import_selected_input() { WORKSPACE_IMPORT_PACKED } else { "platform/apps/api/dist/oex-owned-packed-worker-v2.js" };
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    /// Fixed codec-only resources, rechecked against this admitted manifest and
    /// held across authentication. No arbitrary module or executable selection.
    pub(crate) fn legacy_codec_pins(&self) -> Result<Vec<crate::hosted_runtime_file::Pin>> {
        let mut pins = Vec::new();
        for (name, executable, limit) in [
            (self.manifest.entry.node.as_str(), true, 160 * 1024 * 1024),
            ("platform/packages/storage/dist/service-backup.js", false, 2 * 1024 * 1024),
            ("platform/packages/storage/dist/service-inventory.js", false, 2 * 1024 * 1024),
            ("platform/packages/storage/dist/encrypted-files.js", false, 2 * 1024 * 1024),
        ] {
            let item = self.manifest.files.iter().find(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
                .ok_or_else(|| err("NATIVE_LEGACY_CODEC_UNAVAILABLE"))?;
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, limit)?;
            if item.bytes != Some(bytes.len() as u64) || item.sha256.as_deref() != Some(digest(&bytes).as_str()) { return Err(invalid()); }
            pins.push(pin);
        }
        Ok(pins)
    }
    pub fn backup_resources(&self) -> Result<BackupResources> {
        for (name, executable) in [
            ("platform/scripts/service-backup.mjs", false),
            ("platform/scripts/native-service-backup.mjs", false),
            ("postgres/bin/pg_dump", true),
            ("postgres/bin/pg_restore", true),
        ] {
            if !self
                .manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
            {
                return Err(err("NATIVE_BACKUP_UNAVAILABLE"));
            }
        }
        Ok(BackupResources {
            script: self.root.join("platform/scripts/native-service-backup.mjs"),
            pg_dump: self.root.join("postgres/bin/pg_dump"),
            pg_restore: self.root.join("postgres/bin/pg_restore"),
        })
    }
    pub(crate) fn backup_runtime_bytes(&self) -> Result<u64> {
        let name = "platform/apps/web/dist/freeze-runtime.json";
        let item = self.manifest.files.iter().find(|v| v.path == name && v.link.is_none() && v.executable == Some(false)).ok_or_else(invalid)?;
        let path = self.root.join(name);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 256 * 1024)?;
        if item.bytes != Some(bytes.len() as u64) || item.sha256.as_deref() != Some(digest(&bytes).as_str()) { return Err(invalid()); }
        let total = freeze_backup_runtime_bytes(&self.manifest, &bytes)?;
        pin.check()?;
        Ok(total)
    }
    pub fn static_export_resources(&self, private_editor: bool) -> Result<(PathBuf, PathBuf)> {
        for name in [
            "platform/scripts/native-static-export.mjs",
            "platform/scripts/export-static-publication.mjs",
            "platform/apps/web/dist-static/static.html",
            "platform/apps/web/dist-static/THIRD_PARTY_NOTICES.txt",
        ] {
            if !self
                .manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(false))
            {
                return Err(err("NATIVE_STATIC_EXPORT_UNAVAILABLE"));
            }
        }
        if private_editor {
            let entry = "platform/apps/web/dist-static/static-studio.html";
            if !self.manifest.files.iter().any(|f| f.path == entry && f.link.is_none() && f.executable == Some(false)) {
                return Err(err("NATIVE_STATIC_PRIVATE_EDITOR_UNAVAILABLE"));
            }
            self.recheck()?;
        }
        Ok((
            self.root.join("platform/scripts/native-static-export.mjs"),
            self.root.join("platform/apps/web/dist-static"),
        ))
    }
    pub(crate) fn static_detail_export_resources(
        &self,
        private_editor: bool,
    ) -> Result<(PathBuf, PathBuf)> {
        let provenance = (
            self.manifest.source_commit.as_str(),
            self.manifest.producer_sha256.as_str(),
            self.manifest_sha256.as_str(),
        );
        #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
        let combined_local_validation = CATALOG45_COMPACT_AND_CANDIDATE_TARGET == provenance
            || CATALOG45_CURRENT_ORIGINAL_TARGET == provenance
            || CATALOG45_PRODUCT_IMAGE_FIT_WHEAT_TARGET == provenance
            || CATALOG45_RETAINED_PRODUCT_IMAGE_TARGET == provenance
            || (private_editor
                && ([CATALOG45_PLACED_EXIT_DOORS, CATALOG45_BACKED_BENCH].contains(&provenance)
                    || CATALOG45_CARRIED_RESTORE_BASE == provenance
                    || self.native_model_adoption_original_target()
                    || furnishing_thumbnail_host_matches(provenance)
                    || gpu_model_host_matches(provenance)))
            || (catalog45_recovery_host_matches(provenance)
                && !self.catalog45_pre_ready_preservation_validation()
                && !self.catalog45_pre_ready_hayes_preservation_validation());
        #[cfg(not(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos")))]
        let combined_local_validation = false;
        if (CATALOG40_WHEAT_DETAIL != Some(provenance)
            && !ordinary_hayes_release_host_matches(provenance)
            && !catalog45_native_publication_host_matches(provenance)
            && !combined_local_validation)
            || !static_detail_inventory_matches(&self.manifest.files)
        {
            return Err(err("NATIVE_STATIC_DETAIL_UNAVAILABLE"));
        }
        let resources = self.static_export_resources(private_editor)?;
        if !private_editor {
            self.recheck()?;
        }
        Ok(resources)
    }
    pub fn script(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.script)
    }
    #[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn hayes_session_probe_script(&self) -> Result<Option<PathBuf>> {
        let tuple = (self.manifest.source_commit.as_str(), self.manifest.producer_sha256.as_str(), self.manifest_sha256.as_str());
        let Some((path, pin)) = [
            (HAYES_SESSION_PROBE_SCRIPT, HAYES_SESSION_PROBE_PIN.as_ref()),
            (HAYES_DISPOSITION_PROBE_SCRIPT, HAYES_DISPOSITION_PROBE_PIN.as_ref()),
            (HAYES_RELEASE_PROBE_SCRIPT, HAYES_RELEASE_PROBE_PIN.as_ref()),
        ].into_iter().find_map(|(path, pin)| pin.filter(|pin| pin.host == tuple).map(|pin| (path, pin))) else {
            return Ok(None);
        };
        if !self.manifest.files.iter().any(|file| file.path == path
            && file.link.is_none() && file.executable == Some(false) && file.bytes == Some(pin.script_bytes)
            && file.sha256.as_deref() == Some(pin.script_sha256))
        {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        Ok(Some(contained(&self.root, path)?))
    }

    pub(crate) fn authoring_session_script(&self) -> Result<PathBuf> {
        let name = "platform/scripts/native-workspace-session.mjs";
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_AUTHORING_RENEWAL_UNAVAILABLE"));
        }
        // The one-shot process imports the admitted API/storage modules too.
        // Reuse the complete existing byte admission rather than accepting a
        // caller-selected script or an independently copied dependency tree.
        self.recheck()?;
        Ok(self.root.join(name))
    }
    pub fn postgres(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.postgres)
    }
    pub fn initdb(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.initdb)
    }
}

/// Byte-qualified native migration inputs. This is not an observed schema or an
/// update permit; only the fixed target observer consumes it.
pub(crate) struct TargetInventoryResources {
    pub(crate) platform_root: PathBuf,
    pub(crate) migrations: Vec<u8>,
    pub(crate) migrations_sha256: String,
    pins: Vec<crate::hosted_runtime_file::Pin>,
}
impl TargetInventoryResources {
    pub(crate) fn recheck(&self) -> Result<()> {
        for pin in &self.pins {
            pin.check()?;
        }
        Ok(())
    }
}
impl Payload {
    pub(crate) fn target_inventory_resources(&self) -> Result<TargetInventoryResources> {
        self.recheck()?;
        let mut pins = self.legacy_codec_pins()?;
        for name in [
            "platform/packages/storage/dist/index.js",
            "platform/packages/storage/dist/blobs.js",
        ] {
            let item = self
                .manifest
                .files
                .iter()
                .find(|v| v.path == name && v.link.is_none() && v.executable == Some(false))
                .ok_or_else(invalid)?;
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path {
                return Err(invalid());
            }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            if item.bytes != Some(bytes.len() as u64)
                || item.sha256.as_deref() != Some(digest(&bytes).as_str())
            {
                return Err(invalid());
            }
            pins.push(pin);
        }
        #[derive(Serialize)]
        struct Migration {
            name: String,
            sha256: String,
        }
        let prefix = "platform/database/migrations/";
        let mut rows = std::collections::BTreeMap::new();
        let mut total = 0u64;
        for item in &self.manifest.files {
            let Some(name) = item.path.strip_prefix(prefix) else {
                continue;
            };
            if !name.as_bytes().first().is_some_and(u8::is_ascii_digit) || !name.ends_with(".sql") {
                continue;
            }
            if name.len() > 256
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                || item.link.is_some()
                || item.executable != Some(false)
            {
                return Err(invalid());
            }
            let path = self.root.join(&item.path);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path {
                return Err(invalid());
            }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            total = total
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or_else(invalid)?;
            let hash = digest(&bytes);
            if item.bytes != Some(bytes.len() as u64)
                || item.sha256.as_deref() != Some(hash.as_str())
                || rows.insert(name.to_owned(), hash).is_some()
            {
                return Err(invalid());
            }
            pins.push(pin);
        }
        if rows.is_empty() || rows.len() > 10000 {
            return Err(invalid());
        }
        let rows: Vec<_> = rows
            .into_iter()
            .map(|(name, sha256)| Migration { name, sha256 })
            .collect();
        let migrations = serde_json::to_vec(&rows).map_err(|_| invalid())?;
        if migrations.len() > 65536 {
            return Err(invalid());
        }
        let resources = TargetInventoryResources {
            platform_root: self.root.join("platform"),
            migrations_sha256: digest(&migrations),
            migrations,
            pins,
        };
        resources.recheck()?;
        self.recheck()?;
        Ok(resources)
    }
}

/// authenticated-native-copy-v1: already qualified codec/worker bytes only.
/// A signed migration declaration never grants permission to execute SQL.
const NATIVE_COPY_FILES: &[(&str, &str)] = &[
    ("platform/scripts/native-service-backup.mjs", "72fa2bc234db3aa19975ab05d68b2fc584982cec8d050676a79e2a575bd46feb"),
    ("platform/scripts/service-backup.mjs", "d94be6e99bd966ca2697a58575dcb4a2c3bc9e06a3facca55a53039f377c08aa"),
    ("platform/packages/storage/dist/blobs.d.ts", "5659727be1441d2e279212046be5eb2510c1ae12a48920278855f83c7e32ccf8"),
    ("platform/packages/storage/dist/blobs.js", "f948f6c7f67737b009b032b2cdde3d34fbcc7148eeb851a5b146bd553e239d14"),
    ("platform/packages/storage/dist/encrypted-files.d.ts", "f4cedc62afc12ed39a66ef945d1403a34e7e8804f794663e59c6db8a9ad91c66"),
    ("platform/packages/storage/dist/encrypted-files.js", "c3b11cc821812240cf25f5c172eceac58c61d92c8de6ea4991fcefada44b5fb1"),
    ("platform/packages/storage/dist/encrypted-streams.d.ts", "e3df95cc6dbcbafae693a0b46b22df410637b5580fa37d37d14ee51ff56541d9"),
    ("platform/packages/storage/dist/encrypted-streams.js", "a9c334cf62ff714a6fa5c65d7a8d8a1671244925e1064881b5392c190a51e80c"),
    ("platform/packages/storage/dist/index.d.ts", "681578697596af7b6e63295c14407a864858aba430d4ecf251671eeab3b40720"),
    ("platform/packages/storage/dist/index.js", "e47cff1a28bd576d2c2842ee6de27cb8367e89ff6d2028609d9536b1863c6af1"),
    ("platform/packages/storage/dist/migrations.d.ts", "d9ab9bb5a3d6d10ac82f5ef1495cc1efa44a7c536318d0c429ca831b2d91aee9"),
    ("platform/packages/storage/dist/migrations.js", "ac9c5e10225f8930722eb120148dd40befddf0ab08d2dde4ce682a6cd830dfa1"),
    ("platform/packages/storage/dist/policy.d.ts", "75111683455b8e593182b5ba36d3108eb6e46ab3fa0a2aa183ce198f82d609c4"),
    ("platform/packages/storage/dist/policy.js", "0e25c6ff27120c5fe330db5247736b0ddb3a712b25fc31ed830504a1cec17ff2"),
    ("platform/packages/storage/dist/postgres-blobs.d.ts", "ddf6a4d338be38f936f2d97d6f8e0be6d6eaa734f696c8233cc589729318f59c"),
    ("platform/packages/storage/dist/postgres-blobs.js", "dd9f83ef4f25bacce95d186d167ac8b7b38257f67ee0dedafc6d796af883b823"),
    ("platform/packages/storage/dist/service-backup.d.ts", "eed00e0740aef0b293b205f395886d2e895753b465ab616b5a4e1d3f29026053"),
    ("platform/packages/storage/dist/service-backup.js", "36d1462040133288fafd5b6ecdc0a33aa50567e05f777ee499718d30e329cf1c"),
    ("platform/packages/storage/dist/service-inventory.d.ts", "eb224fb53f33180e9a648ece8d1dc649c97a9eb844bdc40e7ce72899f50d1cb6"),
    ("platform/packages/storage/dist/service-inventory.js", "03b60b30e0f840b6a3c1cd2b842330da43dcb65b5af16a16a5cd7b4996f5fc08"),
];
// Separate immutable same-R5 data-copy recipe. Never a fallback migration or
// production trust exception; every payload still requires ordinary admission.
const NATIVE_R5_COPY_CATALOG: &str = "fcb2ce62fd3d9c5d86d5f37401a91161403981128b229426200e41091b47b8eb";
const NATIVE_R5_COPY_ABSENT: &[&str] = &[
    "platform/packages/storage/dist/migrations.d.ts",
    "platform/packages/storage/dist/migrations.js",
];
const NATIVE_R5_COPY_FILES: &[(&str, &str)] = &[
    ("platform/scripts/native-service-backup.mjs", "cd62e9e8c89f24ea4a002abcb70ac909ce5b25c80e7a4d5e4fd8f5fc95ba5d3b"),
    ("platform/scripts/service-backup.mjs", "b14f6c7790539e689caed08189e3d695efb76a60b582800f1a363d2d415c42c4"),
    ("platform/packages/storage/dist/blobs.d.ts", "5659727be1441d2e279212046be5eb2510c1ae12a48920278855f83c7e32ccf8"),
    ("platform/packages/storage/dist/blobs.js", "f948f6c7f67737b009b032b2cdde3d34fbcc7148eeb851a5b146bd553e239d14"),
    ("platform/packages/storage/dist/encrypted-files.d.ts", "f4cedc62afc12ed39a66ef945d1403a34e7e8804f794663e59c6db8a9ad91c66"),
    ("platform/packages/storage/dist/encrypted-files.js", "c3b11cc821812240cf25f5c172eceac58c61d92c8de6ea4991fcefada44b5fb1"),
    ("platform/packages/storage/dist/encrypted-streams.d.ts", "e3df95cc6dbcbafae693a0b46b22df410637b5580fa37d37d14ee51ff56541d9"),
    ("platform/packages/storage/dist/encrypted-streams.js", "a9c334cf62ff714a6fa5c65d7a8d8a1671244925e1064881b5392c190a51e80c"),
    ("platform/packages/storage/dist/index.d.ts", "392d195f251b6c1703a9bbaa5e2da6ba9ccc15b4021eb2c47e840810cf87b26e"),
    ("platform/packages/storage/dist/index.js", "2eca15b6a7b9d9447d96818dfd3db2022e319ec6cf42350319945b5125932880"),
    ("platform/packages/storage/dist/policy.d.ts", "75111683455b8e593182b5ba36d3108eb6e46ab3fa0a2aa183ce198f82d609c4"),
    ("platform/packages/storage/dist/policy.js", "0e25c6ff27120c5fe330db5247736b0ddb3a712b25fc31ed830504a1cec17ff2"),
    ("platform/packages/storage/dist/postgres-blobs.d.ts", "ddf6a4d338be38f936f2d97d6f8e0be6d6eaa734f696c8233cc589729318f59c"),
    ("platform/packages/storage/dist/postgres-blobs.js", "dd9f83ef4f25bacce95d186d167ac8b7b38257f67ee0dedafc6d796af883b823"),
    ("platform/packages/storage/dist/service-backup.d.ts", "eed00e0740aef0b293b205f395886d2e895753b465ab616b5a4e1d3f29026053"),
    ("platform/packages/storage/dist/service-backup.js", "36d1462040133288fafd5b6ecdc0a33aa50567e05f777ee499718d30e329cf1c"),
    ("platform/packages/storage/dist/service-inventory.d.ts", "eb224fb53f33180e9a648ece8d1dc649c97a9eb844bdc40e7ce72899f50d1cb6"),
    ("platform/packages/storage/dist/service-inventory.js", "03b60b30e0f840b6a3c1cd2b842330da43dcb65b5af16a16a5cd7b4996f5fc08"),
];
const NATIVE_R5_COPY_TOOLS: &[(&str, u64, &str)] = &[
    ("bin/node", 122129232, "e4b5a3af0e05c75de2eae013904145f40fe7fc2a6e6f17510128bf45cca4e79b"),
    ("postgres/bin/postgres", 18295920, "c7d83df0d39322c406688d2149872ed38cbee96a694485c25f2ecf323e7818fc"),
    ("postgres/bin/initdb", 424496, "4a274273db3b9663d5c3d69727ca5a7866830b580c8f6d6636a1adc9d5dc0c29"),
    ("postgres/bin/pg_dump", 973712, "fe95f67530dfe79360a6c333d795c006ce8a5bcb07f233b4d3e38b789409b471"),
    ("postgres/bin/pg_restore", 535504, "cdb1c919b3f37c89030b172f5f7c5b83319ca11b8a6a6f243b445a9b708301f9"),
];
fn copy_files_match(manifest: &Manifest, files: &[(&str, &str)]) -> bool {
    files.iter().all(|(name, hash)| manifest.files.iter().any(|v| v.path == *name && v.link.is_none() && v.executable == Some(false) && v.sha256.as_deref() == Some(*hash)))
}
fn r5_copy_manifest_matches(manifest: &Manifest, catalog: &str) -> bool {
    catalog == NATIVE_R5_COPY_CATALOG && copy_files_match(manifest, NATIVE_R5_COPY_FILES)
        && !NATIVE_R5_COPY_ABSENT.iter().any(|name| manifest.files.iter().any(|v| v.path == *name))
        && NATIVE_R5_COPY_TOOLS.iter().all(|(name, size, hash)| manifest.files.iter().any(|v| v.path == *name && v.link.is_none() && v.executable == Some(true) && v.bytes == Some(*size) && v.sha256.as_deref() == Some(*hash)))
}
fn check_copy_absence(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !matches!(fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound) { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
    }
    Ok(())
}
pub(crate) struct NativeCopyResources {
    pub(crate) inventory: TargetInventoryResources,
    pub(crate) recipe_sha256: String,
    pub(crate) recipe_name: &'static str,
    absent: Vec<PathBuf>,
    pins: Vec<crate::hosted_runtime_file::Pin>,
}
impl NativeCopyResources {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.inventory.recheck()?;
        check_copy_absence(&self.absent)?;
        for pin in &self.pins { pin.check()?; }
        Ok(())
    }
}
impl Payload {
    pub(crate) fn native_copy_resources(&self) -> Result<NativeCopyResources> {
        self.recheck()?;
        self.backup_resources()?;
        let inventory = self.target_inventory_resources()?;
        let current = copy_files_match(&self.manifest, NATIVE_COPY_FILES);
        let (recipe_name, files, recipe_sha256) = if current {
            ("authenticated-native-copy-v1", NATIVE_COPY_FILES, digest(&serde_json::to_vec(NATIVE_COPY_FILES).map_err(|_| invalid())?))
        } else {
            if !r5_copy_manifest_matches(&self.manifest, &inventory.migrations_sha256) { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
            for name in NATIVE_R5_COPY_ABSENT {
                if self.manifest.files.iter().any(|v| v.path == *name)
                    || !matches!(fs::symlink_metadata(self.root.join(name)), Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
                    return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE"));
                }
            }
            ("authenticated-native-r5-copy-v1", NATIVE_R5_COPY_FILES, digest(&serde_json::to_vec(&("authenticated-native-r5-copy-v1", NATIVE_R5_COPY_FILES, NATIVE_R5_COPY_ABSENT, NATIVE_R5_COPY_CATALOG, NATIVE_R5_COPY_TOOLS)).map_err(|_| invalid())?))
        };
        let mut pins = Vec::new();
        for (name, expected) in files {
            let entry = self.manifest.files.iter().find(|v| v.path == *name && v.link.is_none() && v.executable == Some(false)).ok_or_else(|| err("NATIVE_TARGET_COPY_UNAVAILABLE"))?;
            if entry.sha256.as_deref() != Some(*expected) { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            if entry.bytes != Some(bytes.len() as u64) || digest(&bytes) != *expected { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
            pins.push(pin);
        }
        let mut absent = Vec::new();
        if !current {
            for (name, size, hash) in NATIVE_R5_COPY_TOOLS {
                let entry = self.manifest.files.iter().find(|v| v.path == *name && v.link.is_none() && v.executable == Some(true) && v.bytes == Some(*size) && v.sha256.as_deref() == Some(*hash)).ok_or_else(|| err("NATIVE_TARGET_COPY_UNAVAILABLE"))?;
                let path = self.root.join(&entry.path);
                if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
                let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 160 * 1024 * 1024)?;
                if bytes.len() as u64 != *size || digest(&bytes) != *hash { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
                pins.push(pin);
            }
            absent = NATIVE_R5_COPY_ABSENT.iter().map(|name| self.root.join(name)).collect();
        }
        let result = NativeCopyResources { inventory, recipe_sha256, recipe_name, absent, pins };
        result.recheck()?; Ok(result)
    }
}

#[cfg(test)]
mod r5_copy_recipe_tests {
    use super::*;
    fn manifest() -> Manifest {
        let mut files: Vec<_> = NATIVE_R5_COPY_FILES.iter().map(|(path, hash)| serde_json::json!({"path":path,"bytes":1,"sha256":hash,"executable":false})).collect();
        files.extend(NATIVE_R5_COPY_TOOLS.iter().map(|(path, bytes, hash)| serde_json::json!({"path":path,"bytes":bytes,"sha256":hash,"executable":true})));
        serde_json::from_value(serde_json::json!({"format":"exhibitos-native-runtime-v1","mode":"local-validation","platform":"darwin-arm64","nodeVersion":"24.21.0","postgresVersion":"18.6","platformVersion":"0.1.0","protocolVersion":"1","sourceCommit":"a".repeat(40),"producerSHA256":"a".repeat(64),"entry":{"node":"bin/node","script":"platform/scripts/local-runtime.mjs","postgres":"postgres/bin/postgres","initdb":"postgres/bin/initdb"},"files":files,"redistributionQualified":false,"officialSigned":false,"remaining":"Pure manifest dispatch negatives, not Payload/admission/actual execution"})).unwrap()
    }
    #[test]
    fn r5_codec_change_and_cross_recipe_hybrid_are_refused() {
        let mut m=manifest(); assert!(r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
        assert!(!copy_files_match(&m,NATIVE_COPY_FILES));
        m.files[0].sha256=Some(NATIVE_COPY_FILES[0].1.into());
        assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
    }
    #[test]
    fn r5_missing_is_not_accepted_as_changed_or_added_module() {
        let mut m=manifest(); let mut entry: Item=serde_json::from_value(serde_json::json!({"path":NATIVE_R5_COPY_ABSENT[0],"bytes":0,"sha256":"0".repeat(64),"executable":false})).unwrap();
        m.files.push(entry); assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
        m.files.pop(); entry=serde_json::from_value(serde_json::json!({"path":NATIVE_R5_COPY_ABSENT[1],"bytes":0,"sha256":"0".repeat(64),"executable":false})).unwrap();
        m.files.push(entry); assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
    }
    #[test]
    fn r5_tool_bytes_size_and_executable_identity_are_bound() {
        for index in NATIVE_R5_COPY_FILES.len()..NATIVE_R5_COPY_FILES.len()+NATIVE_R5_COPY_TOOLS.len() {
            let mut m=manifest(); m.files[index].bytes=Some(7); assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
            let mut m=manifest(); m.files[index].sha256=Some("0".repeat(64)); assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
            let mut m=manifest(); m.files[index].executable=Some(false); assert!(!r5_copy_manifest_matches(&m,NATIVE_R5_COPY_CATALOG));
        }
    }
    #[test]
    fn r5_undeclared_absent_path_becoming_present_invalidates_resources() {
        let root=std::env::temp_dir().join(format!("exhibitos-r5-absence-{}",uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); let path=root.join("migrations.js");
        check_copy_absence(std::slice::from_ref(&path)).unwrap();
        fs::write(&path,b"new unlisted module").unwrap();
        assert!(check_copy_absence(std::slice::from_ref(&path)).is_err());
        fs::remove_file(path).unwrap(); fs::remove_dir(root).unwrap();
    }
    #[test]
    fn r5_other_catalog_cannot_dispatch_same_codec_to_migration() {
        let m=manifest(); assert!(!r5_copy_manifest_matches(&m,&"a".repeat(64)));
        let current=digest(&serde_json::to_vec(NATIVE_COPY_FILES).unwrap());
        let r5=digest(&serde_json::to_vec(&("authenticated-native-r5-copy-v1",NATIVE_R5_COPY_FILES,NATIVE_R5_COPY_ABSENT,NATIVE_R5_COPY_CATALOG,NATIVE_R5_COPY_TOOLS)).unwrap());
        assert_ne!(current,r5,"The existing source-target recipe digest equality refuses cross-recipe pairs");
    }
}

#[cfg(test)]
mod gpu_preview_resource_tests {
    use super::*;
    fn file(path: &str) -> Item {
        Item { path: format!("{GPU_PREVIEW_WEB_PREFIX}{path}"), bytes: Some(128),
            sha256: Some("a".repeat(64)), executable: Some(false), link: None }
    }
    fn files() -> Vec<Item> {
        GPU_PREVIEW_FIXED.into_iter().chain(["assets/native-viewport-ab12.js", "assets/viewport-CD34.css"])
            .map(file).collect()
    }
    #[test]
    fn preview_closure_contains_only_exact_verified_web_dist_files() {
        let mut rows=files();
        rows.push(file("assets/unrelated.png"));
        let mut outside=file("assets/ignored.js");outside.path="platform/apps/web/dist-static/assets/ignored.js".into();rows.push(outside);
        let mut modules=file("assets/ignored.js");modules.path="platform/node_modules/basis/ignored.js".into();rows.push(modules);
        let paths=gpu_preview_resource_paths(&rows).unwrap();
        assert_eq!(paths.len(),6);
        assert!(paths.contains(&"native-viewport.html".into()));
        assert!(paths.contains(&"basis/exhibitos-basis-worker.js".into()));
        assert!(!paths.iter().any(|path| path.starts_with('/') || path.contains("ignored") || path.ends_with(".png")));
    }
    #[test]
    fn preview_closure_refuses_missing_duplicate_symlink_or_unverified_entries() {
        let mut rows=files();rows.remove(0);assert!(gpu_preview_resource_paths(&rows).is_err());
        let mut rows=files();rows.push(file("basis/basis_transcoder.wasm"));assert!(gpu_preview_resource_paths(&rows).is_err());
        for (link, executable, bytes, hash) in [
            (Some("elsewhere".into()),Some(false),Some(128),Some("a".repeat(64))),
            (None,Some(true),Some(128),Some("a".repeat(64))),
            (None,Some(false),Some(0),Some("a".repeat(64))),
            (None,Some(false),Some(16*1024*1024+1),Some("a".repeat(64))),
            (None,Some(false),Some(128),None),
        ] {
            let mut rows=files();let last=rows.last_mut().unwrap();
            last.link=link;last.executable=executable;last.bytes=bytes;last.sha256=hash;
            assert!(gpu_preview_resource_paths(&rows).is_err());
        }
        let only_fixed:Vec<_>=GPU_PREVIEW_FIXED.into_iter().map(file).collect();
        assert!(gpu_preview_resource_paths(&only_fixed).is_err());
        let mut too_many=files();
        for n in 0..255 {too_many.push(file(&format!("assets/chunk-{n}.js")));}
        assert!(gpu_preview_resource_paths(&too_many).is_err());
    }
    #[test]
    fn preview_asset_paths_are_bounded_ascii_relative_single_level_and_query_free() {
        for path in ["/assets/a.js","assets/../a.js","assets/a/b.js","assets/a\\b.js",
            "assets/a.js?x=1","assets/a.js#x","assets/%2e.js","assets/한글.js","assets/a..js","assets/a.wasm"] {
            assert!(!gpu_preview_asset_path(path),"{path}");
        }
        assert!(!gpu_preview_asset_path(&format!("assets/{}.js","a".repeat(200))));
        assert!(gpu_preview_asset_path("assets/native-viewport-Ab_12.js"));
    }
}

#[cfg(test)]
mod admission_io_measurement {
    use super::*;
    #[test]
    #[ignore = "Explicit immutable runtime; compares fresh declared-file admission IO only, not GUI"]
    fn actual_one_and_two_reader_admission_io() {
        let root = PathBuf::from(std::env::var_os("EXHIBITOS_ADMISSION_IO_RUNTIME").expect("explicit runtime"));
        let expected = std::env::var("EXHIBITOS_ADMISSION_IO_MANIFEST_SHA256").expect("explicit manifest SHA256");
        let bytes = fs::read(root.join("manifest.json")).unwrap();
        assert_eq!(digest(&bytes), expected);
        let payload = Payload::open(&root).unwrap();
        let mut observations = Vec::new();
        // ABBA order; data and OS caches retained. Every observation reads all
        // declared bytes. Final inventory and GUI/runtime startup are excluded.
        for readers in [1, 2, 2, 1] {
            let start = std::time::Instant::now();
            if readers == 1 { verify_manifest_files::<1>(&root, &payload.manifest.files).unwrap(); }
            else { verify_manifest_files::<2>(&root, &payload.manifest.files).unwrap(); }
            observations.push(serde_json::json!({"readers":readers,"ms":start.elapsed().as_secs_f64()*1000.0}));
        }
        println!("ADMISSION_IO_MEASUREMENT={}", serde_json::json!({"manifestSHA256":expected,"observations":observations,"scope":"declared-file byte/path/type/mode/link verification only; not complete admission, startup or GUI","cacheCold":false}));
    }
}
