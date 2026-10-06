// SPDX-License-Identifier: Apache-2.0
//! Five current Manager host files only; never DB/blob/config-volume inventory proof.
use crate::*;
pub(super) const FILES: [(&str, &str, u64); 5] = [
    (
        "manager-bundle-manifest.json",
        "bundle/manifest.json",
        256 * 1024,
    ),
    ("manager-compose.yaml", "bundle/compose.yaml", 256 * 1024),
    ("manager-runtime.env", "runtime.env", 8192),
    ("manager-installed.json", "installed.json", 256 * 1024),
    ("manager-engine.json", "engine.json", 4096),
];
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentFileProof {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDeploymentReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub files: Vec<DeploymentFileProof>,
    pub observed_at: u64,
}
pub(super) fn check_receipt(
    receipt: &crate::restoration::RestorationReceipt,
    plan: &crate::update::Plan,
) -> Result<()> {
    crate::restoration::RestorationBinding::from_plan(plan)?.check_receipt(receipt)
}
pub(super) fn authenticated_files(
    source: &Path,
    workspace: &Path,
    receipt: &crate::restoration::RestorationReceipt,
    plan: &crate::update::Plan,
) -> Result<Vec<DeploymentFileProof>> {
    check_receipt(receipt, plan)?;
    let binding = crate::restoration::RestorationBinding::from_plan(plan)?;
    let bytes = crate::installation_backup::source_bytes(
        &workspace.join("authenticated"),
        "manifest.json",
        16 * 1024 * 1024,
        true,
    )?;
    binding.authenticated(
        &receipt.backup_id,
        &receipt.authenticated_manifest_sha256,
        &bytes,
    )?;
    let manifest: Value =
        serde_json::from_slice(&bytes).map_err(|_| err("RESTORE_RESULT_INVALID"))?;
    compare(source, &manifest)
}
/// Only call after exact authenticated raw manifest binding. No caller success flags.
pub(super) fn compare(root: &Path, manifest: &Value) -> Result<Vec<DeploymentFileProof>> {
    let files = manifest["files"]
        .as_array()
        .ok_or_else(|| err("UPDATE_SOURCE_DEPLOYMENT_INVALID"))?;
    let mut proof = Vec::new();
    for (name, path, limit) in FILES {
        let matches: Vec<_> = files.iter().filter(|f| f["name"] == name).collect();
        if matches.len() != 1 {
            return Err(err("UPDATE_SOURCE_DEPLOYMENT_INVALID"));
        }
        let f = matches[0];
        let bytes = f["bytes"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= limit)
            .ok_or_else(|| err("UPDATE_SOURCE_DEPLOYMENT_INVALID"))?;
        let sha = f["sha256"]
            .as_str()
            .filter(|h| hash_valid(h))
            .ok_or_else(|| err("UPDATE_SOURCE_DEPLOYMENT_INVALID"))?;
        if f["role"] != "configuration" {
            return Err(err("UPDATE_SOURCE_DEPLOYMENT_INVALID"));
        }
        let current = crate::installation_backup::source_bytes(root, path, limit, true)?;
        if current.len() as u64 != bytes || digest(&current) != sha {
            return Err(err("UPDATE_SOURCE_DEPLOYMENT_MISMATCH"));
        }
        proof.push(DeploymentFileProof {
            path: path.into(),
            bytes,
            sha256: sha.into(),
        });
    }
    Ok(proof)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (PathBuf, Value) {
        let root =
            std::env::temp_dir().join(format!("exhibitos-source-deployment-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.join("bundle")).unwrap();
        fs::set_permissions(root.join("bundle"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut files = Vec::new();
        for (name, path, _) in FILES {
            let p = root.join(path);
            fs::write(&p, path.as_bytes()).unwrap();
            fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
            files.push(serde_json::json!({"name":name,"role":"configuration","bytes":path.len(),"sha256":digest(path.as_bytes())}));
        }
        (root, serde_json::json!({"files":files}))
    }
    #[test]
    fn exact_five_files_match_without_returning_private_values() {
        let (p, m) = fixture();
        let proof = compare(&p, &m).unwrap();
        assert_eq!(proof.len(), 5);
        for f in proof {
            assert_eq!(f.sha256, digest(f.path.as_bytes()));
        }
    }
    #[test]
    fn each_changed_host_file_refuses_even_when_length_is_equal() {
        for (_, path, _) in FILES {
            let (p, m) = fixture();
            fs::write(p.join(path), vec![b'x'; path.len()]).unwrap();
            assert_eq!(
                compare(&p, &m).unwrap_err().code,
                "UPDATE_SOURCE_DEPLOYMENT_MISMATCH"
            );
        }
    }
    #[test]
    fn missing_duplicate_wrong_role_and_invalid_hash_or_size_refuse() {
        for variant in 0..5 {
            let (p, mut m) = fixture();
            match variant {
                0 => {
                    m["files"].as_array_mut().unwrap().remove(0);
                }
                1 => {
                    let f = m["files"][0].clone();
                    m["files"].as_array_mut().unwrap().push(f);
                }
                2 => m["files"][0]["role"] = "deployment".into(),
                3 => m["files"][0]["sha256"] = "not-a-hash".into(),
                _ => m["files"][0]["bytes"] = 0.into(),
            };
            assert_eq!(
                compare(&p, &m).unwrap_err().code,
                "UPDATE_SOURCE_DEPLOYMENT_INVALID"
            );
        }
    }
    #[test]
    fn aliases_and_shared_or_public_host_files_are_not_proof() {
        let (p, m) = fixture();
        let path = p.join("runtime.env");
        fs::rename(&path, p.join("retained-env")).unwrap();
        symlink(p.join("retained-env"), &path).unwrap();
        assert!(compare(&p, &m).is_err());
        let (p, m) = fixture();
        fs::hard_link(p.join("runtime.env"), p.join("shared-env")).unwrap();
        assert!(compare(&p, &m).is_err());
        let (p, m) = fixture();
        fs::set_permissions(p.join("runtime.env"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(compare(&p, &m).is_err());
    }
    #[test]
    fn bound_completed_receipt_cannot_substitute_any_source_identity() {
        let mut plan = super::super::super::tests::plan();
        plan.backup_id = Uuid::new_v4().to_string();
        let make = || crate::restoration::RestorationReceipt {
            id: Uuid::new_v4().to_string(),
            operation: "restored-and-running".into(),
            backup_id: plan.backup_id.clone(),
            authenticated_manifest_sha256: plan.backup_manifest.clone(),
            bundle_id: "synthetic-bundle".into(),
            project_name: "exhibitos-synthetic".into(),
            open_url: "http://127.0.0.1:19000".into(),
            at: 0,
            source_verification: Some(crate::restoration::RestorationProof {
                inventory_sha256: plan.source_inventory.clone(),
                schema_sha256: plan.source_schema.clone(),
                runtime_image_sha256: plan.source_image.clone(),
            }),
        };
        check_receipt(&make(), &plan).unwrap();
        for field in 0..6 {
            let mut receipt = make();
            match field {
                0 => receipt.backup_id = "foreign".into(),
                1 => receipt.authenticated_manifest_sha256 = "0".repeat(64),
                2 => {
                    receipt
                        .source_verification
                        .as_mut()
                        .unwrap()
                        .inventory_sha256 = "0".repeat(64)
                }
                3 => receipt.source_verification.as_mut().unwrap().schema_sha256 = "0".repeat(64),
                4 => {
                    receipt
                        .source_verification
                        .as_mut()
                        .unwrap()
                        .runtime_image_sha256 = "0".repeat(64)
                }
                _ => receipt.source_verification = None,
            };
            let error = check_receipt(&receipt, &plan).unwrap_err();
            assert_eq!(
                error.code,
                if field == 5 {
                    "UPDATE_SOURCE_PROOF_MISSING"
                } else {
                    "UPDATE_RESTORE_BINDING_MISMATCH"
                }
            );
        }
    }
}
