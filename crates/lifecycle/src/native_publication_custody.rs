// SPDX-License-Identifier: Apache-2.0
//! Explicit custody of an empty, abandoned ordinary publication stage in the
//! qualified local validation runtime. The failed request remains recorded.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crate::maintenance_source::DirectoryPin;
use std::{ffi::CString, os::{fd::AsRawFd, unix::fs::MetadataExt}};

fn refused() -> LifecycleError { err("NATIVE_PUBLICATION_STAGE_CUSTODY_UNCONFIRMED") }

#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    old_request_id: String,
    new_request_id: String,
    document_sha256: String,
    legacy_journal_sha256: String,
    new_intent_sha256: String,
    stage_device: u64,
    stage_inode: u64,
}

#[derive(Eq, PartialEq)]
pub(crate) struct Plan {
    input: Input,
    root: PathBuf,
    owner: (String, String, String),
    source_id: String,
    source_revision: i32,
    source_edit_version: u64,
    source_draft: Value,
    copy_id: String,
    copy_revision: i32,
    copy_draft: Value,
}

fn studio_revision(etag: &str) -> Result<i32> {
    let (revision, hash) = etag.strip_prefix("\"studio-r").and_then(|v| v.strip_suffix('"'))
        .and_then(|v| v.split_once('-')).ok_or_else(refused)?;
    if !hash_valid(hash) { return Err(refused()); }
    revision.parse::<i32>().ok().filter(|n| *n > 0 && n.to_string() == revision).ok_or_else(refused)
}

impl InstallationController {
    pub(crate) fn ordinary_publication_custody_plan(&self, token: &str, input: &Input) -> Result<Plan> {
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(refused)?;
            let _guard = profile_lock(profile)?;
            if service.root != profile.root.join("local-runtime") || input.old_request_id == input.new_request_id
                || !uuid(&input.old_request_id) || !uuid(&input.new_request_id) || input.stage_inode == 0 {
                return Err(refused());
            }
            let (document_file, document) = self.document_unlocked(service)?;
            let (publication_file, publication) = self.publication_unlocked(service)?;
            let owner = service.personal_identity()?;
            let (draft, etag, edit_version) = match (&document.draft, &document.etag, document.saved_edit_version) {
                (Some(draft), Some(etag), Some(edit)) if document.pending.is_none() && draft["editVersion"].as_u64() == Some(edit) => (draft, etag, edit),
                _ => return Err(refused()),
            };
            let pending = publication.pending.as_ref().ok_or_else(refused)?;
            let copy_etag = pending.snapshot_etag.as_deref().ok_or_else(refused)?;
            let source_id = native_document::draft_id(draft).ok_or_else(refused)?;
            let copy_id = native_document::draft_id(&pending.snapshot).ok_or_else(refused)?;
            if publication.completed.is_some() || !publication.native_original_attempts.is_empty() || pending.mode.is_some()
                || pending.publish_request_id != input.old_request_id || pending.source_exhibition_id != source_id
                || pending.source_etag != *etag || source_id == copy_id
                || ![draft, &pending.snapshot].into_iter().all(|value| value["candidate"]["artworks"].as_array().is_some_and(Vec::is_empty)) {
                return Err(refused());
            }
            let pinned = |file: &str, limit, expected: &str| -> Result<Vec<u8>> {
                let bytes = installation_backup::source_bytes(&profile.root, file, limit, true)?;
                if digest(&bytes) != expected { return Err(refused()); }
                Ok(bytes)
            };
            pinned(&document_file, 2_200_000, &input.document_sha256)?;
            pinned(&publication_file, 2_200_000, &input.legacy_journal_sha256)?;
            let intent: Value = serde_json::from_slice(&pinned(&format!(".automation-saved-head-export-{}.json", input.new_request_id),
                8192, &input.new_intent_sha256)?).map_err(|_| refused())?;
            let request = &intent["request"];
            if intent["version"] != 1 || intent["runtimeManifestSHA256"] != super::native_static_export::SAVED_HEAD_RUNTIME
                || intent["exhibitionId"] != source_id || request["operation"] != "export-static-saved-head"
                || request["requestId"] != input.new_request_id || request["workspaceId"] != owner.0
                || request["tenantId"] != owner.1 || request["principalId"] != owner.2
                || request["expectedRevision"].as_u64() != Some(document.revision)
                || request["expectedEditVersion"].as_u64() != Some(edit_version) || request["command"]["etag"] != *etag {
                return Err(refused());
            }
            Ok(Plan { input: input.clone(), root: service.root.clone(), owner, source_id: source_id.into(),
                source_revision: studio_revision(etag)?, source_edit_version: edit_version, source_draft: draft.clone(),
                copy_id: copy_id.into(), copy_revision: studio_revision(copy_etag)?, copy_draft: pending.snapshot.clone() })
        })
    }
}

