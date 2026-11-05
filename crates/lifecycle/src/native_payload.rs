// SPDX-License-Identifier: Apache-2.0
//! App-contained macOS runtime admission. No installation, process launch or
//! credential read occurs here. UI mode cannot bypass payload verification.
use super::*;
use std::collections::BTreeSet;

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
}
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V5_VALIDATION_PROVENANCE: (&str, &str, &str) = (
    "742412c152037c20a4c32e5ccce8535be46822aa",
    "d0dac02c2b8cc5344a1aef48042d64fdd5f6d61a26851ae6c27cee03bcef7364",
    "40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
const WORKSPACE_V5_CAPABILITY: (&str, u64, &str) = (
    "platform/scripts/native-workspace-material-capability-v5.json",
    700,
    "8c5f7f44fb7b0a898611977064f95d4988eeff4945093c0df351f7b6baaf217a",
);
#[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
fn workspace_v5_validation_matches(manifest: &Manifest, manifest_hash: &str) -> bool {
    let (path, bytes, sha) = WORKSPACE_V5_CAPABILITY;
    manifest.mode == "local-validation" && !manifest.redistribution_qualified && !manifest.official_signed
        && (manifest.source_commit.as_str(), manifest.producer_sha256.as_str(), manifest_hash)
            == WORKSPACE_V5_VALIDATION_PROVENANCE
        && manifest.files.iter().any(|item| {
            item.path == path && item.link.is_none() && item.executable == Some(false)
                && item.bytes == Some(bytes) && item.sha256.as_deref() == Some(sha)
        })
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
        let (source, producer, hash) = WORKSPACE_V5_VALIDATION_PROVENANCE;
        let (path, bytes, sha) = WORKSPACE_V5_CAPABILITY;
        let mut manifest = fixture();
        manifest.source_commit = source.into();
        manifest.producer_sha256 = producer.into();
        assert!(!workspace_v5_validation_matches(&manifest, hash));
        manifest.mode = "local-validation".into();
        manifest.redistribution_qualified = false;
        manifest.official_signed = false;
        assert!(!workspace_v5_validation_matches(&manifest, hash));
        manifest.files.push(Item { path: path.into(), bytes: Some(bytes), sha256: Some(sha.into()), executable: Some(false), link: None });
        assert!(workspace_v5_validation_matches(&manifest, hash));
        assert!(!workspace_v5_validation_matches(&manifest, &"0".repeat(64)));
        manifest.files.last_mut().unwrap().link = Some("other".into());
        assert!(!workspace_v5_validation_matches(&manifest, hash));
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
        let mut payload = Payload { root: PathBuf::new(), manifest, manifest_sha256: hash.into(), workspace_v5_validation: false };
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
            workspace_v5_validation: false,
        })
    }
    /// Available only to the fresh selected validation entry. The normal
    /// payload opener cannot create this state, even in a validation build.
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn open_workspace_v5_validation(root: &Path) -> Result<Self> {
        let mut payload = Self::open(root)?;
        if !workspace_v5_validation_matches(&payload.manifest, &payload.manifest_sha256) {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        }
        payload.workspace_v5_validation = true;
        Ok(payload)
    }
    pub(crate) fn workspace_v5_selected_input(&self) -> bool {
        self.workspace_v5_validation
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
        if !self.normal_oex_028_selected_input()?{return Err(err("NATIVE_OEX_FORMAT_UNAVAILABLE"));}
        Ok((self.node(),self.root.join("platform/apps/api/dist/oex-selected-format.js"),self.root.clone()))
    }
    pub(crate) fn normal_oex_027_selected_input(&self) -> Result<bool> {
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
        let name = "platform/apps/api/dist/oex-owned-chunk-materializer.js";
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    pub(crate) fn oex_preflight(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = "platform/apps/api/dist/oex-owned-preflight.js";
        if !self.manifest.files.iter().any(|f| f.path == name && f.link.is_none() && f.executable == Some(false)) {
            return Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"));
        }
        self.recheck()?;
        Ok((self.node(), self.root.join(name), self.root.clone()))
    }
    /// Fixed codec-only resources, rechecked against this admitted manifest and
    /// held across authentication. No arbitrary module or executable selection.
    pub(crate) fn oex_packed_worker_v2(&self) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let name = "platform/apps/api/dist/oex-owned-packed-worker-v2.js";
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
    pub fn script(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.script)
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
