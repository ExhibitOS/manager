// SPDX-License-Identifier: Apache-2.0
//! Owned, cancellable static export. Ordinary export uses public projection.
//! Explicit detail export uses the selected owner’s private child pipe.
use super::*;
use crate::{
    maintenance_source::DirectoryPin, native_payload::Payload, native_runtime::{Session, StaticDetailAuthority},
};

pub(crate) const SAVED_HEAD_RUNTIME: &str =
    "cfc795c35628e15a9f91b71dcca0587023a0b249b2984a2615973b72bc6ed32f";
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString, OsStr, OsString},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::{OsStrExt, OsStringExt}, fs::MetadataExt, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    time::Instant,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub publication_id: String,
    pub output: PathBuf,
    #[serde(default)]
    pub private_editor: bool,
    #[serde(default, deserialize_with = "read_detail_mode")]
    pub detail_mode: Option<DetailMode>,
    output_parent_identity: Option<DirectoryIdentity>,
    saved_head: Option<SavedHead>,
    #[serde(skip)]
    saved_head_verified: bool,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SavedHead {
    exhibition_id: String,
    etag: String,
    request_id: String,
    revision: u64,
    edit_version: u64,
    revision_sha256: String,
}
impl Input {
    pub(crate) fn verify_saved_head_receipt(&mut self, session: &mut Session) -> Result<()> {
        if let Some(head) = &self.saved_head {
            if !self.private_editor || self.detail_mode.is_some() || self.output_parent_identity.is_none() {
                return Err(err("NATIVE_SAVED_HEAD_EXPORT_UNAVAILABLE"));
            }
            session.confirm_saved_head_publication(head, &self.publication_id)?;
            self.saved_head_verified = true;
        }
        Ok(())
    }
}
impl SavedHead {
    pub(crate) fn receipt_query(&self, publication: &str, tenant: &str, principal: &str, session_hash: &str) -> Result<String> {
        let bad = || err("NATIVE_SAVED_HEAD_PUBLICATION_UNCONFIRMED");
        if ![publication, tenant, principal, &self.exhibition_id, &self.request_id]
            .into_iter().all(crate::installations::uuid)
            || !hash_valid(&self.revision_sha256) || !hash_valid(session_hash)
            || self.edit_version == 0 || self.edit_version > i32::MAX as u64
        {
            return Err(bad());
        }
        let (revision, etag_hash) = self.etag.strip_prefix("\"studio-r")
            .and_then(|etag| etag.strip_suffix('"'))
            .and_then(|etag| etag.split_once('-')).ok_or_else(bad)?;
        let draft_revision = revision.parse::<i32>().ok().filter(|value| *value > 0 && value.to_string() == revision)
            .ok_or_else(bad)?;
        if !hash_valid(etag_hash) { return Err(bad()); }
        // This is the pinned Platform's ordinary idempotency hash. Only its
        // digest enters SQL, along with validated UUIDs, digests and integers.
        let quoted_etag = serde_json::to_string(&self.etag).map_err(|_| bad())?;
        let request_hash = digest(format!(r#"{{"id":"{}","requestId":"{}","match":{quoted_etag}}}"#,
            self.exhibition_id, self.request_id).as_bytes());
        let Self { exhibition_id, request_id, edit_version, revision_sha256, .. } = self;
        Ok(format!("SELECT EXISTS (
            SELECT 1 FROM public.publication_requests r
            JOIN public.studio_publications p ON (p.tenant_id,p.id)=(r.tenant_id,r.publication_id)
            JOIN public.publication_states s ON s.publication_id=p.id
            JOIN public.exhibitions e ON (e.tenant_id,e.id)=(p.tenant_id,p.exhibition_id)
            JOIN public.auth_sessions a ON (a.tenant_id,a.user_id)=(r.tenant_id,r.user_id)
            JOIN public.memberships m ON (m.tenant_id,m.user_id)=(a.tenant_id,a.user_id)
            JOIN public.users u ON u.id=a.user_id
            JOIN public.tenants t ON t.id=a.tenant_id
            WHERE r.tenant_id='{tenant}' AND r.user_id='{principal}' AND r.request_id='{request_id}'
              AND r.payload_sha256='{request_hash}' AND p.id='{publication}' AND p.created_by=r.user_id
              AND p.exhibition_id='{exhibition_id}' AND p.draft_revision={draft_revision}
              AND p.revision_sha256='{revision_sha256}' AND s.status='published'
              AND e.studio_managed AND e.deleted_at IS NULL AND e.studio_owner_user_id=r.user_id
              AND e.revision=p.draft_revision AND e.metadata->>'editVersion'='{edit_version}'
              AND a.token_hash='{session_hash}' AND NOT a.revoked AND a.expires_at>now()
              AND m.role IN ('admin','curator','artist')
              AND NOT u.disabled AND t.deleted_at IS NULL)"))
    }
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
pub enum DetailMode {
    #[serde(rename = "native-detail-v1")]
    NativeDetailV1,
}
fn read_detail_mode<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<DetailMode>, D::Error> {
    DetailMode::deserialize(input).map(Some)
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub publication_id: String,
    pub revision_sha256: String,
    pub index_sha256: String,
    pub manifest_sha256: String,
    pub files: u64,
    pub bytes: u64,
    pub output: PathBuf,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    pub format: u32,
    pub job_id: String,
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub publication_id: String,
    pub revision_sha256: String,
    pub output: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) output_parent_identity: Option<DirectoryIdentity>,
    pub state: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private_editor: bool,
    pub receipt: Option<Receipt>,
    pub error_code: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    publication_id: String,
    revision_sha256: String,
    index_sha256: String,
    files: Vec<ManifestFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
}
fn bad() -> LifecycleError {
    err("NATIVE_STATIC_EXPORT_INVALID")
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}
fn bounded(root: &Path, name: &str, limit: u64) -> Result<Vec<u8>> {
    installation_backup::source_bytes(root, name, limit, false).map_err(|_| bad())
}
/// Digest of the fixed Pages v1 control bytes. Call only with a receipt whose
/// output has passed verify_output; the index and manifest are rechecked here.
pub fn cache_policy_sha256(receipt: &Receipt) -> Result<String> {
    let manifest_bytes = bounded(&receipt.output, "static-export-manifest.json", 8 * 1024 * 1024)?;
    let index_bytes = bounded(&receipt.output, "static-publication-index.json", 1024 * 1024)?;
    if digest(&manifest_bytes) != receipt.manifest_sha256 || digest(&index_bytes) != receipt.index_sha256 { return Err(bad()); }
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|_| bad())?;
    let index: serde_json::Value = serde_json::from_slice(&index_bytes).map_err(|_| bad())?;
    if index["version"] != 1 { return Err(bad()); }
    let assets = index["assets"].as_array().ok_or_else(bad)?;
    let publication = index.get("publication").ok_or_else(bad)?;
    let files: std::collections::BTreeMap<_, _> = manifest.files.iter().map(|f| (f.path.as_str(), (f.sha256.as_str(), f.bytes))).collect();
    let mut paths = BTreeSet::new();
    for descriptor in std::iter::once(publication).chain(assets.iter()) {
        let path = descriptor["path"].as_str().ok_or_else(bad)?;
        let sha = descriptor["sha256"].as_str().ok_or_else(bad)?;
        let size = descriptor["bytes"].as_u64().ok_or_else(bad)?;
        if files.get(path) != Some(&(sha, size)) { return Err(bad()); }
        if let Some(tail) = path.strip_prefix("data/") {
            let digest_part = tail.strip_suffix(".bin").or_else(|| tail.strip_suffix(".json"));
            if digest_part == Some(sha) && hash_valid(sha) { paths.insert(path.to_owned()); }
        }
    }
    let headers = cache_headers(paths);
    Ok(digest(headers.as_bytes()))
}
fn cache_headers(paths: BTreeSet<String>) -> String {
    paths.into_iter().take(100).map(|path| format!("/{path}\n  Cache-Control: public, max-age=86400, immutable\n")).collect::<Vec<_>>().join("\n")
}
/// A marker is an intent bound by the manifest, not evidence of public headers.
/// The caller must first validate the complete output with verify_output.
pub fn cache_marker_sha256(receipt: &Receipt) -> Result<Option<String>> {
    let manifest_bytes = bounded(&receipt.output, "static-export-manifest.json", 8 * 1024 * 1024)?;
    if digest(&manifest_bytes) != receipt.manifest_sha256 { return Err(bad()); }
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|_| bad())?;
    let Some(entry) = manifest.files.iter().find(|f| f.path == "static-cache-policy.json") else { return Ok(None); };
    let bytes = bounded(&receipt.output, "static-cache-policy.json", 1024)?;
    if bytes.len() as u64 != entry.bytes || digest(&bytes) != entry.sha256 { return Err(bad()); }
    let policy = cache_policy_sha256(receipt)?;
    if !valid_cache_marker(&bytes, &receipt.index_sha256, &policy) { return Err(bad()); }
    Ok(Some(policy))
}
fn valid_cache_marker(bytes: &[u8], index: &str, headers: &str) -> bool {
    hash_valid(index) && hash_valid(headers) && bytes == format!("{{\"version\":1,\"profile\":\"exhibitos-static-cache-v1\",\"indexSha256\":\"{index}\",\"headersSha256\":\"{headers}\"}}").as_bytes()
}
#[cfg(test)]
mod cache_policy_tests {
    use super::*;
    #[test]
    fn private_output_cannot_be_reconciled_as_an_ordinary_export() {
        let root = std::env::temp_dir().join(format!("exhibitos-static-mode-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let publication_id = Uuid::new_v4().to_string();
        let revision = "a".repeat(64);
        let index = b"{}";
        let index_hash = digest(index);
        let mut status: Status = serde_json::from_value(serde_json::json!({
            "format":1, "jobId":Uuid::new_v4(), "workspaceId":Uuid::new_v4(),
            "tenantId":Uuid::new_v4(), "principalId":Uuid::new_v4(),
            "publicationId":publication_id, "revisionSha256":revision,
            "output":root, "state":"running", "receipt":null, "errorCode":null
        })).unwrap();
        let write_bundle = |private_editor| {
            let mut index_html = format!("<meta content=\"{index_hash}\">");
            let mut entries = vec![("static-publication-index.json", index.to_vec())];
            if private_editor {
                index_html.push_str("<meta name=\"exhibitos-private-editor\" content=\"enabled\">");
                entries.push(("studio.html", format!("<meta content=\"{index_hash}\">").into_bytes()));
            }
            entries.push(("index.html", index_html.into_bytes()));
            let files: Vec<_> = entries.into_iter().map(|(name, bytes)| {
                fs::write(root.join(name), &bytes).unwrap();
                serde_json::json!({"path":name,"bytes":bytes.len(),"sha256":digest(&bytes)})
            }).collect();
            fs::write(root.join("static-export-manifest.json"), serde_json::to_vec(&serde_json::json!({
                "format":"exhibitos-static-publication-v1", "publicationId":publication_id,
                "revisionSha256":revision, "indexSha256":index_hash, "files":files
            })).unwrap()).unwrap();
        };
        write_bundle(false);
        assert!(!status.private_editor);
        assert!(verify_output(&status).is_ok());
        status.private_editor = true;
        assert!(verify_output(&status).is_err());
        write_bundle(true);
        assert!(verify_output(&status).is_ok());
        status.private_editor = false;
        assert!(verify_output(&status).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn headers_match_fixed_profile_and_rule_limit() {
        let paths = (0..102).rev().map(|i| format!("data/{i:064x}.bin")).collect();
        let headers = cache_headers(paths);
        assert_eq!(headers.lines().filter(|line| line.starts_with("/data/")).count(), 100);
        assert!(headers.starts_with(&format!("/data/{:064x}.bin\n  Cache-Control: public, max-age=86400, immutable\n", 0)));
        assert!(!headers.contains(&format!("/data/{:064x}.bin", 100)));
        assert!(headers.ends_with('\n'));
        assert!(cache_headers(BTreeSet::new()).is_empty());
    }
    #[test]
    fn marker_is_canonical_and_binds_exact_index_and_policy() {
        let index = "a".repeat(64); let policy = "b".repeat(64);
        let marker = format!("{{\"version\":1,\"profile\":\"exhibitos-static-cache-v1\",\"indexSha256\":\"{index}\",\"headersSha256\":\"{policy}\"}}");
        assert!(valid_cache_marker(marker.as_bytes(), &index, &policy));
        assert!(!valid_cache_marker(format!("{marker}\n").as_bytes(), &index, &policy));
        assert!(!valid_cache_marker(marker.as_bytes(), &"c".repeat(64), &policy));
        assert!(!valid_cache_marker(marker.as_bytes(), &index, &"d".repeat(64)));
        assert!(!valid_cache_marker(marker.replace("exhibitos-static-cache-v1", "static-data-sha256-24h-v1").as_bytes(), &index, &policy));
    }
    #[test]
    fn marker_read_is_bound_to_manifest_and_verified_index() {
        let root = std::env::temp_dir().join(format!("exhibitos-cache-marker-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let payload = b"synthetic publication"; let payload_hash = digest(payload);
        let path = format!("data/{payload_hash}.json"); fs::create_dir(root.join("data")).unwrap();
        fs::write(root.join(&path), payload).unwrap();
        let index = serde_json::to_vec(&serde_json::json!({"version":1,"publication":{"path":path,"bytes":payload.len(),"sha256":payload_hash},"assets":[]})).unwrap();
        let index_hash = digest(&index); fs::write(root.join("static-publication-index.json"), &index).unwrap();
        let headers = format!("/{path}\n  Cache-Control: public, max-age=86400, immutable\n"); let policy = digest(headers.as_bytes());
        let marker = format!("{{\"version\":1,\"profile\":\"exhibitos-static-cache-v1\",\"indexSha256\":\"{index_hash}\",\"headersSha256\":\"{policy}\"}}");
        fs::write(root.join("static-cache-policy.json"), &marker).unwrap();
        let manifest = serde_json::to_vec(&serde_json::json!({"format":"exhibitos-static-publication-v1","publicationId":Uuid::new_v4().to_string(),"revisionSha256":"a".repeat(64),"indexSha256":index_hash,"files":[{"path":path,"bytes":payload.len(),"sha256":payload_hash},{"path":"static-publication-index.json","bytes":index.len(),"sha256":index_hash},{"path":"static-cache-policy.json","bytes":marker.len(),"sha256":digest(marker.as_bytes())}]})).unwrap();
        fs::write(root.join("static-export-manifest.json"), &manifest).unwrap();
        let receipt = Receipt { publication_id: Uuid::new_v4().to_string(), revision_sha256:"a".repeat(64), index_sha256:index_hash, manifest_sha256:digest(&manifest), files:4, bytes:0, output:root.clone() };
        assert_eq!(cache_marker_sha256(&receipt).unwrap(), Some(policy));
        fs::write(root.join("static-cache-policy.json"), b"tampered").unwrap();
        assert!(cache_marker_sha256(&receipt).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
/// Full exact-file verification is also the crash/lost-result reconciliation.
/// A receipt alone, directory existence or successful exit is never sufficient.
pub fn verify_output(status: &Status) -> Result<Receipt> {
    verify_export(status, &|| Ok(()))
}

fn verify_export(status: &Status, check: &impl Fn() -> Result<()>) -> Result<Receipt> {
    check()?;
    let parent = pinned_output_parent(status)?;
    if !status.output.is_absolute()
        || fs::symlink_metadata(&status.output)
            .map_err(|_| bad())?
            .is_symlink()
        || fs::canonicalize(&status.output).map_err(|_| bad())? != status.output
    {
        return Err(bad());
    }
    let raw = bounded(
        &status.output,
        "static-export-manifest.json",
        8 * 1024 * 1024,
    )?;
    let manifest = export_manifest(status, &raw)?;
    let mut expected = BTreeSet::from(["static-export-manifest.json".to_string()]);
    let mut bytes = raw.len() as u64;
    for item in &manifest.files {
        check()?;
        if !safe_name(&item.path)
            || !expected.insert(item.path.clone())
            || !hash_valid(&item.sha256)
            || item.bytes > 25 * 1024 * 1024
        {
            return Err(bad());
        }
        let value = bounded(&status.output, &item.path, 25 * 1024 * 1024)?;
        if value.len() as u64 != item.bytes || digest(&value) != item.sha256 {
            return Err(bad());
        }
        bytes = bytes.checked_add(item.bytes).ok_or_else(bad)?;
    }
    fn walk(
        root: &Path,
        at: &Path,
        depth: usize,
        names: &mut BTreeSet<String>,
        check: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        if depth > 16 {
            return Err(bad());
        }
        for entry in fs::read_dir(at).map_err(|_| bad())? {
            check()?;
            let path = entry.map_err(|_| bad())?.path();
            let stat = fs::symlink_metadata(&path).map_err(|_| bad())?;
            if stat.is_symlink() {
                return Err(bad());
            }
            if stat.is_dir() {
                walk(root, &path, depth + 1, names, check)?;
            } else if stat.is_file() {
                let name = path
                    .strip_prefix(root)
                    .map_err(|_| bad())?
                    .to_str()
                    .ok_or_else(bad)?;
                if names.len() >= 20_000 || !safe_name(name) {
                    return Err(bad());
                }
                names.insert(name.into());
            } else {
                return Err(bad());
            }
        }
        Ok(())
    }
    let mut actual = BTreeSet::new();
    walk(&status.output, &status.output, 0, &mut actual, check)?;
    if actual != expected
        || !expected.contains("index.html")
        || expected.contains("studio.html") != status.private_editor
    {
        return Err(bad());
    }
    verify_export_controls(status, &manifest, |name| {
        bounded(&status.output, name, 1024 * 1024)
    })?;
    if let Some(parent) = parent { parent.check().map_err(|_| bad())?; }
    Ok(Receipt {
        publication_id: status.publication_id.clone(),
        revision_sha256: status.revision_sha256.clone(),
        index_sha256: manifest.index_sha256,
        manifest_sha256: digest(&raw),
        files: expected.len() as u64,
        bytes,
        output: status.output.clone(),
    })
}
fn export_manifest(status: &Status, raw: &[u8]) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(raw).map_err(|_| bad())?;
    if manifest.format != "exhibitos-static-publication-v1"
        || manifest.publication_id != status.publication_id
        || manifest.revision_sha256 != status.revision_sha256
        || !hash_valid(&manifest.index_sha256)
        || manifest.files.is_empty()
        || manifest.files.len() >= 20_000
    {
        return Err(bad());
    }
    Ok(manifest)
}

fn verify_export_controls(
    status: &Status,
    manifest: &Manifest,
    mut read: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<()> {
    if digest(&read("static-publication-index.json")?) != manifest.index_sha256 {
        return Err(bad());
    }
    let html = read("index.html")?;
    let html = String::from_utf8_lossy(&html);
    let private_marker = "<meta name=\"exhibitos-private-editor\" content=\"enabled\">";
    if !html.contains(&manifest.index_sha256)
        || html.contains(private_marker) != status.private_editor
    {
        return Err(bad());
    }
    if status.private_editor
        && !String::from_utf8_lossy(&read("studio.html")?).contains(&manifest.index_sha256)
    {
        return Err(bad());
    }
    Ok(())
}

const DETAIL_UNCONFIRMED: &str = "NATIVE_STATIC_DETAIL_CLEANUP_UNCONFIRMED";
const DETAIL_DURABILITY: &str = "NATIVE_STATIC_DETAIL_DURABILITY_UNCERTAIN";
const DETAIL_CANCELLED: &str = "NATIVE_STATIC_CANCELLED";
const DETAIL_DEADLINE: Duration = Duration::from_secs(120);
const STARTUP_LIMIT: usize = 4096;
const CONTROL_LIMIT: usize = 1024;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectoryIdentity {
    device: u64,
    inode: u64,
}
impl DirectoryIdentity {
    fn read(directory: &File) -> Result<Self> {
        let stat = directory.metadata().map_err(|_| bad())?;
        if !stat.is_dir() {
            return Err(bad());
        }
        Ok(Self {
            device: stat.dev(),
            inode: stat.ino(),
        })
    }
}

const STATIC_COMMIT_UNCONFIRMED: &str = "NATIVE_STATIC_OUTPUT_COMMIT_UNCONFIRMED";

fn committed_output_available(status: &Status) -> bool {
    status.error_code.as_deref() == Some(STATIC_COMMIT_UNCONFIRMED)
        && status.receipt.is_some()
        && pinned_output_parent(status).is_ok_and(|parent| parent.is_some())
        && fs::symlink_metadata(&status.output).is_ok_and(|meta| meta.is_dir())
}

fn pinned_output_parent(status: &Status) -> Result<Option<DirectoryPin>> {
    status.output_parent_identity.map(|expected| {
        let pin = DirectoryPin::open(status.output.parent().ok_or_else(bad)?)
            .map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))?;
        let file = pin.try_clone_file().map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))?;
        if DirectoryIdentity::read(&file)? != expected || check_private_detail_directory(&file).is_err() {
            return Err(err("NATIVE_STATIC_OUTPUT_INVALID"));
        }
        Ok(pin)
    }).transpose()
}

// The ordinary worker still exports using its existing Platform contract. For
// installed CLI exports it writes only inside a private managed-profile stage.
struct PinnedOrdinaryOutput {
    parent: DirectoryPin,
    staging_root: DirectoryPin,
    profile: DirectoryPin,
    worker: Status,
}
impl PinnedOrdinaryOutput {
    fn create(profile: &Path, status: &Status, parent: DirectoryPin) -> Result<Self> {
        let profile_pin = DirectoryPin::open(profile)?;
        let profile_file = profile_pin.try_clone_file()?;
        check_private_detail_directory(&profile_file)?;
        if DirectoryIdentity::read(&profile_file)?.device
            != DirectoryIdentity::read(&parent.try_clone_file()?)?.device
        {
            return Err(err("NATIVE_STATIC_OUTPUT_INVALID"));
        }
        let name = format!(".native-static-stage-{}", status.job_id);
        let file = create_detail_directory(&profile_file, OsStr::new(&name))?;
        let stage = profile.join(name);
        let staging_root = DirectoryPin::open(&stage)?;
        if DirectoryIdentity::read(&file)? != DirectoryIdentity::read(&staging_root.try_clone_file()?)? {
            return Err(bad());
        }
        let mut worker = status.clone();
        worker.output = stage.join("output");
        worker.output_parent_identity = None;
        Ok(Self { parent, staging_root, profile: profile_pin, worker })
    }

    fn verify(&self, receipt: Receipt) -> Result<(Receipt, File)> {
        let stage = self.staging_root.try_clone_file()?;
        let output = open_detail_at(&stage, OsStr::new("output"), libc::O_RDONLY | libc::O_DIRECTORY)?;
        let check = || {
            self.profile.check()?;
            self.staging_root.check()?;
            let named = open_detail_at(&stage, OsStr::new("output"), libc::O_RDONLY | libc::O_DIRECTORY)?;
            if DirectoryIdentity::read(&output)? != DirectoryIdentity::read(&named)? {
                return Err(bad());
            }
            Ok(())
        };
        let verified = verify_export(&self.worker, &check)?;
        if receipt != verified {
            return Err(bad());
        }
        check()?;
        Ok((verified, output))
    }

    fn commit(&self, target: &Path, mut receipt: Receipt, output: &File) -> Result<(Receipt, bool)> {
        let parent = self.parent.try_clone_file().map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))?;
        let stage = self.staging_root.try_clone_file()?;
        let leaf = detail_leaf(target.file_name().ok_or_else(bad)?)?;
        let named = open_detail_at(&stage, OsStr::new("output"), libc::O_RDONLY | libc::O_DIRECTORY)?;
        if DirectoryIdentity::read(output)? != DirectoryIdentity::read(&named)? {
            return Err(bad());
        }
        self.parent.check().map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))?;
        if unsafe { libc::renameatx_np(stage.as_raw_fd(), c"output".as_ptr(),
                                       parent.as_raw_fd(), leaf.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                err("NATIVE_STATIC_OUTPUT_EXISTS")
            } else { bad() });
        }
        let confirmed = parent.sync_all().is_ok() && self.parent.check().is_ok();
        // Return only the published destination. Best-effort cleanup may leave
        // an empty private stage, but never removes an unrelated directory.
        if let (Ok(profile), Some(name)) = (self.profile.try_clone_file(), self.worker.output.parent().and_then(Path::file_name)) {
            let _ = unlink_exact_detail(&profile, name, &stage, true);
        }
        receipt.output = target.into();
        Ok((receipt, confirmed))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetailJournal {
    mode: DetailMode,
    status: Status,
    runtime_generation: String,
    parent: DirectoryIdentity,
    wrapper: DirectoryIdentity,
    stage: DirectoryIdentity,
    destination_basename: String,
    manifest_sha256: Option<String>,
    child_group: Option<u32>,
    child_returned: bool,
    cleanup: Option<DetailCleanup>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DetailCleanup {
    Cancelled,
    Collision,
    Failed,
}
impl DetailCleanup {
    fn from_error(error: &LifecycleError) -> Self {
        match error.code.as_str() {
            DETAIL_CANCELLED => Self::Cancelled,
            "NATIVE_STATIC_OUTPUT_EXISTS" => Self::Collision,
            _ => Self::Failed,
        }
    }
    fn status(self, journal: &DetailJournal) -> Status {
        let mut status = journal.status.clone();
        let (state, error) = match self {
            Self::Cancelled => ("cancelled", DETAIL_CANCELLED),
            Self::Collision => ("failed", "NATIVE_STATIC_OUTPUT_EXISTS"),
            Self::Failed => ("failed", "NATIVE_STATIC_EXPORT_FAILED"),
        };
        status.state = state.into();
        status.error_code = Some(error.into());
        status
    }
}
impl DetailJournal {
    fn validate(&self, identity: &(String, String, String)) -> Result<()> {
        if self.status.format != 1
            || self.status.state != "running"
            || self.status.receipt.is_some()
            || self.status.error_code.is_some()
            || !uuid(&self.status.job_id)
            || !uuid(&self.status.publication_id)
            || !uuid(&self.runtime_generation)
            || !hash_valid(&self.status.revision_sha256)
            || !self.status.output.is_absolute()
            || self.status.output.file_name() != Some(OsStr::new(&self.destination_basename))
            || self.parent.device != self.wrapper.device
            || self.parent.device != self.stage.device
            || self
                .child_group
                .is_some_and(|group| !(2..=i32::MAX as u32).contains(&group))
            || self.child_returned && self.child_group.is_none()
            || self
                .manifest_sha256
                .as_deref()
                .is_some_and(|hash| !hash_valid(hash))
            || (
                &self.status.workspace_id,
                &self.status.tenant_id,
                &self.status.principal_id,
            ) != (&identity.0, &identity.1, &identity.2)
        {
            return Err(bad());
        }
        Ok(())
    }
    fn child_drained(&self) -> bool {
        self.child_returned || self.child_group.is_some_and(owned_child_group_absent)
    }
    fn held_status(&self, error: &str) -> Status {
        let mut status = self.status.clone();
        status.state = "interrupted".into();
        status.error_code = Some(error.into());
        status
    }
    fn wrapper_name(&self) -> String {
        format!(".exhibitos-detail-{}", self.status.job_id)
    }
}

fn detail_leaf(name: &OsStr) -> Result<CString> {
    if name.is_empty() || name == "." || name == ".." || name.as_bytes().contains(&b'/') {
        return Err(bad());
    }
    CString::new(name.as_bytes()).map_err(|_| bad())
}
fn open_detail_at(parent: &File, name: &OsStr, flags: libc::c_int) -> Result<File> {
    let name = detail_leaf(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(bad());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn detail_stat_at(parent: &File, name: &OsStr) -> Result<Option<libc::stat>> {
    let name = detail_leaf(name)?;
    let mut stat = std::mem::MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(unsafe { stat.assume_init() }));
    }
    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(bad())
}

fn unlink_exact_detail(parent: &File, name: &OsStr, held: &File, directory: bool) -> Result<()> {
    let named = detail_stat_at(parent, name)?.ok_or_else(bad)?;
    let stat = held.metadata().map_err(|_| bad())?;
    let expected_kind = if directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    };
    if (named.st_dev as u64, named.st_ino) != (stat.dev(), stat.ino())
        || named.st_mode & libc::S_IFMT != expected_kind
    {
        return Err(bad());
    }
    let name = detail_leaf(name)?;
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(bad());
    }
    Ok(())
}

struct DetailDirectoryStream(*mut libc::DIR);
impl Drop for DetailDirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
fn detail_directory_names(directory: &File, limit: usize) -> Result<Vec<OsString>> {
    // Reopen "." relative to the pinned directory for a private enumeration offset.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(bad());
    }
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err(bad());
    }
    let stream = DetailDirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *libc::__error() } != 0 {
                return Err(bad());
            }
            return Ok(names);
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        if names.len() == limit {
            return Err(bad());
        }
        names.push(OsString::from_vec(name.to_vec()));
    }
}

