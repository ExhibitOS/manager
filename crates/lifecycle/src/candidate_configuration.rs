// SPDX-License-Identifier: Apache-2.0
//! Fresh complete supported remapped candidate configuration and original image bytes.
use super::*;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateConfigurationReceipt {
    pub source_instance: String,
    pub candidate_instance: String,
    pub candidate_bundle_id: String,
    pub authenticated_manifest_sha256: String,
    pub configuration_volume: String,
    pub files: Vec<source_deployment::DeploymentFileProof>,
    pub freeze_key: source_configuration::NativeConfigurationProof,
    pub images: Vec<source_image_bytes::ImageArchiveProof>,
    pub export_workspace: String,
    pub image_archives_retained: bool,
    pub observed_at: u64,
    pub candidate_data_inventory_verified: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
struct PrivateBytes(Vec<u8>);
impl Drop for PrivateBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
impl std::ops::Deref for PrivateBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}
fn candidate_files(
    target: &Path,
    expected: &crate::BundleManifest,
    compose: &[u8],
    environment: &[u8],
) -> crate::Result<Vec<source_deployment::DeploymentFileProof>> {
    let manifest = serde_json::to_vec(expected).map_err(|_| crate::err("STATE_INVALID"))?;
    let bytes: [&[u8]; 5] = [&manifest, compose, environment, &manifest, b"\"docker\""];
    let mut proofs = Vec::new();
    for ((_, path, limit), wanted) in source_deployment::FILES.iter().zip(bytes) {
        let mut actual = crate::installation_backup::source_bytes(target, path, *limit, true)?;
        let matches = actual == wanted;
        let proof = source_deployment::DeploymentFileProof {
            path: (*path).into(),
            bytes: actual.len() as u64,
            sha256: crate::digest(&actual),
        };
        actual.fill(0);
        if !matches {
            return Err(crate::err("UPDATE_SOURCE_CONFIGURATION_MISMATCH"));
        }
        proofs.push(proof);
    }
    Ok(proofs)
}
impl ExecutionSession<'_> {
    /// No activation/preflight, no caller success flags. Success retires this new export only.
    pub fn verify_restored_candidate_configuration(
        &self,
        image: &str,
        export_parent: &Path,
        acknowledged: bool,
    ) -> crate::Result<CandidateConfigurationReceipt> {
        let mut result = self.inspect_restored_candidate(acknowledged, |ctx| {
            self.observe_candidate_configuration_at(ctx, image, export_parent)
        })?;
        source_image_bytes::retire_verified(Path::new(&result.export_workspace), &result.images)?;
        result.image_archives_retained = false;
        Ok(result)
    }
    pub(super) fn observe_candidate_configuration_at(
        &self,
        ctx: &candidate_inventory::CandidateContext<'_>,
        image: &str,
        export_parent: &Path,
    ) -> crate::Result<CandidateConfigurationReceipt> {
        self.validate_candidate_export_parent(export_parent)?;
        recovery_space::check(&self.source, ctx, &[export_parent], 1)?;
        let authenticated = source_full_configuration::scope(ctx.raw)?;
        // Every original host configuration file is checked against authenticated backup.
        let source_files = source_deployment::compare(&self.source.root, &authenticated)?;
        let original = self.source.manifest()?;
        let inventory = crate::installation_backup::source_bytes(
            &ctx.workspace.join("configuration"),
            "manager-image-inventory.json",
            1048576,
            true,
        )?;
        let images = source_images::bound_inventory(&inventory, &authenticated, &original)?;
        let source_mappings = source_images::observe(&self.source, ctx.workspace, ctx.raw)?;
        let config: crate::Value = serde_json::from_slice(&crate::run(
            "docker",
            &crate::compose_args(&original, &["config", "--format", "json"]),
            Some(&self.source.root.join("bundle")),
            30,
        )?)
        .map_err(|_| crate::err("ENGINE_OUTPUT_INVALID"))?;
        let image_for = |service: &str| -> crate::Result<String> {
            let reference = config["services"][service]["image"]
                .as_str()
                .ok_or_else(|| crate::err("UPDATE_SOURCE_IMAGES_INVALID"))?;
            let selected: Vec<_> = images
                .iter()
                .filter(|i| {
                    i.reference == reference || crate::same_registry_pin(&i.reference, reference)
                })
                .collect();
            if selected.len() != 1 {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_INVALID"));
            }
            Ok(selected[0].content_id.clone())
        };
        let database = image_for("database")?;
        let platform = image_for("platform")?;
        if platform.strip_prefix("sha256:") != Some(ctx.plan.source_image.as_str())
            || ctx.manifest.ports.len() != 1
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        let candidate_compose=crate::installation_backup::source_bytes(ctx.root,"bundle/compose.yaml",1048576,true)?;
        let subnet=crate::restoration_network::receipt_compose_subnet(&candidate_compose,ctx.receipt.network_subnet.as_deref())?;
        let (expected, compose) = crate::restoration::remapped_bundle(
            &original,
            &images,
            &ctx.manifest.bundle_id,
            ctx.manifest.ports[0],
            &database,
            &platform,
            subnet.as_deref(),
        )?;
        let (expected, compose) = if crate::restoration_network::compose_gateway(&candidate_compose)?.is_some() {
            crate::restoration::bind_explicit_gateway((expected, compose), subnet.as_deref().ok_or_else(|| crate::err("RESTORE_LAYOUT_UNSUPPORTED"))?)?
        } else { (expected, compose) };
        if let Some(subnet)=subnet.as_deref() {
            let network=crate::backup_creation::inspected("docker",&["network".into(),"inspect".into(),format!("{}_default",expected.project_name)])?;
            if network["Labels"]["com.exhibitos.bundle"]!=expected.bundle_id || network["Labels"]["com.exhibitos.project"]!=expected.project_name || network["Driver"]!="bridge" {return Err(crate::err("OWNERSHIP_CONFLICT"));}
            crate::restoration_network::verify_observed(&network,subnet)?;
        }
        let original_environment = PrivateBytes(crate::installation_backup::source_bytes(
            &self.source.root,
            "runtime.env",
            8192,
            true,
        )?);
        let derived = crate::restoration::remapped_environment(
            &original_environment,
            &original,
            expected.ports[0],
        );
        drop(original_environment);
        let environment = PrivateBytes(derived?);
        let initial = candidate_files(ctx.root, &expected, &compose, &environment);
        let files = initial?;
        let key_expected = source_configuration::expected(ctx.raw)?;
        let observed_key = source_configuration::observe(
            image,
            &ctx.candidate_before.configuration_volume,
            &key_expected,
        )?;
        // The exact generated Compose binds both candidate service containers to image IDs.
        for (service, expected_id) in [("database", &database), ("platform", &platform)] {
            if crate::backup_creation::local_image("docker", expected_id)? != *expected_id {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_MISMATCH"));
            }
            let (_, actual) =
                crate::backup_creation::one_container(ctx.target, &expected, "docker", service)?;
            if actual["Image"] != *expected_id {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_MISMATCH"));
            }
        }
        let exported = export_parent.join(format!(
            "candidate-image-observation-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&exported).map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exported, fs::Permissions::from_mode(0o700))
                .map_err(|_| crate::err("STATE_UNAVAILABLE"))?;
        }
        installations::private_directory(&exported)?;
        let fresh_images = source_image_bytes::export(&exported, &images)?;
        let repeated = candidate_files(ctx.root, &expected, &compose, &environment);
        drop(environment);
        if repeated? != files
            || source_deployment::compare(&self.source.root, &authenticated)? != source_files
            || source_configuration::observe(
                image,
                &ctx.candidate_before.configuration_volume,
                &key_expected,
            )? != observed_key
            || source_images::observe(&self.source, ctx.workspace, ctx.raw)? != source_mappings
            || ctx.source_before.runtime_image_sha256 != ctx.candidate_before.runtime_image_sha256
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        for (service, expected_id) in [("database", &database), ("platform", &platform)] {
            let (_, actual) =
                crate::backup_creation::one_container(ctx.target, &expected, "docker", service)?;
            if actual["Image"] != *expected_id {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_MISMATCH"));
            }
        }
        Ok(CandidateConfigurationReceipt {
            source_instance: ctx.plan.source_instance.clone(),
            candidate_instance: ctx.plan.target_instance.clone(),
            candidate_bundle_id: expected.bundle_id,
            authenticated_manifest_sha256: ctx.receipt.authenticated_manifest_sha256.clone(),
            configuration_volume: ctx.candidate_before.configuration_volume.clone(),
            files,
            freeze_key: observed_key,
            images: fresh_images,
            export_workspace: exported.to_string_lossy().into_owned(),
            image_archives_retained: true,
            observed_at: crate::now(),
            candidate_data_inventory_verified: false,
            preflight_verified: false,
            update_executed: false,
        })
    }

    pub(super) fn validate_candidate_export_parent(
        &self,
        export_parent: &Path,
    ) -> crate::Result<()> {
        if export_parent.starts_with(&self.store.profile)
            || export_parent.starts_with(&self.store.root)
        {
            return Err(crate::err("UPDATE_ARTIFACT_UNAVAILABLE"));
        }
        installations::private_directory(export_parent)?;
        if fs::canonicalize(export_parent).ok().as_deref() != Some(export_parent) {
            return Err(crate::err("UPDATE_ARTIFACT_UNAVAILABLE"));
        }
        Ok(())
    }
    pub(super) fn recheck_candidate_configuration_at(
        &self,
        ctx: &candidate_inventory::CandidateContext<'_>,
        image: &str,
        receipt: &CandidateConfigurationReceipt,
    ) -> crate::Result<()> {
        if receipt.files.len() != source_deployment::FILES.len()
            || receipt.source_instance != ctx.plan.source_instance
            || receipt.candidate_instance != ctx.plan.target_instance
            || receipt.candidate_bundle_id != ctx.manifest.bundle_id
            || receipt.authenticated_manifest_sha256 != ctx.plan.backup_manifest
            || receipt.configuration_volume != ctx.candidate_before.configuration_volume
            || receipt.preflight_verified
            || receipt.update_executed
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        for ((_, path, limit), file) in source_deployment::FILES.iter().zip(&receipt.files) {
            let mut current =
                crate::installation_backup::source_bytes(ctx.root, path, *limit, true)?;
            let matched = file.path == *path
                && current.len() as u64 == file.bytes
                && crate::digest(&current) == file.sha256;
            current.fill(0);
            if !matched {
                return Err(crate::err("UPDATE_TARGET_CHANGED"));
            }
        }
        let authenticated = source_full_configuration::scope(ctx.raw)?;
        source_deployment::compare(&self.source.root, &authenticated)?;
        source_images::observe(&self.source, ctx.workspace, ctx.raw)?;
        if source_configuration::observe(
            image,
            &ctx.candidate_before.configuration_volume,
            &source_configuration::expected(ctx.raw)?,
        )? != receipt.freeze_key
        {
            return Err(crate::err("UPDATE_TARGET_CHANGED"));
        }
        for proof in &receipt.images {
            if crate::backup_creation::local_image("docker", &proof.reference)? != proof.content_id
            {
                return Err(crate::err("UPDATE_SOURCE_IMAGES_MISMATCH"));
            }
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn candidate_host_bytes_are_exact_and_no_secret_values_enter_receipts() {
        let root = std::env::temp_dir().join(format!(
            "exhibitos-candidate-config-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.join("bundle")).unwrap();
        fs::set_permissions(root.join("bundle"), fs::Permissions::from_mode(0o700)).unwrap();
        let m = crate::tests::manifest_for_detection();
        let manifest = serde_json::to_vec(&m).unwrap();
        let compose = b"synthetic-compose";
        let environment = b"synthetic-private-password";
        let bytes: [&[u8]; 5] = [&manifest, compose, environment, &manifest, b"\"docker\""];
        for ((_, path, _), data) in source_deployment::FILES.iter().zip(bytes) {
            fs::write(root.join(path), data).unwrap();
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let proof = candidate_files(&root, &m, compose, environment).unwrap();
        assert_eq!(proof.len(), 5);
        assert!(
            !serde_json::to_string(&proof)
                .unwrap()
                .contains("synthetic-private-password")
        );
        for (_, path, _) in source_deployment::FILES {
            let p = root.join(path);
            let original = fs::read(&p).unwrap();
            fs::write(&p, b"foreign-same-state").unwrap();
            assert_eq!(
                candidate_files(&root, &m, compose, environment)
                    .unwrap_err()
                    .code,
                "UPDATE_SOURCE_CONFIGURATION_MISMATCH"
            );
            assert_eq!(fs::read(&p).unwrap(), b"foreign-same-state");
            fs::write(&p, original).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_remapping_preserves_original_fields_and_refuses_ambiguous_images() {
        let original = crate::tests::manifest_for_detection();
        let before = serde_json::to_vec(&original).unwrap();
        let images = vec![
            crate::restoration::PreservedImage {
                reference: "original-db".into(),
                content_id: format!("sha256:{}", "a".repeat(64)),
                archive: "images/image-0.tar".into(),
                bytes: 3,
                sha256: "c".repeat(64),
            },
            crate::restoration::PreservedImage {
                reference: "original-platform".into(),
                content_id: format!("sha256:{}", "b".repeat(64)),
                archive: "images/image-1.tar".into(),
                bytes: 4,
                sha256: "d".repeat(64),
            },
        ];
        let id = uuid::Uuid::new_v4().to_string();
        let (m, compose) = crate::restoration::remapped_bundle(
            &original,
            &images,
            &id,
            13201,
            &images[0].content_id,
            &images[1].content_id,
            None,
        )
        .unwrap();
        assert_eq!(m.compose_sha256, crate::digest(&compose));
        assert_eq!(m.services, original.services);
        assert_eq!(m.schema_version, original.schema_version);
        assert_eq!(m.images[0].reference, images[0].content_id);
        assert_eq!(m.project_name, format!("exhibitos-{id}"));
        assert_eq!(m.open_url, "http://127.0.0.1:13201");
        assert_eq!(serde_json::to_vec(&original).unwrap(), before);
        let (new_manifest,new_compose)=crate::restoration::remapped_bundle(&original,&images,&id,13201,&images[0].content_id,&images[1].content_id,Some("10.240.0.0/28")).unwrap();
        assert_ne!(new_manifest.compose_sha256,m.compose_sha256);
        assert_eq!(new_manifest.compose_sha256,crate::digest(&new_compose));
        assert_eq!(crate::restoration_network::receipt_compose_subnet(&new_compose,Some("10.240.0.0/28")).unwrap().as_deref(),Some("10.240.0.0/28"));
        assert!(crate::restoration_network::receipt_compose_subnet(&new_compose,None).is_err());
        let (explicit, explicit_compose)=crate::restoration::bind_explicit_gateway((new_manifest.clone(),new_compose.clone()),"10.240.0.0/28").unwrap();
        assert_eq!(explicit.compose_sha256,crate::digest(&explicit_compose));
        assert_ne!(explicit.compose_sha256,new_manifest.compose_sha256);
        assert_eq!(crate::restoration_network::compose_gateway(&new_compose).unwrap(),None);
        assert_eq!(crate::restoration_network::compose_gateway(&explicit_compose).unwrap().as_deref(),Some("10.240.0.1"));
        assert!(crate::restoration::bind_explicit_gateway((explicit,explicit_compose),"10.240.0.16/28").is_err());

        assert_eq!(crate::restoration_network::receipt_compose_subnet(&compose,None).unwrap(),None);
        assert!(crate::restoration::remapped_bundle(&original,&images,&id,13201,&images[0].content_id,&images[1].content_id,Some("10.241.0.0/28")).is_err());
        for (port, db, runtime) in [
            (
                80,
                images[0].content_id.as_str(),
                images[1].content_id.as_str(),
            ),
            (
                13201,
                images[0].content_id.as_str(),
                images[0].content_id.as_str(),
            ),
            (13201, "foreign", images[1].content_id.as_str()),
        ] {
            assert!(
                crate::restoration::remapped_bundle(&original, &images, &id, port, db, runtime, None)
                    .is_err()
            );
        }
    }
}
