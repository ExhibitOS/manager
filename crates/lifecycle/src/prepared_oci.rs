// SPDX-License-Identifier: Apache-2.0
//! Development OCI preparation under current intent/profile/source/target fences.
use super::*;
use crate::{Value, err, run, run_observed_input, run_observed_inputs};
use std::collections::BTreeSet;

const QUALIFIER: &str = include_str!("../../../scripts/verify-runtime-oci.py");
const ENTRY: &str = r#"
from types import SimpleNamespace
with os.fdopen(int(sys.argv[3]),'rb',closefd=False) as manifest_fd:
    raw_manifest=manifest_fd.read(16*1024*1024+1)
    assert len(raw_manifest)<=16*1024*1024 and hashlib.sha256(raw_manifest).hexdigest()==sys.argv[5]
source=SimpleNamespace(read_text=lambda:raw_manifest.decode('utf-8'))
args = SimpleNamespace(archive=Path(sys.argv[1]),image=sys.argv[2],source_manifest=source,source_commit=sys.argv[4])
with os.fdopen(0,'rb',closefd=False) as retained:
    proof=qualify(args,retained)
print(json.dumps(proof,separators=(',',':')))
"#;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedOciReceipt {
    pub artifact: PreparedArtifactReceipt,
    pub proof: Value,
    pub cache_imported: bool,
    pub image_already_present: bool,
    pub containers_volumes_tags_preserved: bool,
    pub preflight_verified: bool,
    pub update_executed: bool,
}
fn docker(args: &[&str]) -> crate::Result<Vec<u8>> {
    run(
        "docker",
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
        60,
    )
}
fn images() -> crate::Result<BTreeSet<String>> {
    let bytes = docker(&["image", "ls", "--all", "--quiet", "--no-trunc"])?;
    let text = std::str::from_utf8(&bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let result: BTreeSet<_> = text.lines().map(str::to_owned).collect();
    if result.len() > 10000 {
        return Err(err("ENGINE_OUTPUT_LIMIT"));
    }
    Ok(result)
}
fn cache_state() -> crate::Result<Value> {
    let bytes = docker(&["ps", "--all", "--quiet", "--no-trunc"])?;
    let ids = std::str::from_utf8(&bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let mut containers = Vec::new();
    for id in ids.lines() {
        if containers.len() >= 10000 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || id.len() != 64
        {
            return Err(err("ENGINE_OUTPUT_LIMIT"));
        }
        let rows: Vec<Value> = serde_json::from_slice(&docker(&["inspect", id])?)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        let row = rows
            .first()
            .filter(|_| rows.len() == 1)
            .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
        containers.push(serde_json::json!({"id":row["Id"],"image":row["Image"],"name":row["Name"],"running":row["State"]["Running"],"status":row["State"]["Status"],"mounts":row["Mounts"]}));
    }
    containers.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_owned());
    let bytes = docker(&["image", "ls", "--no-trunc", "--format", "{{json .}}"])?;
    let text = std::str::from_utf8(&bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let mut tags = Vec::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        if row["Repository"] != "<none>" && row["Tag"] != "<none>" {
            tags.push(serde_json::json!([
                row["Repository"],
                row["Tag"],
                row["ID"]
            ]));
        }
    }
    tags.sort_by_key(Value::to_string);
    let bytes = docker(&["volume", "ls", "--quiet"])?;
    let mut volumes = std::str::from_utf8(&bytes)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    volumes.sort();
    Ok(serde_json::json!({"containers":containers,"volumes":volumes,"tags":tags}))
}
impl ExecutionSession<'_> {
    /// Uses the compiled-in bounded qualifier and the retained file as child stdin.
    /// source_commit is a development label expectation, never provenance authority.
    /// Cache import is explicit, tagless and cannot activate the installed candidate.
    pub fn qualify_prepared_oci(
        &self,
        artifact: &mut PreparedArtifact,
        python: &Path,
        source_commit: &str,
        import_cache: bool,
        acknowledged: bool,
    ) -> crate::Result<PreparedOciReceipt> {
        self.check()?;
        if !acknowledged {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !python.is_absolute()
            || fs::canonicalize(python).ok().as_deref() != Some(python)
            || !fs::metadata(python).is_ok_and(|m| m.is_file())
            || source_commit.len() != 40
            || !source_commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(err("UPDATE_OCI_INPUT_INVALID"));
        }
        self.reverify_prepared_artifact(artifact)?;
        if artifact.verified.release.channel != "development"
            || artifact.verified.release.target != "linux-arm64"
        {
            return Err(err("UPDATE_OCI_PLATFORM_UNVERIFIED"));
        }
        let plan = &artifact.plan;
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&root)?;
        let before = fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let _source_lock = self.source.lock()?;
        let _target_lock = target.lock()?;
        let job = target
            .restoration_status()?
            .filter(|j| j.state == "completed")
            .ok_or_else(|| err("UPDATE_SOURCE_PROOF_MISSING"))?;
        let workspace = root.join(format!("restore-{}", job.id));
        let receipt: crate::restoration::RestorationReceipt =
            crate::read_json(&crate::checked_path(&workspace, "receipt.json")?)?;
        let files =
            source_deployment::authenticated_files(&self.source.root, &workspace, &receipt, plan)?;
        let raw = crate::installation_backup::source_bytes(
            &workspace.join("authenticated"),
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )?;
        crate::restoration::RestorationBinding::from_plan(plan)?.authenticated(
            &receipt.backup_id,
            &receipt.authenticated_manifest_sha256,
            &raw,
        )?;
        let manifest = artifact
            .staged
            .path()
            .parent()
            .ok_or_else(|| err("UPDATE_OCI_INPUT_INVALID"))?
            .join("authenticated-manifest.json");
        let mut out = private_file(&manifest, true).map_err(|e| err(e.code()))?;
        out.write_all(&raw)
            .and_then(|_| out.sync_all())
            .map_err(|_| err("UPDATE_ARTIFACT_UNAVAILABLE"))?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400))
                .map_err(|_| err("UPDATE_ARTIFACT_UNAVAILABLE"))?;
        }
        let mut manifest_options = fs::OpenOptions::new();
        manifest_options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            manifest_options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let manifest_input = manifest_options
            .open(&manifest)
            .map_err(|_| err("UPDATE_OCI_INPUT_INVALID"))?;
        #[cfg(unix)]
        let manifest_descriptor = {
            use std::os::fd::AsRawFd;
            manifest_input.as_raw_fd().to_string()
        };
        #[cfg(not(unix))]
        let manifest_descriptor = String::new();
        let script = format!("__name__='exhibitos_embedded_oci'\n{QUALIFIER}\n{ENTRY}");
        let args = vec![
            "-I".into(),
            "-c".into(),
            script,
            artifact.staged.path().to_string_lossy().into_owned(),
            format!("sha256:{}", plan.target_image),
            manifest_descriptor,
            source_commit.into(),
            crate::digest(&raw),
        ];
        self.reverify_prepared_artifact(artifact)?;
        let input = artifact
            .staged
            .retained_input()
            .map_err(|e| err(e.code()))?;
        let output = run_observed_inputs(
            python
                .to_str()
                .ok_or_else(|| err("UPDATE_OCI_INPUT_INVALID"))?,
            &args,
            None,
            300,
            Some(input),
            Some(manifest_input),
            || self.check(),
        )
        .map_err(|_| err("UPDATE_OCI_INVALID"))?;
        let proof: Value =
            serde_json::from_slice(&output).map_err(|_| err("UPDATE_OCI_INVALID"))?;
        if proof["artifactSha256"] != artifact.verified.release.artifact.sha256
            || proof["artifactBytes"].as_u64() != Some(artifact.verified.release.artifact.bytes)
            || proof["runtimeImageSha256"] != artifact.plan.target_image
            || proof["targetSchemaSha256"] != artifact.plan.target_schema
            || proof["version"] != artifact.verified.release.version
        {
            return Err(err("UPDATE_OCI_INVALID"));
        }
        let image = format!("sha256:{}", artifact.plan.target_image);
        let mut already = false;
        let mut preserved = false;
        if import_cache {
            let original_state = cache_state()?;
            let original_images = images()?;
            already = original_images.contains(&image);
            self.reverify_prepared_artifact(artifact)?;
            let input = artifact
                .staged
                .retained_input()
                .map_err(|e| err(e.code()))?;
            run_observed_input(
                "docker",
                &["image".into(), "load".into(), "--quiet".into()],
                None,
                300,
                Some(input),
                || self.check(),
            )
            .map_err(|_| err("UPDATE_CACHE_IMPORT_UNCERTAIN"))?;
            self.reverify_prepared_artifact(artifact)?;
            let current = images()?;
            if !original_images.is_subset(&current)
                || current.difference(&original_images).any(|id| id != &image)
                || cache_state()? != original_state
            {
                return Err(err("UPDATE_CACHE_STATE_CHANGED"));
            }
            let rows: Vec<Value> = serde_json::from_slice(&docker(&["image", "inspect", &image])?)
                .map_err(|_| err("UPDATE_OCI_INVALID"))?;
            let observation = rows
                .first()
                .filter(|_| rows.len() == 1)
                .ok_or_else(|| err("UPDATE_OCI_INVALID"))?;
            if observation["Id"] != image
                || observation["Os"] != "linux"
                || observation["Architecture"] != "arm64"
                || observation["RootFS"]["Layers"] != proof["layerDiffIds"]
                || !observation["RepoTags"].as_array().is_none_or(Vec::is_empty)
            {
                return Err(err("UPDATE_CACHE_STATE_CHANGED"));
            }
            preserved = true;
        }
        if source_deployment::authenticated_files(
            &self.source.root,
            &workspace,
            &receipt,
            &artifact.plan,
        )? != files
            || crate::installation_backup::source_bytes(
                &workspace.join("authenticated"),
                "manifest.json",
                16 * 1024 * 1024,
                true,
            )? != raw
            || crate::installation_backup::source_bytes(
                manifest.parent().unwrap(),
                "authenticated-manifest.json",
                16 * 1024 * 1024,
                false,
            )? != raw
            || !identity(
                &before,
                &fs::symlink_metadata(&root).map_err(|_| err("UPDATE_TARGET_CHANGED"))?,
            )
        {
            return Err(err("UPDATE_SOURCE_CHANGED"));
        }
        Ok(PreparedOciReceipt {
            artifact: self.reverify_prepared_artifact(artifact)?,
            proof,
            cache_imported: import_cache,
            image_already_present: already,
            containers_volumes_tags_preserved: preserved,
            preflight_verified: false,
            update_executed: false,
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn extra_descriptor_survives_exec_without_changing_parent_close_on_exec() {
        use std::os::fd::AsRawFd;
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-extra-input-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&root).unwrap();
        let path = root.join("manifest");
        fs::write(&path, b"authenticated retained manifest").unwrap();
        let original = fs::File::open(&path).unwrap();
        let extra = original.try_clone().unwrap();
        let fd = extra.as_raw_fd();
        let before = unsafe { libc::fcntl(original.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(before & libc::FD_CLOEXEC, 0);
        fs::rename(&path, root.join("retained")).unwrap();
        fs::write(path, b"replacement manifest").unwrap();
        let bytes = run_observed_inputs(
            "/bin/cat",
            &[format!("/dev/fd/{fd}")],
            None,
            5,
            None,
            Some(extra),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(bytes, b"authenticated retained manifest");
        assert_eq!(
            unsafe { libc::fcntl(original.as_raw_fd(), libc::F_GETFD) },
            before
        );
    }
    #[test]
    fn inherited_stdin_reads_original_handle_after_path_replacement() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-retained-stdin-{}", uuid::Uuid::new_v4()));
        installations::new_directory(&root).unwrap();
        let path = root.join("input");
        let mut original = private_file(&path, true).unwrap();
        original.write_all(b"retained original bytes").unwrap();
        original.sync_all().unwrap();
        drop(original);
        let retained = fs::File::open(&path).unwrap();
        fs::rename(&path, root.join("retained")).unwrap();
        fs::write(&path, b"replacement bytes").unwrap();
        let bytes =
            run_observed_input("/bin/cat", &[], None, 5, Some(retained), || Ok(())).unwrap();
        assert_eq!(bytes, b"retained original bytes");
        assert_eq!(fs::read(path).unwrap(), b"replacement bytes");
    }
}
