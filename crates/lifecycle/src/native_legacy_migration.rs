// SPDX-License-Identifier: Apache-2.0
//! Preparation boundaries for authenticated legacy archives. Only the fixed
//! codec child can issue source authority; no profile writer or UI is exposed.
//! In particular, a historical verification
//! receipt is NOT authority to authenticate another key or create a native owner.
use crate::installations::native_backup::Secret;
use crate::{
    BundleManifest, LifecycleError, Result, err, hash_valid, installation_backup,
    native_installation::Ports, native_payload::Payload, publication_source,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};
#[path = "native_legacy_codec.rs"]
mod codec;
pub(crate) use codec::activation_resources;
pub(crate) use codec::authenticate;
pub(crate) use codec::inspection_resources;
pub(crate) use codec::schema_resources;
#[path = "native_legacy_stage.rs"]
mod stage;
pub(crate) use stage::Candidate;

const CONFIGURATION: [&str; 7] = [
    "manager-bundle-manifest.json",
    "manager-compose.yaml",
    "manager-runtime.env",
    "manager-installed.json",
    "manager-engine.json",
    "manager-image-inventory.json",
    "freeze-signing-key.json",
];
fn invalid() -> LifecycleError {
    err("NATIVE_LEGACY_CONFIGURATION_INVALID")
}

/// Only the fixed existing-codec executor may construct these private
/// fields, AFTER real archive/key authentication. There is deliberately no
/// public/deserializable constructor, receipt conversion, or test-issued permit.
struct CodecEvidence {
    archive: crate::maintenance_source::SourceSnapshot,
    key: crate::hosted_runtime_file::Pin,
    key_path: PathBuf,
    plaintext_manifest: crate::hosted_runtime_file::Pin,
    configuration: Vec<crate::hosted_runtime_file::Pin>,
    archive_root: PathBuf,
    original_root: Option<PathBuf>,
    backup_id: String,
    authenticated_manifest_sha256: String,
    archive_manifest_sha256: String,
    archive_manifest: crate::hosted_runtime_file::Pin,
    quarantine_parent: crate::maintenance_source::DirectoryPin,
    quarantine: crate::maintenance_source::DirectoryPin,
    verified_directory: crate::maintenance_source::DirectoryPin,
    quarantine_root: PathBuf,
}
impl CodecEvidence {
    fn check(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.backup_id).is_err()
            || !hash_valid(&self.authenticated_manifest_sha256)
            || !hash_valid(&self.archive_manifest_sha256)
            || self.configuration.len() != CONFIGURATION.len()
        {
            return Err(invalid());
        }
        self.archive.check()?;
        self.archive_manifest.check()?;
        self.key.check()?;
        self.plaintext_manifest.check()?;
        for pin in &self.configuration {
            pin.check()?;
        }
        self.quarantine_parent.check()?;
        self.quarantine.check()?;
        self.verified_directory.check()?;
        crate::installations::private_directory(&self.quarantine_root)?;
        crate::installations::private_directory(&self.quarantine_root.join("verified"))?;
        Ok(())
    }
}

