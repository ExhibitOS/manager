// SPDX-License-Identifier: Apache-2.0
//! Exact current configuration scope and freshly exported original image bytes.
use super::{
    source_configuration::NativeConfigurationProof, source_deployment::DeploymentFileProof,
    source_image_bytes::ImageArchiveProof,
};
use crate::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationInventoryReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub configuration_volume: String,
    pub files: Vec<NativeConfigurationProof>,
    pub images: Vec<ImageArchiveProof>,
    pub export_workspace: String,
    pub observed_at: u64,
}
pub(super) fn scope(raw: &[u8]) -> Result<Value> {
    let manifest: Value =
        serde_json::from_slice(raw).map_err(|_| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    let mut names = Vec::new();
    for f in manifest["files"]
        .as_array()
        .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?
    {
        if f["role"] == "configuration" {
            names.push(
                f["name"]
                    .as_str()
                    .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?,
            );
        }
    }
    names.sort();
    let mut expected: Vec<_> = super::source_deployment::FILES
        .iter()
        .map(|v| v.0)
        .chain(["freeze-signing-key.json", "manager-image-inventory.json"])
        .collect();
    expected.sort();
    if names != expected {
        return Err(err("UPDATE_SOURCE_CONFIGURATION_SCOPE_UNSUPPORTED"));
    }
    Ok(manifest)
}
pub(super) fn generated_inventory(
    images: &[ImageArchiveProof],
    manifest: &Value,
) -> Result<NativeConfigurationProof> {
    let values:Vec<_>=images.iter().map(|i|serde_json::json!({"reference":i.reference,"contentId":i.content_id,"archive":i.archive,"bytes":i.bytes,"sha256":i.sha256})).collect();
    let bytes =
        serde_json::to_vec(&values).map_err(|_| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    let record = manifest["files"]
        .as_array()
        .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?
        .iter()
        .find(|f| f["role"] == "configuration" && f["name"] == "manager-image-inventory.json")
        .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    if record["bytes"].as_u64() != Some(bytes.len() as u64) || record["sha256"] != digest(&bytes) {
        return Err(err("UPDATE_SOURCE_CONFIGURATION_MISMATCH"));
    }
    Ok(NativeConfigurationProof {
        name: "manager-image-inventory.json".into(),
        bytes: bytes.len() as u64,
        sha256: digest(&bytes),
    })
}
pub(super) fn files(
    host: &[DeploymentFileProof],
    key: NativeConfigurationProof,
    images: NativeConfigurationProof,
) -> Result<Vec<NativeConfigurationProof>> {
    if host.len() != 5 {
        return Err(err("UPDATE_SOURCE_CONFIGURATION_INVALID"));
    }
    let mut result = Vec::new();
    for ((name, path, _), file) in super::source_deployment::FILES.iter().zip(host) {
        if file.path != *path {
            return Err(err("UPDATE_SOURCE_CONFIGURATION_INVALID"));
        }
        result.push(NativeConfigurationProof {
            name: (*name).into(),
            bytes: file.bytes,
            sha256: file.sha256.clone(),
        });
    }
    result.extend([key, images]);
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_scope_refuses_unknown_and_duplicate_configuration() {
        let mut files: Vec<_> = super::super::source_deployment::FILES
            .iter()
            .map(|f| serde_json::json!({"role":"configuration","name":f.0}))
            .collect();
        files.extend([
            serde_json::json!({"role":"configuration","name":"freeze-signing-key.json"}),
            serde_json::json!({"role":"configuration","name":"manager-image-inventory.json"}),
        ]);
        let encoded = |v: &Vec<Value>| serde_json::to_vec(&serde_json::json!({"files":v})).unwrap();
        assert!(scope(&encoded(&files)).is_ok());
        files.push(serde_json::json!({"role":"configuration","name":"unknown.json"}));
        assert!(scope(&encoded(&files)).is_err());
        files.pop();
        files.push(files[0].clone());
        assert!(scope(&encoded(&files)).is_err());
    }
}
