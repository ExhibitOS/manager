// SPDX-License-Identifier: Apache-2.0
//! Fresh-install recovery from an authenticated archive. Never overwrites an installation.
use super::backup_creation::{hash_file, inspected, local_image};
use super::installation_backup::{environment_valid, source_bytes};
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestorationJob {
    pub id: String,
    pub state: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestorationReceipt {
    pub id: String,
    pub operation: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub bundle_id: String,
    pub project_name: String,
    pub open_url: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_verification: Option<RestorationProof>,
}
#[path = "restoration_binding.rs"]
mod binding;
pub(crate) use binding::RestorationBinding;
pub use binding::RestorationProof;
struct RestorationRoute<'a> {
    retry: Option<super::retry::RetryLink<'a>>,
    binding: Option<&'a RestorationBinding>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreservedImage {
    pub(crate) reference: String,
    pub(crate) content_id: String,
    pub(crate) archive: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Authentication {
    operation: String,
    files: u64,
    backup_id: String,
    manifest_sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorationContext {
    pub fresh: bool,
    pub job: Option<RestorationJob>,
    pub receipt: Option<RestorationReceipt>,
}
struct Helper<'a> {
    id: &'a str,
    image: &'a str,
    key: &'a Path,
    source: &'a Path,
    workspace: &'a Path,
    network: &'a str,
    extra: &'a [String],
    script: &'a str,
}
const AUTHENTICATE: &str = r#"
import {main} from './scripts/service-backup.mjs';
import {createHash} from 'node:crypto';
import {readFile,mkdir,copyFile} from 'node:fs/promises';
const v=await main(['verify','--key-file','/key','--source','/archive','--destination','/work/authenticated']);
const bytes=await readFile('/work/authenticated/manifest.json'),manifest=JSON.parse(bytes);
await mkdir('/work/configuration',{mode:0o700});await mkdir('/work/deployment',{mode:0o700});await mkdir('/work/deployment/images',{mode:0o700});
const configuration=['manager-bundle-manifest.json','manager-compose.yaml','manager-runtime.env','manager-installed.json','manager-engine.json','manager-image-inventory.json','freeze-signing-key.json'];
for(const f of manifest.files){
 if(f.role==='runtime')throw Error('RESTORE_LAYOUT_UNSUPPORTED');
 if(!['configuration','deployment'].includes(f.role))continue;
 if(!/^files\/[0-9]{8}\.gcm$/.test(f.path))throw Error('RESTORE_LAYOUT_UNSUPPORTED');
 if(f.role==='configuration'&&!configuration.includes(f.name)||f.role==='deployment'&&!/^images\/image-[01]\.tar$/.test(f.name))throw Error('RESTORE_LAYOUT_UNSUPPORTED');
 await copyFile('/work/authenticated/'+f.path.slice(6)+'.plain','/work/'+f.role+'/'+f.name,1);
}
console.log(JSON.stringify({operation:'authenticated-installation',files:v.files,backupId:manifest.id,manifestSha256:createHash('sha256').update(bytes).digest('hex')}));
"#;
const RESTORE: &str = r#"
import {main} from './scripts/service-backup.mjs';
import {readdir,lstat,copyFile,chmod,chown,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
if((await readdir('/data/config')).length)throw Error('RESTORE_CONFIGURATION_NOT_EMPTY');
const result=await main(['restore','--key-file','/key','--source','/archive','--destination','/work/restored','--quiesced','--fresh-destination']);
if(result.operation!=='restored-and-verified')throw Error('RESTORE_RESULT_INVALID');
const current=await readFile('/work/restored/manifest.json'),previous=await readFile('/work/authenticated/manifest.json');
const manifestSha256=createHash('sha256').update(current).digest('hex');
if(manifestSha256!==createHash('sha256').update(previous).digest('hex'))throw Error('RESTORE_SOURCE_CHANGED');
const backupId=JSON.parse(current).id;
await copyFile('/work/restored/configuration/freeze-signing-key.json','/data/config/freeze-signing-key.json',1);
async function own(path){const s=await lstat(path);if(s.isSymbolicLink()||!s.isDirectory()&&!s.isFile())throw Error('RESTORE_PATH_INVALID');if(s.isDirectory())for(const name of await readdir(path))await own(path+'/'+name);const mode=s.isDirectory()?0o700:0o600;if((s.mode&0o7777)!==mode)await chmod(path,mode);if(s.uid!==1000||s.gid!==1000)await chown(path,1000,1000)}
await own('/data/blobs');await own('/data/config');
console.log(JSON.stringify({operation:'restored-and-verified',manifestSha256,backupId}));
"#;
/// Keeps the newly created candidate namespace pinned throughout its enclosing stage.
pub(crate) struct StageDirectory {
    #[cfg(windows)]
    _native: super::windows_private::PrivateDirectory,
}
pub(crate) fn directory(path: &Path) -> Result<StageDirectory> {
    #[cfg(windows)]
    {
        Ok(StageDirectory {
            _native: super::windows_private::PrivateDirectory::create(path)?,
        })
    }
    #[cfg(not(windows))]
    {
        fs::create_dir(path).map_err(|_| err("STATE_UNAVAILABLE"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        Ok(StageDirectory {})
    }
}
pub(crate) fn private_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(windows)]
    {
        let parent = path.parent().ok_or_else(|| err("RESTORE_PATH_INVALID"))?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| err("RESTORE_PATH_INVALID"))?;
        write_private_new(parent, name, bytes, 16 * 1024 * 1024)
    }
    #[cfg(not(windows))]
    {
        let mut file = private_options()
            .open(path)
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))
    }
}
pub(crate) fn fresh_root(root: &Path) -> Result<()> {
    #[cfg(windows)]
    let directory = super::windows_private::PrivateDirectory::inspect(root)?;
    for item in fs::read_dir(root).map_err(|_| err("STATE_UNAVAILABLE"))? {
        let item = item.map_err(|_| err("STATE_UNAVAILABLE"))?;
        if item.file_name() != "operation.lock" {
            return Err(err("RESTORE_FRESH_ROOT_REQUIRED"));
        }
        #[cfg(windows)]
        directory.check_existing_record("operation.lock")?;
    }
    #[cfg(windows)]
    directory.check()?;
    Ok(())
}
pub(crate) fn preserved_images(
    bytes: &[u8],
    original: &BundleManifest,
) -> Result<Vec<PreservedImage>> {
    let images: Vec<PreservedImage> =
        serde_json::from_slice(bytes).map_err(|_| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    if images.len() != 2
        || images
            .iter()
            .map(|i| i.bytes)
            .try_fold(0u64, u64::checked_add)
            .is_none_or(|n| n > 2 * 1024 * 1024 * 1024)
        || original.images.len() != 2
        || images.iter().enumerate().any(|(index, image)| {
            image.reference != original.images[index].reference
                || !image
                    .content_id
                    .strip_prefix("sha256:")
                    .is_some_and(hash_valid)
                || image.archive != format!("images/image-{index}.tar")
                || image.bytes == 0
                || image.bytes > 2 * 1024 * 1024 * 1024
                || !hash_valid(&image.sha256)
        })
    {
        return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
    }
    Ok(images)
}
pub(crate) fn remapped_environment(
    bytes: &[u8],
    original: &BundleManifest,
    port: u16,
) -> Result<Vec<u8>> {
    environment_valid(bytes, original)?;
    if port < 1024 {
        return Err(err("BACKUP_PATH_INVALID"));
    }
    let original_text =
        std::str::from_utf8(bytes).map_err(|_| err("BACKUP_CONFIGURATION_INVALID"))?;
    let password = original_text
        .lines()
        .find_map(|line| line.strip_prefix("POSTGRES_PASSWORD="))
        .ok_or_else(|| err("BACKUP_CONFIGURATION_INVALID"))?;
    if !password
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(err("BACKUP_CONFIGURATION_INVALID"));
    }
    Ok(original_text
        .lines()
        .map(|line| {
            if line.starts_with("EXHIBITOS_PORT=") {
                format!("EXHIBITOS_PORT={port}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect::<String>()
        .into_bytes())
}
fn validate_source_layout(config: &Value, bytes: &[u8], m: &BundleManifest) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| err("BACKUP_CONFIGURATION_INVALID"))?;
    let mut expected = BTreeMap::<String, String>::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| err("BACKUP_CONFIGURATION_INVALID"))?;
        expected.insert(key.into(), value.into());
    }
    for (key, value) in [
        ("NODE_ENV", "production".to_string()),
        ("AUTH_ORIGIN", format!("http://127.0.0.1:{}", m.ports[0])),
        ("BLOB_ROOT", "/data/blobs".to_string()),
        ("CONFIG_ROOT", "/data/config".to_string()),
    ] {
        expected.insert(key.into(), value);
    }
    let expected = serde_json::to_value(expected).map_err(|_| err("STATE_INVALID"))?;
    if m.ports.len() != 1
        || config["services"]["platform"]["environment"] != expected
        || config["services"]["database"]["environment"]
            != serde_json::json!({"POSTGRES_DB":"exhibitos","POSTGRES_USER":"exhibitos","POSTGRES_PASSWORD":expected["POSTGRES_PASSWORD"]})
    {
        return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
    }
    for (service, targets) in [
        ("database", vec!["/var/lib/postgresql"]),
        ("platform", vec!["/data/blobs", "/data/config"]),
    ] {
        let value = &config["services"][service];
        for field in [
            "command",
            "entrypoint",
            "user",
            "extra_hosts",
            "dns",
            "devices",
            "configs",
            "secrets",
            "network_mode",
            "pid",
            "ipc",
            "cap_add",
            "sysctls",
            "gpus",
        ] {
            if value.get(field).is_some_and(|v| !v.is_null()) {
                return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
            }
        }
        let volumes = value["volumes"]
            .as_array()
            .ok_or_else(|| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
        if volumes.len() != targets.len()
            || targets.iter().any(|target| {
                volumes
                    .iter()
                    .filter(|v| {
                        v["target"] == *target && v["type"] == "volume" && v["read_only"] != true
                    })
                    .count()
                    != 1
            })
        {
            return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
        }
    }
    if config["volumes"].as_object().is_none_or(|v| v.len() != 3)
        || config["networks"]
            .as_object()
            .is_none_or(|v| v.len() != 1 || !v.contains_key("default"))
    {
        return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
    }
    Ok(())
}
fn labels(m: &BundleManifest) -> Value {
    serde_json::json!({"com.exhibitos.bundle":m.bundle_id,"com.exhibitos.project":m.project_name,"com.exhibitos.schema":m.schema_version})
}
fn compose(m: &BundleManifest, database: &str, platform: &str) -> Value {
    let labels = labels(m);
    serde_json::json!({"services":{
        "database":{"image":database,"pull_policy":"never","environment":{"POSTGRES_PASSWORD":"${POSTGRES_PASSWORD:?required}","POSTGRES_DB":"exhibitos","POSTGRES_USER":"exhibitos"},"volumes":["database:/var/lib/postgresql"],"labels":labels,"healthcheck":{"test":["CMD-SHELL","pg_isready -U exhibitos -d exhibitos"],"interval":"2s","timeout":"3s","retries":30},"restart":"unless-stopped"},
        "platform":{"image":platform,"pull_policy":"never","user":"1000:1000","env_file":["../runtime.env"],"environment":{"NODE_ENV":"production","EXHIBITOS_PORT":"${EXHIBITOS_PORT}","AUTH_ORIGIN":"http://127.0.0.1:${EXHIBITOS_PORT}","BLOB_ROOT":"/data/blobs","CONFIG_ROOT":"/data/config"},"ports":["127.0.0.1:${EXHIBITOS_PORT}:8080"],"volumes":["objects:/data/blobs","configuration:/data/config"],"labels":labels,"depends_on":{"database":{"condition":"service_healthy"}},"read_only":true,"tmpfs":["/tmp:rw,nosuid,nodev,size=256m,mode=1777"],"cap_drop":["ALL"],"security_opt":["no-new-privileges:true"],"restart":"unless-stopped","stop_grace_period":"30s"}},
        "volumes":{"database":{"labels":labels},"objects":{"labels":labels},"configuration":{"labels":labels}},"networks":{"default":{"labels":labels}}})
}
/// Exact supported restoration remapping, shared by writer and fresh verifier.
pub(crate) fn remapped_bundle(
    original: &BundleManifest,
    images: &[PreservedImage],
    bundle_id: &str,
    port: u16,
    database: &str,
    platform: &str,
) -> Result<(BundleManifest, Vec<u8>)> {
    if Uuid::parse_str(bundle_id).is_err()
        || port < 1024
        || images.len() != 2
        || database == platform
        || [database, platform]
            .iter()
            .any(|id| images.iter().filter(|i| i.content_id == **id).count() != 1)
    {
        return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
    }
    let mut manifest = original.clone();
    manifest.bundle_id = bundle_id.into();
    manifest.project_name = format!("exhibitos-{bundle_id}");
    manifest.ports = vec![port];
    manifest.open_url = format!("http://127.0.0.1:{port}");
    manifest.readiness_url = format!("{}/api/v1/readiness", manifest.open_url);
    manifest.preferred_engine = Some("docker".into());
    manifest.images = images
        .iter()
        .enumerate()
        .map(|(i, image)| Image {
            reference: image.content_id.clone(),
            archive: Some(Archive {
                path: format!("image-{i}.tar"),
                bytes: image.bytes,
                sha256: image.sha256.clone(),
            }),
        })
        .collect();
    let encoded = serde_json::to_vec(&compose(&manifest, database, platform))
        .map_err(|_| err("STATE_INVALID"))?;
    manifest.compose_sha256 = digest(&encoded);
    Ok((manifest, encoded))
}
impl LifecycleService {
    pub(crate) fn recover_restoration(&self) -> Result<()> {
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        if let Some(mut job) = self.restoration_status()?
            && job.state == "running"
        {
            job.state = "interrupted".into();
            job.error_code = Some("INTERRUPTED".into());
            job.updated_at = now();
            write_json(&self.root, "restoration.json", &job)?;
        }
        Ok(())
    }
    /// Read one coherent candidate snapshot; never creates or overwrites a candidate.
    pub fn restoration_context(&self) -> Result<RestorationContext> {
        let _lock = self.lock()?;
        let job = self.restoration_status()?;
        let fresh = match fresh_root(&self.root) {
            Ok(()) => true,
            Err(error) if error.code == "RESTORE_FRESH_ROOT_REQUIRED" => false,
            Err(error) => return Err(error),
        };
        let receipt = match &job {
            Some(job) if job.state == "completed" => Some(read_json(&checked_path(
                &self.root,
                &format!("restore-{}/receipt.json", job.id),
            )?)?),
            _ => None,
        };
        Ok(RestorationContext {
            fresh,
            job,
            receipt,
        })
    }
    pub fn restoration_status(&self) -> Result<Option<RestorationJob>> {
        let path = self.root.join("restoration.json");
        if !path.exists() {
            return Ok(None);
        }
        let job: RestorationJob = read_json(&checked_path(&self.root, "restoration.json")?)?;
        if Uuid::parse_str(&job.id).is_err()
            || !["running", "completed", "failed", "interrupted"].contains(&job.state.as_str())
            || job.stage.is_empty()
            || job.stage.len() > 64
        {
            return Err(err("STATE_INVALID"));
        }
        if job.state == "completed" {
            let receipt: RestorationReceipt = read_json(&checked_path(
                &self.root,
                &format!("restore-{}/receipt.json", job.id),
            )?)?;
            let manifest = self.manifest()?;
            if job.stage != "complete"
                || job.error_code.is_some()
                || receipt.id != job.id
                || receipt.operation != "restored-and-running"
                || Uuid::parse_str(&receipt.backup_id).is_err()
                || !hash_valid(&receipt.authenticated_manifest_sha256)
                || receipt
                    .source_verification
                    .as_ref()
                    .is_some_and(|p| !p.valid())
                || receipt.bundle_id != manifest.bundle_id
                || receipt.project_name != manifest.project_name
                || receipt.open_url != manifest.open_url
            {
                return Err(err("STATE_INVALID"));
            }
        }
        Ok(Some(job))
    }
    pub(crate) fn ensure_restoration_complete(&self, action: &Action) -> Result<()> {
        if *action != Action::Stop
            && self
                .restoration_status()?
                .is_some_and(|job| job.state != "completed")
        {
            return Err(err("RESTORE_RECOVERY_REQUIRED"));
        }
        Ok(())
    }
    fn restoration_helper(&self, request: Helper<'_>) -> Result<Vec<u8>> {
        let Helper {
            id,
            image,
            key,
            source,
            workspace,
            network,
            extra,
            script,
        } = request;
        let auxiliary =
            self.prepare_restoration_auxiliary(id, image, |args| run("docker", args, None, 60))?;
        let container = format!("exhibitos-restore-{id}");
        let mut args = vec![
            "run".into(),
            "--rm".into(),
            "--pull".into(),
            "never".into(),
            "--name".into(),
            container.clone(),
            "--label".into(),
            format!("com.exhibitos.restoration={id}"),
            "--network".into(),
            network.into(),
            "--read-only".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges:true".into(),
            "--pids-limit".into(),
            "128".into(),
            "--memory".into(),
            "1536m".into(),
            "--tmpfs".into(),
            "/tmp:rw,nosuid,nodev,size=256m,mode=1777".into(),
        ];
        if network == "none" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = fs::metadata(workspace).map_err(|_| err("STATE_UNAVAILABLE"))?;
                args.extend(["--user".into(), format!("{}:{}", meta.uid(), meta.gid())]);
            }
        } else {
            args.extend([
                "--user".into(),
                "0:0".into(),
                "--cap-add".into(),
                "DAC_OVERRIDE".into(),
                "--cap-add".into(),
                "CHOWN".into(),
            ]);
        }
        for mount in [
            format!("type=bind,source={},target=/key,readonly", key.display()),
            format!(
                "type=bind,source={},target=/archive,readonly",
                source.display()
            ),
            format!("type=bind,source={},target=/work", workspace.display()),
        ] {
            args.extend(["--mount".into(), mount]);
        }
        if let Some(volume) = auxiliary {
            args.extend([
                "--mount".into(),
                format!("type=volume,source={volume},target=/var/lib/postgresql,volume-nocopy"),
            ]);
        }
        args.extend_from_slice(extra);
        args.extend([
            "--entrypoint".into(),
            "node".into(),
            image.into(),
            "--input-type=module".into(),
            "-e".into(),
            script.into(),
        ]);
        // A client error/timeout does not prove the daemon helper is stopped. Preserve it
        // for exact-owned reconciliation; named auxiliary storage survives --rm exits.
        self.run_maintenance_helper("restoration", id, &args)
    }
    /// Explicitly acknowledged fresh Docker target only. Original archive/key remain read-only.
    pub fn restore_backup(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
        port: u16,
        acknowledged: bool,
    ) -> Result<RestorationReceipt> {
        let _lock = self.lock()?;
        self.restore_backup_locked(image, key, source, port, acknowledged, None)
    }
    pub(crate) fn restore_backup_locked(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
        port: u16,
        acknowledged: bool,
        retry: Option<super::retry::RetryLink<'_>>,
    ) -> Result<RestorationReceipt> {
        self.restore_backup_routed(
            image,
            key,
            source,
            port,
            acknowledged,
            RestorationRoute {
                retry,
                binding: None,
            },
        )
    }
    pub(crate) fn restore_update_candidate(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
        port: u16,
        binding: &RestorationBinding,
    ) -> Result<RestorationReceipt> {
        let lock = self.lock()?;
        self.restore_update_candidate_locked(image, key, source, port, binding, &lock)
    }
    /// The caller retains this exact candidate operation guard through its final
    /// profile/authority checks. This never adopts a caller verification receipt.
    pub(crate) fn restore_update_candidate_locked(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
        port: u16,
        binding: &RestorationBinding,
        guard: &OperationGuard,
    ) -> Result<RestorationReceipt> {
        binding.validate()?;
        self.check_restoration_guard(guard)?;
        let result = self.restore_backup_routed(
            image,
            key,
            source,
            port,
            true,
            RestorationRoute {
                retry: None,
                binding: Some(binding),
            },
        );
        self.check_restoration_guard(guard)?;
        result
    }
    pub(crate) fn check_restoration_guard(&self, guard: &OperationGuard) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::symlink_metadata(self.root.join("operation.lock"))
                .map_err(|_| err("UPDATE_FENCE_UNAVAILABLE"))?;
            let held = guard
                .metadata()
                .map_err(|_| err("UPDATE_FENCE_UNAVAILABLE"))?;
            if !current.is_file()
                || current.file_type().is_symlink()
                || current.uid() != unsafe { libc::geteuid() }
                || current.nlink() != 1
                || current.mode() & 0o777 != 0o600
                || (current.dev(), current.ino()) != (held.dev(), held.ino())
            {
                return Err(err("UPDATE_FENCE_UNAVAILABLE"));
            }
        }
        #[cfg(windows)]
        self.root_guard.check_record(guard, "operation.lock")?;
        #[cfg(not(any(unix, windows)))]
        {
            let _ = guard;
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        Ok(())
    }
    fn restore_backup_routed(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
        port: u16,
        acknowledged: bool,
        route: RestorationRoute<'_>,
    ) -> Result<RestorationReceipt> {
        let RestorationRoute { retry, binding } = route;
        if cfg!(windows) {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        if !acknowledged {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        maintenance::input_path(&self.root, true)?;
        maintenance::input_path(key, false)?;
        maintenance::input_path(source, true)?;
        if key.starts_with(source)
            || key.starts_with(&self.root)
            || source.starts_with(&self.root)
            || self.root.starts_with(source)
        {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        fresh_root(&self.root)?;
        // Existing/failed targets are identified before a new-operation space
        // budget; low disk must not disguise the no-overwrite refusal.
        if let Some(binding) = binding
            && fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < binding.required_free_bytes()
        {
            return Err(err("STORAGE_QUOTA"));
        }
        if port < 1024 {
            return Err(err("BACKUP_PATH_INVALID"));
        }
        let reserved_port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .map_err(|_| err("PORT_IN_USE"))?;
        if fs2::available_space(&self.root).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < 2 * 1024 * 1024 * 1024
        {
            return Err(err("STORAGE_QUOTA"));
        }
        let probe = self
            .detect()?
            .into_iter()
            .find(|p| p.kind == "docker" && p.available)
            .ok_or_else(|| err("ENGINE_UNAVAILABLE"))?;
        if probe.kind != "docker" {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        local_image("docker", image)?;
        let id = Uuid::new_v4().to_string();
        if let Some(link) = retry.as_ref() {
            link.reserved(&id)?;
        }
        let workspace = self.root.join(format!("restore-{id}"));
        let _workspace_guard = directory(&workspace)?;
        let mut job = RestorationJob {
            id: id.clone(),
            state: "running".into(),
            stage: "authenticating".into(),
            error_code: None,
            created_at: now(),
            updated_at: now(),
        };
        write_json(&self.root, "restoration.json", &job)?;
        if let Some(link) = retry {
            link.started(&id)?;
        }
        self.begin_maintenance("restoration", &id, &job.stage)?;
        let outcome = (|| -> Result<RestorationReceipt> {
            let bytes = self.restoration_helper(Helper {
                id: &id,
                image,
                key,
                source,
                workspace: &workspace,
                network: "none",
                extra: &[],
                script: AUTHENTICATE,
            })?;
            let authentication: Authentication =
                serde_json::from_slice(&bytes).map_err(|_| err("RESTORE_RESULT_INVALID"))?;
            if authentication.operation != "authenticated-installation"
                || authentication.files == 0
                || authentication.files > 10000
                || Uuid::parse_str(&authentication.backup_id).is_err()
                || !hash_valid(&authentication.manifest_sha256)
            {
                return Err(err("RESTORE_RESULT_INVALID"));
            }
            if hash_file(&workspace.join("authenticated/manifest.json"))?.sha256
                != authentication.manifest_sha256
            {
                return Err(err("RESTORE_RESULT_INVALID"));
            }
            if let Some(binding) = binding {
                binding.authenticated(
                    &authentication.backup_id,
                    &authentication.manifest_sha256,
                    &source_bytes(
                        &workspace.join("authenticated"),
                        "manifest.json",
                        16 * 1024 * 1024,
                        true,
                    )?,
                )?;
            }
            job.stage = "validating-installation".into();
            job.updated_at = now();
            self.maintenance_stage("restoration", &id, &job.stage)?;
            write_json(&self.root, "restoration.json", &job)?;
            let configuration = workspace.join("configuration");
            let original_bytes = source_bytes(
                &configuration,
                "manager-bundle-manifest.json",
                1024 * 1024,
                true,
            )?;
            let original: BundleManifest =
                serde_json::from_slice(&original_bytes).map_err(|_| err("BUNDLE_INVALID"))?;
            let installed: BundleManifest = serde_json::from_slice(&source_bytes(
                &configuration,
                "manager-installed.json",
                1024 * 1024,
                true,
            )?)
            .map_err(|_| err("BUNDLE_INVALID"))?;
            if serde_json::to_value(&original).ok() != serde_json::to_value(&installed).ok()
                || original.services.len() != 2
                || !original.services.iter().any(|s| s == "database")
                || !original.services.iter().any(|s| s == "platform")
            {
                return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
            }
            let original_engine: String = serde_json::from_slice(&source_bytes(
                &configuration,
                "manager-engine.json",
                64,
                true,
            )?)
            .map_err(|_| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
            if original_engine != "docker"
                || original
                    .preferred_engine
                    .as_deref()
                    .is_some_and(|e| e != "docker")
            {
                return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
            }
            let original_env = source_bytes(&configuration, "manager-runtime.env", 8192, true)?;
            let environment = remapped_environment(&original_env, &original, port)?;
            let images = preserved_images(
                &source_bytes(
                    &configuration,
                    "manager-image-inventory.json",
                    1024 * 1024,
                    true,
                )?,
                &original,
            )?;
            source_bytes(&configuration, "freeze-signing-key.json", 1024 * 1024, true)?;
            let source_stage = workspace.join("source-installation");
            let _source_stage_guard = directory(&source_stage)?;
            let _source_bundle_guard = directory(&source_stage.join("bundle"))?;
            private_bytes(&source_stage.join("bundle/manifest.json"), &original_bytes)?;
            private_bytes(
                &source_stage.join("bundle/compose.yaml"),
                &source_bytes(&configuration, "manager-compose.yaml", 256 * 1024, true)?,
            )?;
            private_bytes(&source_stage.join("runtime.env"), &original_env)?;
            let source_service = LifecycleService::bound_existing(source_stage)?;
            let validated = source_service.manifest()?;
            source_service.validate_compose(&validated, "docker")?;
            let old_config: Value = serde_json::from_slice(&run(
                "docker",
                &compose_args(&original, &["config", "--format", "json"]),
                Some(&source_service.root.join("bundle")),
                30,
            )?)
            .map_err(|_| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
            let image_for = |service: &str| -> Result<String> {
                let reference = old_config["services"][service]["image"]
                    .as_str()
                    .ok_or_else(|| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
                images
                    .iter()
                    .find(|i| i.reference == reference)
                    .map(|i| i.content_id.clone())
                    .ok_or_else(|| err("RESTORE_LAYOUT_UNSUPPORTED"))
            };
            validate_source_layout(&old_config, &original_env, &original)?;
            let database = image_for("database")?;
            let platform = image_for("platform")?;
            if database == platform
                || old_config["services"]["database"]["environment"]["POSTGRES_DB"] != "exhibitos"
                || old_config["services"]["database"]["environment"]["POSTGRES_USER"] != "exhibitos"
                || old_config["services"]["platform"]["environment"]["BLOB_ROOT"] != "/data/blobs"
                || old_config["services"]["platform"]["environment"]["CONFIG_ROOT"]
                    != "/data/config"
            {
                return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));
            }
            if let Some(binding) = binding {
                binding.runtime_image(&platform)?;
            }
            let (manifest, encoded) = remapped_bundle(
                &original,
                &images,
                &Uuid::new_v4().to_string(),
                port,
                &database,
                &platform,
            )?;
            let bundle = self.root.join("bundle");
            let _destination_bundle_guard = directory(&bundle)?;
            for (index, preserved) in images.iter().enumerate() {
                self.maintenance_checkpoint("restoration", &id)?;
                let path = checked_path(&workspace.join("deployment"), &preserved.archive)?;
                let hashed = hash_file(&path)?;
                if hashed.bytes != preserved.bytes || hashed.sha256 != preserved.sha256 {
                    return Err(err("IMAGE_INTEGRITY"));
                }
                let filename = format!("image-{index}.tar");
                fs::rename(&path, bundle.join(&filename)).map_err(|_| err("STATE_UNAVAILABLE"))?;
            }
            private_bytes(&bundle.join("compose.yaml"), &encoded)?;
            write_json(&bundle, "manifest.json", &manifest)?;
            private_bytes(&self.root.join("runtime.env"), &environment)?;
            // Reject collisions before creating any volume/network. The new identity is not source identity.
            for (kind, args, names) in [
                (
                    "volume",
                    vec!["volume", "ls", "--format", "{{.Name}}"],
                    vec![
                        format!("{}_database", manifest.project_name),
                        format!("{}_objects", manifest.project_name),
                        format!("{}_configuration", manifest.project_name),
                    ],
                ),
                (
                    "network",
                    vec!["network", "ls", "--format", "{{.Name}}"],
                    vec![format!("{}_default", manifest.project_name)],
                ),
            ] {
                let values = String::from_utf8(run(
                    "docker",
                    &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    None,
                    30,
                )?)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                if values.lines().any(|line| names.iter().any(|n| line == n)) {
                    let _ = kind;
                    return Err(err("OWNERSHIP_CONFLICT"));
                }
            }
            job.stage = "importing-images".into();
            job.updated_at = now();
            self.maintenance_stage("restoration", &id, &job.stage)?;
            write_json(&self.root, "restoration.json", &job)?;
            self.operation(&Action::Install)?;
            for expected in [&database, &platform] {
                local_image("docker", expected)?;
            }
            self.validate_volumes(&manifest, "docker")?;
            self.validate_ownership(&manifest, "docker")?;
            job.stage = "creating-fresh-target".into();
            job.updated_at = now();
            self.maintenance_stage("restoration", &id, &job.stage)?;
            write_json(&self.root, "restoration.json", &job)?;
            run(
                "docker",
                &compose_args(&manifest, &["create", "--no-build", "--pull", "never"]),
                Some(&bundle),
                180,
            )?;
            self.validate_ownership(&manifest, "docker")?;
            self.validate_volumes(&manifest, "docker")?;
            for suffix in ["database", "objects", "configuration"] {
                let volume = inspected(
                    "docker",
                    &[
                        "volume".into(),
                        "inspect".into(),
                        format!("{}_{suffix}", manifest.project_name),
                    ],
                )?;
                if volume["Labels"] != labels(&manifest)
                    && !(volume["Labels"]["com.exhibitos.bundle"] == manifest.bundle_id
                        && volume["Labels"]["com.exhibitos.project"] == manifest.project_name
                        && volume["Labels"]["com.exhibitos.schema"] == manifest.schema_version)
                    || volume["Driver"] != "local"
                    || volume["Options"].as_object().is_some_and(|v| !v.is_empty())
                {
                    return Err(err("OWNERSHIP_CONFLICT"));
                }
            }
            run(
                "docker",
                &compose_args(
                    &manifest,
                    &[
                        "up",
                        "--detach",
                        "--no-build",
                        "--pull",
                        "never",
                        "database",
                    ],
                ),
                Some(&bundle),
                180,
            )?;
            let deadline = Instant::now() + Duration::from_secs(90);
            loop {
                self.maintenance_checkpoint("restoration", &id)?;
                let ids = String::from_utf8(run(
                    "docker",
                    &compose_args(&manifest, &["ps", "--all", "--quiet", "database"]),
                    Some(&bundle),
                    30,
                )?)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                let db = inspected("docker", &["inspect".into(), ids.trim().into()])?;
                if db["State"]["Health"]["Status"] == "healthy" {
                    break;
                }
                if Instant::now() > deadline {
                    return Err(err("READINESS_TIMEOUT"));
                }
                thread::sleep(Duration::from_millis(500));
            }
            let network = format!("{}_default", manifest.project_name);
            let net = inspected(
                "docker",
                &["network".into(), "inspect".into(), network.clone()],
            )?;
            if net["Labels"]["com.exhibitos.bundle"] != manifest.bundle_id
                || net["Labels"]["com.exhibitos.project"] != manifest.project_name
                || net["Labels"]["com.exhibitos.schema"] != manifest.schema_version
                || net["Driver"] != "bridge"
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            job.stage = "restoring-and-verifying".into();
            job.updated_at = now();
            self.maintenance_stage("restoration", &id, &job.stage)?;
            write_json(&self.root, "restoration.json", &job)?;
            let extras = vec![
                "--env-file".into(),
                self.root.join("runtime.env").to_string_lossy().into_owned(),
                "-e".into(),
                "BLOB_ROOT=/data/blobs".into(),
                "--mount".into(),
                format!(
                    "type=volume,source={}_objects,target=/data/blobs",
                    manifest.project_name
                ),
                "--mount".into(),
                format!(
                    "type=volume,source={}_configuration,target=/data/config",
                    manifest.project_name
                ),
            ];
            let restored: Value = serde_json::from_slice(&self.restoration_helper(Helper {
                id: &id,
                image,
                key,
                source,
                workspace: &workspace,
                network: &network,
                extra: &extras,
                script: RESTORE,
            })?)
            .map_err(|_| err("RESTORE_RESULT_INVALID"))?;
            if restored
                != serde_json::json!({"operation":"restored-and-verified","manifestSha256":authentication.manifest_sha256,"backupId":authentication.backup_id})
            {
                return Err(err("RESTORE_RESULT_INVALID"));
            }
            job.stage = "checking-runtime".into();
            job.updated_at = now();
            self.maintenance_stage("restoration", &id, &job.stage)?;
            write_json(&self.root, "restoration.json", &job)?;
            drop(reserved_port);
            self.operation(&Action::Start)?;
            let receipt = RestorationReceipt {
                id: id.clone(),
                operation: "restored-and-running".into(),
                backup_id: authentication.backup_id,
                authenticated_manifest_sha256: authentication.manifest_sha256,
                bundle_id: manifest.bundle_id,
                project_name: manifest.project_name,
                open_url: manifest.open_url,
                at: now(),
                source_verification: binding.map(|b| b.proof().clone()),
            };
            Ok(receipt)
        })();
        let _terminal = self.maintenance_finish_guard("restoration", &id)?;
        let outcome = if self.maintenance_requested("restoration", &id)? {
            if outcome
                .as_ref()
                .err()
                .is_some_and(|e| e.code != "CANCELLED")
            {
                Err(err("CANCEL_UNCERTAIN"))
            } else {
                self.maintenance_checkpoint("restoration", &id).and(outcome)
            }
        } else {
            outcome
        };
        job.updated_at = now();
        match outcome {
            Ok(receipt) => {
                write_json(&workspace, "receipt.json", &receipt)?;
                job.state = "completed".into();
                job.stage = "complete".into();
                write_json(&self.root, "restoration.json", &job)?;
                self.finish_maintenance("completed", None)?;
                Ok(receipt)
            }
            Err(error) => {
                job.state = if error.code == "CANCELLED" {
                    "interrupted"
                } else {
                    "failed"
                }
                .into();
                job.error_code = Some(error.code.clone());
                write_json(&self.root, "restoration.json", &job)?;
                // Pause only this new candidate, after rechecking exact ownership. Preserve all volumes/data.
                if let Ok(m) = self.manifest()
                    && self.validate_ownership(&m, "docker").is_ok()
                {
                    let _ = run(
                        "docker",
                        &compose_args(&m, &["stop", "--timeout", "30"]),
                        Some(&self.root.join("bundle")),
                        180,
                    );
                }
                let state = match error.code.as_str() {
                    "CANCELLED" => "confirmed",
                    "CANCEL_UNCERTAIN" => "uncertain",
                    _ => "failed",
                };
                self.finish_maintenance(state, Some(&error.code))?;
                Err(err(
                    if ["CANCELLED", "CANCEL_UNCERTAIN"].contains(&error.code.as_str()) {
                        &error.code
                    } else {
                        "RESTORE_FAILED"
                    },
                ))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_checks_freshness_and_failure_without_mutating_candidates() {
        let root = std::env::temp_dir().join(format!("restore-context-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        assert!(service.restoration_context().unwrap().fresh);
        private_bytes(
            &service.root.join("operator-data"),
            b"synthetic retained data",
        )
        .unwrap();
        let context = service.restoration_context().unwrap();
        assert!(!context.fresh);
        assert!(context.job.is_none());
        assert!(context.receipt.is_none());
        let job = RestorationJob {
            id: Uuid::new_v4().to_string(),
            state: "failed".into(),
            stage: "restoring-and-verifying".into(),
            error_code: Some("ENGINE_OPERATION_FAILED".into()),
            created_at: 1,
            updated_at: 2,
        };
        write_json(&service.root, "restoration.json", &job).unwrap();
        let before = fs::read(service.root.join("restoration.json")).unwrap();
        let context = service.restoration_context().unwrap();
        assert_eq!(context.job.unwrap().state, "failed");
        assert!(context.receipt.is_none());
        assert_eq!(
            before,
            fs::read(service.root.join("restoration.json")).unwrap()
        );
        assert_eq!(
            fs::read(service.root.join("operator-data")).unwrap(),
            b"synthetic retained data"
        );
        let _lock = service.lock().unwrap();
        assert_eq!(service.restoration_context().err().unwrap().code, "BUSY");
    }
    #[test]
    fn failed_or_interrupted_restore_never_allows_writer_resumption() {
        let root = std::env::temp_dir().join(format!("restore-gate-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        for state in ["running", "failed", "interrupted"] {
            let job = RestorationJob {
                id: Uuid::new_v4().to_string(),
                state: state.into(),
                stage: "restoring".into(),
                error_code: None,
                created_at: 1,
                updated_at: 1,
            };
            write_json(&service.root, "restoration.json", &job).unwrap();
            for action in [
                Action::Install,
                Action::Start,
                Action::Restart,
                Action::Retry,
            ] {
                assert_eq!(
                    service
                        .ensure_restoration_complete(&action)
                        .unwrap_err()
                        .code,
                    "RESTORE_RECOVERY_REQUIRED"
                );
            }
            assert!(service.ensure_restoration_complete(&Action::Stop).is_ok());
        }
        let mut job = service.restoration_status().unwrap().unwrap();
        job.state = "running".into();
        write_json(&service.root, "restoration.json", &job).unwrap();
        let reopened = LifecycleService::new(service.root.clone()).unwrap();
        assert_eq!(
            reopened.restoration_status().unwrap().unwrap().state,
            "interrupted"
        );
    }
    #[test]
    fn existing_installation_and_unacknowledged_input_fail_before_engine_access() {
        let root = std::env::temp_dir().join(format!("restore-fresh-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        assert_eq!(
            service
                .restore_backup("tag", &service.root, &service.root, 13200, false)
                .unwrap_err()
                .code,
            if cfg!(windows) {
                "BACKUP_PLATFORM_UNVERIFIED"
            } else {
                "BACKUP_OPERATOR_ACK_REQUIRED"
            }
        );
        private_bytes(&service.root.join("retained"), b"keep").unwrap();
        assert_eq!(
            fresh_root(&service.root).unwrap_err().code,
            "RESTORE_FRESH_ROOT_REQUIRED"
        );
        assert_eq!(fs::read(service.root.join("retained")).unwrap(), b"keep");
    }
    #[test]
    fn remapping_preserves_accounts_and_rejects_silent_configuration_loss() {
        let mut m = crate::tests::manifest_for_detection();
        m.ports = vec![13200];
        let text = format!(
            "EXHIBITOS_PORT=13200\nPOSTGRES_PASSWORD=synthetic-password\nDATABASE_URL=postgresql://exhibitos:synthetic-password@database:5432/exhibitos\nADMIN_SUBJECT=local-admin\nADMIN_PASSWORD=synthetic-admin-password\nTENANT_ID={}\n",
            Uuid::new_v4()
        );
        let attack = text.replace("synthetic-password", "x@foreign.invalid:5432/other?x=");
        assert!(remapped_environment(attack.as_bytes(), &m, 13201).is_err());
        let mut empty = m.clone();
        empty.ports.clear();
        assert!(remapped_environment(text.as_bytes(), &empty, 13201).is_err());
        let changed = remapped_environment(text.as_bytes(), &m, 13201).unwrap();
        assert_eq!(
            std::str::from_utf8(&changed).unwrap(),
            text.replace("EXHIBITOS_PORT=13200", "EXHIBITOS_PORT=13201")
        );
        assert!(
            remapped_environment(format!("{text}UNKNOWN=keep\n").as_bytes(), &m, 13201).is_err()
        );
        let mut config = compose(&m, "database-image", "platform-image");
        let mut env = BTreeMap::<String, String>::new();
        for line in text.lines() {
            let (k, v) = line.split_once('=').unwrap();
            env.insert(k.into(), v.into());
        }
        for (k, v) in [
            ("NODE_ENV", "production"),
            ("AUTH_ORIGIN", "http://127.0.0.1:13200"),
            ("BLOB_ROOT", "/data/blobs"),
            ("CONFIG_ROOT", "/data/config"),
        ] {
            env.insert(k.into(), v.into());
        }
        config["services"]["platform"]["environment"] = serde_json::to_value(env).unwrap();
        config["services"]["database"]["environment"]["POSTGRES_PASSWORD"] =
            "synthetic-password".into();
        config["services"]["platform"]
            .as_object_mut()
            .unwrap()
            .remove("user");
        // Compose normalization converts mount strings into typed mount descriptions.
        config["services"]["database"]["volumes"] =
            serde_json::json!([{"type":"volume","target":"/var/lib/postgresql"}]);
        config["services"]["platform"]["volumes"] = serde_json::json!([{"type":"volume","target":"/data/blobs"},{"type":"volume","target":"/data/config"}]);
        config["services"]["database"]["command"] = Value::Null;
        config["services"]["platform"]["entrypoint"] = Value::Null;
        assert!(validate_source_layout(&config, text.as_bytes(), &m).is_ok());
        config["services"]["platform"]["entrypoint"] = serde_json::json!(["unexpected-command"]);
        assert!(validate_source_layout(&config, text.as_bytes(), &m).is_err());
        config["services"]["platform"]["entrypoint"] = Value::Null;
        config["services"]["platform"]["environment"]["UNSUPPORTED_CONFIG"] = "preserve".into();
        assert_eq!(
            validate_source_layout(&config, text.as_bytes(), &m)
                .unwrap_err()
                .code,
            "RESTORE_LAYOUT_UNSUPPORTED"
        );
    }
    #[test]
    fn image_identity_inventory_and_completed_receipt_are_not_assumed() {
        let mut m = crate::tests::manifest_for_detection();
        m.images = vec![
            Image {
                reference: format!("sha256:{}", "a".repeat(64)),
                archive: None,
            },
            Image {
                reference: format!("sha256:{}", "b".repeat(64)),
                archive: None,
            },
        ];
        let mut inventory = serde_json::json!([{"reference":m.images[0].reference,"contentId":m.images[0].reference,"archive":"images/image-0.tar","bytes":1,"sha256":"a".repeat(64)},{"reference":m.images[1].reference,"contentId":m.images[1].reference,"archive":"images/image-1.tar","bytes":1,"sha256":"b".repeat(64)}]);
        assert!(preserved_images(&serde_json::to_vec(&inventory).unwrap(), &m).is_ok());
        inventory[0]["archive"] = "../original-data".into();
        assert!(preserved_images(&serde_json::to_vec(&inventory).unwrap(), &m).is_err());
        let service = LifecycleService::new(
            std::env::temp_dir().join(format!("restore-receipt-{}", Uuid::new_v4())),
        )
        .unwrap();
        let job = RestorationJob {
            id: Uuid::new_v4().to_string(),
            state: "completed".into(),
            stage: "complete".into(),
            error_code: None,
            created_at: 1,
            updated_at: 1,
        };
        write_json(&service.root, "restoration.json", &job).unwrap();
        assert!(service.restoration_status().is_err());
    }
}
