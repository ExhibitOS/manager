// SPDX-License-Identifier: Apache-2.0
//! Fresh Engine reference mappings, never image/archive byte-integrity proof.
use crate::*;
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentImageMapping {
    pub reference: String,
    pub content_id: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceImageReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub images: Vec<CurrentImageMapping>,
    pub observed_at: u64,
}
fn file_record<'a>(manifest: &'a Value, name: &str, role: &str) -> Result<&'a Value> {
    let files = manifest["files"]
        .as_array()
        .ok_or_else(|| err("UPDATE_SOURCE_IMAGES_INVALID"))?;
    let selected: Vec<_> = files.iter().filter(|f| f["name"] == name).collect();
    if selected.len() != 1 || selected[0]["role"] != role {
        return Err(err("UPDATE_SOURCE_IMAGES_INVALID"));
    }
    Ok(selected[0])
}
fn bound_inventory(
    bytes: &[u8],
    manifest: &Value,
    original: &BundleManifest,
) -> Result<Vec<crate::restoration::PreservedImage>> {
    let record = file_record(manifest, "manager-image-inventory.json", "configuration")?;
    if bytes.is_empty()
        || bytes.len() > 1048576
        || record["bytes"].as_u64() != Some(bytes.len() as u64)
        || record["sha256"] != digest(bytes)
    {
        return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
    }
    let images = crate::restoration::preserved_images(bytes, original)?;
    for image in &images {
        let archive = file_record(manifest, &image.archive, "deployment")?;
        if archive["bytes"].as_u64() != Some(image.bytes) || archive["sha256"] != image.sha256 {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
    }
    Ok(images)
}
fn fresh_mappings(
    images: &[crate::restoration::PreservedImage],
    mut inspect: impl FnMut(&str) -> Result<String>,
) -> Result<Vec<CurrentImageMapping>> {
    let mut result = Vec::new();
    for image in images {
        let current = inspect(&image.reference)?;
        if current != image.content_id {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
        result.push(CurrentImageMapping {
            reference: image.reference.clone(),
            content_id: current,
        });
    }
    for image in &result {
        if inspect(&image.reference)? != image.content_id {
            return Err(err("UPDATE_SOURCE_IMAGES_CHANGED"));
        }
    }
    Ok(result)
}
pub(super) fn observe(
    source: &LifecycleService,
    workspace: &Path,
    raw: &[u8],
) -> Result<Vec<CurrentImageMapping>> {
    let manifest: Value =
        serde_json::from_slice(raw).map_err(|_| err("UPDATE_SOURCE_IMAGES_INVALID"))?;
    let original = source.manifest()?;
    let bytes = crate::installation_backup::source_bytes(
        &workspace.join("configuration"),
        "manager-image-inventory.json",
        1048576,
        true,
    )?;
    let expected = bound_inventory(&bytes, &manifest, &original)?;
    let mappings = fresh_mappings(&expected, |reference| {
        crate::backup_creation::local_image("docker", reference)
    })?;
    // Also bind both currently installed containers, not just the available image store.
    let config: Value = serde_json::from_slice(&run(
        "docker",
        &compose_args(&original, &["config", "--format", "json"]),
        Some(&source.root.join("bundle")),
        30,
    )?)
    .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    for service in ["platform", "database"] {
        let reference = config["services"][service]["image"]
            .as_str()
            .ok_or_else(|| err("UPDATE_SOURCE_IMAGES_INVALID"))?;
        let selected: Vec<_> = mappings
            .iter()
            .filter(|m| m.reference == reference || same_registry_pin(&m.reference, reference))
            .collect();
        if selected.len() != 1 {
            return Err(err("UPDATE_SOURCE_IMAGES_INVALID"));
        }
        let (_, actual) =
            crate::backup_creation::one_container(source, &original, "docker", service)?;
        if actual["Image"] != selected[0].content_id {
            return Err(err("UPDATE_SOURCE_IMAGES_MISMATCH"));
        }
    }
    // Re-read the authenticated configuration copy and raw manifest after Engine calls.
    if crate::installation_backup::source_bytes(
        &workspace.join("configuration"),
        "manager-image-inventory.json",
        1048576,
        true,
    )? != bytes
        || crate::installation_backup::source_bytes(
            &workspace.join("authenticated"),
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )? != raw
    {
        return Err(err("UPDATE_SOURCE_IMAGES_CHANGED"));
    }
    Ok(mappings)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image(reference: &str) -> crate::restoration::PreservedImage {
        crate::restoration::PreservedImage {
            reference: reference.into(),
            content_id: format!("sha256:{}", "a".repeat(64)),
            archive: "images/image-0.tar".into(),
            bytes: 1,
            sha256: "b".repeat(64),
        }
    }
    #[test]
    fn historical_mapping_is_not_an_engine_observation() {
        let images = [image("pinned")];
        let mut calls = 0;
        let v = fresh_mappings(&images, |_| {
            calls += 1;
            Ok(images[0].content_id.clone())
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(v[0].content_id, images[0].content_id);
        assert_eq!(
            fresh_mappings(&images, |_| Ok(format!("sha256:{}", "c".repeat(64))))
                .unwrap_err()
                .code,
            "UPDATE_SOURCE_IMAGES_MISMATCH"
        );
        let mut calls = 0;
        assert_eq!(
            fresh_mappings(&images, |_| {
                calls += 1;
                Ok(if calls == 1 {
                    images[0].content_id.clone()
                } else {
                    format!("sha256:{}", "c".repeat(64))
                })
            })
            .unwrap_err()
            .code,
            "UPDATE_SOURCE_IMAGES_CHANGED"
        );
        assert!(fresh_mappings(&images, |_| Err(err("IMAGE_INTEGRITY"))).is_err());
    }
    #[test]
    fn bound_mapping_and_archive_records_must_match_authentic_bytes() {
        let reference = format!("sha256:{}", "a".repeat(64));
        let m:BundleManifest=serde_json::from_value(serde_json::json!({"schemaVersion":"1","bundleId":"synthetic","version":"1","protocolVersion":"1","composeSha256":"a".repeat(64),"projectName":"synthetic","services":[],"images":[{"reference":reference},{"reference":format!("sha256:{}","c".repeat(64))}],"ports":[],"openUrl":"","readinessUrl":"","minimumFreeBytes":0})).unwrap();
        let rows = serde_json::json!([{"reference":m.images[0].reference,"contentId":m.images[0].reference,"archive":"images/image-0.tar","bytes":1,"sha256":"b".repeat(64)},{"reference":m.images[1].reference,"contentId":m.images[1].reference,"archive":"images/image-1.tar","bytes":2,"sha256":"d".repeat(64)}]);
        let bytes = serde_json::to_vec(&rows).unwrap();
        let manifest = serde_json::json!({"files":[{"name":"manager-image-inventory.json","role":"configuration","bytes":bytes.len(),"sha256":digest(&bytes)},{"name":"images/image-0.tar","role":"deployment","bytes":1,"sha256":"b".repeat(64)},{"name":"images/image-1.tar","role":"deployment","bytes":2,"sha256":"d".repeat(64)}]});
        assert!(bound_inventory(&bytes, &manifest, &m).is_ok());
        let mut bad = manifest.clone();
        bad["files"][1]["sha256"] = "0".repeat(64).into();
        assert!(bound_inventory(&bytes, &bad, &m).is_err());
        let mut bad = manifest.clone();
        bad["files"][0]["role"] = "blob".into();
        assert!(bound_inventory(&bytes, &bad, &m).is_err());
        let mut bad = manifest.clone();
        bad["files"]
            .as_array_mut()
            .unwrap()
            .push(manifest["files"][0].clone());
        assert!(bound_inventory(&bytes, &bad, &m).is_err());
        assert!(bound_inventory(b"[]", &manifest, &m).is_err());
    }
    #[test]
    #[ignore = "requires the qualified existing local images and Docker"]
    fn actual_engine_current_image_mappings() {
        let mut expected = Vec::new();
        for id in [
            "sha256:335f8f2c1437841266c41e79912b1160b03ce500511acc94afa338c4c8f6215b",
            "sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06",
        ] {
            let mut row = image(id);
            row.content_id = id.into();
            expected.push(row);
        }
        assert_eq!(
            fresh_mappings(&expected, |r| crate::backup_creation::local_image(
                "docker", r
            ))
            .unwrap()
            .len(),
            2
        );
        expected[0].content_id = format!("sha256:{}", "0".repeat(64));
        assert_eq!(
            fresh_mappings(&expected, |r| crate::backup_creation::local_image(
                "docker", r
            ))
            .unwrap_err()
            .code,
            "UPDATE_SOURCE_IMAGES_MISMATCH"
        );
    }
}
