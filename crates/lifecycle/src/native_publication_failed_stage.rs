// SPDX-License-Identifier: Apache-2.0
//! Cold custody for the reviewed Catalog057 pre-commit failure. This is an
//! exact maintenance input, not authority to abandon another publication.
use super::*;
use crate::{
    native_payload::{
        FurnishingTransition, Payload, SIGNED_PASSIVE_STOCK_CATALOGUE_RUNTIME,
        WorkspaceFurnishingCatalog,
    },
    native_runtime::{FurnishingTransitionWriterGuard, check_cold_catalog057_publication_custody},
};

const SOURCE_MANIFEST: &str = SIGNED_PASSIVE_STOCK_CATALOGUE_RUNTIME.2;
const WORKSPACE: &str = "6eb0a964-0261-483d-8807-94516cc517cf";
const TENANT: &str = "f9318b62-0f69-4b52-8f5d-958fb6dca70d";
const PRINCIPAL: &str = "73d9231c-82a0-49fb-bb39-9b07af09c720";
const EXHIBITION: &str = "be8a8204-1c20-4fdc-b362-584405800ff0";
const ETAG: &str = "\"studio-r3-f41e658e12a057a5c652e6bf7582163a3c539be85f30f70eb7a88178bff05db0\"";
const GENERATION: &str = "b9c84111-4cee-459f-9597-dc9da4e4957b";
const REQUEST: &str = "a88a9d21-f1c0-4dc2-8397-b3d766ab5a73";
const PUBLICATION: &str = "0a51589a-6ed1-4abb-b74b-d838062daa8e";
const SNAPSHOT: &str = "75b9c49c244a6bb9cd255b0cd3cbce60d8dfb9877c7efd4696f0572b9fe727d2";
const MARKER_SHA: &str = "c1e8b2ccb97ffe52d3f0d38dece7620c4625f938ceb44695ba23e8d076b9c46a";
const INTENT_SHA: &str = "e40dcd6f12e04b3fa9d7dd0417f9836d17dfde5277d92ff4162d97a4d74d68b8";
const FAILED_LOG_SHA: &str = "a7fe8fb188252e6803cb209847406eb119b73c7512d11c2f622a1b2d25bece28";
const REVIEWED_DOCUMENT_SHA: &str =
    "2e43ded1f0cc33a4f45854ef88126a0e77eeb53761bdcecc23900a6d2ebb7e72";
const DEVICE: u64 = 16_777_244;
const PROFILE_INODE: u64 = 52_850_051;
const STAGE_INODE: u64 = 52_852_527;
const MARKER_INODE: u64 = 52_852_530;

pub(crate) fn catalog057_custody_owner(owner: (&str, &str, &str)) -> bool {
    owner == (WORKSPACE, TENANT, PRINCIPAL)
}

pub(crate) fn check_retained_catalog057_stage(root: &Path, generation: &str) -> Result<()> {
    let profile = root.parent().ok_or_else(refused)?;
    let info = DirectoryPin::open(profile)?
        .try_clone_file()?
        .metadata()
        .map_err(|_| refused())?;
    if generation != GENERATION
        || (info.dev(), info.ino()) != (DEVICE, PROFILE_INODE)
        || root != profile.join("local-runtime")
        || !Plan::absent(&root.join(format!("native-publication-{WORKSPACE}")))?
    {
        return Err(refused());
    }
    stage_pin(
        &root.join(format!(".retained-native-publication-{REQUEST}")),
        (DEVICE, STAGE_INODE),
        (DEVICE, MARKER_INODE),
        MARKER_SHA,
    )?;
    Ok(())
}

