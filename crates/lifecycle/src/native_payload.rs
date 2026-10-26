// SPDX-License-Identifier: Apache-2.0
//! App-contained macOS runtime admission. No installation, process launch or
//! credential read occurs here. UI mode cannot bypass payload verification.
use super::*;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    mode: String,
    platform: String,
    node_version: String,
    postgres_version: String,
    platform_version: String,
    protocol_version: String,
    source_commit: String,
    #[serde(rename = "producerSHA256")]
    producer_sha256: String,
    entry: Entries,
    files: Vec<Item>,
    redistribution_qualified: bool,
    official_signed: bool,
    remaining: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entries {
    node: String,
    script: String,
    postgres: String,
    initdb: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    path: String,
    bytes: Option<u64>,
    sha256: Option<String>,
    executable: Option<bool>,
    link: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub state: String,
    pub platform_version: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub redistribution_qualified: bool,
    pub official_signed: bool,
}
pub struct Payload {
    root: PathBuf,
    manifest: Manifest,
    manifest_sha256: String,
}
/// Optional maintenance capability. Construction requires all fixed resources
/// in the already byte-verified payload; older payloads remain usable for
/// authoring, but cannot silently fall back to host PostgreSQL tools.
pub struct BackupResources {
    pub script: PathBuf,
    pub pg_dump: PathBuf,
    pub pg_restore: PathBuf,
}
fn invalid() -> LifecycleError {
    err("NATIVE_RUNTIME_PAYLOAD_INVALID")
}
fn relative_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 2048
        && !name.contains(['\\', '\0', '\r', '\n'])
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn contained(root: &Path, relative: &str) -> Result<PathBuf> {
    if !relative_name(relative) {
        return Err(invalid());
    }
    let path = fs::canonicalize(root.join(relative)).map_err(|_| invalid())?;
    if !path.starts_with(root) || path == root {
        return Err(invalid());
    }
    Ok(path)
}
fn actual_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    names: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 64 {
        return Err(invalid());
    }
    for entry in fs::read_dir(directory).map_err(|_| invalid())? {
        let path = entry.map_err(|_| invalid())?.path();
        let stat = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if stat.is_dir() && !stat.is_symlink() {
            actual_files(root, &path, depth + 1, names)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?;
            if relative == "manifest.json" {
                continue;
            }
            if names.len() >= 50_000 || !relative_name(relative) {
                return Err(invalid());
            }
            names.insert(relative.into());
        }
    }
    Ok(())
}
impl Payload {
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute()
            || fs::symlink_metadata(root)
                .map_err(|_| invalid())?
                .is_symlink()
        {
            return Err(invalid());
        }
        let root = fs::canonicalize(root).map_err(|_| invalid())?;
        if fs::symlink_metadata(&root)
            .map_err(|_| invalid())?
            .is_symlink()
        {
            return Err(invalid());
        }
        let path = root.join("manifest.json");
        let mut options = OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let mut file = options.open(path).map_err(|_| invalid())?;
        let stat = file.metadata().map_err(|_| invalid())?;
        if !stat.is_file() || stat.len() > 16 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|_| invalid())?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if manifest.format != "exhibitos-native-runtime-v1"
            || manifest.platform != "darwin-arm64"
            || !cfg!(target_arch = "aarch64")
            || manifest.node_version != "24.21.0"
            || manifest.postgres_version != "18.6"
            || manifest.platform_version != "0.1.0"
            || manifest.protocol_version != "1"
            || manifest.source_commit.len() != 40
            || !manifest
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !hash_valid(&manifest.producer_sha256)
            || manifest.files.is_empty()
            || manifest.files.len() > 50_000
            || manifest.remaining.len() > 4096
        {
            return Err(invalid());
        }
        // A local validation copy is not accepted by a production build.
        if manifest.mode == "local-validation" {
            if !cfg!(debug_assertions)
                || manifest.official_signed
                || manifest.redistribution_qualified
            {
                return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
            }
        } else if manifest.mode != "release"
            || !manifest.redistribution_qualified
            || !manifest.official_signed
        {
            return Err(err("NATIVE_RUNTIME_UNQUALIFIED"));
        } else {
            // Flags remain necessary declarations. The running helper and its
            // owning sealed app must independently satisfy the pinned official
            // Developer ID identity before any payload file is admitted.
            crate::native_signed_admission::admit(&root)?;
        }
        if manifest.entry.node != "bin/node"
            || manifest.entry.script != "platform/scripts/local-runtime.mjs"
            || manifest.entry.postgres != "postgres/bin/postgres"
            || manifest.entry.initdb != "postgres/bin/initdb"
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        for item in &manifest.files {
            if !relative_name(&item.path)
                || item.path == "manifest.json"
                || !names.insert(item.path.clone())
            {
                return Err(invalid());
            }
            let path = contained(&root, &item.path)?;
            let raw = fs::symlink_metadata(root.join(&item.path)).map_err(|_| invalid())?;
            match (&item.link, item.bytes, &item.sha256, item.executable) {
                (Some(link), None, None, None) => {
                    if !raw.is_symlink() || link.len() > 2048 {
                        return Err(invalid());
                    }
                    let expected = fs::canonicalize(
                        root.join(&item.path)
                            .parent()
                            .ok_or_else(invalid)?
                            .join(link),
                    )
                    .map_err(|_| invalid())?;
                    if expected != path || !expected.starts_with(&root) {
                        return Err(invalid());
                    }
                }
                (None, Some(size), Some(hash), Some(executable)) => {
                    if raw.is_symlink() || !raw.is_file() || raw.len() != size || !hash_valid(hash)
                    {
                        return Err(invalid());
                    }
                    use std::os::unix::fs::PermissionsExt;
                    if (raw.permissions().mode() & 0o111 != 0) != executable {
                        return Err(invalid());
                    }
                    let mut file = File::open(path).map_err(|_| invalid())?;
                    let mut digest = Sha256::new();
                    let mut block = [0u8; 64 * 1024];
                    loop {
                        let length = file.read(&mut block).map_err(|_| invalid())?;
                        if length == 0 {
                            break;
                        }
                        digest.update(&block[..length]);
                    }
                    if format!("{:x}", digest.finalize()) != *hash {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        let mut actual = BTreeSet::new();
        actual_files(&root, &root, 0, &mut actual)?;
        if actual != names {
            return Err(invalid());
        }
        for (name, executable) in [
            (manifest.entry.node.as_str(), true),
            (manifest.entry.script.as_str(), false),
            (manifest.entry.postgres.as_str(), true),
            (manifest.entry.initdb.as_str(), true),
        ] {
            if !manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            root,
            manifest,
            manifest_sha256: digest(&bytes),
        })
    }
    /// Fresh byte admission before a migration writes or runs maintenance tools.
    pub(crate) fn recheck(&self) -> Result<()> {
        let current = Self::open(&self.root)?;
        if current.manifest_sha256 != self.manifest_sha256 { return Err(invalid()); }
        Ok(())
    }
    pub fn context(&self) -> Context {
        Context {
            state: "bytes_verified".into(),
            platform_version: self.manifest.platform_version.clone(),
            source_commit: self.manifest.source_commit.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
            redistribution_qualified: self.manifest.redistribution_qualified,
            official_signed: self.manifest.official_signed,
        }
    }
    pub fn node(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.node)
    }
    /// Fixed codec-only resources, rechecked against this admitted manifest and
    /// held across authentication. No arbitrary module or executable selection.
    pub(crate) fn legacy_codec_pins(&self) -> Result<Vec<crate::hosted_runtime_file::Pin>> {
        let mut pins = Vec::new();
        for (name, executable, limit) in [
            (self.manifest.entry.node.as_str(), true, 160 * 1024 * 1024),
            ("platform/packages/storage/dist/service-backup.js", false, 2 * 1024 * 1024),
            ("platform/packages/storage/dist/service-inventory.js", false, 2 * 1024 * 1024),
            ("platform/packages/storage/dist/encrypted-files.js", false, 2 * 1024 * 1024),
        ] {
            let item = self.manifest.files.iter().find(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
                .ok_or_else(|| err("NATIVE_LEGACY_CODEC_UNAVAILABLE"))?;
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, limit)?;
            if item.bytes != Some(bytes.len() as u64) || item.sha256.as_deref() != Some(digest(&bytes).as_str()) { return Err(invalid()); }
            pins.push(pin);
        }
        Ok(pins)
    }
    pub fn backup_resources(&self) -> Result<BackupResources> {
        for (name, executable) in [
            ("platform/scripts/service-backup.mjs", false),
            ("platform/scripts/native-service-backup.mjs", false),
            ("postgres/bin/pg_dump", true),
            ("postgres/bin/pg_restore", true),
        ] {
            if !self
                .manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(executable))
            {
                return Err(err("NATIVE_BACKUP_UNAVAILABLE"));
            }
        }
        Ok(BackupResources {
            script: self.root.join("platform/scripts/native-service-backup.mjs"),
            pg_dump: self.root.join("postgres/bin/pg_dump"),
            pg_restore: self.root.join("postgres/bin/pg_restore"),
        })
    }
    pub(crate) fn backup_runtime_bytes(&self) -> Result<u64> {
        self.manifest
            .files
            .iter()
            .filter(|f| f.path.starts_with("platform/apps/web/dist/") && f.link.is_none())
            .try_fold(0u64, |sum, file| {
                sum.checked_add(file.bytes.unwrap_or(0))
                    .ok_or_else(|| err("NATIVE_BACKUP_RESOURCE_BUDGET"))
            })
    }
    pub fn static_export_resources(&self) -> Result<(PathBuf, PathBuf)> {
        for name in [
            "platform/scripts/native-static-export.mjs",
            "platform/scripts/export-static-publication.mjs",
            "platform/apps/web/dist-static/static.html",
            "platform/apps/web/dist-static/THIRD_PARTY_NOTICES.txt",
        ] {
            if !self
                .manifest
                .files
                .iter()
                .any(|f| f.path == name && f.link.is_none() && f.executable == Some(false))
            {
                return Err(err("NATIVE_STATIC_EXPORT_UNAVAILABLE"));
            }
        }
        Ok((
            self.root.join("platform/scripts/native-static-export.mjs"),
            self.root.join("platform/apps/web/dist-static"),
        ))
    }
    pub fn script(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.script)
    }
    pub fn postgres(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.postgres)
    }
    pub fn initdb(&self) -> PathBuf {
        self.root.join(&self.manifest.entry.initdb)
    }
}