impl Plan {
    pub(crate) fn owner(&self) -> &(String, String, String) { &self.owner }

    pub(crate) fn absence_query(&self, session_hash: &str) -> Result<String> {
        let (_, tenant, principal) = &self.owner;
        let Input { old_request_id, new_request_id, .. } = &self.input;
        let Self { source_id, source_revision, source_edit_version, copy_id, copy_revision, .. } = self;
        let source = STANDARD.encode(serde_json::to_vec(&self.source_draft).map_err(|_| refused())?);
        let copy = STANDARD.encode(serde_json::to_vec(&self.copy_draft).map_err(|_| refused())?);
        // The exact JSONB comparisons pair the frozen journal ETags with
        // both owned DB revisions. Document bytes travel only on psql stdin.
        Ok(format!("SELECT EXISTS (
          SELECT 1 FROM public.auth_sessions a
          JOIN public.memberships m ON (m.tenant_id,m.user_id)=(a.tenant_id,a.user_id)
          JOIN public.users u ON u.id=a.user_id JOIN public.tenants t ON t.id=a.tenant_id
          JOIN public.exhibitions src ON src.tenant_id=a.tenant_id AND src.studio_owner_user_id=a.user_id
          JOIN public.exhibitions dst ON dst.tenant_id=a.tenant_id AND dst.studio_owner_user_id=a.user_id
          WHERE a.tenant_id='{tenant}' AND a.user_id='{principal}' AND a.token_hash='{session_hash}'
            AND NOT a.revoked AND a.expires_at>now() AND NOT u.disabled AND t.deleted_at IS NULL
            AND m.role IN ('admin','curator','artist') AND src.studio_managed AND dst.studio_managed
            AND src.deleted_at IS NULL AND dst.deleted_at IS NULL
            AND src.id='{source_id}' AND src.revision={source_revision} AND src.metadata->>'editVersion'='{source_edit_version}'
            AND src.metadata=convert_from(decode('{source}','base64'),'UTF8')::jsonb
            AND dst.id='{copy_id}' AND dst.revision={copy_revision} AND dst.metadata->>'editVersion'='1'
            AND dst.metadata=convert_from(decode('{copy}','base64'),'UTF8')::jsonb)
          AND NOT EXISTS (SELECT 1 FROM public.publication_requests WHERE tenant_id='{tenant}'
                          AND request_id IN ('{old_request_id}','{new_request_id}'))
          AND NOT EXISTS (SELECT 1 FROM public.studio_publications WHERE tenant_id='{tenant}')
          AND NOT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname=current_database()
                          AND backend_type='client backend' AND pid<>pg_backend_pid());")
    }

    fn stage_paths(&self) -> (String, String) {
        (format!("native-publication-{}", self.owner.0), format!(".retained-native-publication-{}", self.input.old_request_id))
    }