fn create_detail_directory(parent: &File, name: &OsStr) -> Result<File> {
    let leaf = detail_leaf(name)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), leaf.as_ptr(), 0o700) } != 0 {
        return Err(bad());
    }
    let directory = open_detail_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    check_private_detail_directory(&directory)?;
    Ok(directory)
}
fn check_private_detail_directory(directory: &File) -> Result<()> {
    let stat = directory.metadata().map_err(|_| bad())?;
    if !stat.is_dir() || stat.uid() != unsafe { libc::geteuid() } || stat.mode() & 0o777 != 0o700 {
        return Err(bad());
    }
    Ok(())
}

struct DetailStage {
    parent: DirectoryPin,
    parent_file: File,
    wrapper: File,
    directory: File,
    #[cfg(test)]
    path: PathBuf,
}
impl DetailStage {
    fn create(status: &Status) -> Result<Self> {
        let parent_path = status.output.parent().ok_or_else(bad)?;
        let parent = DirectoryPin::open(parent_path)?;
        let parent_file = parent.try_clone_file()?;
        let wrapper_name = format!(".exhibitos-detail-{}", status.job_id);
        let wrapper = create_detail_directory(&parent_file, OsStr::new(&wrapper_name))?;
        let directory = create_detail_directory(&wrapper, OsStr::new("output"))?;
        directory
            .sync_all()
            .and_then(|_| wrapper.sync_all())
            .and_then(|_| parent_file.sync_all())
            .map_err(|_| bad())?;
        parent.check()?;
        Ok(Self {
            parent,
            parent_file,
            wrapper,
            directory,
            #[cfg(test)]
            path: parent_path.join(wrapper_name).join("output"),
        })
    }
    fn check(&self, journal: &DetailJournal) -> Result<()> {
        self.parent.check()?;
        let wrapper = open_detail_at(
            &self.parent_file,
            OsStr::new(&journal.wrapper_name()),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        check_private_detail_directory(&wrapper)?;
        let stage = open_detail_at(
            &wrapper,
            OsStr::new("output"),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        if DirectoryIdentity::read(&self.parent_file)? != journal.parent
            || DirectoryIdentity::read(&wrapper)? != journal.wrapper
            || DirectoryIdentity::read(&stage)? != journal.stage
        {
            return Err(bad());
        }
        Ok(())
    }
    fn promote(&self, journal: &DetailJournal) -> Result<()> {
        self.check(journal)?;
        let stage = c"output";
        let destination = detail_leaf(OsStr::new(&journal.destination_basename))?;
        if unsafe {
            libc::renameatx_np(
                self.wrapper.as_raw_fd(),
                stage.as_ptr(),
                self.parent_file.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        } != 0
        {
            return Err(
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                    err("NATIVE_STATIC_OUTPUT_EXISTS")
                } else {
                    bad()
                },
            );
        }
        Ok(())
    }
    fn sync_parents(&self) -> Result<()> {
        self.wrapper
            .sync_all()
            .and_then(|_| self.parent_file.sync_all())
            .map_err(|_| err(DETAIL_DURABILITY))
    }
}

fn open_promoted_detail(journal: &DetailJournal) -> Result<Option<(DirectoryPin, File)>> {
    let parent_path = journal.status.output.parent().ok_or_else(bad)?;
    let parent = DirectoryPin::open(parent_path)?;
    let directory = parent.try_clone_file()?;
    if DirectoryIdentity::read(&directory)? != journal.parent {
        return Err(bad());
    }
    if promoted_identity(journal, &parent, &directory)? {
        Ok(Some((parent, directory)))
    } else {
        Ok(None)
    }
}
fn promoted_identity(
    journal: &DetailJournal,
    parent: &DirectoryPin,
    directory: &File,
) -> Result<bool> {
    parent.check()?;
    let leaf = OsStr::new(&journal.destination_basename);
    let Some(named) = detail_stat_at(directory, leaf)? else {
        return Ok(false);
    };
    if named.st_mode & libc::S_IFMT != libc::S_IFDIR
        || (named.st_dev as u64, named.st_ino) != (journal.stage.device, journal.stage.inode)
    {
        return Ok(false);
    }
    let destination = open_detail_at(
        directory,
        OsStr::new(&journal.destination_basename),
        libc::O_RDONLY | libc::O_DIRECTORY,
    )?;
    Ok(DirectoryIdentity::read(&destination)? == journal.stage)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct DetailStamp {
    device: u64,
    inode: u64,
    mode: libc::mode_t,
    uid: u32,
    gid: u32,
    links: u64,
    size: i64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl From<libc::stat> for DetailStamp {
    fn from(stat: libc::stat) -> Self {
        Self {
            device: stat.st_dev as u64,
            inode: stat.st_ino,
            mode: stat.st_mode,
            uid: stat.st_uid,
            gid: stat.st_gid,
            links: u64::from(stat.st_nlink),
            size: stat.st_size,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        }
    }
}

impl DetailStamp {
    fn held(file: &File) -> Result<Self> {
        let mut stat = std::mem::MaybeUninit::uninit();
        if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(bad());
        }
        Ok(unsafe { stat.assume_init() }.into())
    }

    fn check_named(self, parent: &File, name: &OsStr) -> Result<()> {
        if detail_stat_at(parent, name)?.map(Self::from) != Some(self) {
            return Err(bad());
        }
        Ok(())
    }

    fn is_directory(self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFDIR
    }
}

fn open_stage_entry(parent: &File, name: &OsStr) -> Result<(File, DetailStamp)> {
    let stamp = DetailStamp::from(detail_stat_at(parent, name)?.ok_or_else(bad)?);
    let flags = match stamp.mode & libc::S_IFMT {
        libc::S_IFDIR => libc::O_RDONLY | libc::O_DIRECTORY,
        libc::S_IFREG => libc::O_RDONLY,
        _ => return Err(bad()),
    };
    let file = open_detail_at(parent, name, flags)?;
    if DetailStamp::held(&file)? != stamp {
        return Err(bad());
    }
    Ok((file, stamp))
}

struct DetailTree {
    stamp: DetailStamp,
    entries: BTreeMap<OsString, DetailNode>,
}

enum DetailNode {
    File(DetailStamp),
    Directory(DetailTree),
}

impl DetailNode {
    fn stamp(&self) -> DetailStamp {
        match self {
            Self::File(stamp) => *stamp,
            Self::Directory(tree) => tree.stamp,
        }
    }
}

impl DetailTree {
    fn recheck(&self, directory: &File, check: &impl Fn() -> Result<()>) -> Result<()> {
        check()?;
        let names = detail_directory_names(directory, self.entries.len())?;
        if DetailStamp::held(directory)? != self.stamp || names.len() != self.entries.len() {
            return Err(bad());
        }
        for name in names {
            check()?;
            let entry = self.entries.get(&name).ok_or_else(bad)?;
            if let DetailNode::Directory(tree) = entry {
                let child = open_detail_at(directory, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
                tree.recheck(&child, check)?;
            }
            entry.stamp().check_named(directory, &name)?;
        }
        if DetailStamp::held(directory)? != self.stamp {
            return Err(bad());
        }
        Ok(())
    }
}

fn verify_and_sync_detail_file(
    file: &File,
    stamp: DetailStamp,
    limit: u64,
    check: &impl Fn() -> Result<()>,
    sync: &impl Fn(&File) -> std::io::Result<()>,
) -> Result<(Vec<u8>, String)> {
    if stamp.mode & libc::S_IFMT != libc::S_IFREG
        || stamp.links != 1
        || stamp.size <= 0
        || stamp.size as u64 > limit
    {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad())?;
    if bytes.len() as i64 != stamp.size || DetailStamp::held(file)? != stamp {
        return Err(bad());
    }
    let hash = digest(&bytes);
    check()?;
    sync(file).map_err(|_| err(DETAIL_DURABILITY))?;
    if DetailStamp::held(file)? != stamp {
        return Err(bad());
    }
    Ok((bytes, hash))
}

struct DetailVerifier<'a> {
    root: DetailStamp,
    manifest: DetailStamp,
    expected: BTreeMap<&'a str, &'a ManifestFile>,
    controls: BTreeMap<String, Vec<u8>>,
    remaining: usize,
}

impl DetailVerifier<'_> {
    fn capture(
        &mut self,
        directory: &File,
        prefix: &str,
        depth: usize,
        check: &impl Fn() -> Result<()>,
    ) -> Result<DetailTree> {
        check()?;
        let stamp = DetailStamp::held(directory)?;
        if depth > 16
            || !stamp.is_directory()
            || stamp.device != self.root.device
            || stamp.uid != self.root.uid
            || stamp.mode & 0o022 != 0
            || depth == 0 && stamp != self.root
        {
            return Err(bad());
        }
        let names = detail_directory_names(directory, self.remaining)?;
        self.remaining -= names.len();
        let mut entries = BTreeMap::new();
        for name in names {
            check()?;
            let leaf = name.to_str().ok_or_else(bad)?;
            let path = format!("{prefix}{leaf}");
            if !safe_name(&path) {
                return Err(bad());
            }
            let (item, held) = open_stage_entry(directory, &name)?;
            let entry = if held.is_directory() {
                DetailNode::Directory(self.capture(&item, &format!("{path}/"), depth + 1, check)?)
            } else if path == "static-export-manifest.json" {
                if held != self.manifest {
                    return Err(bad());
                }
                DetailNode::File(held)
            } else {
                let expected = self.expected.remove(path.as_str()).ok_or_else(bad)?;
                let control = matches!(
                    path.as_str(),
                    "index.html" | "studio.html" | "static-publication-index.json"
                );
                let limit = if control {
                    1024 * 1024
                } else {
                    25 * 1024 * 1024
                };
                if held.device != self.root.device
                    || held.uid != self.root.uid
                    || held.mode & 0o022 != 0
                    || held.size as u64 != expected.bytes
                {
                    return Err(bad());
                }
                let (bytes, hash) =
                    verify_and_sync_detail_file(&item, held, limit, check, &File::sync_all)?;
                if hash != expected.sha256 {
                    return Err(bad());
                }
                if control {
                    self.controls.insert(path, bytes);
                }
                DetailNode::File(held)
            };
            entry.stamp().check_named(directory, &name)?;
            entries.insert(name, entry);
        }
        check()?;
        directory.sync_all().map_err(|_| err(DETAIL_DURABILITY))?;
        if DetailStamp::held(directory)? != stamp {
            return Err(bad());
        }
        Ok(DetailTree { stamp, entries })
    }
}

fn verify_detail_stage(
    stage: &DetailStage,
    journal: &DetailJournal,
    expected_manifest: &str,
    check: &impl Fn() -> Result<()>,
) -> Result<(Receipt, DetailTree)> {
    check()?;
    stage.check(journal)?;
    let root = DetailStamp::held(&stage.directory)?;
    let manifest_name = OsStr::new("static-export-manifest.json");
    let (manifest_file, stamp) = open_stage_entry(&stage.directory, manifest_name)?;
    if stamp.device != root.device || stamp.uid != root.uid || stamp.mode & 0o022 != 0 {
        return Err(bad());
    }
    let (raw, manifest_hash) = verify_and_sync_detail_file(
        &manifest_file,
        stamp,
        8 * 1024 * 1024,
        check,
        &File::sync_all,
    )?;
    if manifest_hash != expected_manifest {
        return Err(bad());
    }
    stamp.check_named(&stage.directory, manifest_name)?;
    let manifest = export_manifest(&journal.status, &raw)?;
    let mut verifier = DetailVerifier {
        root,
        manifest: stamp,
        expected: BTreeMap::new(),
        controls: BTreeMap::new(),
        remaining: 40_000,
    };
    let mut bytes = raw.len() as u64;
    for item in &manifest.files {
        if !safe_name(&item.path)
            || item.path == "static-export-manifest.json"
            || !hash_valid(&item.sha256)
            || item.bytes > 25 * 1024 * 1024
            || verifier.expected.insert(&item.path, item).is_some()
        {
            return Err(bad());
        }
        bytes = bytes.checked_add(item.bytes).ok_or_else(bad)?;
    }
    let tree = verifier.capture(&stage.directory, "", 0, check)?;
    if !verifier.expected.is_empty()
        || !matches!(tree.entries.get(manifest_name), Some(DetailNode::File(stamp)) if *stamp == verifier.manifest)
        || verifier.controls.contains_key("studio.html") != journal.status.private_editor
    {
        return Err(bad());
    }
    verify_export_controls(&journal.status, &manifest, |name| {
        verifier.controls.remove(name).ok_or_else(bad)
    })?;
    check()?;
    stage
        .wrapper
        .sync_all()
        .map_err(|_| err(DETAIL_DURABILITY))?;
    Ok((
        Receipt {
            publication_id: journal.status.publication_id.clone(),
            revision_sha256: journal.status.revision_sha256.clone(),
            index_sha256: manifest.index_sha256,
            manifest_sha256: manifest_hash,
            files: manifest.files.len() as u64 + 1,
            bytes,
            output: journal.status.output.clone(),
        },
        tree,
    ))
}

fn clear_detail_tree(directory: &File, depth: usize, remaining: &mut usize) -> Result<()> {
    if depth > 16 {
        return Err(bad());
    }
    let names = detail_directory_names(directory, *remaining)?;
    *remaining -= names.len();
    for name in names {
        let item = open_detail_at(directory, &name, libc::O_RDONLY)?;
        let stat = item.metadata().map_err(|_| bad())?;
        if stat.is_dir() {
            clear_detail_tree(&item, depth + 1, remaining)?;
        } else if !stat.is_file() || stat.nlink() != 1 {
            return Err(bad());
        }
        unlink_exact_detail(directory, &name, &item, stat.is_dir())?;
    }
    directory.sync_all().map_err(|_| bad())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Startup<'a> {
    version: u32,
    job_id: &'a str,
    runtime_generation: &'a str,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    publication_id: &'a str,
    revision_sha256: &'a str,
    origin: &'a str,
    session_token: &'a str,
    csrf_token: &'a str,
}
struct StartupFrame(Vec<u8>);
impl StartupFrame {
    fn encode(
        status: &Status,
        authority: &StaticDetailAuthority<'_>,
        origin: &str,
    ) -> Result<Self> {
        let token = |value: &str| {
            value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        };
        if !uuid(authority.runtime_generation)
            || !token(authority.session_token)
            || !token(authority.csrf_token)
        {
            return Err(bad());
        }
        let mut frame = Self(Vec::with_capacity(STARTUP_LIMIT + 4));
        frame.0.extend_from_slice(&[0; 4]);
        serde_json::to_writer(
            &mut frame.0,
            &Startup {
                version: 1,
                job_id: &status.job_id,
                runtime_generation: authority.runtime_generation,
                workspace_id: authority.workspace_id,
                tenant_id: authority.tenant_id,
                principal_id: authority.principal_id,
                publication_id: &status.publication_id,
                revision_sha256: &status.revision_sha256,
                origin,
                session_token: authority.session_token,
                csrf_token: authority.csrf_token,
            },
        )
        .map_err(|_| bad())?;
        let size = frame.0.len() - 4;
        if !(1..=STARTUP_LIMIT).contains(&size) {
            return Err(bad());
        }
        frame.0[..4].copy_from_slice(&(size as u32).to_be_bytes());
        Ok(frame)
    }
}
impl Drop for StartupFrame {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // Keep the wipe observable to the optimizer before deallocation.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetailFrame {
    version: u32,
    kind: FrameKind,
    job_id: String,
    manifest_sha256: String,
}
#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FrameKind {
    Staged,
    VerifiedStaging,
    Ready,
}

fn read_detail_frame(
    input: &mut impl Read,
    check: &impl Fn() -> Result<()>,
) -> Result<DetailFrame> {
    let mut prefix = [0; 4];
    detail_read(input, &mut prefix, check)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if !(1..=CONTROL_LIMIT).contains(&size) {
        return Err(bad());
    }
    let mut bytes = vec![0; size];
    detail_read(input, &mut bytes, check)?;
    let frame: DetailFrame = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if frame.version != 1 || !uuid(&frame.job_id) || !hash_valid(&frame.manifest_sha256) {
        return Err(bad());
    }
    Ok(frame)
}
fn detail_read(
    input: &mut impl Read,
    mut bytes: &mut [u8],
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    while !bytes.is_empty() {
        check()?;
        match input.read(bytes) {
            Ok(0) => return Err(bad()),
            Ok(n) => bytes = &mut bytes[n..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return Err(bad()),
        }
    }
    Ok(())
}
fn detail_write(
    output: &mut impl Write,
    mut bytes: &[u8],
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    while !bytes.is_empty() {
        check()?;
        match output.write(bytes) {
            Ok(0) => return Err(bad()),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return Err(bad()),
        }
    }
    Ok(())
}
fn detail_nonblocking(fd: libc::c_int) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(bad());
    }
    Ok(())
}
fn detail_stdout_end(output: &mut impl Read, check: &impl Fn() -> Result<()>) -> Result<()> {
    loop {
        check()?;
        match output.read(&mut [0]) {
            Ok(0) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            _ => return Err(bad()),
        }
    }
}

