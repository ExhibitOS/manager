// SPDX-License-Identifier: Apache-2.0
//! Quiesced installed-runtime backup producer. Never restores or deletes source volumes.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupCreationJob {
    pub id: String,
    pub operation: String,
    pub state: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupCreationReceipt {
    pub id: String,
    pub backup_id: String,
    pub operation: String,
    pub files: u64,
    pub image: String,
    pub authenticated_manifest_sha256: String,
    pub writers_paused: bool,
    pub at: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileHash {
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperResult {
    operation: String,
    backup_id: String,
    files: u64,
    authenticated_manifest_sha256: String,
    archive_inventory: BTreeMap<String, FileHash>,
}
struct Layout {
    database: String,
    platform: String,
    network: String,
    blobs: String,
    configuration: String,
}
const HELPER: &str = r#"
import {main} from './scripts/service-backup.mjs';
import {createHash} from 'node:crypto';
import {open,readdir,lstat,readFile} from 'node:fs/promises';
const made=await main(['create','--key-file','/backup-key','--destination','/work/archive','--quiesced']);
const verified=await main(['verify','--key-file','/backup-key','--source','/work/archive','--destination','/work/authenticated']);
const manifest=await readFile('/work/authenticated/manifest.json');
const archiveInventory={};
for(const name of ['complete.json','writing.json','manifest.gcm',...(await readdir('/work/archive/files')).map(n=>'files/'+n)]){
 const path='/work/archive/'+name,stat=await lstat(path);
 if(!stat.isFile()||stat.isSymbolicLink()||stat.nlink!==1||(stat.mode&0o7777)!==0o600)throw Error('PRIVATE_ARCHIVE_INVALID');
 const f=await open(path,'r');try{const hash=createHash('sha256');for await(const chunk of f.createReadStream({autoClose:false}))hash.update(chunk);archiveInventory[name]={bytes:stat.size,sha256:hash.digest('hex')}}finally{await f.close()}
}
console.log(JSON.stringify({operation:'created-and-authenticated',backupId:made.backupId,files:verified.files,authenticatedManifestSha256:createHash('sha256').update(manifest).digest('hex'),archiveInventory}));
"#;
fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir(path).map_err(|_| err("STATE_UNAVAILABLE"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
    }
    Ok(())
}
pub(crate) fn hash_file(path: &Path) -> Result<FileHash> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut f = options
        .open(path)
        .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
    let before = f.metadata().map_err(|_| err("BACKUP_RESULT_INVALID"))?;
    if !before.is_file() || before.len() == 0 || before.len() > 8 * 1024 * 1024 * 1024 {
        return Err(err("BACKUP_RESULT_INVALID"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.nlink() != 1 || before.mode() & 0o7777 != 0o600 {
            return Err(err("BACKUP_PRIVATE_PERMISSIONS"));
        }
    }
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = f
            .read(&mut buffer)
            .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > before.len() {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
        hash.update(&buffer[..n]);
    }
    let after = f.metadata().map_err(|_| err("BACKUP_RESULT_INVALID"))?;
    let current = fs::symlink_metadata(path).map_err(|_| err("BACKUP_RESULT_INVALID"))?;
    if count != before.len() || after.len() != count || !current.is_file() {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (
            before.dev(),
            before.ino(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
            before.mode(),
            before.nlink(),
        ) != (
            after.dev(),
            after.ino(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
            after.mode(),
            after.nlink(),
        ) || (after.dev(), after.ino()) != (current.dev(), current.ino())
        {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
    }
    Ok(FileHash {
        bytes: count,
        sha256: format!("{:x}", hash.finalize()),
    })
}
fn labels_valid(labels: &Value, m: &BundleManifest) -> bool {
    labels["com.exhibitos.bundle"] == m.bundle_id
        && labels["com.exhibitos.project"] == m.project_name
        && labels["com.exhibitos.schema"] == m.schema_version
}
pub(crate) fn inspected(engine: &str, args: &[String]) -> Result<Value> {
    let value: Value = serde_json::from_slice(&run(engine, args, None, 30)?)
        .map_err(|_| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    if value.as_array().is_none_or(|v| v.len() != 1) {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    Ok(value[0].clone())
}
pub(crate) fn local_image(engine: &str, reference: &str) -> Result<String> {
    let image = inspected(
        engine,
        &["image".into(), "inspect".into(), reference.into()],
    )?;
    let id = image["Id"]
        .as_str()
        .filter(|v| v.strip_prefix("sha256:").is_some_and(hash_valid))
        .ok_or_else(|| err("IMAGE_INTEGRITY"))?;
    if reference.starts_with("sha256:") && !same_local_image_id(id, reference)
        || reference.contains("@sha256:")
            && !image["RepoDigests"].as_array().is_some_and(|v| {
                v.iter().any(|pin| {
                    pin.as_str()
                        .is_some_and(|v| same_registry_pin(v, reference))
                })
            })
    {
        return Err(err("IMAGE_INTEGRITY"));
    }
    Ok(id.into())
}
fn one_container(
    service: &LifecycleService,
    m: &BundleManifest,
    engine: &str,
    name: &str,
) -> Result<(String, Value)> {
    let data = run(
        engine,
        &compose_args(m, &["ps", "--all", "--quiet", name]),
        Some(&service.root.join("bundle")),
        30,
    )?;
    let text = String::from_utf8(data).map_err(|_| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    let id = text.trim();
    if id.len() < 12 || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let value = inspected(engine, &["inspect".into(), id.into()])?;
    if !labels_valid(&value["Config"]["Labels"], m)
        || value["Config"]["Labels"]["com.docker.compose.service"] != name
        || value["State"]["Paused"] == true
        || value["State"]["Restarting"] == true
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok((
        value["Id"]
            .as_str()
            .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?
            .into(),
        value,
    ))
}
fn volume_for(
    engine: &str,
    m: &BundleManifest,
    config: &Value,
    service: &str,
    target: &str,
    actual: &Value,
) -> Result<String> {
    let volumes = config["services"][service]["volumes"]
        .as_array()
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    let selected: Vec<_> = volumes
        .iter()
        .filter(|v| v["target"] == target && v["type"] == "volume")
        .collect();
    if selected.len() != 1 {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let source = selected[0]["source"]
        .as_str()
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    let name = config["volumes"][source]["name"]
        .as_str()
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    if !name.starts_with(&(m.project_name.clone() + "_"))
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let volume = inspected(engine, &["volume".into(), "inspect".into(), name.into()])?;
    if !labels_valid(&volume["Labels"], m)
        || volume["Driver"] != "local"
        || volume["Options"].as_object().is_some_and(|v| !v.is_empty())
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    let mounts = actual["Mounts"]
        .as_array()
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    if mounts
        .iter()
        .filter(|v| {
            v["Destination"] == target
                && v["Name"] == name
                && v["Type"] == "volume"
                && v["RW"] == true
        })
        .count()
        != 1
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(name.into())
}
fn layout(
    service: &LifecycleService,
    m: &BundleManifest,
    engine: &str,
    prepared: &Path,
) -> Result<Layout> {
    if m.services.len() != 2
        || !m.services.iter().any(|s| s == "database")
        || !m.services.iter().any(|s| s == "platform")
        || m.images.len() != 2
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    service.validate_compose(m, engine)?;
    service.validate_ownership(m, engine)?;
    service.validate_volumes(m, engine)?;
    let config: Value = serde_json::from_slice(&run(
        engine,
        &compose_args(m, &["config", "--format", "json"]),
        Some(&service.root.join("bundle")),
        30,
    )?)
    .map_err(|_| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    let environment = String::from_utf8(
        fs::read(prepared.join("manager-runtime.env"))
            .map_err(|_| err("BACKUP_CONFIGURATION_INVALID"))?,
    )
    .map_err(|_| err("BACKUP_CONFIGURATION_INVALID"))?;
    let environment: BTreeMap<_, _> = environment
        .lines()
        .filter_map(|s| s.split_once('='))
        .collect();
    for (key, value) in &environment {
        if config["services"]["platform"]["environment"][*key] != *value {
            return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
        }
    }
    if config["services"]["platform"]["environment"]["BLOB_ROOT"] != "/data/blobs"
        || config["services"]["platform"]["environment"]["CONFIG_ROOT"] != "/data/config"
        || config["services"]["database"]["environment"]["POSTGRES_USER"] != "exhibitos"
        || config["services"]["database"]["environment"]["POSTGRES_DB"] != "exhibitos"
        || config["services"]["database"]["environment"]["POSTGRES_PASSWORD"]
            != environment["POSTGRES_PASSWORD"]
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let (database, db) = one_container(service, m, engine, "database")?;
    let (platform, app) = one_container(service, m, engine, "platform")?;
    let actual_environment = |value: &Value| -> Result<BTreeMap<String, String>> {
        let values = value["Config"]["Env"]
            .as_array()
            .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
        let mut env = BTreeMap::new();
        for item in values {
            let (key, value) = item
                .as_str()
                .and_then(|v| v.split_once('='))
                .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
            if env.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
            }
        }
        Ok(env)
    };
    let actual_app = actual_environment(&app)?;
    let actual_database = actual_environment(&db)?;
    if environment
        .iter()
        .any(|(key, value)| actual_app.get(*key).map(String::as_str) != Some(*value))
        || actual_database.get("POSTGRES_PASSWORD").map(String::as_str)
            != Some(environment["POSTGRES_PASSWORD"])
    {
        return Err(err("BACKUP_CONFIGURATION_INVALID"));
    }
    if db["State"]["Running"] != true
        || app["Mounts"].as_array().is_none_or(|v| v.len() != 2)
        || db["Mounts"].as_array().is_none_or(|v| v.len() != 1)
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    for (name, actual) in [("database", &db), ("platform", &app)] {
        let pin = config["services"][name]["image"]
            .as_str()
            .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
        if actual["Image"] != local_image(engine, pin)? {
            return Err(err("IMAGE_INTEGRITY"));
        }
    }
    let blobs = volume_for(engine, m, &config, "platform", "/data/blobs", &app)?;
    let configuration = volume_for(engine, m, &config, "platform", "/data/config", &app)?;
    volume_for(engine, m, &config, "database", "/var/lib/postgresql", &db)?;
    let network = config["networks"]["default"]["name"]
        .as_str()
        .filter(|v| *v == format!("{}_default", m.project_name))
        .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?
        .to_owned();
    let net = inspected(
        engine,
        &["network".into(), "inspect".into(), network.clone()],
    )?;
    if !labels_valid(&net["Labels"], m)
        || net["Driver"] != "bridge"
        || !net["Containers"]
            .as_object()
            .is_some_and(|v| v.len() == 2 && v.contains_key(&database) && v.contains_key(&platform))
        || !db["NetworkSettings"]["Networks"][&network]["Aliases"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == "database"))
        || app["NetworkSettings"]["Networks"]
            .as_object()
            .is_none_or(|v| v.len() != 1)
        || db["NetworkSettings"]["Networks"]
            .as_object()
            .is_none_or(|v| v.len() != 1)
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    Ok(Layout {
        database,
        platform,
        network,
        blobs,
        configuration,
    })
}
fn validate_copy(root: &Path, result: &HelperResult) -> Result<()> {
    if result.operation != "created-and-authenticated"
        || Uuid::parse_str(&result.backup_id).is_err()
        || result.files == 0
        || result.files > 10000
        || !hash_valid(&result.authenticated_manifest_sha256)
        || result.archive_inventory.len() != result.files as usize + 3
    {
        return Err(err("BACKUP_RESULT_INVALID"));
    }
    let mut actual = Vec::new();
    for item in fs::read_dir(root).map_err(|_| err("BACKUP_RESULT_INVALID"))? {
        let item = item.map_err(|_| err("BACKUP_RESULT_INVALID"))?;
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
        if name == "files" {
            let meta =
                fs::symlink_metadata(item.path()).map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(err("BACKUP_RESULT_INVALID"));
            }
            for file in fs::read_dir(item.path()).map_err(|_| err("BACKUP_RESULT_INVALID"))? {
                actual.push(format!(
                    "files/{}",
                    file.map_err(|_| err("BACKUP_RESULT_INVALID"))?
                        .file_name()
                        .into_string()
                        .map_err(|_| err("BACKUP_RESULT_INVALID"))?
                ));
            }
        } else {
            actual.push(name);
        }
    }
    actual.sort();
    if actual != result.archive_inventory.keys().cloned().collect::<Vec<_>>() {
        return Err(err("BACKUP_RESULT_INVALID"));
    }
    for (name, expected) in &result.archive_inventory {
        let file_path = name.strip_prefix("files/").is_some_and(|s| {
            s.len() == 12
                && s.ends_with(".gcm")
                && s.as_bytes()[..8].iter().all(|b| b.is_ascii_digit())
        });
        if !["complete.json", "writing.json", "manifest.gcm"].contains(&name.as_str()) && !file_path
            || !hash_valid(&expected.sha256)
        {
            return Err(err("BACKUP_RESULT_INVALID"));
        }
        let value = hash_file(&checked_path(root, name)?)?;
        if value.bytes != expected.bytes || value.sha256 != expected.sha256 {
            return Err(err("BACKUP_RESULT_INVALID"));
        }
    }
    Ok(())
}
fn validate_backup_helper(helper: &Value, jobs: &[BackupCreationJob]) -> Result<()> {
    let name = helper["Name"].as_str();
    let owner = helper["Config"]["Labels"]["com.exhibitos.backup"].as_str();
    let known_name = jobs
        .iter()
        .any(|job| name == Some(format!("/exhibitos-backup-{}", job.id).as_str()));
    let known_owner = jobs.iter().any(|job| owner == Some(job.id.as_str()));
    if !known_name && !known_owner {
        return Ok(());
    }
    if !jobs.iter().any(|job| {
        owner == Some(job.id.as_str())
            && name == Some(format!("/exhibitos-backup-{}", job.id).as_str())
    }) {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    match helper["State"]["Running"].as_bool() {
        Some(true) => Err(err("BACKUP_ORPHAN_PENDING")),
        Some(false) => Ok(()),
        None => Err(err("ENGINE_OUTPUT_INVALID")),
    }
}
impl LifecycleService {
    pub(crate) fn recover_backup_jobs(&self) -> Result<()> {
        let guard = match self.lock() {
            Ok(v) => v,
            Err(e) if e.code == "BUSY" => return Ok(()),
            Err(e) => return Err(e),
        };
        let _guard = guard;
        for mut job in self.read_backup_jobs()? {
            if job.state == "running" {
                job.state = "interrupted".into();
                job.error_code = Some("INTERRUPTED".into());
                job.updated_at = now();
                write_json(
                    &self.root.join(format!("backup-creation-{}", job.id)),
                    "job.json",
                    &job,
                )?;
            }
        }
        Ok(())
    }
    fn read_backup_jobs(&self) -> Result<Vec<BackupCreationJob>> {
        let mut result = Vec::new();
        for item in fs::read_dir(&self.root).map_err(|_| err("STATE_UNAVAILABLE"))? {
            let item = item.map_err(|_| err("STATE_UNAVAILABLE"))?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(id) = name.strip_prefix("backup-creation-") else {
                continue;
            };
            if Uuid::parse_str(id).is_err()
                || !item
                    .file_type()
                    .is_ok_and(|v| v.is_dir() && !v.is_symlink())
            {
                return Err(err("STATE_INVALID"));
            }
            if result.len() >= 1000 {
                return Err(err("JOB_HISTORY_FULL"));
            }
            let job: BackupCreationJob = read_json(&item.path().join("job.json"))?;
            if job.id != id
                || job.operation != "create"
                || !["running", "completed", "failed", "interrupted"].contains(&job.state.as_str())
            {
                return Err(err("STATE_INVALID"));
            }
            result.push(job);
        }
        result.sort_by_key(|job| job.created_at);
        Ok(result)
    }
    /// Must run under the operation lock before allowing managed writers to resume.
    pub(crate) fn ensure_backup_helpers_idle(&self, engine: &str) -> Result<()> {
        let jobs = self.read_backup_jobs()?;
        if jobs.is_empty() {
            return Ok(());
        }
        // Union label and name scopes: a renamed helper or removed ownership label
        // must not silently stop fencing writers. Foreign installations remain ignored.
        let mut helpers = std::collections::BTreeSet::new();
        for filter in ["label=com.exhibitos.backup", "name=exhibitos-backup-"] {
            let output = String::from_utf8(run(
                engine,
                &[
                    "ps".into(),
                    "--all".into(),
                    "--filter".into(),
                    filter.into(),
                    "--format".into(),
                    "{{.ID}}".into(),
                ],
                None,
                30,
            )?)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
            helpers.extend(output.lines().map(str::to_owned));
        }
        for id in &helpers {
            if id.len() < 12 || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            let helper = inspected(engine, &["inspect".into(), id.clone()])?;
            validate_backup_helper(&helper, &jobs)?;
        }
        Ok(())
    }
    pub fn backup_jobs(&self) -> Result<Vec<BackupCreationJob>> {
        self.recover_backup_jobs()?;
        self.read_backup_jobs()
    }
    /// Pauses the installed Platform writer and leaves it paused on success or failure.
    /// External DB/blob/configuration writers must be independently quiesced by the operator.
    pub fn create_backup(
        &self,
        image: &str,
        key: &Path,
        external_writers_quiesced: bool,
    ) -> Result<BackupCreationReceipt> {
        let _lock = self.lock()?;
        if cfg!(windows) {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        if !external_writers_quiesced {
            return Err(err("BACKUP_OPERATOR_ACK_REQUIRED"));
        }
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        super::maintenance::input_path(&self.root, true)?;
        super::maintenance::input_path(key, false)?;
        if key.starts_with(&self.root) {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        let previous_jobs = self.read_backup_jobs()?;
        if previous_jobs.len() >= 1000 {
            return Err(err("JOB_HISTORY_FULL"));
        }
        let m = self.manifest()?;
        let engine = self.engine(&m, false)?;
        if engine != "docker" {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        self.ensure_backup_helpers_idle(&engine)?;
        local_image(&engine, image)?;
        let prepared = self.prepare_installation_backup_locked()?;
        let preparation = self
            .root
            .join(format!("backup-preparation-{}", prepared.id));
        let layout = layout(self, &m, &engine, &preparation)?;
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("backup-creation-{id}"));
        private_directory(&workspace)?;
        let mut job = BackupCreationJob {
            id: id.clone(),
            operation: "create".into(),
            state: "running".into(),
            stage: "preflight".into(),
            error_code: None,
            created_at: now(),
            updated_at: now(),
        };
        write_json(&workspace, "job.json", &job)?;
        self.begin_maintenance("backup", &id, &job.stage)?;
        let container = format!("exhibitos-backup-{id}");
        let volume = format!("exhibitos-backup-work-{id}");
        let outcome = (|| -> Result<BackupCreationReceipt> {
            let stage = |job: &mut BackupCreationJob, value: &str| -> Result<()> {
                job.stage = value.into();
                job.updated_at = now();
                self.maintenance_stage("backup", &id, value)?;
                write_json(&workspace, "job.json", job)
            };
            if fs2::available_space(&workspace).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                < 2 * 1024 * 1024 * 1024
            {
                return Err(err("STORAGE_QUOTA"));
            }
            stage(&mut job, "saving-images")?;
            let deployment = workspace.join("images");
            private_directory(&deployment)?;
            let mut artifacts = BTreeMap::new();
            let mut image_inventory = Vec::new();
            let mut image_bytes = 0;
            for (index, pin) in m.images.iter().enumerate() {
                self.maintenance_checkpoint("backup", &id)?;
                let content = local_image(&engine, &pin.reference)?;
                let filename = format!("image-{index}.tar");
                let path = deployment.join(&filename);
                private_options()
                    .open(&path)
                    .map_err(|_| err("STATE_UNAVAILABLE"))?;
                run(
                    &engine,
                    &[
                        "image".into(),
                        "save".into(),
                        "--output".into(),
                        path.to_string_lossy().into_owned(),
                        content.clone(),
                    ],
                    None,
                    1800,
                )?;
                let hashed = hash_file(&path)?;
                image_bytes += hashed.bytes;
                if image_bytes > 2 * 1024 * 1024 * 1024 {
                    return Err(err("STORAGE_QUOTA"));
                }
                artifacts.insert(format!("images/{filename}"),serde_json::json!({"path":format!("/deployment/{filename}"),"bytes":hashed.bytes,"sha256":hashed.sha256}));
                image_inventory.push(serde_json::json!({"reference":pin.reference,"contentId":content,"archive":format!("images/{filename}"),"bytes":hashed.bytes,"sha256":hashed.sha256}));
            }
            write_json(
                &preparation,
                "manager-image-inventory.json",
                &image_inventory,
            )?;
            let mut configuration: BTreeMap<String, String> = [
                "manager-bundle-manifest.json",
                "manager-compose.yaml",
                "manager-runtime.env",
                "manager-installed.json",
                "manager-engine.json",
                "manager-image-inventory.json",
            ]
            .iter()
            .map(|name| (name.to_string(), format!("/installation/{name}")))
            .collect();
            configuration.insert(
                "freeze-signing-key.json".into(),
                "/configuration/freeze-signing-key.json".into(),
            );
            stage(&mut job, "pausing-writers")?;
            run(
                &engine,
                &compose_args(&m, &["stop", "--timeout", "30", "platform"]),
                Some(&self.root.join("bundle")),
                180,
            )?;
            let paused = inspected(&engine, &["inspect".into(), layout.platform.clone()])?;
            let database = inspected(&engine, &["inspect".into(), layout.database.clone()])?;
            if paused["State"]["Running"] != false
                || database["State"]["Running"] != true
                || !labels_valid(&paused["Config"]["Labels"], &m)
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            stage(&mut job, "encrypting-and-authenticating")?;
            run(
                &engine,
                &[
                    "volume".into(),
                    "create".into(),
                    "--label".into(),
                    format!("com.exhibitos.backup={id}"),
                    volume.clone(),
                ],
                None,
                30,
            )?;
            let owned_work = inspected(
                &engine,
                &["volume".into(), "inspect".into(), volume.clone()],
            )?;
            if owned_work["Labels"]["com.exhibitos.backup"] != id
                || owned_work["Driver"] != "local"
                || owned_work["Options"]
                    .as_object()
                    .is_some_and(|v| !v.is_empty())
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            let mut args = vec![
                "run".into(),
                "--pull".into(),
                "never".into(),
                "--name".into(),
                container.clone(),
                "--label".into(),
                format!("com.exhibitos.backup={id}"),
                "--network".into(),
                layout.network.clone(),
                "--user".into(),
                "0:0".into(),
                "--read-only".into(),
                "--cap-drop".into(),
                "ALL".into(),
                "--cap-add".into(),
                "DAC_OVERRIDE".into(),
                "--security-opt".into(),
                "no-new-privileges:true".into(),
                "--pids-limit".into(),
                "128".into(),
                "--memory".into(),
                "1536m".into(),
                "--tmpfs".into(),
                "/tmp:rw,nosuid,nodev,size=256m,mode=1777".into(),
                "--env-file".into(),
                preparation
                    .join("manager-runtime.env")
                    .to_string_lossy()
                    .into_owned(),
            ];
            for mount in [
                format!(
                    "type=bind,source={},target=/backup-key,readonly",
                    key.display()
                ),
                format!(
                    "type=bind,source={},target=/installation,readonly",
                    preparation.display()
                ),
                format!(
                    "type=bind,source={},target=/deployment,readonly",
                    deployment.display()
                ),
                format!(
                    "type=volume,source={},target=/data/blobs,readonly",
                    layout.blobs
                ),
                format!(
                    "type=volume,source={},target=/configuration,readonly",
                    layout.configuration
                ),
                format!("type=volume,source={volume},target=/work"),
            ] {
                args.extend(["--mount".into(), mount]);
            }
            args.extend([
                "-e".into(),
                "BLOB_ROOT=/data/blobs".into(),
                "-e".into(),
                "BACKUP_MAX_DUMP_BYTES=2147483648".into(),
                "-e".into(),
                "BACKUP_CONFIGURATION_FILES=".to_owned()
                    + &serde_json::to_string(&configuration).map_err(|_| err("STATE_INVALID"))?,
                "-e".into(),
                "BACKUP_DEPLOYMENT_FILES=".to_owned()
                    + &serde_json::to_string(&artifacts).map_err(|_| err("STATE_INVALID"))?,
                "--entrypoint".into(),
                "node".into(),
                image.into(),
                "--input-type=module".into(),
                "-e".into(),
                HELPER.into(),
            ]);
            let result: HelperResult =
                serde_json::from_slice(&self.run_maintenance_helper("backup", &id, &args)?)
                    .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            stage(&mut job, "copying-authenticated-archive")?;
            run(
                &engine,
                &[
                    "cp".into(),
                    format!("{container}:/work/archive"),
                    workspace.to_string_lossy().into_owned(),
                ],
                None,
                1800,
            )?;
            validate_copy(&workspace.join("archive"), &result)?;
            let value = BackupCreationReceipt {
                id: id.clone(),
                backup_id: result.backup_id,
                operation: result.operation,
                files: result.files,
                image: image.into(),
                authenticated_manifest_sha256: result.authenticated_manifest_sha256,
                writers_paused: true,
                at: now(),
            };

            Ok(value)
        })();
        // Keep stopped/failed helpers and all volumes; cancellation must verify stop.
        let _terminal = self.maintenance_finish_guard("backup", &id)?;
        let outcome = if self.maintenance_requested("backup", &id)? {
            if outcome
                .as_ref()
                .err()
                .is_some_and(|e| e.code != "CANCELLED")
            {
                Err(err("CANCEL_UNCERTAIN"))
            } else {
                self.maintenance_checkpoint("backup", &id).and(outcome)
            }
        } else {
            outcome
        };
        job.updated_at = now();
        if let Ok(receipt) = &outcome {
            write_json(&workspace, "receipt.json", receipt)?;
            job.state = "completed".into();
            job.stage = "complete".into();
        } else {
            job.state = if outcome
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "CANCELLED")
            {
                "interrupted"
            } else {
                "failed"
            }
            .into();
            job.error_code = outcome.as_ref().err().map(|e| e.code.clone());
        }
        write_json(&workspace, "job.json", &job)?;
        let terminal = match job.error_code.as_deref() {
            None => "completed",
            Some("CANCELLED") => "confirmed",
            Some("CANCEL_UNCERTAIN") => "uncertain",
            _ => "failed",
        };
        self.finish_maintenance(terminal, job.error_code.as_deref())?;
        outcome.map_err(|e| {
            err(
                if ["CANCELLED", "CANCEL_UNCERTAIN"].contains(&e.code.as_str()) {
                    &e.code
                } else {
                    "BACKUP_CREATION_FAILED"
                },
            )
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_helpers_gate_writers_and_ambiguous_identity_fails_closed() {
        let id = Uuid::new_v4().to_string();
        let jobs = vec![BackupCreationJob {
            id: id.clone(),
            operation: "create".into(),
            state: "interrupted".into(),
            stage: "synthetic".into(),
            error_code: Some("INTERRUPTED".into()),
            created_at: 1,
            updated_at: 2,
        }];
        let mut helper = serde_json::json!({"Name":format!("/exhibitos-backup-{id}"),"Config":{"Labels":{"com.exhibitos.backup":id}},"State":{"Running":true}});
        assert_eq!(
            validate_backup_helper(&helper, &jobs).unwrap_err().code,
            "BACKUP_ORPHAN_PENDING"
        );
        helper["State"]["Running"] = false.into();
        assert!(validate_backup_helper(&helper, &jobs).is_ok());
        helper["State"]["Running"] = "false".into();
        assert_eq!(
            validate_backup_helper(&helper, &jobs).unwrap_err().code,
            "ENGINE_OUTPUT_INVALID"
        );
        helper["Name"] = "/renamed-helper".into();
        assert_eq!(
            validate_backup_helper(&helper, &jobs).unwrap_err().code,
            "OWNERSHIP_CONFLICT"
        );
        helper["Name"] = format!("/exhibitos-backup-{}", jobs[0].id).into();
        helper["Config"]["Labels"]["com.exhibitos.backup"] = Uuid::new_v4().to_string().into();
        assert_eq!(
            validate_backup_helper(&helper, &jobs).unwrap_err().code,
            "OWNERSHIP_CONFLICT"
        );
        helper["Name"] = "/foreign-helper".into();
        helper["State"]["Running"] = true.into();
        assert!(validate_backup_helper(&helper, &jobs).is_ok());
    }
    #[test]
    fn ack_image_and_key_boundaries_precede_engine_or_writer_changes() {
        let root = std::env::temp_dir().join(format!("backup-create-test-{}", Uuid::new_v4()));
        let _ = LifecycleService::new(root.clone()).unwrap();
        let service = LifecycleService::new(fs::canonicalize(root).unwrap()).unwrap();
        assert_eq!(
            service
                .create_backup("image:latest", Path::new("relative"), false)
                .unwrap_err()
                .code,
            "BACKUP_OPERATOR_ACK_REQUIRED"
        );
        assert_eq!(
            service
                .create_backup("image:latest", Path::new("relative"), true)
                .unwrap_err()
                .code,
            "BACKUP_IMAGE_INVALID"
        );
        assert_eq!(
            service
                .create_backup(
                    &format!("sha256:{}", "a".repeat(64)),
                    Path::new("relative"),
                    true
                )
                .unwrap_err()
                .code,
            "BACKUP_PATH_INVALID"
        );
        assert!(service.backup_jobs().unwrap().is_empty());
    }
    #[test]
    fn interrupted_records_recover_and_active_operation_is_not_relabelled() {
        let root = std::env::temp_dir().join(format!("backup-job-test-{}", Uuid::new_v4()));
        let _ = LifecycleService::new(root.clone()).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::new(root.clone()).unwrap();
        let id = Uuid::new_v4().to_string();
        let workspace = root.join(format!("backup-creation-{id}"));
        private_directory(&workspace).unwrap();
        let job = BackupCreationJob {
            id,
            operation: "create".into(),
            state: "running".into(),
            stage: "pausing-writers".into(),
            error_code: None,
            created_at: now(),
            updated_at: now(),
        };
        write_json(&workspace, "job.json", &job).unwrap();
        let guard = service.lock().unwrap();
        assert_eq!(service.backup_jobs().unwrap()[0].state, "running");
        drop(guard);
        let reopened = LifecycleService::new(root).unwrap();
        let job = &reopened.backup_jobs().unwrap()[0];
        assert_eq!(job.state, "interrupted");
        assert_eq!(job.stage, "pausing-writers");
        assert_eq!(job.error_code.as_deref(), Some("INTERRUPTED"));
        assert!(!workspace.join("receipt.json").exists());
    }
    #[test]
    fn copied_cipher_inventory_must_match_exact_private_bytes() {
        let root = std::env::temp_dir().join(format!("backup-copy-test-{}", Uuid::new_v4()));
        private_directory(&root).unwrap();
        private_directory(&root.join("files")).unwrap();
        let mut inventory = BTreeMap::new();
        for name in [
            "complete.json",
            "writing.json",
            "manifest.gcm",
            "files/00000000.gcm",
        ] {
            let mut file = private_options().open(root.join(name)).unwrap();
            file.write_all(b"synthetic cipher").unwrap();
            drop(file);
            inventory.insert(name.into(), hash_file(&root.join(name)).unwrap());
        }
        let value = HelperResult {
            operation: "created-and-authenticated".into(),
            backup_id: Uuid::new_v4().to_string(),
            files: 1,
            authenticated_manifest_sha256: "a".repeat(64),
            archive_inventory: inventory,
        };
        assert!(validate_copy(&root, &value).is_ok());
        fs::write(root.join("manifest.gcm"), b"changed").unwrap();
        assert!(validate_copy(&root, &value).is_err());
        fs::write(root.join("unexpected"), b"new").unwrap();
        assert!(validate_copy(&root, &value).is_err());
    }
}
