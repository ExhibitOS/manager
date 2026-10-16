// SPDX-License-Identifier: Apache-2.0
//! Read-only complete archive/current-profile comparison; no plaintext extraction.
use super::*;
use std::io::Write;
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostCurrentReceipt {
    pub files: usize,
    pub bytes: u64,
    pub manifest_sha256: String,
    pub current_profile_matched: bool,
    pub plaintext_files_created: usize,
    pub external_volumes_saved: bool,
}
// These are bounded native dispatch records, not the service/configuration or
// trust authority being restored. They are necessarily written AFTER a host
// checkpoint (reference publication, app-close request and atomic helper claim).
// Authenticate every archived byte as before; project only these exact namespaces
// out of the archive/current comparison. Never accept caller-supplied exclusions.
const RUNTIME_CONTROLS: [&str; 4] = [
    "update-execution-references",
    "update-apply-handoffs",
    "update-cancel-handoffs",
    "update-rollback-handoffs",
];
fn runtime_control(path: &str) -> bool {
    RUNTIME_CONTROLS
        .iter()
        .any(|root| path == *root || path.strip_prefix(root).is_some_and(|s| s.starts_with('/')))
}
fn validate_runtime_controls(profile: &Path, current: &Inventory) -> Result<()> {
    for root in RUNTIME_CONTROLS {
        let Some(directory) = current.items.iter().find(|item| item.path == root) else {
            continue;
        };
        if directory.kind != "directory" {
            return Err(fail());
        }
        installations::private_directory(&profile.join(root))?;
        let mut count = 0;
        for item in current
            .items
            .iter()
            .filter(|item| item.path.starts_with(&format!("{root}/")))
        {
            count += 1;
            let reference = root == "update-execution-references";
            let limit = if reference || root == "update-rollback-handoffs" {
                16384
            } else {
                8192
            };
            if count > if reference { 128 } else { 384 }
                || item.kind != "file"
                || item.bytes > limit
            {
                return Err(fail());
            }
            let name = item
                .path
                .strip_prefix(&format!("{root}/"))
                .ok_or_else(fail)?;
            if reference {
                let id = name
                    .strip_suffix(".json")
                    .filter(|id| installations::uuid(id))
                    .ok_or_else(fail)?;
                crate::execution_reference::load(profile, id)?;
                continue;
            }
            let (id, suffix) = name.split_once('.').ok_or_else(fail)?;
            if !installations::uuid(id)
                || !["request.json", "claim.json", "outcome.json"].contains(&suffix)
            {
                return Err(fail());
            }
            let bytes = source_bytes(&profile.join(root), name, limit, true)?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| fail())?;
            if value["id"].as_str() != Some(id) {
                return Err(fail());
            }
            // Parse the actual native closed schemas; none of these observations
            // grants execution. Consumers still rebind authority/op/generation,
            // registry and reference digests and enforce single claim ownership.
            match (root, suffix == "request.json") {
                ("update-apply-handoffs", true) => {
                    let _: crate::update_execution_handoff::Request =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                ("update-apply-handoffs", false) => {
                    let _: crate::update_execution_handoff::Receipt =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                ("update-cancel-handoffs", true) => {
                    let _: crate::update_handoff::Request =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                ("update-cancel-handoffs", false) => {
                    let _: crate::update_handoff::Receipt =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                ("update-rollback-handoffs", true) => {
                    let _: crate::update_rollback_handoff::Request =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                ("update-rollback-handoffs", false) => {
                    let _: crate::update_rollback_handoff::Receipt =
                        serde_json::from_slice(&bytes).map_err(|_| fail())?;
                }
                _ => return Err(fail()),
            }
        }
    }
    Ok(())
}
fn current_items_match(profile: &Path, current: &Inventory, archived: &Inventory) -> Result<bool> {
    validate_runtime_controls(profile, current)?;
    Ok(current
        .items
        .iter()
        .filter(|item| !runtime_control(&item.path))
        .eq(archived
            .items
            .iter()
            .filter(|item| !runtime_control(&item.path))))
}
struct Comparison {
    profile: PathBuf,
    expected: String,
    prefix: Vec<u8>,
    manifest_len: Option<usize>,
    manifest: Vec<u8>,
    inventory: Option<Inventory>,
    next: usize,
    remaining: u64,
    hash: Sha256,
}
impl Comparison {
    fn new(profile: &Path, expected: &str) -> Self {
        Self {
            profile: profile.into(),
            expected: expected.into(),
            prefix: Vec::new(),
            manifest_len: None,
            manifest: Vec::new(),
            inventory: None,
            next: 0,
            remaining: 0,
            hash: Sha256::new(),
        }
    }
    fn finish_item(&mut self) -> Result<()> {
        let m = self.inventory.as_ref().ok_or_else(fail)?;
        while let Some(e) = m.items.get(self.next) {
            if e.kind != "directory" && !m.content_references.contains_key(&e.path) {
                if self.remaining != 0 {
                    return Ok(());
                }
                if Some(format!("{:x}", self.hash.clone().finalize())) != e.sha256 {
                    return Err(fail());
                }
            }
            self.next += 1;
            self.hash = Sha256::new();
            self.remaining = m
                .items
                .get(self.next)
                .filter(|e| e.kind == "file" && !m.content_references.contains_key(&e.path))
                .map_or(0, |e| e.bytes);
        }
        Ok(())
    }
    fn accept(&mut self, mut bytes: &[u8]) -> Result<()> {
        if self.prefix.len() < 8 {
            let n = bytes.len().min(8 - self.prefix.len());
            self.prefix.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.prefix.len() < 8 {
                return Ok(());
            }
            let len = u64::from_be_bytes(self.prefix.as_slice().try_into().map_err(|_| fail())?);
            if len == 0 || len > MANIFEST_LIMIT {
                return Err(fail());
            }
            self.manifest_len = Some(len as usize);
        }
        if self.inventory.is_none() {
            let len = self.manifest_len.ok_or_else(fail)?;
            let n = bytes.len().min(len - self.manifest.len());
            self.manifest.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.manifest.len() < len {
                return Ok(());
            }
            if digest(&self.manifest) != self.expected {
                return Err(fail());
            }
            let m: Inventory = serde_json::from_slice(&self.manifest).map_err(|_| fail())?;
            validate_inventory(&m, &self.profile)?;
            // Do not accept archive-controlled omissions of current profile paths.
            let snapshot = capture(&self.profile)?;
            let mut excluded =
                BTreeSet::from(["operation.lock".into(), "profile-session.lock".into()]);
            for r in &snapshot.spaces {
                if r.available {
                    excluded.insert(format!("{}/operation.lock", r.relative));
                }
            }
            if m.excluded_locks != excluded.iter().cloned().collect::<Vec<_>>() {
                return Err(fail());
            }
            let current = inventory(&self.profile, &excluded, &m.id)?;
            if !current_items_match(&self.profile, &current, &m)?
                || current.registry_sha256 != m.registry_sha256
                || current.root_windows_permissions != m.root_windows_permissions
            {
                return Err(err("HOST_SOURCE_CHANGED"));
            }
            self.inventory = Some(m);
            while self
                .inventory
                .as_ref()
                .unwrap()
                .items
                .get(self.next)
                .is_some_and(|e| e.kind == "directory")
            {
                self.next += 1;
            }
            if let Some(e) = self.inventory.as_ref().unwrap().items.get(self.next) {
                self.remaining = e.bytes;
            }
            self.finish_item()?;
        }
        while !bytes.is_empty() {
            if self
                .inventory
                .as_ref()
                .ok_or_else(fail)?
                .items
                .get(self.next)
                .is_none()
            {
                return Err(fail());
            }
            let n = bytes.len().min(self.remaining as usize);
            if n == 0 {
                return Err(fail());
            }
            self.hash.update(&bytes[..n]);
            self.remaining -= n as u64;
            bytes = &bytes[n..];
            self.finish_item()?;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<HostCurrentReceipt> {
        self.finish_item()?;
        let m = self.inventory.as_ref().ok_or_else(fail)?;
        if self.next != m.items.len() || self.remaining != 0 {
            return Err(fail());
        }
        let excluded = m.excluded_locks.iter().cloned().collect();
        let current = inventory(&self.profile, &excluded, &m.id)?;
        if !current_items_match(&self.profile, &current, m)?
            || current.registry_sha256 != m.registry_sha256
            || current.root_windows_permissions != m.root_windows_permissions
        {
            return Err(err("HOST_SOURCE_CHANGED"));
        }
        Ok(HostCurrentReceipt {
            files: m.items.iter().filter(|e| e.kind == "file").count(),
            bytes: m.total_bytes,
            manifest_sha256: self.expected,
            current_profile_matched: true,
            plaintext_files_created: 0,
            external_volumes_saved: false,
        })
    }
}
impl Write for Comparison {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.accept(bytes)
            .map_err(|_| std::io::Error::other("HOST_CHECKPOINT_INVALID"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn verify_host_current_borrowed(
    profile: &Path,
    archive: &Path,
    key: &[u8; 32],
    session: &ProfileSession,
    expected: &str,
) -> Result<HostCurrentReceipt> {
    session.check_exclusive(profile)?;
    if !hash_valid(expected) || archive.starts_with(profile) {
        return Err(fail());
    }
    let mut source = file(archive)?;
    let before = source.metadata().map_err(|_| fail())?;
    if !archive_permissions(&source, &before)? || before.len() > DATA_LIMIT + 16 * 1024 * 1024 {
        return Err(fail());
    }
    let mut sink = Comparison::new(profile, expected);
    // No success receipt exists until final-frame authentication AND EOF succeed.
    super::super::super::maintenance_stream::open(&mut source, &mut sink, key, CONTEXT, DATA_LIMIT)
        .map_err(|_| fail())?;
    if !unchanged(&before, &source.metadata().map_err(|_| fail())?)
        || !unchanged(&before, &fs::symlink_metadata(archive).map_err(|_| fail())?)
    {
        return Err(fail());
    }
    file_check(&source)?;
    let receipt = sink.finish()?;
    session.check_exclusive(profile)?;
    Ok(receipt)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::fixture;
    use super::*;
    fn prepared() -> (
        super::super::tests::FixtureRoot,
        PathBuf,
        PathBuf,
        String,
        Vec<u8>,
    ) {
        let (root, profile, key, archive) = fixture();
        installations::new_directory(&profile.join("synthetic")).unwrap();
        write_new(&profile.join("synthetic/empty"), b"").unwrap();
        write_new(
            &profile.join("synthetic/data"),
            &vec![19; 2 * 1024 * 1024 + 17],
        )
        .unwrap();
        let r = checkpoint_host(&profile, &key, &archive, true, true).unwrap();
        let mut plain = Vec::new();
        super::super::super::super::maintenance_stream::open(
            &mut file(&archive).unwrap(),
            &mut plain,
            &[17; 32],
            CONTEXT,
            DATA_LIMIT,
        )
        .unwrap();
        (root, profile, archive, r.manifest_sha256, plain)
    }
    fn reference_bytes(profile: &Path, id: &str) -> Vec<u8> {
        let path = profile.parent().unwrap().join("synthetic-input");
        serde_json::to_vec(&serde_json::json!({
            "format":1,"id":id,"installationId":Uuid::new_v4().to_string(),
            "scope":"default","operationId":"synthetic-op","generation":3,
            "registrySha256":"a".repeat(64),"artifact":path,"artifactSha256":"b".repeat(64),
            "artifactBytes":7,"python":path,"sourceCommit":"c".repeat(40),
            "maintenanceImage":format!("sha256:{}","d".repeat(64)),"exportParent":path,
            "hostArchive":path,"trustArchive":path,"pairBinding":path,"keyFile":path,
            "qualificationDestination":path,"qualificationSha256":"e".repeat(64),"createdAt":21
        }))
        .unwrap()
    }
    #[test]
    fn current_host_runtime_controls_allow_reference_and_app_close_claim_but_not_service_change() {
        let (_root, profile, archive, expected, plain) = prepared();
        let original = fs::read(&archive).unwrap();
        let references = profile.join("update-execution-references");
        installations::new_directory(&references).unwrap();
        let id = Uuid::new_v4().to_string();
        write_new(
            &references.join(format!("{id}.json")),
            &reference_bytes(&profile, &id),
        )
        .unwrap();
        let mut comparison = Comparison::new(&profile, &expected);
        comparison.accept(&plain).unwrap();
        let handoffs = profile.join("update-apply-handoffs");
        installations::new_directory(&handoffs).unwrap();
        let request = Uuid::new_v4().to_string();
        let installation = Uuid::new_v4().to_string();
        let bytes = serde_json::to_vec(&serde_json::json!({"id":request,
            "installationId":installation,"scope":"default","registrySha256":"a".repeat(64),
            "referenceSha256":"b".repeat(64),"createdAt":21,
            "input":{"referenceId":id,"operationId":"synthetic-op","generation":3,
                "preserveData":true,"closeAppAndApply":true,"externalWritersQuiesced":true}}))
        .unwrap();
        write_new(&handoffs.join(format!("{request}.request.json")), &bytes).unwrap();
        let claim = serde_json::to_vec(&serde_json::json!({"id":request,
            "installationId":installation,"state":"failed","code":null,
            "createdAt":21,"requestSha256":digest(&bytes)}))
        .unwrap();
        write_new(&handoffs.join(format!("{request}.claim.json")), &claim).unwrap();
        assert!(comparison.finish().unwrap().current_profile_matched);
        let session = session_lock(&profile, true).unwrap();
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &expected)
                .is_ok()
        );
        write_new(
            &profile.join("synthetic/real-service-change"),
            b"not a runtime control",
        )
        .unwrap();
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &expected)
                .is_err()
        );
        assert_eq!(fs::read(&archive).unwrap(), original);
    }
    #[test]
    fn current_host_runtime_controls_refuse_unrecognized_names_schema_and_oversized_records() {
        let (_root, profile, _archive, expected, plain) = prepared();
        let references = profile.join("update-execution-references");
        installations::new_directory(&references).unwrap();
        let id = Uuid::new_v4().to_string();
        let path = references.join(format!("{id}.json"));
        let check = || {
            let mut c = Comparison::new(&profile, &expected);
            c.accept(&plain).and_then(|_| c.finish()).is_err()
        };
        write_new(&path, b"{}").unwrap();
        assert!(check());
        fs::remove_file(&path).unwrap();
        write_new(&path, &vec![b' '; 16385]).unwrap();
        assert!(check());
        fs::remove_file(&path).unwrap();
        write_new(
            &references.join("not-a-reference.json"),
            &reference_bytes(&profile, &id),
        )
        .unwrap();
        assert!(check());
        fs::remove_file(references.join("not-a-reference.json")).unwrap();
        std::os::unix::fs::symlink(profile.join("synthetic/data"), &path).unwrap();
        assert!(check());
    }
    #[test]
    fn current_host_public_boundary_refuses_missing_ack_key_and_changed_source_without_copy() {
        let (root, profile, archive, expected, _) = prepared();
        let key = root.path.join("key.bin");
        let bytes = fs::read(&archive).unwrap();
        for (apps, writers) in [(false, true), (true, false)] {
            assert_eq!(
                super::verify_host_current(&profile, &key, &archive, &expected, apps, writers)
                    .unwrap_err()
                    .code,
                "HOST_WRITER_ACK_REQUIRED"
            );
        }
        let count = fs::read_dir(&root.path).unwrap().count();
        let receipt =
            super::verify_host_current(&profile, &key, &archive, &expected, true, true).unwrap();
        assert!(receipt.current_profile_matched);
        assert_eq!(receipt.plaintext_files_created, 0);
        assert!(!receipt.external_volumes_saved);
        let wrong = root.path.join("wrong-key.bin");
        write_new(&wrong, &[18; 32]).unwrap();
        assert!(
            super::verify_host_current(&profile, &wrong, &archive, &expected, true, true).is_err()
        );
        fs::remove_file(wrong).unwrap();
        assert_eq!(fs::read_dir(&root.path).unwrap().count(), count);
        write_new(&profile.join("synthetic/late"), b"late addition").unwrap();
        assert!(
            super::verify_host_current(&profile, &key, &archive, &expected, true, true).is_err()
        );
        assert_eq!(fs::read(&archive).unwrap(), bytes);
        assert_eq!(
            fs::read(profile.join("synthetic/late")).unwrap(),
            b"late addition"
        );
    }
    #[test]
    fn current_host_stream_checks_every_byte_mode_empty_file_and_fragment_without_extraction() {
        let (_root, profile, archive, expected, plain) = prepared();
        let session = session_lock(&profile, true).unwrap();
        let before = fs::read_dir(archive.parent().unwrap()).unwrap().count();
        let r = verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &expected)
            .unwrap();
        assert!(r.current_profile_matched);
        assert_eq!(r.plaintext_files_created, 0);
        assert_eq!(
            before,
            fs::read_dir(archive.parent().unwrap()).unwrap().count()
        );
        for size in [1, 7, 4093, 1024 * 1024] {
            let mut sink = Comparison::new(&profile, &expected);
            for bytes in plain.chunks(size) {
                sink.accept(bytes).unwrap();
            }
            assert!(sink.finish().unwrap().current_profile_matched);
        }
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            profile.join("synthetic/data"),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &expected)
                .is_err()
        );
    }
    #[test]
    fn current_host_stream_refuses_late_current_change_and_authenticated_bad_payload_or_omissions()
    {
        let (_root, profile, _archive, expected, plain) = prepared();
        let mut sink = Comparison::new(&profile, &expected);
        sink.accept(&plain).unwrap();
        write_new(&profile.join("synthetic/added"), b"late addition").unwrap();
        assert!(sink.finish().is_err());
        fs::remove_file(profile.join("synthetic/added")).unwrap();
        let mut changed = plain.clone();
        *changed.last_mut().unwrap() ^= 1;
        let mut sink = Comparison::new(&profile, &expected);
        assert!(sink.accept(&changed).is_err());
        let len = u64::from_be_bytes(plain[..8].try_into().unwrap()) as usize;
        let mut m: Inventory = serde_json::from_slice(&plain[8..8 + len]).unwrap();
        m.excluded_locks.push("synthetic/data".into());
        m.excluded_locks.sort();
        let encoded = serde_json::to_vec(&m).unwrap();
        let mut bad = (encoded.len() as u64).to_be_bytes().to_vec();
        bad.extend(&encoded);
        assert!(
            Comparison::new(&profile, &digest(&encoded))
                .accept(&bad)
                .is_err()
        );
        let mut sink = Comparison::new(&profile, &expected);
        sink.accept(&plain[..plain.len() - 1]).unwrap();
        assert!(sink.finish().is_err());
        let mut extra = plain.clone();
        extra.push(0);
        assert!(Comparison::new(&profile, &expected).accept(&extra).is_err());
    }
    #[test]
    fn current_host_stream_never_accepts_wrong_key_manifest_truncation_or_final_trailer() {
        let (root, profile, archive, expected, _plain) = prepared();
        let session = session_lock(&profile, true).unwrap();
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[18; 32], &session, &expected)
                .is_err()
        );
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &"f".repeat(64))
                .is_err()
        );
        let original = fs::read(&archive).unwrap();
        for (name, bytes) in [
            ("truncated", original[..original.len() - 1].to_vec()),
            ("trailing", [original.clone(), vec![0]].concat()),
        ] {
            let bad = root.path.join(name);
            write_new(&bad, &bytes).unwrap();
            assert!(
                verify_host_current_borrowed(&profile, &bad, &[17; 32], &session, &expected)
                    .is_err()
            );
        }
        assert!(
            verify_host_current_borrowed(&profile, &archive, &[17; 32], &session, &expected)
                .is_ok()
        );
    }
}