pub(crate) fn check_reviewed_catalog057_document(
    profile: &Path,
    document: &native_document::DocumentState,
) -> Result<()> {
    let proof_root = profile.parent().ok_or_else(refused)?;
    let reviewed = verified_bytes(
        proof_root,
        "update-sdk-ready-document.json",
        2_200_000,
        REVIEWED_DOCUMENT_SHA,
    )?;
    let reviewed: native_document::DocumentState =
        serde_json::from_slice(&reviewed).map_err(|_| refused())?;
    if document != &reviewed
        || document.etag.as_deref() != Some(ETAG)
        || document.pending.is_some()
        || document.draft.as_ref().and_then(native_document::draft_id) != Some(EXHIBITION)
        || document.saved_edit_version
            != document
                .draft
                .as_ref()
                .and_then(|d| d["editVersion"].as_u64())
    {
        return Err(refused());
    }
    Ok(())
}

fn verified_bytes(root: &Path, name: &str, limit: u64, expected: &str) -> Result<Vec<u8>> {
    let bytes = installation_backup::source_bytes(root, name, limit, true)?;
    if digest(&bytes) != expected {
        return Err(refused());
    }
    Ok(bytes)
}

fn marker_matches(bytes: &[u8], expected_sha: &str) -> Result<()> {
    if digest(bytes) != expected_sha {
        return Err(refused());
    }
    let marker: Value = serde_json::from_slice(bytes).map_err(|_| refused())?;
    if marker
        != serde_json::json!({
            "version": 1, "tenantId": TENANT, "userId": PRINCIPAL, "exhibitionId": EXHIBITION,
            "etag": ETAG, "requestId": REQUEST, "publicationId": PUBLICATION, "snapshotHash": SNAPSHOT,
            "objects": [],
        })
    {
        return Err(refused());
    }
    Ok(())
}

fn stage_pin(
    path: &Path,
    stage: (u64, u64),
    marker: (u64, u64),
    expected_sha: &str,
) -> Result<DirectoryPin> {
    private_directory(path)?;
    let directory = DirectoryPin::open(path)?;
    let info = directory
        .try_clone_file()?
        .metadata()
        .map_err(|_| refused())?;
    let mut entries = fs::read_dir(path).map_err(|_| refused())?;
    if (info.dev(), info.ino()) != stage
        || entries
            .next()
            .transpose()
            .map_err(|_| refused())?
            .is_none_or(|e| e.file_name() != "attempt.json")
        || entries.next().is_some()
    {
        return Err(refused());
    }
    let marker_path = path.join("attempt.json");
    let marker_info = fs::symlink_metadata(&marker_path).map_err(|_| refused())?;
    if (marker_info.dev(), marker_info.ino()) != marker {
        return Err(refused());
    }
    let bytes = installation_backup::source_bytes(path, "attempt.json", 454, true)?;
    marker_matches(&bytes, expected_sha)?;
    directory.check()?;
    Ok(directory)
}

fn retain_stage(
    parent: &DirectoryPin,
    stage: &Path,
    retained: &Path,
    pin: impl Fn(&Path) -> Result<DirectoryPin>,
) -> Result<()> {
    if !Plan::absent(retained)? {
        return Err(refused());
    }
    let held = pin(stage)?;
    let directory = parent.try_clone_file()?;
    held.try_clone_file()?.sync_all().map_err(|_| refused())?;
    parent.check()?;
    held.check()?;
    let source_name = stage
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(refused)?;
    let retained_name = retained
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(refused)?;
    rename_exclusive(&directory, source_name, retained_name)?;
    directory.sync_all().map_err(|_| refused())?;
    parent.check()?;
    if !Plan::absent(stage)? {
        return Err(refused());
    }
    pin(retained)?;
    Ok(())
}