// Metadata consistency only: this parser cannot issue AuthenticatedLegacyConfig.
// The existing Platform codec remains the sole archive/crypto authority.
struct ParsedConfiguration {
    manifest: BundleManifest,
    backup_id: String,
    manifest_sha256: String,
    declared_source_url: String,
    original_origin: String,
    environment: Secret,
    credentials: publication_source::LocalPublicationCredentials,
    freeze_signing_key: Secret,
    configuration_sha256: BTreeMap<String, String>,
    images: Vec<crate::restoration::PreservedImage>,
    signing_record: crate::source_configuration::NativeConfigurationProof,
}
fn parse_configuration(
    authenticated_manifest_bytes: &[u8],
    files: &BTreeMap<String, Secret>,
) -> Result<ParsedConfiguration> {
    if authenticated_manifest_bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    let authenticated_manifest: serde_json::Value =
        serde_json::from_slice(authenticated_manifest_bytes).map_err(|_| invalid())?;
    let expected: BTreeSet<&str> = CONFIGURATION.into_iter().collect();
    if files.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected
        || authenticated_manifest["kind"] != "service-backup"
        || !matches!(
            authenticated_manifest["schemaVersion"].as_str(),
            Some("1.0.0-draft.1" | "1.0.0-draft.2")
        )
    {
        return Err(invalid());
    }
    let backup_id = authenticated_manifest["id"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == *id))
        .ok_or_else(invalid)?
        .to_string();
    let identity = &authenticated_manifest["sourceIdentity"];
    if identity["database"] != "exhibitos"
        || !["systemIdentifier", "databaseOid"].iter().all(|name| {
            identity[*name].as_str().is_some_and(|v| {
                !v.is_empty()
                    && v.len() <= 20
                    && v.bytes().all(|b| b.is_ascii_digit())
                    && v.parse::<u64>().is_ok_and(|n| n > 0)
            })
        })
    {
        return Err(invalid());
    }
    let entries = authenticated_manifest["files"]
        .as_array()
        .ok_or_else(invalid)?;
    let mut seen = BTreeSet::new();
    let mut hashes = BTreeMap::new();
    for entry in entries.iter().filter(|f| f["role"] == "configuration") {
        let name = entry["name"]
            .as_str()
            .filter(|n| expected.contains(n))
            .ok_or_else(invalid)?;
        if !seen.insert(name) {
            return Err(invalid());
        }
        let bytes = &files.get(name).ok_or_else(invalid)?.0;
        let limit = if name == "manager-compose.yaml" {
            256 * 1024
        } else if name == "manager-runtime.env" {
            8192
        } else {
            1024 * 1024
        };
        if bytes.is_empty()
            || bytes.len() > limit
            || entry["bytes"].as_u64() != Some(bytes.len() as u64)
            || entry["sha256"].as_str() != Some(crate::digest(bytes).as_str())
        {
            return Err(invalid());
        }
        hashes.insert(name.to_owned(), crate::digest(bytes));
    }
    if seen != expected {
        return Err(invalid());
    }
    let read = |name: &str| -> Result<&[u8]> { Ok(&files.get(name).ok_or_else(invalid)?.0) };
    let manifest: BundleManifest =
        serde_json::from_slice(read("manager-bundle-manifest.json")?).map_err(|_| invalid())?;
    let installed: BundleManifest =
        serde_json::from_slice(read("manager-installed.json")?).map_err(|_| invalid())?;
    let engine: String =
        serde_json::from_slice(read("manager-engine.json")?).map_err(|_| invalid())?;
    // Same pure declaration admission as the existing legacy restoration path.
    // Archived compose/images are retained evidence; never executed by native.
    if serde_json::to_value(&manifest).ok() != serde_json::to_value(&installed).ok()
        || manifest.schema_version != "1.0.0-draft.1"
        || manifest.protocol_version != "1"
        || manifest.services.len() != 2
        || !manifest.services.iter().any(|s| s == "database")
        || !manifest.services.iter().any(|s| s == "platform")
        || engine != "docker"
        || manifest
            .preferred_engine
            .as_deref()
            .is_some_and(|s| s != "docker")
        || !hash_valid(&manifest.compose_sha256)
        || crate::digest(read("manager-compose.yaml")?) != manifest.compose_sha256
        || manifest.ports.is_empty()
        || manifest.ports.iter().any(|p| *p < 1024)
        || manifest.ports.iter().collect::<BTreeSet<_>>().len() != manifest.ports.len()
    {
        return Err(invalid());
    }
    let images =
        crate::restoration::preserved_images(read("manager-image-inventory.json")?, &manifest)?;
    let signing_record = crate::source_configuration::expected(authenticated_manifest_bytes)?;
    let environment = read("manager-runtime.env")?;
    installation_backup::environment_valid(environment, &manifest)?;
    // Preserve the authenticated declared address verbatim. A port or hostname
    // rewrite can invalidate existing freeze signatures and offline grants.
    let declared_source_url = manifest.open_url.clone();
    if declared_source_url.is_empty()
        || declared_source_url.len() > 2048
        || declared_source_url.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    // The packaged legacy profile binds AUTH_ORIGIN to this declared loopback
    // port. Keep its original open URL too; a path is not part of the authority.
    // Other addresses remain verbatim and require a later authenticated route.
    let original_origin = crate::loopback_url(&declared_source_url, &manifest.ports)
        .map(|(address, _)| format!("http://{address}"))
        .unwrap_or_else(|_| declared_source_url.clone());
    let credentials = publication_source::parse(environment, original_origin.clone())?;
    Ok(ParsedConfiguration {
        manifest,
        backup_id,
        manifest_sha256: crate::digest(authenticated_manifest_bytes),
        declared_source_url,
        original_origin,
        environment: Secret(environment.to_vec()),
        credentials,
        freeze_signing_key: Secret(read("freeze-signing-key.json")?.to_vec()),
        configuration_sha256: hashes,
        images,
        signing_record,
    })
}

