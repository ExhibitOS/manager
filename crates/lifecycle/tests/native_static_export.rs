// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]
use exhibitos_lifecycle::installations::native_static_export::{Status, verify_output};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::symlink};
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[test]
fn recovered_completion_requires_exact_files_and_refuses_tampering_links_and_traversal() {
    let root = std::env::temp_dir().join(format!(
        "exhibitos-static-integrity-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let index = b"{\"version\":1}";
    let html = format!("<html>{}</html>", hash(index));
    let data = b"synthetic transport-integrity asset";
    fs::write(root.join("static-publication-index.json"), index).unwrap();
    fs::write(root.join("index.html"), &html).unwrap();
    fs::write(root.join("asset.bin"), data).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let revision = "a".repeat(64);
    let manifest = json!({"format":"exhibitos-static-publication-v1","publicationId":id,"revisionSha256":revision,"indexSha256":hash(index),"files":[{"path":"static-publication-index.json","bytes":index.len(),"sha256":hash(index)},{"path":"index.html","bytes":html.len(),"sha256":hash(html.as_bytes())},{"path":"asset.bin","bytes":data.len(),"sha256":hash(data)}]});
    let raw = serde_json::to_vec(&manifest).unwrap();
    let file = root.join("static-export-manifest.json");
    fs::write(&file, &raw).unwrap();
    let status:Status=serde_json::from_value(json!({"format":1,"jobId":uuid::Uuid::new_v4().to_string(),"workspaceId":uuid::Uuid::new_v4().to_string(),"tenantId":uuid::Uuid::new_v4().to_string(),"principalId":uuid::Uuid::new_v4().to_string(),"publicationId":id,"revisionSha256":revision,"output":root,"state":"running","receipt":null,"errorCode":null})).unwrap();
    let accepted = verify_output(&status).unwrap();
    assert_eq!(accepted.files, 4);
    assert_eq!(accepted.manifest_sha256, hash(&raw));
    fs::write(root.join("asset.bin"), b"tampered").unwrap();
    assert!(verify_output(&status).is_err());
    fs::write(root.join("asset.bin"), data).unwrap();
    fs::write(root.join("unlisted.txt"), b"preserve").unwrap();
    assert!(verify_output(&status).is_err());
    assert_eq!(fs::read(root.join("unlisted.txt")).unwrap(), b"preserve");
    fs::remove_file(root.join("unlisted.txt")).unwrap();
    fs::remove_file(root.join("asset.bin")).unwrap();
    symlink(root.join("index.html"), root.join("asset.bin")).unwrap();
    assert!(verify_output(&status).is_err());
    assert_eq!(fs::read(root.join("index.html")).unwrap(), html.as_bytes());
    fs::remove_file(root.join("asset.bin")).unwrap();
    fs::write(root.join("asset.bin"), data).unwrap();
    let mut traversal = manifest.clone();
    traversal["files"][2]["path"] = json!("../outside.bin");
    fs::write(&file, serde_json::to_vec(&traversal).unwrap()).unwrap();
    assert!(verify_output(&status).is_err());
    fs::write(&file, &raw).unwrap();
    assert!(verify_output(&status).is_ok());
    fs::remove_dir_all(root).unwrap();
}