    fn absent(path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(_) => Err(refused()),
        }
    }

    fn empty_stage(&self, path: &Path) -> Result<DirectoryPin> {
        let stage = DirectoryPin::open(path)?;
        let meta = stage.try_clone_file()?.metadata().map_err(|_| refused())?;
        if meta.dev() != self.input.stage_device || meta.ino() != self.input.stage_inode
            || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o7777 != 0o700
            || fs::read_dir(path).map_err(|_| refused())?.next().is_some() {
            return Err(refused());
        }
        stage.check()?;
        Ok(stage)
    }

    pub(crate) fn check_stage(&self) -> Result<bool> {
        let (stage, retained) = self.stage_paths();
        let (stage, retained) = (self.root.join(stage), self.root.join(retained));
        let tenant_objects = self.root.join("blobs").join(&self.owner.1);
        private_directory(&tenant_objects)?;
        DirectoryPin::open(&tenant_objects)?.check()?;
        if !Self::absent(&tenant_objects.join("publications"))? { return Err(refused()); }
        match (Self::absent(&stage)?, Self::absent(&retained)?) {
            (false, true) => { self.empty_stage(&stage)?; Ok(false) }
            (true, false) => { self.empty_stage(&retained)?; Ok(true) }
            _ => Err(refused()),
        }
    }

    pub(crate) fn retain(&self) -> Result<Value> {
        let parent = DirectoryPin::open(&self.root)?;
        let directory = parent.try_clone_file()?;
        let (stage, retained) = self.stage_paths();
        if !self.check_stage()? {
            let old = self.empty_stage(&self.root.join(&stage))?;
            old.try_clone_file()?.sync_all().map_err(|_| refused())?;
            parent.check()?;
            old.check()?;
            let src = CString::new(stage).map_err(|_| refused())?;
            let dst = CString::new(retained.as_str()).map_err(|_| refused())?;
            if unsafe { libc::renameatx_np(directory.as_raw_fd(), src.as_ptr(), directory.as_raw_fd(), dst.as_ptr(), libc::RENAME_EXCL) } != 0 {
                return Err(refused());
            }
        }
        directory.sync_all().map_err(|_| refused())?;
        parent.check()?;
        if !self.check_stage()? { return Err(refused()); }
        Ok(serde_json::json!({"retained":retained, "device":self.input.stage_device, "inode":self.input.stage_inode,
            "oldRequestId":self.input.old_request_id, "newRequestId":self.input.new_request_id}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custody_rejects_nonempty_wrong_inode_and_objects_then_preserves_exact_inode() {
        use std::os::unix::fs::DirBuilderExt;
        let private = |path: &Path| fs::DirBuilder::new().mode(0o700).create(path).unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap().join(Uuid::new_v4().to_string());
        private(&root);
        let owner = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
        let blobs = root.join("blobs");
        let tenant = blobs.join(&owner.1);
        private(&blobs);
        private(&tenant);
        let stage = root.join(format!("native-publication-{}", owner.0));
        private(&stage);
        let m = stage.metadata().unwrap();
        let mut plan = Plan { input: Input { old_request_id: Uuid::new_v4().to_string(), new_request_id: Uuid::new_v4().to_string(),
            document_sha256: String::new(), legacy_journal_sha256: String::new(), new_intent_sha256: String::new(),
            stage_device: m.dev(), stage_inode: m.ino() }, root: root.clone(), owner,
            source_id: String::new(), source_revision: 6, source_edit_version: 6, source_draft: Value::Null,
            copy_id: String::new(), copy_revision: 1, copy_draft: Value::Null };
        fs::write(stage.join("attempt.json"), b"retained attempt").unwrap();
        assert!(plan.retain().is_err());
        assert!(stage.join("attempt.json").is_file());
        fs::remove_file(stage.join("attempt.json")).unwrap();
        plan.input.stage_inode += 1;
        assert!(plan.retain().is_err());
        plan.input.stage_inode = m.ino();
        private(&tenant.join("publications"));
        assert!(plan.retain().is_err());
        fs::remove_dir(tenant.join("publications")).unwrap();
        let receipt = plan.retain().unwrap();
        let retained = root.join(receipt["retained"].as_str().unwrap());
        assert!(!stage.exists());
        assert_eq!(retained.metadata().unwrap().ino(), m.ino());
        assert_eq!(plan.retain().unwrap(), receipt);
        fs::remove_dir_all(root).unwrap();
    }
}