/// Byte-qualified native migration inputs. This is not an observed schema or an
/// update permit; only the fixed target observer consumes it.
pub(crate) struct TargetInventoryResources {
    pub(crate) platform_root: PathBuf,
    pub(crate) migrations: Vec<u8>,
    pub(crate) migrations_sha256: String,
    pins: Vec<crate::hosted_runtime_file::Pin>,
}
impl TargetInventoryResources {
    pub(crate) fn recheck(&self) -> Result<()> {
        for pin in &self.pins {
            pin.check()?;
        }
        Ok(())
    }
}
impl Payload {
    pub(crate) fn target_inventory_resources(&self) -> Result<TargetInventoryResources> {
        self.recheck()?;
        let mut pins = self.legacy_codec_pins()?;
        for name in [
            "platform/packages/storage/dist/index.js",
            "platform/packages/storage/dist/blobs.js",
        ] {
            let item = self
                .manifest
                .files
                .iter()
                .find(|v| v.path == name && v.link.is_none() && v.executable == Some(false))
                .ok_or_else(invalid)?;
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path {
                return Err(invalid());
            }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            if item.bytes != Some(bytes.len() as u64)
                || item.sha256.as_deref() != Some(digest(&bytes).as_str())
            {
                return Err(invalid());
            }
            pins.push(pin);
        }
        #[derive(Serialize)]
        struct Migration {
            name: String,
            sha256: String,
        }
        let prefix = "platform/database/migrations/";
        let mut rows = std::collections::BTreeMap::new();
        let mut total = 0u64;
        for item in &self.manifest.files {
            let Some(name) = item.path.strip_prefix(prefix) else {
                continue;
            };
            if !name.as_bytes().first().is_some_and(u8::is_ascii_digit) || !name.ends_with(".sql") {
                continue;
            }
            if name.len() > 256
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                || item.link.is_some()
                || item.executable != Some(false)
            {
                return Err(invalid());
            }
            let path = self.root.join(&item.path);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path {
                return Err(invalid());
            }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            total = total
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or_else(invalid)?;
            let hash = digest(&bytes);
            if item.bytes != Some(bytes.len() as u64)
                || item.sha256.as_deref() != Some(hash.as_str())
                || rows.insert(name.to_owned(), hash).is_some()
            {
                return Err(invalid());
            }
            pins.push(pin);
        }
        if rows.is_empty() || rows.len() > 10000 {
            return Err(invalid());
        }
        let rows: Vec<_> = rows
            .into_iter()
            .map(|(name, sha256)| Migration { name, sha256 })
            .collect();
        let migrations = serde_json::to_vec(&rows).map_err(|_| invalid())?;
        if migrations.len() > 65536 {
            return Err(invalid());
        }
        let resources = TargetInventoryResources {
            platform_root: self.root.join("platform"),
            migrations_sha256: digest(&migrations),
            migrations,
            pins,
        };
        resources.recheck()?;
        self.recheck()?;
        Ok(resources)
    }
}