fn check_detail(deadline: Instant, cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(err(DETAIL_CANCELLED));
    }
    if Instant::now() >= deadline {
        return Err(err("NATIVE_STATIC_DETAIL_TIMEOUT"));
    }
    Ok(())
}
// A reused group is conservatively still present. Never signal a reopened group.
fn owned_child_group_absent(group: u32) -> bool {
    let Ok(group) = libc::pid_t::try_from(group) else {
        return false;
    };
    if group < 2 {
        return false;
    }
    (unsafe { libc::kill(-group, 0) }) == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}
fn stop_detail_child(child: &mut Child) -> bool {
    // Let the closed stdin reach the cooperative JS cancellation first.
    for grace in [Duration::from_millis(250), Duration::from_secs(5)] {
        let until = Instant::now() + grace;
        while Instant::now() < until {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            thread::sleep(Duration::from_millis(25));
        }
        if grace == Duration::from_millis(250) {
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
        }
    }
    let _ = child.kill();
    child.wait().is_ok()
}
fn wait_detail_child(child: &mut Child, check: &impl Fn() -> Result<()>) -> Result<()> {
    loop {
        check()?;
        match child.try_wait().map_err(|_| bad())? {
            Some(exit) if exit.success() => return Ok(()),
            Some(_) => return Err(bad()),
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn detail_child_command(
    node: &Path,
    script: &Path,
    origin: &str,
    publication_id: &str,
    runtime: &Path,
    private_editor: bool,
) -> Command {
    let mut command = Command::new(node);
    command.args([
        script.as_os_str(),
        OsStr::new(origin),
        OsStr::new(publication_id),
        runtime.as_os_str(),
        OsStr::new("."),
    ]);
    if private_editor {
        command.arg("--private-editor");
    }
    // Child diagnostics may contain fetch errors; no detail stderr is retained.
    command
        .arg("--native-detail-v1")
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LANG", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

fn spawn_detail_child(mut command: Command, stage: &File) -> std::io::Result<Child> {
    let stage_fd = stage.as_raw_fd();
    // The borrowed stage stays open until spawn returns. The child changes cwd
    // through that descriptor before exec; it needs no inherited stage FD.
    unsafe {
        command.pre_exec(move || {
            if libc::fchdir(stage_fd) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

struct DetailRun {
    payload: Arc<Payload>,
    script: PathBuf,
    viewer: PathBuf,
    origin: String,
    startup: StartupFrame,
    stage: DetailStage,
    journal: DetailJournal,
}
impl DetailRun {
    fn run(
        self,
        controller: &InstallationController,
        token: &str,
        cancel: &AtomicBool,
        commit_gate: &Mutex<()>,
    ) -> Result<()> {
        let Self {
            payload,
            script,
            viewer,
            origin,
            startup,
            stage,
            mut journal,
        } = self;
        let deadline = Instant::now() + DETAIL_DEADLINE;
        let check = || check_detail(deadline, cancel);
        let command = detail_child_command(
            &payload.node(),
            &script,
            &origin,
            &journal.status.publication_id,
            &viewer,
            journal.status.private_editor,
        );
        let startup_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        let mut child = match check()
            .and_then(|_| stage.check(&journal))
            .and_then(|_| {
                spawn_detail_child(command, &stage.directory)
                    .map(OwnedChild)
                    .map_err(|_| bad())
            })
        {
            Ok(child) => child,
            Err(error) => {
                drop(startup);
                // No child exists to admit a server request. This job can be retired.
                return finish_detail_failure(controller, token, &mut journal, error);
            }
        };
        let mut stdin = child.0.stdin.take();
        let mut stdout = child.0.stdout.take();
        let group = child.0.id();
        journal.child_group = Some(group);
        if controller
            .write_detail_journal(token, &journal, false)
            .is_err()
        {
            drop(startup);
            drop(stdin);
            drop(stdout);
            if stop_detail_child(&mut child.0) && owned_child_group_absent(group) {
                journal.child_returned = true;
                let _ = controller.write_detail_journal(token, &journal, false);
            }
            return Err(err(DETAIL_UNCONFIRMED));
        }
        let started = (|| {
            let input = stdin.as_mut().ok_or_else(bad)?;
            detail_nonblocking(input.as_raw_fd())?;
            detail_write(input, &startup.0, &|| {
                check_detail(startup_deadline, cancel)
            })
        })();
        drop(startup);
        let verified = (|| -> Result<(Receipt, DetailTree)> {
            started?;
            let input = stdin.as_mut().ok_or_else(bad)?;
            let output = stdout.as_mut().ok_or_else(bad)?;
            detail_nonblocking(output.as_raw_fd())?;
            let staged = read_detail_frame(output, &check)?;
            if staged.kind != FrameKind::Staged || staged.job_id != journal.status.job_id {
                return Err(bad());
            }
            // STAGED relinquishes all child writes to this tree.
            let (receipt, tree) = verify_detail_stage(&stage, &journal, &staged.manifest_sha256, &check)?;
            journal.manifest_sha256 = Some(staged.manifest_sha256);
            controller.write_detail_journal(token, &journal, false)?;
            let ack = serde_json::to_vec(&DetailFrame {
                version: 1,
                kind: FrameKind::VerifiedStaging,
                job_id: journal.status.job_id.clone(),
                manifest_sha256: receipt.manifest_sha256.clone(),
            })
            .map_err(|_| bad())?;
            tree.recheck(&stage.directory, &check)?;
            stage.check(&journal)?;
            detail_write(input, &(ack.len() as u32).to_be_bytes(), &check)?;
            detail_write(input, &ack, &check)?;
            let ready = read_detail_frame(output, &check)?;
            if ready.kind != FrameKind::Ready
                || ready.job_id != journal.status.job_id
                || ready.manifest_sha256 != receipt.manifest_sha256
            {
                return Err(bad());
            }
            detail_stdout_end(output, &check)?;
            wait_detail_child(&mut child.0, &check)?;
            if !owned_child_group_absent(group) {
                return Err(err(DETAIL_UNCONFIRMED));
            }
            Ok((receipt, tree))
        })();
        drop(stdin);
        drop(stdout);
        let (receipt, tree) = match verified {
            Ok(verified) => verified,
            Err(_) => {
                // Reaping is necessary but a lost READY is never server settlement.
                if stop_detail_child(&mut child.0) && owned_child_group_absent(group) {
                    journal.child_returned = true;
                    controller.write_detail_journal(token, &journal, false)?;
                }
                return Err(err(DETAIL_UNCONFIRMED));
            }
        };
        journal.child_returned = true;
        let promoted = commit_detail(&stage, &journal, commit_gate, &|| {
            tree.recheck(&stage.directory, &check)?;
            check()
        });
        if let Err(error) = promoted {
            return finish_detail_failure(controller, token, &mut journal, error);
        }
        let finished = stage.sync_parents().and_then(|()| {
            controller.finish_detail_journal(token, &journal, complete_detail(&journal, receipt))
        });
        if finished.is_err() {
            // Keep or re-establish the active record when retirement could not
            // be verified. A matching rename is reconciled only by identity.
            let _ = controller.write_detail_journal(token, &journal, false);
            return Err(err(DETAIL_DURABILITY));
        }
        // A caller-writable parent has no atomic compare-and-unlink.
        // Keep the empty private wrapper instead of removing its mutable basename.
        Ok(())
    }
}
fn commit_detail(
    stage: &DetailStage,
    journal: &DetailJournal,
    gate: &Mutex<()>,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    let _commit = gate.lock().map_err(|_| bad())?;
    check()?;
    stage.promote(journal)
}

fn complete_detail(journal: &DetailJournal, receipt: Receipt) -> Status {
    let mut status = journal.status.clone();
    status.state = "completed".into();
    status.receipt = Some(receipt);
    status
}
fn finish_detail_failure(
    controller: &InstallationController,
    token: &str,
    journal: &mut DetailJournal,
    error: LifecycleError,
) -> Result<()> {
    // The caller proved no child ran, or positively proved child and server drain.
    // Durable cleanup authorization precedes removal of any staged path.
    journal.cleanup = Some(DetailCleanup::from_error(&error));
    controller.write_detail_journal(token, journal, false)?;
    resume_detail_cleanup(controller, token, journal).map(|_| ())
}

fn resume_detail_cleanup(
    controller: &InstallationController,
    token: &str,
    journal: &DetailJournal,
) -> Result<Status> {
    let cleanup = journal.cleanup.ok_or_else(bad)?;
    remove_detail_stage(journal)?;
    let status = cleanup.status(journal);
    controller.finish_detail_journal(token, journal, status.clone())?;
    Ok(status)
}

fn remove_detail_stage(journal: &DetailJournal) -> Result<()> {
    let parent_path = journal.status.output.parent().ok_or_else(bad)?;
    let parent = DirectoryPin::open(parent_path)?;
    let parent_file = parent.try_clone_file()?;
    if DirectoryIdentity::read(&parent_file)? != journal.parent
        || promoted_identity(journal, &parent, &parent_file)?
    {
        return Err(bad());
    }
    let wrapper_name = journal.wrapper_name();
    let wrapper_name = OsStr::new(&wrapper_name);
    if detail_stat_at(&parent_file, wrapper_name)?.is_some() {
        let wrapper = open_detail_at(
            &parent_file,
            wrapper_name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        check_private_detail_directory(&wrapper)?;
        if DirectoryIdentity::read(&wrapper)? != journal.wrapper {
            return Err(bad());
        }
        let stage_name = OsStr::new("output");
        if detail_stat_at(&wrapper, stage_name)?.is_some() {
            let stage = open_detail_at(&wrapper, stage_name, libc::O_RDONLY | libc::O_DIRECTORY)?;
            if DirectoryIdentity::read(&stage)? != journal.stage {
                return Err(bad());
            }
            clear_detail_tree(&stage, 0, &mut 40_000)?;
            unlink_exact_detail(&wrapper, stage_name, &stage, true)?;
            wrapper.sync_all().map_err(|_| bad())?;
        }
        // The outer name is in a caller-controlled parent. Keep the empty
        // wrapper and refuse to retire while unrelated entries are present.
        detail_directory_names(&wrapper, 0)?;
    }
    parent.check()?;
    parent_file.sync_all().map_err(|_| bad())
}

fn detail_journal_name(static_name: &str) -> String {
    format!("detail-{static_name}")
}
fn read_detail_file(
    root: &Path,
    name: &str,
    identity: &(String, String, String),
) -> Result<Option<DetailJournal>> {
    match fs::symlink_metadata(root.join(name)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(bad()),
        Ok(_) => {
            let journal: DetailJournal = serde_json::from_slice(
                &installation_backup::source_bytes(root, name, 65536, true)?,
            )
            .map_err(|_| bad())?;
            journal.validate(identity)?;
            Ok(Some(journal))
        }
    }
}

impl InstallationController {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions, target_os = "macos"))]
    pub(crate) fn check_catalog45_recovery_export_unlocked(
        &self, service: &LifecycleService, generation: &str, document: &native_document::DocumentState,
    ) -> Result<()> {
        let root = &self.profile.as_ref().ok_or_else(bad)?.root;
        let name = format!("native-static-export-{}.json", digest(service.root.as_os_str().as_encoded_bytes()));
        let identity = service.personal_identity()?;
        let (_, upload) = self.upload_unlocked(service)?;
        let (_, publication_state) = self.publication_unlocked(service)?;
        let publication = publication_state.completed.as_ref().ok_or_else(bad)?;
        if upload.intent.is_some() || upload.job_id.is_some() || upload.file_name.is_some() || upload.compact_variant.is_some()
            || document.etag.as_deref() != Some(publication.source_etag.as_str())
            || document.draft.as_ref().and_then(|draft| draft["exhibitionId"].as_str()) != Some(publication.source_exhibition_id.as_str())
            || publication.mode.as_deref() != Some("native-original-v1")
            || !publication_state.native_original_attempts.get(&publication.publish_request_id).is_some_and(|attempt|
                attempt.export_claimed && Some(attempt.plan_binding_sha256.as_str()) == publication.plan_binding_sha256.as_deref()
            )
        {
            return Err(bad());
        }
        let detail_name = detail_journal_name(&name);
        if read_detail_file(root, &detail_name, &identity)?.is_some() {
            return Err(bad());
        }
        let terminal_name = format!("terminal-{detail_name}");
        let (detail, status): (DetailJournal, Status) =
            serde_json::from_slice(&installation_backup::source_bytes(
                root, &terminal_name, 65536, true,
            )?).map_err(|_| bad())?;
        detail.validate(&identity)?;
        let latest: Status = serde_json::from_slice(&installation_backup::source_bytes(
            root, &name, 65536, true,
        )?).map_err(|_| bad())?;
        if detail.runtime_generation != generation
            || !matches!(detail.cleanup, Some(DetailCleanup::Cancelled))
            || detail.child_group.is_some() || detail.child_returned
            || detail.manifest_sha256.is_some()
            || !detail.status.private_editor
            || detail.status.publication_id != publication.publication.publication_id
            || detail.status.revision_sha256 != publication.publication.revision_sha256
            || DetailCleanup::Cancelled.status(&detail) != status
            || status != latest
        {
            return Err(bad());
        }
        let parent = DirectoryPin::open(detail.status.output.parent().ok_or_else(bad)?)?;
        let directory = parent.try_clone_file()?;
        let wrapper = open_detail_at(
            &directory, OsStr::new(&detail.wrapper_name()), libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        check_private_detail_directory(&wrapper)?;
        if DirectoryIdentity::read(&directory)? != detail.parent
            || DirectoryIdentity::read(&wrapper)? != detail.wrapper
            || detail_stat_at(&directory, OsStr::new(&detail.destination_basename))?.is_some()
            || !detail_directory_names(&wrapper, 0)?.is_empty()
        {
            return Err(bad());
        }
        parent.check()
    }

    fn static_export_location(
        &self,
        token: &str,
    ) -> Result<(PathBuf, String, (String, String, String))> {
        self.with_current(token, |service| {
            let profile = self
                .profile
                .as_ref()
                .ok_or_else(|| err("NATIVE_MANAGED_PROFILE_REQUIRED"))?;
            Ok((
                profile.root.clone(),
                format!(
                    "native-static-export-{}.json",
                    digest(service.root.as_os_str().as_encoded_bytes())
                ),
                service.personal_identity()?,
            ))
        })
    }
    fn read_detail_journal(&self, token: &str) -> Result<Option<DetailJournal>> {
        let Some(profile) = &self.profile else { return Ok(None); };
        self.with_current(token, |service| {
            let _lock = profile_lock(profile)?;
            let static_name = format!("native-static-export-{}.json",
                digest(service.root.as_os_str().as_encoded_bytes()));
            let name = detail_journal_name(&static_name);
            match fs::symlink_metadata(profile.root.join(&name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(bad()),
                Ok(_) => read_detail_file(&profile.root, &name, &service.personal_identity()?),
            }
        })
    }
    fn write_detail_journal(&self, token: &str, journal: &DetailJournal, new: bool) -> Result<()> {
        let (root, name, identity) = self.static_export_location(token)?;
        let name = detail_journal_name(&name);
        let _lock = profile_lock(self.profile.as_ref().ok_or_else(bad)?)?;
        journal.validate(&identity)?;
        match read_detail_file(&root, &name, &identity)? {
            None if new => {}
            Some(previous) if !new && previous.status.job_id == journal.status.job_id => {}
            _ => return Err(err("BUSY")),
        }
        write_json(&root, &name, journal)?;
        File::open(&root).and_then(|directory| directory.sync_all()).map_err(|_| bad())
    }
    fn finish_detail_journal(&self, token: &str, journal: &DetailJournal, status: Status) -> Result<()> {
        let (root, name, identity) = self.static_export_location(token)?;
        let detail_name = detail_journal_name(&name);
        let _lock = profile_lock(self.profile.as_ref().ok_or_else(bad)?)?;
        let prior = read_detail_file(&root, &detail_name, &identity)?.ok_or_else(bad)?;
        if prior.status.job_id != journal.status.job_id || status.job_id != prior.status.job_id {
            return Err(bad());
        }
        let directory = File::open(&root).map_err(|_| bad())?;
        // Keep the correlated proof even after the active hold retires.
        let terminal_name = format!("terminal-{detail_name}");
        write_json(&root, &terminal_name, &(journal, &status))?;
        write_json(&root, &name, &status)?;
        directory.sync_all().map_err(|_| bad())?;
        fs::remove_file(root.join(&detail_name)).map_err(|_| bad())?;
        directory.sync_all().map_err(|_| {
            // Best effort restore of the still-authoritative hold on uncertainty.
            let _ = write_json(&root, &detail_name, &prior);
            err(DETAIL_DURABILITY)
        })
    }
    pub(super) fn read_static_export(&self, token: &str) -> Result<Option<Status>> {
        if let Some(detail) = self.read_detail_journal(token)? {
            return Ok(Some(detail.held_status(DETAIL_UNCONFIRMED)));
        }
        let (root, name, identity) = self.static_export_location(token)?;
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        match fs::symlink_metadata(root.join(&name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(bad()),
            Ok(_) => {
                let value: Status = serde_json::from_slice(&installation_backup::source_bytes(
                    &root, &name, 65536, true,
                )?)
                .map_err(|_| bad())?;
                if value.format != 1
                    || !uuid(&value.job_id)
                    || !uuid(&value.publication_id)
                    || !hash_valid(&value.revision_sha256)
                    || !value.output.is_absolute()
                    || !matches!(
                        value.state.as_str(),
                        "running" | "completed" | "cancelled" | "failed" | "interrupted"
                    )
                    || (
                        value.workspace_id.clone(),
                        value.tenant_id.clone(),
                        value.principal_id.clone(),
                    ) != identity
                {
                    return Err(bad());
                }
                Ok(Some(value))
            }
        }
    }
    fn write_static_export(&self, token: &str, value: &Status) -> Result<()> {
        let existing = self.read_static_export(token)?;
        if existing
            .as_ref()
            .is_some_and(|old| old.state == "running" && old.job_id != value.job_id)
        {
            return Err(err("BUSY"));
        }
        let (root, name, identity) = self.static_export_location(token)?;
        if (&value.workspace_id, &value.tenant_id, &value.principal_id)
            != (&identity.0, &identity.1, &identity.2)
        {
            return Err(bad());
        }
        let profile = self.profile.as_ref().ok_or_else(bad)?;
        let _lock = profile_lock(profile)?;
        write_json(&root, &name, value)?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))
    }
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
pub struct Exporter {
    controller: Arc<InstallationController>,
    state: Arc<Mutex<Option<Status>>>,
    cancel: Arc<AtomicBool>,
    commit_gate: Arc<Mutex<()>>,
    detail_token: Option<String>,
    commit_recovery_attempted: Option<String>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Exporter {
    pub fn new(controller: Arc<InstallationController>) -> Self {
        Self {
            controller,
            state: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            commit_gate: Arc::new(Mutex::new(())),
            detail_token: None,
            commit_recovery_attempted: None,
            worker: None,
        }
    }
    pub fn active(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| !w.is_finished())
    }
    pub fn cancel(&self) {
        let _commit = self.commit_gate.lock().unwrap_or_else(|error| error.into_inner());
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub(crate) fn detail_held(&self, token: &str) -> Result<bool> {
        Ok(self.controller.read_detail_journal(token)?.is_some())
    }
    pub(crate) fn stop_detail_child(&mut self, token: Option<&str>) -> Result<()> {
        let Some(token) = token.map(str::to_owned).or_else(|| self.detail_token.clone()) else {
            return Ok(());
        };
        if let Some(active_token) = &self.detail_token {
            if *active_token != token && self.active() {
                return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
            }
            if *active_token == token {
                self.cancel();
                if let Some(worker) = self.worker.take() {
                    worker.join().map_err(|_| err(DETAIL_UNCONFIRMED))?;
                }
            }
        }
        if let Some(journal) = self.controller.read_detail_journal(&token)? {
            if !journal.child_drained() && self.recover_detail(&token, &journal)?.is_none() {
                return Err(err(DETAIL_UNCONFIRMED));
            }
        } else if self.detail_token.as_deref() == Some(&token) {
            self.detail_token = None;
        }
        Ok(())
    }
    fn reap(&mut self) -> Result<()> {
        if self
            .worker
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
        {
            self.worker.take().unwrap().join().map_err(|_| bad())?;
        }
        Ok(())
    }
    pub fn status(&mut self, token: &str) -> Result<Option<Status>> {
        self.reap()?;
        if self.active() {
            let value = self.state.lock().map_err(|_| bad())?.clone();
            let identity = self.controller.static_export_location(token)?.2;
            if value.as_ref().is_some_and(|v| {
                (&v.workspace_id, &v.tenant_id, &v.principal_id)
                    != (&identity.0, &identity.1, &identity.2)
            }) {
                return Err(err("NATIVE_OTHER_INSTALLATION_ACTIVE"));
            }
            return Ok(value);
        }
        if let Some(journal) = self.controller.read_detail_journal(token)? {
            let recovered = self.recover_detail(token, &journal);
            return Ok(Some(match recovered {
                Ok(Some(status)) => status,
                Ok(None) => journal.held_status(DETAIL_UNCONFIRMED),
                Err(_) => journal.held_status(DETAIL_DURABILITY),
            }));
        }
        let value = self.controller.read_static_export(token)?;
        if let Some(value) = &value {
            // Reopening a running job never re-executes the exporter. Verify its
            // atomic final output asynchronously; retain incomplete files.
            let recover_commit = self.commit_recovery_attempted.as_deref() != Some(&value.job_id)
                && committed_output_available(value);
            if value.state == "running" || recover_commit {
                if recover_commit { self.commit_recovery_attempted = Some(value.job_id.clone()); }
                let mut checking = value.clone();
                checking.state = "running".into();
                self.launch(token.into(), checking.clone(), None, None);
                return Ok(Some(checking));
            }
        }
        Ok(value)
    }
    pub(crate) fn start(
        &mut self,
        token: String,
        input: Input,
        payload: Arc<Payload>,
        port: u16,
        detail_authority: Option<StaticDetailAuthority<'_>>,
    ) -> Result<Status> {
        let current = self.status(&token)?;
        if (self.active() && !input.saved_head_verified) || self.detail_held(&token)? {
            return Err(err("BUSY"));
        }
        let (workspace_id, tenant_id, principal_id, revision_sha256) = if let Some(head) = input.saved_head {
            if !input.saved_head_verified { return Err(err("NATIVE_SAVED_HEAD_PUBLICATION_UNCONFIRMED")); }
            let document = self.controller.native_document(&token)?;
            if document.pending.is_some() || document.revision != head.revision
                || document.etag.as_deref() != Some(head.etag.as_str())
                || document.saved_edit_version != Some(head.edit_version)
                || !document.draft.as_ref().is_some_and(|draft| {
                    super::native_document::draft_id(draft) == Some(head.exhibition_id.as_str())
                        && draft["editVersion"].as_u64() == Some(head.edit_version)
                })
            {
                return Err(err("NATIVE_DOCUMENT_STATE_CHANGED"));
            }
            (document.workspace_id, document.tenant_id, document.principal_id, head.revision_sha256)
        } else {
            let publication = self.controller.native_publication(&token)?;
            if publication.pending.is_some() {
                return Err(err("NATIVE_PUBLICATION_PENDING"));
            }
            let completed = publication.completed.ok_or_else(|| err("NATIVE_PUBLICATION_REQUIRED"))?;
            if completed.publication.publication_id != input.publication_id
                || completed.publication.status != "published"
            {
                return Err(err("NATIVE_PUBLICATION_STATE_CHANGED"));
            }
            (publication.workspace_id, publication.tenant_id, publication.principal_id, completed.publication.revision_sha256)
        };
        if input.saved_head_verified && let Some(status) = current
            && status.publication_id == input.publication_id && status.revision_sha256 == revision_sha256
            && status.output == input.output && status.output_parent_identity == input.output_parent_identity
            && status.private_editor && status.workspace_id == workspace_id && status.tenant_id == tenant_id
            && status.principal_id == principal_id
        {
            if status.state == "completed" && status.receipt.as_ref() != Some(&verify_output(&status)?) {
                return Err(bad());
            }
            return Ok(status);
        }
        if self.active() { return Err(err("BUSY")); }
        let parent = input.output.parent().ok_or_else(bad)?;
        if !input.output.is_absolute()
            || input.output.as_os_str().len() > 4096
            || input.output.file_name().is_none()
            || fs::canonicalize(parent).map_err(|_| err("NATIVE_STATIC_OUTPUT_INVALID"))? != parent
            || !fs::metadata(parent).map_err(|_| bad())?.is_dir()
        {
            return Err(err("NATIVE_STATIC_OUTPUT_INVALID"));
        }
        match fs::symlink_metadata(&input.output) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(err("NATIVE_STATIC_OUTPUT_EXISTS")),
        }
        // All optional capability bytes were included in Payload::open's exact
        // inventory. An older runtime is refused instead of searching PATH.
        let detail = input.detail_mode == Some(DetailMode::NativeDetailV1);
        if detail != detail_authority.is_some()
            || (input.output_parent_identity.is_some() && (detail || !input.private_editor))
        {
            return Err(bad());
        }
        let (script, viewer) = if detail {
            payload.static_detail_export_resources(input.private_editor)?
        } else {
            payload.static_export_resources(input.private_editor)?
        };
        let value = Status {
            format: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            workspace_id,
            tenant_id,
            principal_id,
            publication_id: input.publication_id,
            revision_sha256,
            output: input.output,
            output_parent_identity: input.output_parent_identity,
            state: "running".into(),
            private_editor: input.private_editor,
            receipt: None,
            error_code: None,
        };
        if let Some(authority) = detail_authority {
            if port != authority.port
                || (value.workspace_id.as_str(), value.tenant_id.as_str(), value.principal_id.as_str())
                    != (authority.workspace_id, authority.tenant_id, authority.principal_id)
            {
                return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH"));
            }
            let origin = format!("http://127.0.0.1:{port}");
            let startup = StartupFrame::encode(&value, &authority, &origin)?;
            let destination_basename = value.output.file_name().and_then(OsStr::to_str)
                .ok_or_else(bad)?.to_owned();
            let stage = DetailStage::create(&value)?;
            let journal = DetailJournal {
                mode: DetailMode::NativeDetailV1, status: value.clone(),
                runtime_generation: authority.runtime_generation.into(),
                parent: DirectoryIdentity::read(&stage.parent_file)?,
                wrapper: DirectoryIdentity::read(&stage.wrapper)?,
                stage: DirectoryIdentity::read(&stage.directory)?,
                destination_basename, manifest_sha256: None,
                child_group: None, child_returned: false, cleanup: None,
            };
            self.controller.write_detail_journal(&token, &journal, true)?;
            self.launch_detail(token, DetailRun { payload, script, viewer, origin, startup, stage, journal });
        } else {
            let pinned = pinned_output_parent(&value)?.map(|parent| {
                let profile = self.controller.static_export_location(&token)?.0;
                PinnedOrdinaryOutput::create(&profile, &value, parent)
            }).transpose()?;
            self.controller.write_static_export(&token, &value)?;
            self.launch(token, value.clone(), Some((payload, script, viewer, port)), pinned);
        }
        Ok(value)
    }
    fn launch_detail(&mut self, token: String, run: DetailRun) {
        self.detail_token = Some(token.clone());
        self.cancel.store(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let commit_gate = self.commit_gate.clone();
        let shared = self.state.clone();
        let controller = self.controller.clone();
        let journal = run.journal.clone();
        if let Ok(mut state) = shared.lock() {
            *state = Some(journal.status.clone());
        }
        self.worker = Some(thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                run.run(&controller, &token, &cancel, &commit_gate)));
            let value = match result {
                Ok(Ok(())) => controller.read_static_export(&token).ok().flatten()
                    .unwrap_or_else(|| journal.held_status(DETAIL_UNCONFIRMED)),
                Ok(Err(error)) if error.code == DETAIL_DURABILITY =>
                    journal.held_status(DETAIL_DURABILITY),
                _ => journal.held_status(DETAIL_UNCONFIRMED),
            };
            if let Ok(mut state) = shared.lock() {
                *state = Some(value);
            }
        }));
    }
    fn recover_detail(&self, token: &str, journal: &DetailJournal) -> Result<Option<Status>> {
        if journal.cleanup.is_some() {
            return resume_detail_cleanup(&self.controller, token, journal).map(Some);
        }
        let Some(manifest) = &journal.manifest_sha256 else { return Ok(None); };
        let Some((parent, directory)) = open_promoted_detail(journal)? else {
            return Ok(None);
        };
        let receipt = verify_output(&journal.status)?;
        if &receipt.manifest_sha256 != manifest || !promoted_identity(journal, &parent, &directory)? {
            return Err(bad());
        }
        let wrapper = open_detail_at(&directory, OsStr::new(&journal.wrapper_name()),
            libc::O_RDONLY | libc::O_DIRECTORY)?;
        if DirectoryIdentity::read(&wrapper)? != journal.wrapper { return Err(bad()); }
        // The rename is admitted only after the settled child terminal boundary.
        wrapper.sync_all().and_then(|_| directory.sync_all()).map_err(|_| bad())?;
        if !promoted_identity(journal, &parent, &directory)? { return Err(bad()); }
        let status = complete_detail(journal, receipt);
        self.controller.finish_detail_journal(token, journal, status.clone())?;
        Ok(Some(status))
    }
    pub(crate) fn reconcile_detail_stop(&self, token: &str) -> Result<()> {
        if self.active() { return Ok(()); }
        let Some(mut journal) = self.controller.read_detail_journal(token)? else { return Ok(()); };
        if self.recover_detail(token, &journal)?.is_some() || !journal.child_drained() {
            return Ok(());
        }
        let Ok(_fence) = self.controller.with_current(token, |service| {
            crate::native_runtime::detail_drained_generation(service, &journal.runtime_generation)
        }) else {
            return Ok(());
        };
        finish_detail_failure(&self.controller, token, &mut journal, err(DETAIL_CANCELLED))
    }
    fn launch(
        &mut self,
        token: String,
        mut value: Status,
        execute: Option<(Arc<Payload>, PathBuf, PathBuf, u16)>,
        pinned: Option<PinnedOrdinaryOutput>,
    ) {
        self.detail_token = None;
        self.cancel.store(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let commit_gate = self.commit_gate.clone();
        let shared = self.state.clone();
        let controller = self.controller.clone();
        if let Ok(mut state) = shared.lock() {
            *state = Some(value.clone());
        }
        self.worker = Some(thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(Receipt, bool)> {
                    if let Some((payload, script, viewer, port)) = execute {
                        let (root, _, _) = controller.static_export_location(&token)?;
                        let stdout_name = format!("native-static-{}.stdout", value.job_id);
                        let stderr_name = format!("native-static-{}.stderr", value.job_id);
                        use std::os::unix::fs::OpenOptionsExt;
                        let create = |name: &str| {
                            OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .mode(0o600)
                                .open(root.join(name))
                                .map_err(|_| bad())
                        };
                        let stdout = create(&stdout_name)?;
                        let stderr = create(&stderr_name)?;
                        let worker = pinned.as_ref().map_or(&value, |output| &output.worker);
                        let mut command = std::process::Command::new(payload.node());
                        command.args([
                                    script.as_os_str(),
                                    std::ffi::OsStr::new(&format!("http://127.0.0.1:{port}")),
                                    std::ffi::OsStr::new(&value.publication_id),
                                    viewer.as_os_str(),
                                    worker.output.as_os_str(),
                                ]);
                        if value.private_editor {
                            command.arg("--private-editor");
                        }
                        let mut child = OwnedChild(
                            command.env_clear()
                                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                                .env("LANG", "en_US.UTF-8")
                                .stdin(Stdio::piped())
                                .stdout(Stdio::from(stdout))
                                .stderr(Stdio::from(stderr))
                                .spawn()
                                .map_err(|_| err("NATIVE_STATIC_EXPORT_FAILED"))?,
                        );
                        let deadline = std::time::Instant::now() + Duration::from_secs(300);
                        let mut stopping = None;
                        let exit = loop {
                            if let Some(exit) = child.0.try_wait().map_err(|_| bad())? {
                                break exit;
                            }
                            let overflow = [&stdout_name, &stderr_name].iter().any(|n| {
                                fs::metadata(root.join(n)).map_or(true, |s| s.len() > 65536)
                            });
                            if stopping.is_none()
                                && (cancel.load(Ordering::SeqCst)
                                    || std::time::Instant::now() > deadline
                                    || overflow)
                            {
                                // Signal only this Child handle; no process-name or
                                // listener sweep and no output deletion.
                                unsafe {
                                    libc::kill(child.0.id() as i32, libc::SIGTERM);
                                }
                                stopping = Some(std::time::Instant::now());
                            }
                            if stopping.is_some_and(|at| at.elapsed() > Duration::from_secs(5)) {
                                let _ = child.0.kill();
                            }
                            thread::sleep(Duration::from_millis(50));
                        };
                        // Cancellation can race the final rename. A verified final
                        // result wins; otherwise keep every incomplete output.
                        if cancel.load(Ordering::SeqCst) {
                            if pinned.is_some() { return Err(err("NATIVE_STATIC_CANCELLED")); }
                            return verify_output(&value).map(|receipt| (receipt, true))
                                .map_err(|_| err("NATIVE_STATIC_CANCELLED"));
                        }
                        if !exit.success() || stopping.is_some() {
                            if value.private_editor && stopping.is_none()
                                && installation_backup::source_bytes(&root, &stderr_name, 1024, true)
                                    .is_ok_and(|bytes| bytes == b"STATIC_PRIVATE_EDITOR_RIGHTS\n")
                            {
                                return Err(err("NATIVE_STATIC_PRIVATE_EDITOR_RIGHTS"));
                            }
                            return Err(err("NATIVE_STATIC_EXPORT_FAILED"));
                        }
                        let raw =
                            installation_backup::source_bytes(&root, &stdout_name, 65536, true)?;
                        let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| bad())?;
                        if let Some(pinned) = &pinned {
                            let (receipt, output) = pinned.verify(receipt)?;
                            let _commit = commit_gate.lock().map_err(|_| bad())?;
                            if cancel.load(Ordering::SeqCst) { return Err(err("NATIVE_STATIC_CANCELLED")); }
                            return pinned.commit(&value.output, receipt, &output);
                        }
                        let verified = verify_output(&value)?;
                        if serde_json::to_value(receipt).map_err(|_| bad())?
                            != serde_json::to_value(&verified).map_err(|_| bad())?
                        {
                            return Err(bad());
                        }
                        return Ok((verified, true));
                    }
                    let verified = verify_output(&value).map_err(|_| err("NATIVE_STATIC_INTERRUPTED"))?;
                    if value.error_code.as_deref() == Some(STATIC_COMMIT_UNCONFIRMED) {
                        let parent = pinned_output_parent(&value)?.ok_or_else(bad)?;
                        if value.receipt.as_ref() != Some(&verified)
                            || parent.try_clone_file()?.sync_all().is_err() || parent.check().is_err()
                        {
                            return Err(err(STATIC_COMMIT_UNCONFIRMED));
                        }
                    }
                    Ok((verified, true))
                }));
            match result {
                Ok(Ok((receipt, confirmed))) => {
                    value.state = if confirmed { "completed" } else { "interrupted" }.into();
                    value.receipt = Some(receipt);
                    value.error_code = (!confirmed).then(|| STATIC_COMMIT_UNCONFIRMED.into());
                }
                _ if value.error_code.as_deref() == Some(STATIC_COMMIT_UNCONFIRMED) => {
                    // A verification failure does not erase the committed receipt.
                    value.state = "interrupted".into();
                }
                other => {
                    let code = match other {
                        Ok(Err(e)) => e.code,
                        _ => "NATIVE_STATIC_EXPORT_FAILED".into(),
                    };
                    value.state = match code.as_str() {
                        "NATIVE_STATIC_CANCELLED" => "cancelled",
                        "NATIVE_STATIC_INTERRUPTED" => "interrupted",
                        _ => "failed",
                    }
                    .into();
                    value.error_code = Some(code);
                }
            }
            if controller.write_static_export(&token, &value).is_err() {
                value.state = "failed".into();
                value.error_code = Some("STATE_UNAVAILABLE".into());
            }
            if let Ok(mut state) = shared.lock() {
                *state = Some(value);
            }
        }));
    }
}
impl Drop for Exporter {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;
    use std::io::Cursor;

    const JOB: &str = "30a37355-6bc3-4a94-af50-8eafcc7c77e2";

    #[test]
    fn saved_head_receipt_uses_the_exact_ordinary_request_and_rejects_caller_proof() {
        let mut wire = serde_json::json!({
            "publicationId": JOB, "output": "/tmp/export", "savedHead": {
                "exhibitionId": JOB, "requestId": JOB, "etag": format!("\"studio-r6-{}\"", "a".repeat(64)),
                "revision": 18, "editVersion": 6, "revisionSha256": "b".repeat(64)
            }
        });
        let input: Input = serde_json::from_value(wire.clone()).unwrap();
        assert!(!input.saved_head_verified);
        let mut head = input.saved_head.unwrap();
        let session_hash = "c".repeat(64);
        let query = head.receipt_query(JOB, JOB, JOB, &session_hash).unwrap();
        assert!(query.contains("r.payload_sha256='e3a34d5fb0b2c5246b094394e254459b7d2fd7ee93c44059af69ecab08f256b9'"));
        assert!(query.contains("p.draft_revision=6"));
        head.etag = format!("\"studio-r06-{}\"", "a".repeat(64));
        assert!(head.receipt_query(JOB, JOB, JOB, &session_hash).is_err());
        wire["savedHeadVerified"] = true.into();
        assert!(serde_json::from_value::<Input>(wire).is_err());
    }

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn static_detail_mode_is_explicit_and_closed() {
        let mut input = serde_json::json!({"publicationId": JOB, "output": "/tmp/export"});
        assert!(
            serde_json::from_value::<Input>(input.clone())
                .unwrap()
                .detail_mode
                .is_none()
        );
        input["detailMode"] = "native-detail-v1".into();
        assert!(
            serde_json::from_value::<Input>(input.clone())
                .unwrap()
                .detail_mode
                .is_some()
        );
        for unsupported in [
            serde_json::Value::Null,
            "native-detail-v2".into(),
            true.into(),
        ] {
            input["detailMode"] = unsupported;
            assert!(serde_json::from_value::<Input>(input.clone()).is_err());
        }
    }

    #[test]
    fn static_detail_startup_carries_exact_checked_runtime_generation() {
        let (root, _, journal) = stage_fixture();
        let status = &journal.status;
        let token = "S".repeat(43);
        let csrf = "C".repeat(43);
        let authority = StaticDetailAuthority {
            workspace_id: &status.workspace_id,
            tenant_id: &status.tenant_id,
            principal_id: &status.principal_id,
            runtime_generation: &journal.runtime_generation,
            port: 3001,
            session_token: &token,
            csrf_token: &csrf,
        };
        let frame = StartupFrame::encode(status, &authority, "http://127.0.0.1:3001").unwrap();
        let (prefix, body) = frame.0.split_at(4);
        assert_eq!(
            u32::from_be_bytes(prefix.try_into().unwrap()) as usize,
            body.len()
        );
        let wire: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 11);
        assert_eq!(wire["runtimeGeneration"], journal.runtime_generation);
        assert_eq!(wire["jobId"], status.job_id);
        assert_eq!(wire["origin"], "http://127.0.0.1:3001");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_requires_closed_frames_and_terminal_eof() {
        let staged = format!(
            r#"{{"version":1,"kind":"staged","jobId":"{JOB}","manifestSha256":"{}"}}"#,
            "a".repeat(64)
        );
        let mut input = Cursor::new(frame(staged.as_bytes()));
        let actual = read_detail_frame(&mut input, &|| Ok(())).unwrap();
        assert!(actual.kind == FrameKind::Staged);
        assert_eq!(actual.job_id, JOB);
        assert!(detail_stdout_end(&mut input, &|| Ok(())).is_ok());
        let mut trailing = Cursor::new([frame(staged.as_bytes()), vec![1]].concat());
        assert!(read_detail_frame(&mut trailing, &|| Ok(())).is_ok());
        assert!(detail_stdout_end(&mut trailing, &|| Ok(())).is_err());
        let duplicate = staged.replace(r#""version":1"#, r#""version":1,"version":1"#);
        let extra = staged.replace(r#""version":1"#, r#""version":1,"extra":1"#);
        for invalid in [
            frame(duplicate.as_bytes()),
            frame(extra.as_bytes()),
            frame(&[0xff]),
            frame(&vec![b'x'; CONTROL_LIMIT + 1]),
            frame(staged.as_bytes())[..8].to_vec(),
        ] {
            assert!(read_detail_frame(&mut Cursor::new(invalid), &|| Ok(())).is_err());
        }
    }

    #[test]
    fn static_detail_terminal_requires_owned_child_exit_zero() {
        for (code, expected) in [("0", true), ("7", false)] {
            let mut child = std::process::Command::new("/bin/sh")
                .args(["-c", &format!("exit {code}")])
                .spawn()
                .unwrap();
            let cancel = AtomicBool::new(false);
            let deadline = Instant::now() + Duration::from_secs(2);
            assert_eq!(
                wait_detail_child(&mut child, &|| check_detail(deadline, &cancel)).is_ok(),
                expected
            );
            child.wait().unwrap();
        }
    }

    #[test]
    fn ordinary_static_commit_refuses_replacement_output_parent() {
        let (root, _legacy_stage, journal) = stage_fixture();
        let profile = root.join("profile");
        let destination = root.join("destination");
        let moved = root.join("moved");
        let root_file = File::open(&root).unwrap();
        let _profile = create_detail_directory(&root_file, OsStr::new("profile")).unwrap();
        let parent = create_detail_directory(&root_file, OsStr::new("destination")).unwrap();
        let mut status = journal.status;
        status.output = destination.join("bundle");
        status.output_parent_identity = Some(DirectoryIdentity::read(&parent).unwrap());
        status.private_editor = true;
        let stage = PinnedOrdinaryOutput::create(&profile, &status, pinned_output_parent(&status).unwrap().unwrap()).unwrap();
        let stage_file = stage.staging_root.try_clone_file().unwrap();
        let output = create_detail_directory(&stage_file, OsStr::new("output")).unwrap();
        fs::write(stage.worker.output.join("index.html"), "fixture").unwrap();
        let receipt = Receipt {
            publication_id: status.publication_id.clone(),
            revision_sha256: status.revision_sha256.clone(),
            index_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            files: 1,
            bytes: 7,
            output: stage.worker.output.clone(),
        };
        fs::rename(&destination, &moved).unwrap();
        let _replacement = create_detail_directory(&root_file, OsStr::new("destination")).unwrap();
        assert!(pinned_output_parent(&status).is_err());
        assert!(stage.commit(&status.output, receipt.clone(), &output).is_err());
        assert!(!destination.join("bundle").exists() && !moved.join("bundle").exists());
        assert!(stage.worker.output.join("index.html").exists());
        fs::remove_dir(&destination).unwrap();
        fs::rename(&moved, &destination).unwrap();
        let (committed, confirmed) = stage.commit(&status.output, receipt, &output).unwrap();
        assert!(confirmed);
        assert_eq!(committed.output, status.output);
        assert_eq!(fs::read(status.output.join("index.html")).unwrap(), b"fixture");
        status.state = "interrupted".into();
        status.error_code = Some(STATIC_COMMIT_UNCONFIRMED.into());
        status.receipt = Some(committed);
        fs::rename(&destination, &moved).unwrap();
        let _replacement = create_detail_directory(&root_file, OsStr::new("destination")).unwrap();
        assert!(!committed_output_available(&status));
        fs::remove_dir(&destination).unwrap();
        fs::rename(&moved, &destination).unwrap();
        assert!(committed_output_available(&status));
        fs::remove_dir_all(&root).unwrap();
    }

    fn stage_fixture() -> (PathBuf, DetailStage, DetailJournal) {
        let root =
            std::env::temp_dir().join(format!("exhibitos-native-detail-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let status = Status {
            format: 1,
            job_id: JOB.into(),
            workspace_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(),
            principal_id: Uuid::new_v4().to_string(),
            publication_id: Uuid::new_v4().to_string(),
            revision_sha256: "b".repeat(64),
            output: root.join("bundle"),
            output_parent_identity: None,
            state: "running".into(),
            private_editor: false,
            receipt: None,
            error_code: None,
        };
        let stage = DetailStage::create(&status).unwrap();
        let journal = DetailJournal {
            mode: DetailMode::NativeDetailV1,
            status: status.clone(),
            runtime_generation: Uuid::new_v4().to_string(),
            parent: DirectoryIdentity::read(&stage.parent_file).unwrap(),
            wrapper: DirectoryIdentity::read(&stage.wrapper).unwrap(),
            stage: DirectoryIdentity::read(&stage.directory).unwrap(),
            destination_basename: "bundle".into(),
            manifest_sha256: None,
            child_group: None,
            child_returned: false,
            cleanup: None,
        };
        (root, stage, journal)
    }

    #[test]
    fn static_detail_child_uses_held_cwd_across_ancestor_replacement() {
        let (root, stage, mut journal) = stage_fixture();
        let wrapper = stage.path.parent().unwrap();
        let first_location = root.join("before-spawn");
        fs::rename(wrapper, &first_location).unwrap();
        fs::create_dir_all(&stage.path).unwrap();

        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf ready; read -r signal && printf pinned > proof",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = OwnedChild(spawn_detail_child(command, &stage.directory).unwrap());
        let mut ready = [0; 5];
        child
            .0
            .stdout
            .take()
            .unwrap()
            .read_exact(&mut ready)
            .unwrap();
        assert_eq!(&ready, b"ready");

        let final_location = root.join("after-spawn");
        fs::rename(first_location, &final_location).unwrap();
        child.0.stdin.take().unwrap().write_all(b"write\n").unwrap();
        assert!(child.0.wait().unwrap().success());
        assert_eq!(
            fs::read(final_location.join("output/proof")).unwrap(),
            b"pinned"
        );
        assert!(fs::read_dir(&stage.path).unwrap().next().is_none());

        assert!(stage.check(&journal).is_err());
        journal.cleanup = Some(DetailCleanup::Cancelled);
        assert!(remove_detail_stage(&journal).is_err());
        assert!(final_location.join("output/proof").is_file());
        assert!(stage.path.is_dir());
        assert!(!journal.status.output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_child_never_execs_when_fchdir_fails() {
        let (root, stage, _) = stage_fixture();
        let invalid_stage = stage.path.join("regular-file");
        fs::write(&invalid_stage, b"regular").unwrap();
        let file = File::open(invalid_stage).unwrap();
        let unexpected = root.join("unexpected");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf unexpected > \"$1\"", "fixture"])
            .arg(&unexpected);
        assert!(spawn_detail_child(command, &file).is_err());
        assert!(!unexpected.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct CandidateFreeFixture {
        node: PathBuf,
        cli: PathBuf,
        runtime: PathBuf,
        group_evidence: PathBuf,
        origin: String,
        publication_id: String,
        revision_sha256: String,
        runtime_generation: String,
    }

    #[test]
    #[ignore = "requires the queued Platform CLI and candidate-free loopback fixture"]
    fn static_detail_platform_cli_interop() {
        let fixture_path = std::env::var_os("EXHIBITOS_DETAIL_INTEROP_FIXTURE").unwrap();
        let fixture: CandidateFreeFixture =
            serde_json::from_slice(&fs::read(fixture_path).unwrap()).unwrap();
        let (root, stage, mut journal) = stage_fixture();
        journal.status.publication_id = fixture.publication_id;
        journal.status.revision_sha256 = fixture.revision_sha256;
        journal.runtime_generation = fixture.runtime_generation;
        let status = &journal.status;
        let authority = StaticDetailAuthority {
            workspace_id: &status.workspace_id,
            tenant_id: &status.tenant_id,
            principal_id: &status.principal_id,
            runtime_generation: &journal.runtime_generation,
            port: fixture.origin.rsplit_once(':').unwrap().1.parse().unwrap(),
            session_token: &"s".repeat(43),
            csrf_token: &"c".repeat(43),
        };
        let startup = StartupFrame::encode(status, &authority, &fixture.origin).unwrap();
        let command = detail_child_command(
            &fixture.node,
            &fixture.cli,
            &fixture.origin,
            &status.publication_id,
            &fixture.runtime,
            false,
        );
        let mut child = OwnedChild(spawn_detail_child(command, &stage.directory).unwrap());
        let group = child.0.id();
        fs::write(fixture.group_evidence, group.to_string()).unwrap();
        let mut input = child.0.stdin.take().unwrap();
        let mut output = child.0.stdout.take().unwrap();
        detail_nonblocking(input.as_raw_fd()).unwrap();
        detail_nonblocking(output.as_raw_fd()).unwrap();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(20);
        let check = || check_detail(deadline, &cancel);

        let wrapper = stage.path.parent().unwrap();
        let moved = root.join("moved-wrapper");
        fs::rename(wrapper, &moved).unwrap();
        fs::create_dir_all(&stage.path).unwrap();
        fs::write(stage.path.join("keep"), b"unrelated").unwrap();
        detail_write(&mut input, &startup.0, &check).unwrap();
        drop(startup);
        let staged = read_detail_frame(&mut output, &check).unwrap();
        assert!(staged.kind == FrameKind::Staged);
        assert_eq!(staged.job_id, status.job_id);
        assert!(moved.join("output/static-export-manifest.json").is_file());
        assert_eq!(fs::read(stage.path.join("keep")).unwrap(), b"unrelated");
        assert_eq!(fs::read_dir(&stage.path).unwrap().count(), 1);
        assert!(verify_detail_stage(&stage, &journal, &staged.manifest_sha256, &check).is_err());
        let unrelated = root.join("unrelated-wrapper");
        fs::rename(wrapper, &unrelated).unwrap();
        fs::rename(moved, wrapper).unwrap();
        let (receipt, tree) =
            verify_detail_stage(&stage, &journal, &staged.manifest_sha256, &check).unwrap();
        tree.recheck(&stage.directory, &check).unwrap();
        stage.check(&journal).unwrap();

        let ack = serde_json::to_vec(&DetailFrame {
            version: 1,
            kind: FrameKind::VerifiedStaging,
            job_id: status.job_id.clone(),
            manifest_sha256: receipt.manifest_sha256.clone(),
        })
        .unwrap();
        detail_write(&mut input, &(ack.len() as u32).to_be_bytes(), &check).unwrap();
        detail_write(&mut input, &ack, &check).unwrap();
        let ready = read_detail_frame(&mut output, &check).unwrap();
        assert!(ready.kind == FrameKind::Ready);
        assert_eq!(ready.job_id, status.job_id);
        assert_eq!(ready.manifest_sha256, receipt.manifest_sha256);
        detail_stdout_end(&mut output, &check).unwrap();
        wait_detail_child(&mut child.0, &check).unwrap();
        assert!(owned_child_group_absent(group));
        let gate = Mutex::new(());
        commit_detail(&stage, &journal, &gate, &|| {
            tree.recheck(&stage.directory, &check)?;
            check()
        })
        .unwrap();
        stage.sync_parents().unwrap();
        assert_eq!(
            verify_output(status).unwrap().manifest_sha256,
            receipt.manifest_sha256
        );
        assert_eq!(
            fs::read(unrelated.join("output/keep")).unwrap(),
            b"unrelated"
        );
        println!(
            "candidate-free CLI settled, child group {group} drained, Manager promoted {} files",
            receipt.files
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn write_detail_fixture(stage: &DetailStage, journal: &DetailJournal) -> String {
        let index = b"{}";
        let index_hash = digest(index);
        let html = format!("<meta content=\"{index_hash}\">");
        fs::create_dir(stage.path.join("assets")).unwrap();
        let files = [
            ("index.html", html.as_bytes()),
            ("static-publication-index.json", index),
            ("assets/payload", b"original"),
        ];
        let files: Vec<_> = files
            .into_iter()
            .map(|(path, bytes)| {
                fs::write(stage.path.join(path), bytes).unwrap();
                serde_json::json!({"path":path, "bytes":bytes.len(), "sha256":digest(bytes)})
            })
            .collect();
        let manifest = serde_json::to_vec(&serde_json::json!({
            "format": "exhibitos-static-publication-v1",
            "publicationId": journal.status.publication_id,
            "revisionSha256": journal.status.revision_sha256,
            "indexSha256": index_hash,
            "files": files,
        }))
        .unwrap();
        fs::write(stage.path.join("static-export-manifest.json"), &manifest).unwrap();
        digest(&manifest)
    }

    #[test]
    fn static_detail_recheck_refuses_replaced_ancestor_with_matching_bytes() {
        let (root, stage, journal) = stage_fixture();
        let manifest = write_detail_fixture(&stage, &journal);
        let (receipt, tree) = verify_detail_stage(&stage, &journal, &manifest, &|| Ok(())).unwrap();
        assert_eq!(receipt.manifest_sha256, manifest);
        assert_eq!(receipt.files, 4);
        tree.recheck(&stage.directory, &|| Ok(())).unwrap();
        stage.check(&journal).unwrap();

        fs::rename(stage.path.join("assets"), root.join("displaced-assets")).unwrap();
        fs::create_dir(stage.path.join("assets")).unwrap();
        fs::write(stage.path.join("assets/payload"), b"original").unwrap();
        assert!(tree.recheck(&stage.directory, &|| Ok(())).is_err());
        assert!(!journal.status.output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_recheck_refuses_same_inode_change_before_commit() {
        let (root, stage, journal) = stage_fixture();
        let manifest = write_detail_fixture(&stage, &journal);
        let (_, tree) = verify_detail_stage(&stage, &journal, &manifest, &|| Ok(())).unwrap();
        let payload = stage.path.join("assets/payload");
        let original_inode = fs::metadata(&payload).unwrap().ino();
        fs::write(&payload, b"modified").unwrap();
        let changed = File::open(&payload).unwrap();
        changed
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(60))
            .unwrap();
        assert_eq!(changed.metadata().unwrap().ino(), original_inode);
        let gate = Mutex::new(());
        assert!(
            commit_detail(&stage, &journal, &gate, &|| {
                tree.recheck(&stage.directory, &|| Ok(()))
            })
            .is_err()
        );
        assert!(!journal.status.output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_file_refuses_failed_sync_and_same_inode_mutation_during_sync() {
        let (root, stage, _) = stage_fixture();
        let payload = stage.path.join("payload");
        fs::write(&payload, b"original").unwrap();
        let file = open_detail_at(&stage.directory, OsStr::new("payload"), libc::O_RDONLY).unwrap();
        let stamp = DetailStamp::held(&file).unwrap();
        let failed = verify_and_sync_detail_file(&file, stamp, 8, &|| Ok(()), &|synced| {
            assert_eq!(synced.as_raw_fd(), file.as_raw_fd());
            Err(std::io::Error::other("injected sync failure"))
        });
        assert_eq!(failed.unwrap_err().code, DETAIL_DURABILITY);

        let file = open_detail_at(&stage.directory, OsStr::new("payload"), libc::O_RDONLY).unwrap();
        let changed = verify_and_sync_detail_file(&file, stamp, 8, &|| Ok(()), &|synced| {
            assert_eq!(synced.as_raw_fd(), file.as_raw_fd());
            fs::write(&payload, b"modified").unwrap();
            synced
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(60))
                .unwrap();
            synced.sync_all().unwrap();
            Ok(())
        });
        assert!(changed.is_err());
        assert_eq!(file.metadata().unwrap().ino(), stamp.inode);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_cancel_before_sync_stops_durability_work() {
        let (root, stage, _) = stage_fixture();
        fs::write(stage.path.join("payload"), b"original").unwrap();
        let file = open_detail_at(&stage.directory, OsStr::new("payload"), libc::O_RDONLY).unwrap();
        let stamp = DetailStamp::held(&file).unwrap();
        let cancelled =
            verify_and_sync_detail_file(&file, stamp, 8, &|| Err(err(DETAIL_CANCELLED)), &|_| {
                panic!("sync after cancellation")
            });
        assert_eq!(cancelled.unwrap_err().code, DETAIL_CANCELLED);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_stage_cancel_and_collision_leave_destination_untouched() {
        let (root, stage, journal) = stage_fixture();
        let status = &journal.status;
        let identity = (
            status.workspace_id.clone(),
            status.tenant_id.clone(),
            status.principal_id.clone(),
        );
        journal.validate(&identity).unwrap();
        assert!(
            journal
                .validate(&(identity.0, identity.1, Uuid::new_v4().to_string()))
                .is_err()
        );
        fs::write(stage.path.join("payload"), b"approved").unwrap();
        let gate = Mutex::new(());
        let error =
            commit_detail(&stage, &journal, &gate, &|| Err(err(DETAIL_CANCELLED))).unwrap_err();
        assert_eq!(error.code, DETAIL_CANCELLED);
        assert!(!status.output.exists());
        fs::create_dir(&status.output).unwrap();
        assert!(open_promoted_detail(&journal).unwrap().is_none());
        let error = commit_detail(&stage, &journal, &gate, &|| Ok(())).unwrap_err();
        assert_eq!(error.code, "NATIVE_STATIC_OUTPUT_EXISTS");
        assert!(fs::read_dir(&status.output).unwrap().next().is_none());
        assert_eq!(fs::read(stage.path.join("payload")).unwrap(), b"approved");
        fs::remove_dir(&status.output).unwrap();
        commit_detail(&stage, &journal, &gate, &|| Ok(())).unwrap();
        stage.sync_parents().unwrap();
        assert_eq!(
            fs::read(status.output.join("payload")).unwrap(),
            b"approved"
        );
        let (parent, directory) = open_promoted_detail(&journal).unwrap().unwrap();
        assert!(promoted_identity(&journal, &parent, &directory).unwrap());
        fs::rename(&status.output, root.join("displaced")).unwrap();
        fs::create_dir(&status.output).unwrap();
        fs::write(status.output.join("payload"), b"approved").unwrap();
        assert!(!promoted_identity(&journal, &parent, &directory).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn static_detail_cleanup_can_resume_after_wrapper_removal() {
        let (root, stage, mut journal) = stage_fixture();
        journal.cleanup = Some(DetailCleanup::Cancelled);
        fs::write(stage.path.join("partial"), b"owned partial output").unwrap();
        fs::create_dir(&journal.status.output).unwrap();
        fs::write(journal.status.output.join("keep"), b"unrelated destination").unwrap();
        let unrelated = stage.path.parent().unwrap().join("unexpected");
        fs::write(&unrelated, b"unrelated staging sibling").unwrap();
        assert!(remove_detail_stage(&journal).is_err());
        assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated staging sibling");
        fs::remove_file(unrelated).unwrap();
        remove_detail_stage(&journal).unwrap();
        let wrapper = stage.path.parent().unwrap();
        assert!(wrapper.is_dir());
        assert!(fs::read_dir(wrapper).unwrap().next().is_none());
        // The empty wrapper may later disappear independently. Restart still
        // retires the durable cleanup intent without touching the destination.
        fs::remove_dir(wrapper).unwrap();
        remove_detail_stage(&journal).unwrap();
        assert_eq!(
            fs::read(journal.status.output.join("keep")).unwrap(),
            b"unrelated destination"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_cleanup_refuses_swapped_stage_and_leaf() {
        let (root, stage, mut journal) = stage_fixture();
        journal.cleanup = Some(DetailCleanup::Cancelled);
        let file_path = stage.path.join("leaf");
        fs::write(&file_path, b"owned leaf").unwrap();
        let file = open_detail_at(&stage.directory, OsStr::new("leaf"), libc::O_RDONLY).unwrap();
        fs::rename(&file_path, stage.path.join("original-leaf")).unwrap();
        fs::write(&file_path, b"replacement leaf").unwrap();
        assert!(unlink_exact_detail(&stage.directory, OsStr::new("leaf"), &file, false).is_err());
        assert_eq!(fs::read(&file_path).unwrap(), b"replacement leaf");

        let displaced = stage.path.with_file_name("original-stage");
        fs::rename(&stage.path, &displaced).unwrap();
        fs::create_dir(&stage.path).unwrap();
        fs::write(stage.path.join("keep"), b"replacement tree").unwrap();
        assert!(remove_detail_stage(&journal).is_err());
        assert_eq!(
            fs::read(stage.path.join("keep")).unwrap(),
            b"replacement tree"
        );
        assert_eq!(
            fs::read(displaced.join("original-leaf")).unwrap(),
            b"owned leaf"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn static_detail_cannot_infer_drain_from_missing_or_live_process_group() {
        let (root, _, mut journal) = stage_fixture();
        assert!(!journal.child_drained());
        let current = unsafe { libc::getpgrp() } as u32;
        journal.child_group = Some(current);
        assert!(!journal.child_drained());
        assert!(!owned_child_group_absent(0));
        fs::remove_dir_all(root).unwrap();
    }
}
