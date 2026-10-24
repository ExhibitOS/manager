// SPDX-License-Identifier: Apache-2.0
//! Inspect the existing immutable static publication export; no token, network,
//! folder dialog or deployment operation. Hosted and native consumers share it.
use crate::LifecycleError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
fn refused(code: &str) -> LifecycleError {
    LifecycleError { code: code.into(), guidance: "연결이나 배포를 완료하지 못했습니다. 저장된 자료는 그대로입니다. 계정 권한과 제공자의 실제 작업 상태를 확인하세요.".into() }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub title: String,
    pub publication_id: String,
    pub project_name: String,
    pub revision_sha256: String,
    pub index_sha256: String,
}
pub struct SourcePin {
    pub path: String,
    pub sha256: String,
    pub info: SourceInfo,
}
pub fn inspect(path: &str) -> Result<SourcePin, LifecycleError> {
    let root = std::fs::canonicalize(path).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    let (bytes, manifest_pin) = crate::hosted_runtime_file::bounded(
        &root.join("static-export-manifest.json"),
        8 * 1024 * 1024,
    )?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if manifest["format"] != "exhibitos-static-publication-v1" {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let publication_id = manifest["publicationId"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control))
        .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
        .to_owned();
    let (index, index_pin) = crate::hosted_runtime_file::bounded(
        &root.join("static-publication-index.json"),
        1024 * 1024,
    )?;
    if format!("{:x}", Sha256::digest(&index)) != manifest["indexSha256"].as_str().unwrap_or("") {
        return Err(refused("HOSTED_STATIC_BUNDLE_CHANGED"));
    }
    let index: serde_json::Value =
        serde_json::from_slice(&index).map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    let public_path = index["publication"]["path"]
        .as_str()
        .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if public_path.len() != 74
        || !public_path.starts_with("data/")
        || !public_path.ends_with(".json")
        || !public_path[5..69].bytes().all(|b| b.is_ascii_hexdigit())
        || std::fs::symlink_metadata(root.join("data"))
            .map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
            .is_symlink()
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let (public_bytes, public_pin) =
        crate::hosted_runtime_file::bounded(&root.join(public_path), 25 * 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&public_bytes))
        != index["publication"]["sha256"].as_str().unwrap_or("")
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_CHANGED"));
    }
    let public: serde_json::Value = serde_json::from_slice(&public_bytes)
        .map_err(|_| refused("HOSTED_STATIC_BUNDLE_INVALID"))?;
    if public["publication"]["status"] != "published"
        || public["publication"]["id"] != publication_id
        || public["publication"]["revisionSha256"] != manifest["revisionSha256"]
    {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let title = public["exhibition"]["title"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
        .unwrap_or("공개 전시")
        .to_owned();
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "exhibition" } else { slug };
    let suffix: String = publication_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    if suffix.is_empty() {
        return Err(refused("HOSTED_STATIC_BUNDLE_INVALID"));
    }
    let project_name = format!(
        "{}-{}",
        slug.chars()
            .take(40)
            .collect::<String>()
            .trim_end_matches('-'),
        suffix.to_lowercase()
    );
    manifest_pin.check()?;
    index_pin.check()?;
    public_pin.check()?;
    Ok(SourcePin {
        path: root
            .to_str()
            .ok_or_else(|| refused("HOSTED_STATIC_BUNDLE_INVALID"))?
            .to_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        info: SourceInfo {
            title,
            publication_id,
            project_name,
            revision_sha256: manifest["revisionSha256"].as_str().unwrap_or("").into(),
            index_sha256: manifest["indexSha256"].as_str().unwrap_or("").into(),
        },
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
    };
    struct Fixture {
        root: PathBuf,
        public_path: String,
        public_bytes: Vec<u8>,
    }
    impl Fixture {
        fn new(status: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "exhibitos-publication-source-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(root.join("data")).unwrap();
            let publication = "12345678-1234-4234-8234-123456789abc";
            let revision = "a".repeat(64);
            let public_bytes = serde_json::to_vec(&serde_json::json!({"publication":{"id":publication,"status":status,"revisionSha256":revision},"exhibition":{"title":"한글 전시"}})).unwrap();
            let public_sha = format!("{:x}", Sha256::digest(&public_bytes));
            let public_path = format!("data/{public_sha}.json");
            let index = serde_json::to_vec(
                &serde_json::json!({"publication":{"path":public_path,"sha256":public_sha}}),
            )
            .unwrap();
            let manifest = serde_json::to_vec(&serde_json::json!({"format":"exhibitos-static-publication-v1","publicationId":publication,"revisionSha256":revision,"indexSha256":format!("{:x}",Sha256::digest(&index))})).unwrap();
            for (name, bytes) in [
                (public_path.as_str(), public_bytes.as_slice()),
                ("static-publication-index.json", index.as_slice()),
                ("static-export-manifest.json", manifest.as_slice()),
            ] {
                fs::write(root.join(name), bytes).unwrap();
                fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o600)).unwrap();
            }
            Self {
                root,
                public_path,
                public_bytes,
            }
        }
        fn inspect(&self) -> Result<SourcePin, LifecycleError> {
            inspect(self.root.to_str().unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn existing_public_projection_produces_the_same_source_binding_and_safe_project_name() {
        let fixture = Fixture::new("published");
        let selected = fixture.inspect().unwrap();
        assert_eq!(
            selected.path,
            fixture.root.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(
            selected.sha256,
            format!(
                "{:x}",
                Sha256::digest(fs::read(fixture.root.join("static-export-manifest.json")).unwrap())
            )
        );
        assert_eq!(selected.info.title, "한글 전시");
        assert_eq!(
            selected.info.publication_id,
            "12345678-1234-4234-8234-123456789abc"
        );
        assert_eq!(selected.info.project_name, "exhibition-12345678");
        assert_eq!(
            fs::read(fixture.root.join(&fixture.public_path)).unwrap(),
            fixture.public_bytes
        );
    }
    #[test]
    fn changed_projection_is_refused_and_inspection_never_repairs_input() {
        let fixture = Fixture::new("published");
        let manifest = fs::read(fixture.root.join("static-export-manifest.json")).unwrap();
        fs::write(fixture.root.join(&fixture.public_path), b"changed").unwrap();
        assert_eq!(
            fixture.inspect().err().unwrap().code,
            "HOSTED_STATIC_BUNDLE_CHANGED"
        );
        assert_eq!(
            fs::read(fixture.root.join(&fixture.public_path)).unwrap(),
            b"changed"
        );
        assert_eq!(
            fs::read(fixture.root.join("static-export-manifest.json")).unwrap(),
            manifest
        );
    }
    #[test]
    fn nonpublic_or_linked_data_is_refused_without_touching_original_bytes() {
        let unpublished = Fixture::new("draft");
        assert_eq!(
            unpublished.inspect().err().unwrap().code,
            "HOSTED_STATIC_BUNDLE_INVALID"
        );
        let linked = Fixture::new("published");
        fs::rename(linked.root.join("data"), linked.root.join("retained-data")).unwrap();
        symlink("retained-data", linked.root.join("data")).unwrap();
        assert_eq!(
            linked.inspect().err().unwrap().code,
            "HOSTED_STATIC_BUNDLE_INVALID"
        );
        let name = linked.public_path.strip_prefix("data/").unwrap();
        assert_eq!(
            fs::read(linked.root.join("retained-data").join(name)).unwrap(),
            linked.public_bytes
        );
    }
}