/// authenticated-native-copy-v1: already qualified codec/worker bytes only.
/// A signed migration declaration never grants permission to execute SQL.
const NATIVE_COPY_FILES: &[(&str, &str)] = &[
    ("platform/scripts/native-service-backup.mjs", "72fa2bc234db3aa19975ab05d68b2fc584982cec8d050676a79e2a575bd46feb"),
    ("platform/scripts/service-backup.mjs", "d94be6e99bd966ca2697a58575dcb4a2c3bc9e06a3facca55a53039f377c08aa"),
    ("platform/packages/storage/dist/blobs.d.ts", "5659727be1441d2e279212046be5eb2510c1ae12a48920278855f83c7e32ccf8"),
    ("platform/packages/storage/dist/blobs.js", "f948f6c7f67737b009b032b2cdde3d34fbcc7148eeb851a5b146bd553e239d14"),
    ("platform/packages/storage/dist/encrypted-files.d.ts", "f4cedc62afc12ed39a66ef945d1403a34e7e8804f794663e59c6db8a9ad91c66"),
    ("platform/packages/storage/dist/encrypted-files.js", "c3b11cc821812240cf25f5c172eceac58c61d92c8de6ea4991fcefada44b5fb1"),
    ("platform/packages/storage/dist/encrypted-streams.d.ts", "e3df95cc6dbcbafae693a0b46b22df410637b5580fa37d37d14ee51ff56541d9"),
    ("platform/packages/storage/dist/encrypted-streams.js", "a9c334cf62ff714a6fa5c65d7a8d8a1671244925e1064881b5392c190a51e80c"),
    ("platform/packages/storage/dist/index.d.ts", "681578697596af7b6e63295c14407a864858aba430d4ecf251671eeab3b40720"),
    ("platform/packages/storage/dist/index.js", "e47cff1a28bd576d2c2842ee6de27cb8367e89ff6d2028609d9536b1863c6af1"),
    ("platform/packages/storage/dist/migrations.d.ts", "d9ab9bb5a3d6d10ac82f5ef1495cc1efa44a7c536318d0c429ca831b2d91aee9"),
    ("platform/packages/storage/dist/migrations.js", "ac9c5e10225f8930722eb120148dd40befddf0ab08d2dde4ce682a6cd830dfa1"),
    ("platform/packages/storage/dist/policy.d.ts", "75111683455b8e593182b5ba36d3108eb6e46ab3fa0a2aa183ce198f82d609c4"),
    ("platform/packages/storage/dist/policy.js", "0e25c6ff27120c5fe330db5247736b0ddb3a712b25fc31ed830504a1cec17ff2"),
    ("platform/packages/storage/dist/postgres-blobs.d.ts", "ddf6a4d338be38f936f2d97d6f8e0be6d6eaa734f696c8233cc589729318f59c"),
    ("platform/packages/storage/dist/postgres-blobs.js", "dd9f83ef4f25bacce95d186d167ac8b7b38257f67ee0dedafc6d796af883b823"),
    ("platform/packages/storage/dist/service-backup.d.ts", "eed00e0740aef0b293b205f395886d2e895753b465ab616b5a4e1d3f29026053"),
    ("platform/packages/storage/dist/service-backup.js", "36d1462040133288fafd5b6ecdc0a33aa50567e05f777ee499718d30e329cf1c"),
    ("platform/packages/storage/dist/service-inventory.d.ts", "eb224fb53f33180e9a648ece8d1dc649c97a9eb844bdc40e7ce72899f50d1cb6"),
    ("platform/packages/storage/dist/service-inventory.js", "03b60b30e0f840b6a3c1cd2b842330da43dcb65b5af16a16a5cd7b4996f5fc08"),
];
pub(crate) struct NativeCopyResources {
    pub(crate) inventory: TargetInventoryResources,
    pub(crate) recipe_sha256: String,
    pins: Vec<crate::hosted_runtime_file::Pin>,
}
impl NativeCopyResources {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.inventory.recheck()?;
        for pin in &self.pins { pin.check()?; }
        Ok(())
    }
}
impl Payload {
    pub(crate) fn native_copy_resources(&self) -> Result<NativeCopyResources> {
        self.recheck()?;
        self.backup_resources()?;
        let inventory = self.target_inventory_resources()?;
        let mut pins = Vec::new();
        for (name, expected) in NATIVE_COPY_FILES {
            let entry = self.manifest.files.iter().find(|v| v.path == *name && v.link.is_none() && v.executable == Some(false)).ok_or_else(|| err("NATIVE_TARGET_COPY_UNAVAILABLE"))?;
            if entry.sha256.as_deref() != Some(*expected) { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
            let path = self.root.join(name);
            if fs::canonicalize(&path).map_err(|_| invalid())? != path { return Err(invalid()); }
            let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
            if entry.bytes != Some(bytes.len() as u64) || digest(&bytes) != *expected { return Err(err("NATIVE_TARGET_COPY_UNAVAILABLE")); }
            pins.push(pin);
        }
        let result = NativeCopyResources { inventory, recipe_sha256: digest(&serde_json::to_vec(NATIVE_COPY_FILES).map_err(|_| invalid())?), pins };
        result.recheck()?; Ok(result)
    }
}
