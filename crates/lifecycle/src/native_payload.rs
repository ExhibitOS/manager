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

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) struct HayesSessionProbePin {
    pub(crate) host: (&'static str, &'static str, &'static str),
    script_bytes: u64,
    script_sha256: &'static str,
}

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) const HAYES_SESSION_PROBE_PIN: Option<HayesSessionProbePin> = Some(HayesSessionProbePin {
    host: (
        "8cd9e8e1ccc622f4d228eaf5e25dda9b9128a260",
        "1d46b4eb7bfd5b774aebad364a3131b09c3c42427dd633bb6622434b21bb60a3",
        "a756b012b20fea4485a38f8a7acd2d2eefa475bc96047aafca271778fc7847a4",
    ),
    script_bytes: 7307,
    script_sha256: "cf761453a0567ec1a9e2d0be8e336bec757ac8af323a47541a73080f3aa8f7f8",
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

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Catalog39Upgrade(RuntimeTuple);

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
pub(crate) fn saved_workspace_transition_supported(pair: (&str, &str)) -> bool {
    WORKSPACE_V5_SAVED_TRANSITIONS.contains(&pair) || Catalog40Transition::from_manifests(pair).is_some()
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
}

#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Catalog40Transition {
    White,
    Tangent,
    Upgrade39(Catalog39Upgrade),
    SameCatalog { source: RuntimeTuple, target: RuntimeTuple },
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
impl Catalog40Transition {
    pub(crate) fn from_manifests(pair: (&str, &str)) -> Option<Self> {
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
        (pair == (source.2, target.2)).then_some(Self::SameCatalog { source, target })
    }

    #[cfg(test)]
    pub(crate) fn test_upgrade() -> Self {
        Self::Upgrade39(Catalog39Upgrade((
            "aabb0000aabb0000aabb0000aabb0000aabb0000",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )))
    }

    pub(crate) fn upgrades_catalog(self) -> bool {
        matches!(self, Self::Upgrade39(_))
    }

    pub(crate) fn source_catalog(self) -> WorkspaceFurnishingCatalog {
        if self.upgrades_catalog() { WorkspaceFurnishingCatalog::MaterialPublication039 }
        else { WorkspaceFurnishingCatalog::MaterialMultimap040 }
    }

    pub(crate) fn source(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::White => CATALOG40_FRAME_SOURCE,
            Self::Tangent => CATALOG40_FRAME_TARGET,
            Self::Upgrade39(_) => CATALOG39_UPGRADE_SOURCE,
            Self::SameCatalog { source, .. } => source,
        }
    }

    pub(crate) fn target(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::White => CATALOG40_FRAME_TARGET,
            Self::Tangent => CATALOG40_TANGENT_TARGET,
            Self::Upgrade39(target) => target.0,
            Self::SameCatalog { target, .. } => target,
        }
    }
}
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
        }
    }

    pub(crate) fn uses_writer_lease(self) -> bool {
        matches!(self, Self::Writer037 | Self::Publication038 | Self::MaterialPublication039
            | Self::MaterialMultimap040 | Self::FramePreparation041 | Self::HayesOriginal042)
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
        let transition = Catalog40Transition::same_catalog_from_manifests((source.2, target.2), Some(source), Some(target)).unwrap();
        assert_eq!(transition.source(), source);
        assert_eq!(transition.target(), target);
        assert_eq!(transition.source_catalog(), WorkspaceFurnishingCatalog::MaterialMultimap040);
        assert!(!transition.upgrades_catalog());
        assert!(Catalog40Transition::same_catalog_from_manifests((target.2, source.2), Some(source), Some(target)).is_none());
        assert!(Catalog40Transition::same_catalog_from_manifests((CATALOG40_TANGENT_TARGET.2, target.2), Some(source), Some(target)).is_none());
        assert!(Catalog40Transition::same_catalog_from_manifests((source.2, target.2), Some(source), None).is_none());
        let reviewed = CATALOG40_EXB_TARGET.unwrap();
        assert!(saved_workspace_transition_target(reviewed.2));
        assert_eq!(Catalog40Transition::from_manifests((source.2, reviewed.2)).unwrap().target(), reviewed);
        assert!(Catalog40Transition::from_manifests((reviewed.2, source.2)).is_none());
        let restore = CATALOG40_RESTORE_PROVENANCE_TARGET.unwrap();
        assert!(saved_workspace_transition_target(restore.2));
        assert_eq!(Catalog40Transition::from_manifests((reviewed.2, restore.2)).unwrap().source(), reviewed);
        assert!(Catalog40Transition::from_manifests((restore.2, reviewed.2)).is_none());
        assert!(Catalog40Transition::from_manifests((source.2, restore.2)).is_none());
        let frames = CATALOG40_FRAME_RECORD_TARGET.unwrap();
        let refresh = Catalog40Transition::from_manifests((restore.2, frames.2)).unwrap();
        assert_eq!(refresh.target(), frames);
        assert_eq!(refresh.source_catalog(), WorkspaceFurnishingCatalog::MaterialMultimap040);
        assert!(saved_workspace_transition_target(frames.2));
        assert!(Catalog40Transition::from_manifests((frames.2, restore.2)).is_none());
        assert!(Catalog40Transition::from_manifests((reviewed.2, frames.2)).is_none());
        let frame_preparation = WORKSPACE_FRAME_PREPARATION_VALIDATION_PROVENANCE[0];
        assert!(Catalog40Transition::from_manifests((frame_preparation.2, frames.2)).is_none());
        assert!(Catalog40Transition::from_manifests((frames.2, frame_preparation.2)).is_none());
        let replacement = CATALOG40_ARTWORK_REPLACEMENT_TARGET.unwrap();
        let transition = Catalog40Transition::from_manifests((frames.2, replacement.2)).unwrap();
        assert_eq!(transition.source(), frames);
        assert_eq!(transition.target(), replacement);
        assert!(!transition.upgrades_catalog());
        assert!(saved_workspace_transition_target(replacement.2));
        assert!(Catalog40Transition::from_manifests((restore.2, replacement.2)).is_none());
        assert!(Catalog40Transition::from_manifests((replacement.2, frames.2)).is_none());
        let grant = CATALOG40_WHEAT_GRANT.unwrap();
        let detail = CATALOG40_WHEAT_DETAIL.unwrap();
        let transition = Catalog40Transition::from_manifests((grant.2, detail.2)).unwrap();
        assert_eq!(transition.source(), grant);
        assert_eq!(transition.target(), detail);
        assert!(!transition.upgrades_catalog());
        assert!(saved_workspace_transition_target(detail.2));
        assert!(Catalog40Transition::from_manifests((detail.2, grant.2)).is_none());
        assert!(Catalog40Transition::from_manifests((replacement.2, detail.2)).is_none());
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
            let path = contained(&root, &item.path)?;
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
                    if expected != path || !expected.starts_with(&root) {
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
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        payload.workspace_v5_validation = selected;
        Ok(payload)
    }
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn open_saved_workspace_transition(root: &Path) -> Result<Self> {
        let payload = Self::open(root)?;
        let context = payload.context();
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
        if [Catalog40Transition::White, Catalog40Transition::Tangent].iter().any(|pair|
            context.manifest_sha256 == pair.source().2 || context.manifest_sha256 == pair.target().2)
        {
            if payload.workspace_furnishing_catalog()? == Some(WorkspaceFurnishingCatalog::MaterialMultimap040) {
                return Ok(payload);
            }
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        Self::open_workspace_v5_validation(root, false)
    }

    pub(crate) fn workspace_v5_selected_input(&self) -> bool {
        self.workspace_v5_validation
    }
    pub(crate) fn workspace_furnishing_validation(&self) -> Result<bool> {
        Ok(self.workspace_furnishing_catalog()?.is_some())
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
        let pinned_portal = portal.into_iter().chain(artwork_replacement).chain(wheat_grant).chain(CATALOG40_WHEAT_DETAIL).any(|(source, producer, manifest)|
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
            .chain(portal).chain(artwork_replacement).chain(wheat_grant).chain(CATALOG40_WHEAT_DETAIL).collect::<Vec<_>>();
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
        let probe_pinned = hayes_session_probe_matches(tuple);
        let probe_declared = self.manifest.files.iter().any(|file| file.path == HAYES_SESSION_PROBE_SCRIPT);
        if probe_declared != probe_pinned {
            return Err(unqualified());
        }
        let hayes_pinned = hayes_validation_host_matches(tuple);
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
        if !cfg!(debug_assertions) || self.manifest.mode != "local-validation"
            || self.manifest.redistribution_qualified || self.manifest.official_signed
            || !regular(WORKSPACE_FURNISHING_CAPABILITY)
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
                if !matches!(catalog, WorkspaceFurnishingCatalog::FramePreparation041 | WorkspaceFurnishingCatalog::HayesOriginal042)
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                41
            } else if file.path.starts_with(HAYES_ORIGINAL_DIRECTORY) {
                let (path, bytes, hash) = HAYES_ORIGINAL_SCHEMA;
                if catalog != WorkspaceFurnishingCatalog::HayesOriginal042
                    || file.path != path || file.bytes != Some(bytes) || file.sha256.as_deref() != Some(hash)
                {
                    return Err(invalid());
                }
                42
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
        if rows.len() != catalog.version() as usize { return Err(invalid()); }
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
            if version != WorkspaceImport::V7 || !workspace_import_files_match(&payload.manifest, version, Some(marker)) {
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
        if CATALOG40_WHEAT_DETAIL != Some(provenance)
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
        let Some(pin) = HAYES_SESSION_PROBE_PIN.as_ref().filter(|pin| pin.host == tuple) else {
            return Ok(None);
        };
        if !self.manifest.files.iter().any(|file| file.path == HAYES_SESSION_PROBE_SCRIPT
            && file.link.is_none() && file.executable == Some(false) && file.bytes == Some(pin.script_bytes)
            && file.sha256.as_deref() == Some(pin.script_sha256))
        {
            return Err(err("HAYES_OPERATION_UNAVAILABLE"));
        }
        Ok(Some(contained(&self.root, HAYES_SESSION_PROBE_SCRIPT)?))
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