impl InstallationController {
    pub(crate) fn retain_catalog057_failed_publication_stage(
        &self,
        token: &str,
        source_path: &Path,
        target_path: &Path,
    ) -> Result<Value> {
        let source = Payload::open_saved_workspace_transition(source_path)?;
        let target = Payload::open_saved_workspace_transition(target_path)?;
        let from = source.context();
        let to = target.context();
        if from.manifest_sha256 != SOURCE_MANIFEST
            || source.workspace_furnishing_catalog()?
                != Some(WorkspaceFurnishingCatalog::DefaultCatalogue057)
            || source.normal_oex_selected_input()?
            || !matches!(FurnishingTransition::from_manifests((SOURCE_MANIFEST, &to.manifest_sha256)),
                Some(step) if step.target_catalog() == WorkspaceFurnishingCatalog::DefaultCatalogue057
                    && step.target() == (to.source_commit.as_str(), target.producer_sha256(), to.manifest_sha256.as_str()))
        {
            return Err(refused());
        }
        self.with_current(token, |service| {
            let profile = self.profile.as_ref().ok_or_else(refused)?;
            let _profile_guard = profile_lock(profile)?;
            self._profile_session
                .as_ref()
                .ok_or_else(refused)?
                .check_exclusive(&profile.root)?;
            let profile_pin = DirectoryPin::open(&profile.root)?;
            let profile_info = profile_pin
                .try_clone_file()?
                .metadata()
                .map_err(|_| refused())?;
            if (profile_info.dev(), profile_info.ino()) != (DEVICE, PROFILE_INODE)
                || service.root != profile.root.join("local-runtime")
                || service.personal_identity()?
                    != (WORKSPACE.into(), TENANT.into(), PRINCIPAL.into())
            {
                return Err(refused());
            }
            let _operation = service.lock()?;
            let writer = FurnishingTransitionWriterGuard::acquire_existing(&service.root)?;
            check_cold_catalog057_publication_custody(service, &source, &writer, GENERATION)?;
            let prepared = service.native_prepared()?.ok_or_else(refused)?;
            if prepared.kind != "personal"
                || prepared.manifest_sha256 != SOURCE_MANIFEST
                || prepared.tenant_id != TENANT
            {
                return Err(refused());
            }
            // Verify the existing environment binding, which also refuses a
            // pending transition fence. Do not expose its values in the receipt.
            for value in service
                .personal_environment(&source, prepared.ports)?
                .into_values()
            {
                value.into_bytes().fill(0);
            }

            let proof_root = profile.root.parent().ok_or_else(refused)?;
            let (_, document) = self.document_unlocked(service)?;
            check_reviewed_catalog057_document(&profile.root, &document)?;
            let (_, publication) = self.publication_unlocked(service)?;
            if publication.pending.is_some()
                || publication.completed.is_some()
                || !publication.native_original_attempts.is_empty()
            {
                return Err(refused());
            }
            let intent = verified_bytes(
                &profile.root,
                &format!(".automation-saved-head-export-{REQUEST}.json"),
                8192,
                INTENT_SHA,
            )?;
            let intent: Value = serde_json::from_slice(&intent).map_err(|_| refused())?;
            let output = proof_root.join("sdk-output");
            private_directory(&output)?;
            let metadata = fs::metadata(&output).map_err(|_| refused())?;
            let output_device = metadata.dev().to_string();
            let output_inode = metadata.ino().to_string();
            let leaf = output.join("private-v123");
            if intent["request"]["outputPath"].as_str().map(Path::new) != Some(leaf.as_path())
                || intent["outputDevice"].as_str() != Some(output_device.as_str())
                || intent["outputInode"].as_str() != Some(output_inode.as_str())
                || !Plan::absent(&leaf)?
            {
                return Err(refused());
            }

            // For this invocation, the independently reviewed FK throw is
            // before the success callback/COMMIT and publication object writes.
            verified_bytes(
                &service.root,
                &format!("native-runs/{GENERATION}/process.log"),
                16 * 1024 * 1024,
                FAILED_LOG_SHA,
            )?;
            let tenant = service.root.join("blobs").join(TENANT);
            private_directory(&tenant)?;
            let objects = tenant.join("publications");
            if !Plan::absent(&objects)? {
                private_directory(&objects)?;
                if !Plan::absent(&objects.join(PUBLICATION))? {
                    return Err(refused());
                }
            }
            let parent = DirectoryPin::open(&service.root)?;
            let stage_name = format!("native-publication-{WORKSPACE}");
            let retained_name = format!(".retained-native-publication-{REQUEST}");
            profile_pin.check()?;
            check_cold_catalog057_publication_custody(service, &source, &writer, GENERATION)?;
            retain_stage(
                &parent,
                &service.root.join(stage_name),
                &service.root.join(&retained_name),
                |path| {
                    stage_pin(
                        path,
                        (DEVICE, STAGE_INODE),
                        (DEVICE, MARKER_INODE),
                        MARKER_SHA,
                    )
                },
            )?;
            Ok(serde_json::json!({
                "retained": retained_name, "device": DEVICE, "inode": STAGE_INODE,
                "markerSha256": MARKER_SHA, "markerBytes": 454,
                "oldRequestId": REQUEST, "publicationId": PUBLICATION, "etag": ETAG,
                "generation": GENERATION, "sourceManifestSha256": SOURCE_MANIFEST,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    fn marker() -> Value {
        serde_json::json!({
            "version": 1, "tenantId": TENANT, "userId": PRINCIPAL, "exhibitionId": EXHIBITION,
            "etag": ETAG, "requestId": REQUEST, "publicationId": PUBLICATION, "snapshotHash": SNAPSHOT,
            "objects": [],
        })
    }

    #[test]
    fn exact_catalog057_failed_marker_checks_hash_scope_and_zero_objects() {
        let bytes = serde_json::to_vec(&marker()).unwrap();
        let hash = digest(&bytes);
        assert!(marker_matches(&bytes, &hash).is_ok());
        assert!(marker_matches(&bytes, &"0".repeat(64)).is_err());
        for (key, wrong) in [
            ("tenantId", Value::String(WORKSPACE.into())),
            ("userId", Value::String(TENANT.into())),
            ("etag", Value::String("\"studio-r2-wrong\"".into())),
            ("objects", serde_json::json!([{"bytes": 1}])),
            ("extra", Value::Bool(true)),
        ] {
            let mut changed = marker();
            changed[key] = wrong;
            let changed = serde_json::to_vec(&changed).unwrap();
            assert!(marker_matches(&changed, &digest(&changed)).is_err());
        }
        assert!(!catalog057_custody_owner((WORKSPACE, PRINCIPAL, TENANT)));
    }

    #[test]
    fn exact_catalog057_stage_preserves_bytes_and_refuses_links_extra_entries_and_collision() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(Uuid::new_v4().to_string());
        let private = |p: &Path| fs::DirBuilder::new().mode(0o700).create(p).unwrap();
        private(&root);
        let stage = root.join("active");
        let retained = root.join("retained");
        private(&stage);
        let marker_file = stage.join("attempt.json");
        let bytes = serde_json::to_vec(&marker()).unwrap();
        write_private_new(&stage, "attempt.json", &bytes, 454).unwrap();
        let stage_meta = stage.metadata().unwrap();
        let marker_meta = marker_file.metadata().unwrap();
        let stage_identity = (stage_meta.dev(), stage_meta.ino());
        let marker_identity = (marker_meta.dev(), marker_meta.ino());
        let hash = digest(&bytes);
        let pin = |p: &Path| stage_pin(p, stage_identity, marker_identity, &hash);
        let parent = DirectoryPin::open(&root).unwrap();

        fs::hard_link(&marker_file, root.join("alias")).unwrap();
        assert!(pin(&stage).is_err());
        fs::remove_file(root.join("alias")).unwrap();
        fs::write(stage.join("extra"), b"extra").unwrap();
        assert!(pin(&stage).is_err());
        fs::remove_file(stage.join("extra")).unwrap();
        assert!(
            stage_pin(
                &stage,
                stage_identity,
                (marker_meta.dev(), marker_meta.ino() + 1),
                &hash
            )
            .is_err()
        );
        private(&retained);
        assert!(retain_stage(&parent, &stage, &retained, pin).is_err());
        assert!(marker_file.is_file());
        fs::remove_dir(&retained).unwrap();

        assert!(retain_stage(&parent, &stage, &retained, pin).is_ok());
        assert!(!stage.exists());
        assert_eq!(fs::read(retained.join("attempt.json")).unwrap(), bytes);
        assert!(pin(&retained).is_ok());
        assert!(retain_stage(&parent, &stage, &retained, pin).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
