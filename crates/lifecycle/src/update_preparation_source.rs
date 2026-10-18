// SPDX-License-Identifier: Apache-2.0
//! Native authenticated plan material, never an update execution permit.
use super::installation_backup::source_bytes;
use super::maintenance::VerificationReceipt;
use super::signed_release::trust::{source_deployment, source_images};
use super::*;
const MANIFEST_LIMIT: u64 = 16 * 1024 * 1024;
const CONFIG_LIMIT: u64 = 1024 * 1024;
fn invalid() -> LifecycleError {
    err("UPDATE_PLAN_SOURCE_INVALID")
}
/// No Deserialize or public constructor: only fresh native authentication can
/// produce this material. Existing full qualification remains mandatory.
pub struct AuthenticatedPlanSource {
    verification_id: String,
    backup_id: String,
    manifest_sha256: String,
    inventory_sha256: String,
    schema_sha256: String,
    runtime_image_sha256: String,
    bundle_manifest_sha256: String,
    archive: PathBuf,
}
impl AuthenticatedPlanSource {
    pub fn verification_id(&self) -> &str {
        &self.verification_id
    }
    pub fn backup_id(&self) -> &str {
        &self.backup_id
    }
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    pub fn inventory_sha256(&self) -> &str {
        &self.inventory_sha256
    }
    pub fn schema_sha256(&self) -> &str {
        &self.schema_sha256
    }
    pub fn runtime_image_sha256(&self) -> &str {
        &self.runtime_image_sha256
    }
    pub fn bundle_manifest_sha256(&self) -> &str {
        &self.bundle_manifest_sha256
    }
    pub fn archive(&self) -> &Path {
        &self.archive
    }
}
struct Inputs {
    backup_id: String,
    raw: Vec<u8>,
    manifest: Value,
    bundle_bytes: Vec<u8>,
    image_bytes: Vec<u8>,
    bundle: BundleManifest,
    images: Vec<super::restoration::PreservedImage>,
}
/// Only fixed named configuration records; never interpret their names as paths.
fn configuration(workspace: &Path, manifest: &Value, name: &str) -> Result<Vec<u8>> {
    let files = manifest["files"].as_array().ok_or_else(invalid)?;
    let selected: Vec<_> = files.iter().filter(|f| f["name"] == name).collect();
    if selected.len() != 1 || selected[0]["role"] != "configuration" {
        return Err(invalid());
    }
    let record = selected[0];
    let path = record["path"].as_str().ok_or_else(invalid)?;
    let ascii = path.as_bytes();
    if ascii.len() != 18
        || !ascii.starts_with(b"files/")
        || !ascii.ends_with(b".gcm")
        || !ascii[6..14].iter().all(u8::is_ascii_digit)
    {
        return Err(invalid());
    }
    let length = record["bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= CONFIG_LIMIT)
        .ok_or_else(invalid)?;
    let hash = record["sha256"]
        .as_str()
        .filter(|s| hash_valid(s))
        .ok_or_else(invalid)?;
    // ASCII is validated above, including the string slicing boundary.
    let bytes = source_bytes(
        workspace,
        &format!("plaintext/{}.plain", &path[6..]),
        CONFIG_LIMIT,
        true,
    )?;
    if bytes.len() as u64 != length || digest(&bytes) != hash {
        return Err(invalid());
    }
    Ok(bytes)
}
/// Internal reader. Production supplies the receipt returned by verify_backup in
/// the same call, never a renderer receipt or a deserialized authorization flag.
fn inputs(
    service: &LifecycleService,
    receipt: &VerificationReceipt,
    archive: &Path,
) -> Result<Inputs> {
    if !Uuid::parse_str(&receipt.id).is_ok_and(|id| id.to_string() == receipt.id)
        || receipt.operation != "verified"
        || receipt.files == 0
        || !receipt
            .image
            .strip_prefix("sha256:")
            .is_some_and(hash_valid)
        || !hash_valid(&receipt.authenticated_manifest_sha256)
    {
        return Err(invalid());
    }
    let workspace = service
        .root
        .join(format!("backup-verification-{}", receipt.id));
    let durable: VerificationReceipt =
        serde_json::from_slice(&source_bytes(&workspace, "receipt.json", 4096, true)?)
            .map_err(|_| invalid())?;
    if serde_json::to_value(receipt).map_err(|_| invalid())?
        != serde_json::to_value(durable).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    let source = service.verification_source_unlocked(&receipt.id)?;
    if source.source_path != archive
        || source.image != receipt.image
        || source.authenticated_manifest_sha256 != receipt.authenticated_manifest_sha256
        || source.files != receipt.files
    {
        return Err(invalid());
    }
    let raw = source_bytes(&workspace, "plaintext/manifest.json", MANIFEST_LIMIT, true)?;
    if digest(&raw) != receipt.authenticated_manifest_sha256 {
        return Err(invalid());
    }
    let manifest: Value = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if manifest["id"] != source.backup_id || !manifest["inventory"].is_object() {
        return Err(invalid());
    }
    let bundle_bytes = configuration(&workspace, &manifest, "manager-bundle-manifest.json")?;
    let bundle: BundleManifest = serde_json::from_slice(&bundle_bytes).map_err(|_| invalid())?;
    let image_bytes = configuration(&workspace, &manifest, "manager-image-inventory.json")?;
    let images = source_images::bound_inventory(&image_bytes, &manifest, &bundle)?;
    Ok(Inputs {
        backup_id: source.backup_id,
        raw,
        manifest,
        bundle_bytes,
        image_bytes,
        bundle,
        images,
    })
}
fn plan_source(
    receipt: &VerificationReceipt,
    archive: &Path,
    input: &Inputs,
    platform: &str,
) -> Result<AuthenticatedPlanSource> {
    let selected: Vec<_> = input
        .images
        .iter()
        .filter(|i| i.reference == platform)
        .collect();
    if selected.len() != 1 || !input.bundle.images.iter().any(|i| i.reference == platform) {
        return Err(invalid());
    }
    let runtime = selected[0]
        .content_id
        .strip_prefix("sha256:")
        .filter(|s| hash_valid(s))
        .ok_or_else(invalid)?;
    Ok(AuthenticatedPlanSource {
        verification_id: receipt.id.clone(),
        backup_id: input.backup_id.clone(),
        manifest_sha256: receipt.authenticated_manifest_sha256.clone(),
        inventory_sha256: super::restoration::inventory_fingerprint(&input.manifest["inventory"])?,
        schema_sha256: super::restoration::schema_fingerprint(&input.manifest["inventory"])?,
        runtime_image_sha256: runtime.into(),
        bundle_manifest_sha256: digest(&input.bundle_bytes),
        archive: archive.into(),
    })
}
impl LifecycleService {
    /// Uses the installed trusted verifier, then binds authenticated payloads to the
    /// five original source configuration files and Compose platform image. Does not
    /// restore, stop/start, migrate, register or execute an update.
    pub fn authenticate_update_plan_source(
        &self,
        image: &str,
        key: &Path,
        archive: &Path,
    ) -> Result<AuthenticatedPlanSource> {
        let receipt = self.verify_backup(image, key, archive)?;
        // The verifier owns its lock; reacquire only after successful return.
        let _operation = self.lock()?;
        let before = inputs(self, &receipt, archive)?;
        source_deployment::compare(&self.root, &before.manifest)?;
        let manifest = self.manifest()?;
        let installed: BundleManifest = read_json(&self.root.join("installed.json"))?;
        if serde_json::to_value(installed).map_err(|_| invalid())?
            != serde_json::to_value(&manifest).map_err(|_| invalid())?
        {
            return Err(err("BUNDLE_CHANGED"));
        }
        let engine = self.engine(&manifest, false)?;
        let config: Value = serde_json::from_slice(&run(
            &engine,
            &compose_args(&manifest, &["config", "--format", "json"]),
            Some(&self.root.join("bundle")),
            30,
        )?)
        .map_err(|_| invalid())?;
        let platform = config["services"]["platform"]["image"]
            .as_str()
            .ok_or_else(invalid)?;
        let result = plan_source(&receipt, archive, &before, platform)?;
        // Recheck both archive custody and original source after engine observation.
        let after = inputs(self, &receipt, archive)?;
        source_deployment::compare(&self.root, &after.manifest)?;
        if before.raw != after.raw
            || before.bundle_bytes != after.bundle_bytes
            || before.image_bytes != after.image_bytes
        {
            return Err(invalid());
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture {
        service: LifecycleService,
        archive: PathBuf,
        workspace: PathBuf,
        receipt: VerificationReceipt,
        platform: String,
    }
    fn fixture() -> Fixture {
        // Linkage/parser fixtures are not cryptographic authentication evidence.
        let base = std::env::temp_dir().join(format!("update-plan-source-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let base = fs::canonicalize(base).unwrap();
        let service = LifecycleService::new(base.join("profile")).unwrap();
        let archive = base.join("archive");
        for path in [&archive, &archive.join("files")] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for name in ["complete.json", "writing.json", "manifest.gcm"] {
            write_private_new(&archive, name, b"preserved synthetic cipher", 4096).unwrap();
        }
        let id = Uuid::new_v4().to_string();
        let workspace = service.root.join(format!("backup-verification-{id}"));
        for path in [&workspace, &workspace.join("plaintext")] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let platform = format!("sha256:{}", "a".repeat(64));
        let database = format!("sha256:{}", "b".repeat(64));
        let bundle = serde_json::json!({"schemaVersion":"1","bundleId":"synthetic","version":"1","protocolVersion":"1","composeSha256":"c".repeat(64),"projectName":"exhibitos-synthetic","services":["platform","database"],"images":[{"reference":platform},{"reference":database}],"ports":[12345],"openUrl":"http://127.0.0.1:12345","readinessUrl":"http://127.0.0.1:12345/health","minimumFreeBytes":1});
        let images = serde_json::json!([{"reference":platform,"contentId":platform,"archive":"images/image-0.tar","bytes":1,"sha256":"d".repeat(64)},{"reference":database,"contentId":database,"archive":"images/image-1.tar","bytes":1,"sha256":"e".repeat(64)}]);
        let payloads = [
            serde_json::to_vec(&bundle).unwrap(),
            serde_json::to_vec(&images).unwrap(),
        ];
        let mut files = Vec::new();
        for (index, name) in [
            "manager-bundle-manifest.json",
            "manager-image-inventory.json",
        ]
        .iter()
        .enumerate()
        {
            files.push(serde_json::json!({"role":"configuration","name":name,"path":format!("files/{index:08}.gcm"),"bytes":payloads[index].len(),"sha256":digest(&payloads[index])}));
            write_private_new(
                &workspace.join("plaintext"),
                &format!("{index:08}.gcm.plain"),
                &payloads[index],
                CONFIG_LIMIT as usize,
            )
            .unwrap();
        }
        for index in 0..2 {
            files.push(serde_json::json!({"role":"deployment","name":format!("images/image-{index}.tar"),"path":format!("files/{:08}.gcm",index+2),"bytes":1,"sha256":if index==0{"d".repeat(64)}else{"e".repeat(64)}}));
        }
        for index in 0..files.len() {
            write_private_new(
                &archive,
                &format!("files/{index:08}.gcm"),
                b"preserved synthetic cipher",
                4096,
            )
            .unwrap();
        }
        let raw=serde_json::to_vec(&serde_json::json!({"schemaVersion":"1.0.0-draft.2","kind":"service-backup","id":Uuid::new_v4().to_string(),"inventory":{"schemaDigest":"f".repeat(64),"schemaVersion":"1","migrations":[{"name":"001.sql","sha256":"c".repeat(64)}],"rows":[],"at":"synthetic"},"files":files})).unwrap();
        write_private_new(
            &workspace.join("plaintext"),
            "manifest.json",
            &raw,
            MANIFEST_LIMIT as usize,
        )
        .unwrap();
        let receipt = VerificationReceipt {
            id,
            operation: "verified".into(),
            files: files.len() as u64,
            image: format!("sha256:{}", "f".repeat(64)),
            authenticated_manifest_sha256: digest(&raw),
            at: now(),
        };
        super::super::maintenance_source::SourceSnapshot::capture(&archive)
            .unwrap()
            .preserve(&workspace, &receipt)
            .unwrap();
        write_json(&workspace, "receipt.json", &receipt).unwrap();
        Fixture {
            service,
            archive,
            workspace,
            receipt,
            platform,
        }
    }
    #[test]
    fn authenticated_plan_material_binds_exact_receipt_archive_and_image_role() {
        let f = fixture();
        let input = inputs(&f.service, &f.receipt, &f.archive).unwrap();
        let result = plan_source(&f.receipt, &f.archive, &input, &f.platform).unwrap();
        assert_eq!(result.backup_id(), input.backup_id);
        assert_eq!(result.manifest_sha256(), digest(&input.raw));
        assert_eq!(result.bundle_manifest_sha256(), digest(&input.bundle_bytes));
        assert_eq!(result.runtime_image_sha256(), "a".repeat(64));
        assert_eq!(result.archive(), f.archive);
        assert!(plan_source(&f.receipt, &f.archive, &input, "unknown").is_err());
        assert_eq!(
            plan_source(
                &f.receipt,
                &f.archive,
                &input,
                &format!("sha256:{}", "b".repeat(64))
            )
            .unwrap()
            .runtime_image_sha256(),
            "b".repeat(64)
        );
    }
    #[test]
    fn changed_manifest_receipt_path_or_cipher_fail_preserving_originals() {
        for mode in ["manifest", "receipt", "path", "cipher"] {
            let mut f = fixture();
            let original = fs::read(f.archive.join("manifest.gcm")).unwrap();
            let original_mode = fs::metadata(f.archive.join("manifest.gcm"))
                .unwrap()
                .permissions()
                .mode();
            match mode {
                "manifest" => fs::write(
                    f.workspace.join("plaintext/manifest.json"),
                    b"changed manifest",
                )
                .unwrap(),
                "receipt" => f.receipt.at += 1,
                "cipher" => fs::write(
                    f.archive.join("files/00000000.gcm"),
                    b"changed cipher fixture",
                )
                .unwrap(),
                _ => {}
            }
            let archive = if mode == "path" {
                f.archive.with_extension("other")
            } else {
                f.archive.clone()
            };
            assert!(inputs(&f.service, &f.receipt, &archive).is_err(), "{mode}");
            assert_eq!(fs::read(f.archive.join("manifest.gcm")).unwrap(), original);
            assert_eq!(
                fs::metadata(f.archive.join("manifest.gcm"))
                    .unwrap()
                    .permissions()
                    .mode(),
                original_mode
            );
            assert!(f.workspace.join("receipt.json").is_file());
        }
    }
    #[test]
    fn payload_mutation_and_symlink_cannot_use_verified_receipt() {
        for linked in [false, true] {
            let f = fixture();
            let path = f.workspace.join("plaintext/00000000.gcm.plain");
            if linked {
                fs::rename(&path, path.with_extension("retained")).unwrap();
                symlink(path.with_extension("retained"), &path).unwrap();
            } else {
                fs::write(&path, b"{}").unwrap();
            }
            assert!(inputs(&f.service, &f.receipt, &f.archive).is_err());
        }
    }
    #[test]
    fn public_entry_cannot_promote_a_fixture_receipt_when_verifier_fails() {
        let f = fixture();
        let original = fs::read(f.archive.join("manifest.gcm")).unwrap();
        let error = match f.service.authenticate_update_plan_source(
            "not-an-immutable-image",
            &f.archive.with_extension("missing-key"),
            &f.archive,
        ) {
            Ok(_) => panic!("failed authentication produced plan material"),
            Err(error) => error,
        };
        assert_eq!(error.code, "BACKUP_IMAGE_INVALID");
        assert_eq!(fs::read(f.archive.join("manifest.gcm")).unwrap(), original);
        assert!(f.workspace.join("receipt.json").is_file());
    }
}