/// An authenticated source cannot be manufactured by deserializing a receipt
/// or by supplying a tenant/admin/password through an IPC request. Only the codec
/// child may issue it after full real verification and held-file checks.
pub(crate) struct AuthenticatedLegacyConfig {
    configuration: ParsedConfiguration,
    evidence: CodecEvidence,
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationSummary {
    source_backup_id: String,
    source_manifest_sha256: String,
    original_origin: String,
    declared_source_url: String,
    tenant_id: String,
    admin_subject: String,
    owner_kind: String,
    target: PathBuf,
    target_payload_sha256: String,
    ports: Ports,
    requires_original_writer_handoff: bool,
    requires_database_object_and_freeze_verification: bool,
}
/// Not serializable/deserializable and not an activation permit. This owns the
/// same held source/key/config guards for the later durable stage to recheck.
pub(crate) struct VerifiedMigrationPlan {
    source: AuthenticatedLegacyConfig,
    summary: MigrationSummary,
    target_environment: Secret,
}
fn target_location_requirements(
    configuration: &ParsedConfiguration,
    archive_root: &Path,
    original_root: Option<&Path>,
    target: &Path,
    ports: Ports,
) -> Result<()> {
    ports.validate()?;
    if configuration.original_origin
        != format!("http://127.0.0.1:{}", configuration.manifest.ports[0])
        || ports.web != configuration.manifest.ports[0]
    {
        // HTTPS/public origins are a real supported broader source contract.
        // They require client-visible origin/TLS routing, not silent downgrading
        // or reissuing old grants. The full migration goal remains open for them.
        return Err(err("NATIVE_LEGACY_ORIGIN_ROUTE_REQUIRED"));
    }
    if !target.is_absolute()
        || target
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        || target.starts_with(archive_root)
        || archive_root.starts_with(target)
        || original_root.is_some_and(|root| target.starts_with(root) || root.starts_with(target))
    {
        return Err(err("NATIVE_LEGACY_NEW_TARGET_REQUIRED"));
    }
    let parent = target.parent().ok_or_else(invalid)?;
    if fs::canonicalize(parent).map_err(|_| invalid())? != parent {
        return Err(invalid());
    }
    crate::installations::private_directory(parent)?;
    Ok(())
}
fn target_requirements(
    configuration: &ParsedConfiguration,
    archive_root: &Path,
    original_root: Option<&Path>,
    target: &Path,
    ports: Ports,
) -> Result<()> {
    target_location_requirements(configuration, archive_root, original_root, target, ports)?;
    match fs::symlink_metadata(target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(err("NATIVE_LEGACY_NEW_TARGET_REQUIRED")),
    }
}
impl AuthenticatedLegacyConfig {
    pub(crate) fn into_plan(
        self,
        target: PathBuf,
        payload: &Payload,
        ports: Ports,
    ) -> Result<VerifiedMigrationPlan> {
        self.evidence.check()?;
        if self.evidence.backup_id != self.configuration.backup_id
            || self.evidence.authenticated_manifest_sha256 != self.configuration.manifest_sha256
        {
            return Err(invalid());
        }
        target_requirements(
            &self.configuration,
            &self.evidence.archive_root,
            self.evidence.original_root.as_deref(),
            &target,
            ports,
        )?;
        let manifest_sha256 = payload.context().manifest_sha256;
        if !hash_valid(&manifest_sha256) {
            return Err(invalid());
        }
        // No tenant, owner or account secret comes from the caller. Only fresh
        // target DB credentials change; the authenticated original account stays.
        let database_password = Secret(
            format!(
                "{}{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
            .into_bytes(),
        );
        let password = std::str::from_utf8(&database_password.0).map_err(|_| invalid())?;
        let original_password = std::str::from_utf8(self.configuration.credentials.password())
            .map_err(|_| invalid())?;
        let environment = Secret(format!("EXHIBITOS_RUNTIME_MODE=native\nNATIVE_RUNTIME_MANIFEST_SHA256={manifest_sha256}\nEXHIBITOS_PORT={}\nEXHIBITOS_API_PORT={}\nPOSTGRES_PASSWORD={password}\nDATABASE_URL=postgresql://exhibitos:{password}@127.0.0.1:{}/exhibitos\nADMIN_SUBJECT={}\nADMIN_PASSWORD={}\nTENANT_ID={}\n", ports.web, ports.api, ports.database, self.configuration.credentials.subject, crate::initial_account::encode_password(original_password), self.configuration.credentials.tenant_id).into_bytes());
        let summary = MigrationSummary {
            source_backup_id: self.configuration.backup_id.clone(),
            source_manifest_sha256: self.evidence.authenticated_manifest_sha256.clone(),
            original_origin: self.configuration.original_origin.clone(),
            declared_source_url: self.configuration.declared_source_url.clone(),
            tenant_id: self.configuration.credentials.tenant_id.clone(),
            admin_subject: self.configuration.credentials.subject.clone(),
            owner_kind: "legacy_account".into(),
            target,
            target_payload_sha256: manifest_sha256,
            ports,
            requires_original_writer_handoff: true,
            requires_database_object_and_freeze_verification: true,
        };
        self.evidence.check()?;
        Ok(VerifiedMigrationPlan {
            source: self,
            summary,
            target_environment: environment,
        })
    }
}
impl VerifiedMigrationPlan {
    pub(crate) fn summary(&self) -> &MigrationSummary {
        &self.summary
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        self.source.evidence.check()?;
        target_requirements(
            &self.source.configuration,
            &self.source.evidence.archive_root,
            self.source.evidence.original_root.as_deref(),
            &self.summary.target,
            self.summary.ports,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn parse(
        manifest: &serde_json::Value,
        files: &BTreeMap<String, Secret>,
    ) -> Result<ParsedConfiguration> {
        parse_configuration(&serde_json::to_vec(manifest).unwrap(), files)
    }
    fn index(manifest: &mut serde_json::Value, files: &BTreeMap<String, Secret>) {
        manifest["files"] = serde_json::Value::Array(files.iter().map(|(name, bytes)| serde_json::json!({"role":"configuration","name":name,"bytes":bytes.0.len(),"sha256":crate::digest(&bytes.0)})).collect());
    }
    fn fixture() -> (serde_json::Value, BTreeMap<String, Secret>) {
        let compose = b"synthetic legacy compose evidence; never executed\n".to_vec();
        let bundle = serde_json::json!({"schemaVersion":"1.0.0-draft.1","bundleId":"synthetic-legacy","version":"0.1.0","protocolVersion":"1","composeSha256":crate::digest(&compose),"preferredEngine":"docker","projectName":"exhibitos-synthetic","services":["database","platform"],"images":[{"reference":format!("sha256:{}","a".repeat(64))},{"reference":format!("sha256:{}","b".repeat(64))}],"ports":[13200],"openUrl":"http://127.0.0.1:13200","readinessUrl":"http://127.0.0.1:13200/api/v1/readiness","minimumFreeBytes":1});
        let images: Vec<_> = ["a", "b"].iter().enumerate().map(|(i, hex)| serde_json::json!({"reference":format!("sha256:{}",hex.repeat(64)),"contentId":format!("sha256:{}",hex.repeat(64)),"archive":format!("images/image-{i}.tar"),"bytes":1024,"sha256":"c".repeat(64)})).collect();
        let environment = format!(
            "EXHIBITOS_PORT=13200\nPOSTGRES_PASSWORD=synthetic_database_password\nDATABASE_URL=postgresql://exhibitos:synthetic_database_password@database:5432/exhibitos\nADMIN_SUBJECT=original.admin\nADMIN_PASSWORD=synthetic_admin_password_123\nTENANT_ID={}\n",
            uuid::Uuid::new_v4()
        );
        let mut files = BTreeMap::new();
        for (name, bytes) in [
            (
                "manager-bundle-manifest.json",
                serde_json::to_vec(&bundle).unwrap(),
            ),
            (
                "manager-installed.json",
                serde_json::to_vec(&bundle).unwrap(),
            ),
            ("manager-compose.yaml", compose),
            ("manager-runtime.env", environment.into_bytes()),
            ("manager-engine.json", b"\"docker\"".to_vec()),
            (
                "manager-image-inventory.json",
                serde_json::to_vec(&images).unwrap(),
            ),
            // Metadata-only fixture: no key authentication or crypto permit is issued.
            ("freeze-signing-key.json", b"{\"synthetic\":true}".to_vec()),
        ] {
            files.insert(name.into(), Secret(bytes));
        }
        let mut manifest = serde_json::json!({"schemaVersion":"1.0.0-draft.1","kind":"service-backup","id":uuid::Uuid::new_v4().to_string(),"sourceIdentity":{"systemIdentifier":"123456789","databaseOid":"16384","database":"exhibitos"}});
        index(&mut manifest, &files);
        (manifest, files)
    }
    #[test]
    fn metadata_parser_preserves_original_account_and_configuration_without_issuing_authority() {
        let (manifest, files) = fixture();
        let parsed = parse(&manifest, &files).unwrap();
        assert_eq!(parsed.credentials.subject, "original.admin");
        assert_eq!(
            parsed.credentials.password(),
            b"synthetic_admin_password_123"
        );
        assert_eq!(parsed.original_origin, "http://127.0.0.1:13200");
        assert_eq!(parsed.environment.0, files["manager-runtime.env"].0);
        assert_eq!(
            parsed.freeze_signing_key.0,
            files["freeze-signing-key.json"].0
        );
        assert_eq!(parsed.configuration_sha256.len(), 7);
        // ParsedConfiguration is not AuthenticatedLegacyConfig. No issuer or
        // native owner can be obtained from these unverified synthetic bytes.
    }
    #[test]
    fn plaintext_configuration_requires_exact_manifest_members_and_hashes() {
        let (mut manifest, mut files) = fixture();
        files.get_mut("manager-runtime.env").unwrap().0.push(b' ');
        assert!(parse(&manifest, &files).is_err());
        let (valid, files) = fixture();
        manifest = valid;
        let duplicate = manifest["files"][0].clone();
        manifest["files"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&manifest, &files).is_err());
        let receipt = serde_json::json!({"operation":"verified","id":uuid::Uuid::new_v4().to_string(),"authenticatedManifestSha256":"a".repeat(64),"files":7});
        assert!(parse(&receipt, &files).is_err());
    }
    #[test]
    fn conflicting_layout_source_identity_and_owner_input_are_rejected() {
        let (mut manifest, mut files) = fixture();
        let mut installed: serde_json::Value =
            serde_json::from_slice(&files["manager-installed.json"].0).unwrap();
        installed["projectName"] = "other-source".into();
        files.insert(
            "manager-installed.json".into(),
            Secret(serde_json::to_vec(&installed).unwrap()),
        );
        index(&mut manifest, &files);
        assert!(parse(&manifest, &files).is_err());
        let (mut manifest, files) = fixture();
        manifest["sourceIdentity"]["database"] = "other-source".into();
        assert!(parse(&manifest, &files).is_err());
        let (mut manifest, mut files) = fixture();
        files
            .get_mut("manager-runtime.env")
            .unwrap()
            .0
            .extend_from_slice(b"TENANT_ID=caller-supplied\n");
        index(&mut manifest, &files);
        assert!(parse(&manifest, &files).is_err());
    }
    #[test]
    fn target_must_be_new_and_original_authority_requires_explicit_route() {
        let root =
            std::env::temp_dir().join(format!("native-legacy-plan-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let (manifest, files) = fixture();
        let mut parsed = parse(&manifest, &files).unwrap();
        let ports = Ports {
            web: 13200,
            api: 13201,
            database: 15432,
        };
        let archive = root.join("archive");
        let original = root.join("original");
        let target = root.join("new-native");
        assert!(target_requirements(&parsed, &archive, Some(&original), &target, ports).is_ok());
        fs::create_dir(&target).unwrap();
        assert_eq!(
            target_requirements(&parsed, &archive, Some(&original), &target, ports)
                .unwrap_err()
                .code,
            "NATIVE_LEGACY_NEW_TARGET_REQUIRED"
        );
        assert!(
            target_requirements(
                &parsed,
                &archive,
                Some(&original),
                &original.join("nested"),
                ports
            )
            .is_err()
        );
        assert!(
            target_requirements(
                &parsed,
                &archive,
                Some(&original),
                &root.join("different-port"),
                Ports {
                    web: 13202,
                    ..ports
                }
            )
            .is_err()
        );
        parsed.original_origin = "https://existing.example".into();
        parsed.declared_source_url = parsed.original_origin.clone();
        assert_eq!(
            target_requirements(
                &parsed,
                &archive,
                Some(&original),
                &root.join("https-target"),
                ports
            )
            .unwrap_err()
            .code,
            "NATIVE_LEGACY_ORIGIN_ROUTE_REQUIRED"
        );
        assert_eq!(parsed.original_origin, "https://existing.example");
        fs::remove_dir_all(root).unwrap();
    }
}
