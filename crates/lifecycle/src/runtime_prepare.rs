// SPDX-License-Identifier: Apache-2.0
//! Preparation from fixed application resources; never downloads or changes an existing bundle.
use super::*;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    format: String,
    version: String,
    architecture: String,
    platform_commit: String,
    files: BTreeMap<String, Pin>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    bytes: u64,
    sha256: String,
}
fn failure(code: &str) -> LifecycleError {
    LifecycleError {code:code.into(), guidance:"서버 설치 파일을 준비하지 못했습니다. 현재 설치와 자료는 유지됩니다. 이 Mac용 설치 파일이 포함된 Manager를 사용하거나 준비 상태를 다시 확인하세요.".into()}
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err(failure("PLATFORM_RUNTIME_INVALID"));
        }
    }
    Ok(m)
}
fn pin_file(path: &Path, pin: &Pin) -> Result<()> {
    let m = regular(path)?;
    if m.len() != pin.bytes {
        return Err(failure("PLATFORM_RUNTIME_CHANGED"));
    }
    let mut f = File::open(path).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    let mut hash = Sha256::new();
    let mut bytes = [0; 65536];
    let mut count = 0u64;
    loop {
        let n = f
            .read(&mut bytes)
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > pin.bytes {
            return Err(failure("PLATFORM_RUNTIME_CHANGED"));
        }
        hash.update(&bytes[..n]);
    }
    if count != pin.bytes || format!("{:x}", hash.finalize()) != pin.sha256 {
        return Err(failure("PLATFORM_RUNTIME_CHANGED"));
    }
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    Ok(())
}
pub(super) fn prepare(root: &Path, source: &Path) -> Result<()> {
    prepare_with_network(root, source, super::choose_explicit_local_network)
}
fn prepare_with_network(
    root: &Path,
    source: &Path,
    choose: impl FnOnce() -> Result<ExplicitLocalNetwork>,
) -> Result<()> {
    // This is called with the existing lifecycle operation lock and selected-profile admission.
    // Never repair or replace a user's pre-existing bundle, including a partial one.
    if fs::symlink_metadata(root.join("bundle")).is_ok() {
        return Ok(());
    }
    if fs::symlink_metadata(root.join("installed.json")).is_ok() {
        return Err(failure("PLATFORM_RUNTIME_EXISTING_INSTALLATION"));
    }
    if !cfg!(target_os = "macos") {
        return Err(failure("PLATFORM_RUNTIME_PLATFORM_UNVERIFIED"));
    }
    for entry in fs::read_dir(root).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))? {
        let name = entry
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?
            .file_name();
        if name != "operation.lock"
            && name != ".platform-runtime-preparing"
            && name != ".platform-runtime-selection.json"
        {
            return Err(failure("PLATFORM_RUNTIME_EXISTING_DATA"));
        }
    }
    directory(source)?;
    let receipt = source.join("prepared.json");
    if regular(&receipt)?.len() > 65536 {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let descriptor: Prepared = serde_json::from_slice(
        &fs::read(&receipt).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?,
    )
    .map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    let architecture = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unsupported"
    };
    if descriptor.format != "exhibitos-manager-platform-runtime-v1"
        || descriptor.architecture != architecture
        || descriptor.platform_commit.len() != 40
        || !descriptor
            .platform_commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || descriptor.files.len() < 2
        || descriptor.files.len() > 18
    {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let mut total = 0u64;
    for (name, pin) in &descriptor.files {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || name.starts_with('.')
            || pin.bytes == 0
            || pin.bytes > 8 * 1024 * 1024 * 1024
            || (name == "manifest.json" && pin.bytes > 65536)
            || (name == "compose.yaml" && pin.bytes > 256 * 1024)
            || !hash_valid(&pin.sha256)
        {
            return Err(failure("PLATFORM_RUNTIME_INVALID"));
        }
        total = total
            .checked_add(pin.bytes)
            .ok_or_else(|| failure("PLATFORM_RUNTIME_INVALID"))?;
        pin_file(&source.join(name), pin)?;
    }
    if !descriptor.files.contains_key("manifest.json")
        || !descriptor.files.contains_key("compose.yaml")
    {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let mut manifest: BundleManifest = serde_json::from_slice(
        &fs::read(source.join("manifest.json"))
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?,
    )
    .map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    if manifest.version != descriptor.version
        || manifest.schema_version != "1.0.0-draft.1"
        || manifest.protocol_version != "1"
        || descriptor.files["compose.yaml"].sha256 != manifest.compose_sha256
    {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let archives = manifest
        .images
        .iter()
        .filter_map(|i| i.archive.as_ref())
        .collect::<Vec<_>>();
    if descriptor.files.len() != archives.len() + 2
        || archives.iter().any(|a| {
            descriptor
                .files
                .get(&a.path)
                .is_none_or(|p| p.sha256 != a.sha256 || p.bytes != a.bytes)
        })
    {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let available = fs2::available_space(root).map_err(|_| failure("STORAGE_UNAVAILABLE"))?;
    if available < total.saturating_add(manifest.minimum_free_bytes) {
        return Err(failure("STORAGE_QUOTA"));
    }
    // A fresh installation gets its own Compose project and labels. The durable local
    // binding is reused on retry; the original resource descriptor and archives stay intact.
    let selection = root.join(".platform-runtime-selection.json");
    let template =
        digest(&fs::read(&receipt).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?);
    if !selection.exists() {
        // Reuse the bounded allocator; never embed this machine's addresses in the template.
        if manifest.explicit_local_network.is_some() {
            return Err(failure("PLATFORM_RUNTIME_INVALID"));
        }
        let network = if manifest.preferred_engine.as_deref() == Some("docker") {
            Some(choose()?)
        } else {
            None
        };
        if let Some(policy) = &network {
            policy.validate()?;
        }
        let binding = serde_json::json!({"templateSha256":template,"id":Uuid::new_v4().to_string(),"explicitLocalNetwork":network});
        let mut file = private_options()
            .open(&selection)
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        file.write_all(
            &serde_json::to_vec(&binding).map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?,
        )
        .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        file.sync_all()
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    }
    if regular(&selection)?.len() > 1024 {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    let binding: serde_json::Value = serde_json::from_slice(
        &fs::read(&selection).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?,
    )
    .map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    let local_id = binding["id"]
        .as_str()
        .ok_or_else(|| failure("PLATFORM_RUNTIME_INVALID"))?;
    if binding["templateSha256"] != template
        || Uuid::parse_str(local_id)
            .ok()
            .is_none_or(|id| id.to_string() != local_id)
    {
        return Err(failure("PLATFORM_RUNTIME_CONFLICT"));
    }
    let network = if manifest.preferred_engine.as_deref() == Some("docker") {
        let policy: ExplicitLocalNetwork =
            serde_json::from_value(binding["explicitLocalNetwork"].clone())
                .map_err(|_| failure("PLATFORM_RUNTIME_CONFLICT"))?;
        policy.validate()?;
        Some(policy)
    } else {
        if !binding["explicitLocalNetwork"].is_null() {
            return Err(failure("PLATFORM_RUNTIME_CONFLICT"));
        }
        None
    };
    let project = format!("exhibitos-{local_id}");
    let mut compose: serde_json::Value = serde_json::from_slice(
        &fs::read(source.join("compose.yaml"))
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?,
    )
    .map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    if compose["name"] != manifest.project_name {
        return Err(failure("PLATFORM_RUNTIME_INVALID"));
    }
    fn rebind(
        value: &mut serde_json::Value,
        old_project: &str,
        old_id: &str,
        new_project: &str,
        new_id: &str,
    ) {
        match value {
            serde_json::Value::String(v) if v == old_project => *v = new_project.into(),
            serde_json::Value::String(v) if v == old_id => *v = new_id.into(),
            serde_json::Value::Array(values) => {
                for value in values {
                    rebind(value, old_project, old_id, new_project, new_id)
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rebind(value, old_project, old_id, new_project, new_id)
                }
            }
            _ => {}
        }
    }
    rebind(
        &mut compose,
        &manifest.project_name,
        &manifest.bundle_id,
        &project,
        local_id,
    );
    if let Some(policy) = &network {
        let default = compose["networks"]["default"]
            .as_object_mut()
            .ok_or_else(|| failure("PLATFORM_RUNTIME_INVALID"))?;
        if default.contains_key("ipam") {
            return Err(failure("PLATFORM_RUNTIME_INVALID"));
        }
        default.insert(
            "ipam".into(),
            serde_json::json!({"config":[{"subnet":policy.subnet,"gateway":policy.gateway}]}),
        );
    }
    let compose = serde_json::to_vec(&compose).map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    manifest.project_name = project;
    manifest.bundle_id = local_id.into();
    manifest.explicit_local_network = network;
    manifest.compose_sha256 = digest(&compose);
    super::local_network::bind_compose(&manifest, &compose)?;
    let manifest =
        serde_json::to_vec(&manifest).map_err(|_| failure("PLATFORM_RUNTIME_INVALID"))?;
    let derived = BTreeMap::from([
        ("manifest.json".to_string(), manifest),
        ("compose.yaml".to_string(), compose),
    ]);
    let mut files = descriptor.files.clone();
    for (name, bytes) in &derived {
        files.insert(
            name.clone(),
            Pin {
                bytes: bytes.len() as u64,
                sha256: digest(bytes),
            },
        );
    }
    let stage = root.join(".platform-runtime-preparing");
    match fs::create_dir(&stage) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
                    .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => directory(&stage)?,
        Err(_) => return Err(failure("PLATFORM_RUNTIME_UNAVAILABLE")),
    }
    // A completed file can be reused after interruption. Unknown or changed bytes are preserved and refused.
    for entry in fs::read_dir(&stage).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))? {
        let entry = entry.map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        if !files.contains_key(&entry.file_name().to_string_lossy().to_string()) {
            return Err(failure("PLATFORM_RUNTIME_CONFLICT"));
        }
    }
    for (name, pin) in &files {
        let target = stage.join(name);
        let existing = match fs::symlink_metadata(&target) {
            Ok(_) => Some(regular(&target)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(failure("PLATFORM_RUNTIME_UNAVAILABLE")),
        };
        if existing.as_ref().is_some_and(|m| m.len() == pin.bytes) {
            pin_file(&target, pin)?;
            continue;
        }
        let mut input: Box<dyn Read> = if let Some(bytes) = derived.get(name) {
            Box::new(Cursor::new(bytes))
        } else {
            Box::new(
                File::open(source.join(name))
                    .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?,
            )
        };
        let mut offset = 0u64;
        if let Some(m) = existing {
            if m.len() > pin.bytes {
                return Err(failure("PLATFORM_RUNTIME_CHANGED"));
            }
            let mut prior =
                File::open(&target).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
            let mut a = [0; 65536];
            let mut b = [0; 65536];
            while offset < m.len() {
                let n = (m.len() - offset).min(a.len() as u64) as usize;
                prior
                    .read_exact(&mut a[..n])
                    .map_err(|_| failure("PLATFORM_RUNTIME_CHANGED"))?;
                input
                    .read_exact(&mut b[..n])
                    .map_err(|_| failure("PLATFORM_RUNTIME_CHANGED"))?;
                if a[..n] != b[..n] {
                    return Err(failure("PLATFORM_RUNTIME_CHANGED"));
                }
                offset += n as u64;
            }
        }
        let mut options = fs::OpenOptions::new();
        options.write(true);
        if offset == 0 && !target.exists() {
            options.create_new(true);
        } else {
            options.append(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options
            .open(&target)
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        let mut buffer = [0; 65536];
        let mut count = offset;
        loop {
            let n = input
                .read(&mut buffer)
                .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > pin.bytes {
                return Err(failure("PLATFORM_RUNTIME_CHANGED"));
            }
            output
                .write_all(&buffer[..n])
                .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        }
        output
            .sync_all()
            .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
        pin_file(&target, pin)?;
        pin_file(&source.join(name), &descriptor.files[name])?;
    }
    if fs::symlink_metadata(root.join("bundle")).is_ok() {
        return Err(failure("PLATFORM_RUNTIME_CONFLICT"));
    }
    File::open(&stage)
        .and_then(|f| f.sync_all())
        .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    fs::rename(&stage, root.join("bundle")).map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| failure("PLATFORM_RUNTIME_UNAVAILABLE"))?;
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, PathBuf) {
        let parent =
            std::env::temp_dir().join(format!("exhibitos-runtime-prepare-{}", Uuid::new_v4()));
        let root = parent.join("selected");
        let source = parent.join("resource");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&source).unwrap();
        let compose = br#"{"name":"exhibitos-synthetic","services":{"platform":{"labels":{"com.exhibitos.bundle":"synthetic","com.exhibitos.project":"exhibitos-synthetic"}}},"volumes":{"objects":{"labels":{"com.exhibitos.bundle":"synthetic","com.exhibitos.project":"exhibitos-synthetic"}}},"networks":{}}"#;
        let manifest = serde_json::json!({"schemaVersion":"1.0.0-draft.1","bundleId":"synthetic","version":"0.1.0","protocolVersion":"1","composeSha256":digest(compose),"projectName":"exhibitos-synthetic","services":["platform"],"images":[{"reference":format!("sha256:{}","a".repeat(64))}],"ports":[13200],"openUrl":"http://127.0.0.1:13200","readinessUrl":"http://127.0.0.1:13200/api/v1/readiness","minimumFreeBytes":1});
        let manifest = serde_json::to_vec(&manifest).unwrap();
        fs::write(source.join("manifest.json"), &manifest).unwrap();
        fs::write(source.join("compose.yaml"), compose).unwrap();
        let architecture = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        let receipt = serde_json::json!({"format":"exhibitos-manager-platform-runtime-v1","version":"0.1.0","architecture":architecture,"platformCommit":"a".repeat(40),"files":{"manifest.json":{"bytes":manifest.len(),"sha256":digest(&manifest)},"compose.yaml":{"bytes":compose.len(),"sha256":digest(compose)}}});
        fs::write(
            source.join("prepared.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        (root, source)
    }
    fn docker_fixture() -> (PathBuf, PathBuf) {
        let (root, source) = fixture();
        let mut manifest: BundleManifest =
            serde_json::from_slice(&fs::read(source.join("manifest.json")).unwrap()).unwrap();
        manifest.preferred_engine = Some("docker".into());
        let mut compose: Value =
            serde_json::from_slice(&fs::read(source.join("compose.yaml")).unwrap()).unwrap();
        compose["networks"]["default"] = serde_json::json!({"labels":{"com.exhibitos.bundle":manifest.bundle_id,"com.exhibitos.project":manifest.project_name,"com.exhibitos.schema":manifest.schema_version}});
        let bytes = serde_json::to_vec(&compose).unwrap();
        manifest.compose_sha256 = digest(&bytes);
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        fs::write(source.join("compose.yaml"), &bytes).unwrap();
        fs::write(source.join("manifest.json"), &manifest_bytes).unwrap();
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(source.join("prepared.json")).unwrap()).unwrap();
        for (name, payload) in [("compose.yaml", bytes), ("manifest.json", manifest_bytes)] {
            receipt["files"][name] =
                serde_json::json!({"bytes":payload.len(),"sha256":digest(&payload)});
        }
        fs::write(
            source.join("prepared.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        (root, source)
    }
    fn synthetic_network() -> ExplicitLocalNetwork {
        ExplicitLocalNetwork {
            mode: "explicit-rfc1918-v1".into(),
            subnet: "10.240.0.0/28".into(),
            gateway: "10.240.0.1".into(),
        }
    }
    #[test]
    fn selected_docker_network_matches_existing_manifest_and_compose_contract() {
        let (root, source) = docker_fixture();
        let before = fs::read(source.join("manifest.json")).unwrap();
        prepare_with_network(&root, &source, || Ok(synthetic_network())).unwrap();
        let manifest: BundleManifest =
            serde_json::from_slice(&fs::read(root.join("bundle/manifest.json")).unwrap()).unwrap();
        let compose = fs::read(root.join("bundle/compose.yaml")).unwrap();
        assert_eq!(
            manifest.explicit_local_network.as_ref().unwrap().gateway,
            "10.240.0.1"
        );
        super::super::local_network::bind_compose(&manifest, &compose).unwrap();
        assert_eq!(digest(&compose), manifest.compose_sha256);
        assert_eq!(fs::read(source.join("manifest.json")).unwrap(), before);
        cleanup(&root);
    }
    #[test]
    fn interrupted_preparation_reuses_selected_network_without_rechoosing() {
        let (root, source) = docker_fixture();
        let stage = root.join(".platform-runtime-preparing");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("compose.yaml"), b"changed").unwrap();
        assert_eq!(
            prepare_with_network(&root, &source, || Ok(synthetic_network()))
                .unwrap_err()
                .code,
            "PLATFORM_RUNTIME_CHANGED"
        );
        let before = fs::read(root.join(".platform-runtime-selection.json")).unwrap();
        fs::write(stage.join("compose.yaml"), b"{\"name\"").unwrap();
        prepare_with_network(&root, &source, || {
            panic!("must preserve the selected network on retry")
        })
        .unwrap();
        assert_eq!(
            fs::read(root.join(".platform-runtime-selection.json")).unwrap(),
            before
        );
        cleanup(&root);
    }
    #[test]
    fn refused_allocator_never_publishes_or_changes_the_source() {
        let (root, source) = docker_fixture();
        let before = fs::read(source.join("manifest.json")).unwrap();
        assert_eq!(
            prepare_with_network(&root, &source, || Err(err("ENGINE_NETWORK_CAPACITY")))
                .unwrap_err()
                .code,
            "ENGINE_NETWORK_CAPACITY"
        );
        assert!(!root.join("bundle").exists());
        assert!(!root.join(".platform-runtime-selection.json").exists());
        assert_eq!(fs::read(source.join("manifest.json")).unwrap(), before);
        cleanup(&root);
    }
    fn cleanup(root: &Path) {
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    #[test]
    fn publishes_complete_bundle_without_changing_sources() {
        let (root, source) = fixture();
        let before = fs::read(source.join("manifest.json")).unwrap();
        prepare(&root, &source).unwrap();
        let bound: BundleManifest =
            serde_json::from_slice(&fs::read(root.join("bundle/manifest.json")).unwrap()).unwrap();
        assert!(bound.project_name.starts_with("exhibitos-"));
        assert_ne!(bound.bundle_id, "synthetic");
        assert_eq!(fs::read(source.join("manifest.json")).unwrap(), before);
        cleanup(&root);
    }
    #[test]
    fn separate_installations_get_distinct_project_and_volume_labels() {
        let (root, source) = fixture();
        let other = root.parent().unwrap().join("second");
        fs::create_dir(&other).unwrap();
        prepare(&root, &source).unwrap();
        prepare(&other, &source).unwrap();
        let first: BundleManifest =
            serde_json::from_slice(&fs::read(root.join("bundle/manifest.json")).unwrap()).unwrap();
        let second: BundleManifest =
            serde_json::from_slice(&fs::read(other.join("bundle/manifest.json")).unwrap()).unwrap();
        assert_ne!(first.project_name, second.project_name);
        assert_ne!(first.bundle_id, second.bundle_id);
        let compose: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("bundle/compose.yaml")).unwrap()).unwrap();
        assert_eq!(
            compose["volumes"]["objects"]["labels"]["com.exhibitos.bundle"],
            first.bundle_id
        );
        assert_eq!(
            compose["services"]["platform"]["labels"]["com.exhibitos.project"],
            first.project_name
        );
        cleanup(&root);
    }
    #[test]
    fn existing_user_bundle_is_never_replaced() {
        let (root, source) = fixture();
        fs::create_dir(root.join("bundle")).unwrap();
        fs::write(root.join("bundle/user-data"), b"keep").unwrap();
        prepare(&root, &source).unwrap();
        assert_eq!(fs::read(root.join("bundle/user-data")).unwrap(), b"keep");
        assert!(!root.join("bundle/manifest.json").exists());
        cleanup(&root);
    }
    #[test]
    fn exact_interrupted_prefix_is_resumed() {
        let (root, source) = fixture();
        let stage = root.join(".platform-runtime-preparing");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("compose.yaml"), b"{\"name\"").unwrap();
        prepare(&root, &source).unwrap();
        let compose: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("bundle/compose.yaml")).unwrap()).unwrap();
        assert!(compose["name"].as_str().unwrap().starts_with("exhibitos-"));
        cleanup(&root);
    }
    #[test]
    fn changed_interrupted_prefix_is_retained_and_refused() {
        let (root, source) = fixture();
        let stage = root.join(".platform-runtime-preparing");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("compose.yaml"), b"changed").unwrap();
        assert_eq!(
            prepare(&root, &source).unwrap_err().code,
            "PLATFORM_RUNTIME_CHANGED"
        );
        assert!(!root.join("bundle").exists());
        assert_eq!(fs::read(stage.join("compose.yaml")).unwrap(), b"changed");
        cleanup(&root);
    }
    #[test]
    fn existing_uninstalled_data_refuses_preparation() {
        let (root, source) = fixture();
        fs::write(root.join("runtime.env"), b"synthetic-preserve").unwrap();
        assert_eq!(
            prepare(&root, &source).unwrap_err().code,
            "PLATFORM_RUNTIME_EXISTING_DATA"
        );
        assert_eq!(
            fs::read(root.join("runtime.env")).unwrap(),
            b"synthetic-preserve"
        );
        assert!(!root.join("bundle").exists());
        cleanup(&root);
    }
    #[test]
    fn unavailable_resources_never_create_placeholder_bundle() {
        let (root, source) = fixture();
        assert_eq!(
            prepare(&root, &source.join("missing")).unwrap_err().code,
            "PLATFORM_RUNTIME_UNAVAILABLE"
        );
        assert!(!root.join("bundle").exists());
        cleanup(&root);
    }
}
