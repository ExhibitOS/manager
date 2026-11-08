// SPDX-License-Identifier: Apache-2.0
//! A cold selected-workspace checkpoint, distinct from a full host backup.
use super::*;
use std::os::unix::fs::PermissionsExt;

const SELECTED_CONTEXT: &[u8] = b"ExhibitOS-selected-personal-checkpoint-v1\0";
const SELECTED_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const DATA: &[&str] = &["native-database", "blobs", "config"];
const BEFORE: &[(&str, &str)] = &[
    ("native-personal.json", "workspace/native-personal.before"),
    ("runtime.env", "workspace/runtime-env.before"),
];
const UPLOAD: &str = "native-upload.before";

fn directory(path: String, mode: u32) -> Item {
    Item {
        path,
        kind: "directory".into(),
        mode,
        bytes: 0,
        sha256: None,
    }
}

fn regular(path: String, source: &Path) -> Result<Item> {
    let (bytes, sha256) = hash(source)?;
    let meta = fs::symlink_metadata(source).map_err(|_| fail())?;
    if mode(&meta) != 0o600 {
        return Err(fail());
    }
    Ok(Item {
        path,
        kind: "file".into(),
        mode: 0o600,
        bytes,
        sha256: Some(sha256),
    })
}

fn selected_inventory(
    profile: &Path,
    workspace: &Path,
    upload: &Path,
    id: &str,
    job: &str,
) -> Result<(Inventory, BTreeMap<String, PathBuf>)> {
    let mut items = vec![directory("workspace".into(), 0o700)];
    let mut projected = BTreeMap::new();
    let mut budget = 0;
    for name in DATA {
        let source = workspace.join(name);
        canonical_private(&source)?;
        items.push(directory(format!("workspace/{name}"), 0o700));
        let mut subtree = Vec::new();
        walk(workspace, name, &BTreeSet::new(), &mut subtree, &mut budget)?;
        for mut item in subtree {
            let source = workspace.join(&item.path);
            item.path = format!("workspace/{}", item.path);
            if item.kind == "file" {
                projected.insert(item.path.clone(), source);
            }
            items.push(item);
        }
    }
    for (name, projected_name) in BEFORE {
        let source = workspace.join(name);
        items.push(regular((*projected_name).into(), &source)?);
        projected.insert((*projected_name).into(), source);
    }
    items.push(regular(UPLOAD.into(), upload)?);
    projected.insert(UPLOAD.into(), upload.into());
    items.sort_by(|a, b| a.path.cmp(&b.path));
    let total_bytes = items
        .iter()
        .try_fold(0u64, |sum, item| sum.checked_add(item.bytes))
        .ok_or_else(fail)?;
    let registry_sha256 = hash(&profile.join("installation-selection.json"))?.1;
    let inventory = Inventory {
        format: 1,
        id: id.into(),
        scope: format!("selected-personal-checkpoint:{job}"),
        source_profile_sha256: digest(profile.as_os_str().as_encoded_bytes()),
        registry_sha256,
        total_bytes,
        external_volumes_saved: false,
        host_writer_quiescence: "verified-selected-owner-stopped".into(),
        excluded_locks: Vec::new(),
        items,
        content_references: BTreeMap::new(),
    };
    validate_selected(&inventory, profile)?;
    Ok((inventory, projected))
}

fn validate_selected(m: &Inventory, profile: &Path) -> Result<()> {
    if m.format != 1
        || !installations::uuid(&m.id)
        || !m
            .scope
            .strip_prefix("selected-personal-checkpoint:")
            .is_some_and(installations::uuid)
        || m.source_profile_sha256 != digest(profile.as_os_str().as_encoded_bytes())
        || !hash_valid(&m.registry_sha256)
        || m.external_volumes_saved
        || m.host_writer_quiescence != "verified-selected-owner-stopped"
        || !m.excluded_locks.is_empty()
        || !m.content_references.is_empty()
        || m.items.len() > 50_000
        || m.items.windows(2).any(|pair| pair[0].path >= pair[1].path)
    {
        return Err(fail());
    }
    let mut directories = BTreeSet::new();
    let mut members = BTreeSet::new();
    let mut sum = 0u64;
    for item in &m.items {
        let name = item.path.as_str();
        if !safe_path(name) || item.mode & 0o077 != 0 || !safe_mode(item.mode) {
            return Err(fail());
        }
        if let Some((parent, _)) = name.rsplit_once('/')
            && !directories.contains(parent)
        {
            return Err(fail());
        }
        let mandatory_directory =
            name == "workspace" || DATA.iter().any(|dir| name == format!("workspace/{dir}"));
        let mandatory_file = name == UPLOAD || BEFORE.iter().any(|(_, path)| name == *path);
        let in_data = name
            .strip_prefix("workspace/")
            .and_then(|tail| tail.split_once('/'))
            .is_some_and(|(member, _)| DATA.contains(&member));
        match item.kind.as_str() {
            "directory"
                if (mandatory_directory || in_data) && item.bytes == 0 && item.sha256.is_none() =>
            {
                directories.insert(name);
            }
            "file"
                if (mandatory_file || in_data)
                    && item.sha256.as_deref().is_some_and(hash_valid) => {}
            _ => return Err(fail()),
        }
        if mandatory_directory || mandatory_file {
            members.insert(name);
        }
        sum = sum
            .checked_add(item.bytes)
            .filter(|value| *value <= SELECTED_LIMIT)
            .ok_or_else(fail)?;
    }
    if members.len() != 1 + DATA.len() + BEFORE.len() + 1 || sum != m.total_bytes {
        return Err(fail());
    }
    Ok(())
}

pub(crate) fn protect_selected(
    profile: &Path,
    workspace: &Path,
    upload: &Path,
    job: &str,
    key_file: &Path,
    archive: &Path,
    recovered: &Path,
) -> Result<()> {
    // This route admits managed selected roots only, so the external
    // destination check also fences every descendant of the workspace.
    if workspace == profile || !workspace.starts_with(profile) {
        return Err(fail());
    }
    let parent = archive_parent(profile, archive, key_file)?;
    if recovered.parent() != Some(parent.as_path())
        || recovered == archive
        || recovered == key_file
        || !safe_path(
            archive
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(fail)?,
        )
        || !safe_path(
            recovered
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(fail)?,
        )
    {
        return Err(fail());
    }
    no_destination(archive)?;
    no_destination(recovered)?;
    let key = PrivateKey(external_key(profile, key_file)?);
    let parent_pin = crate::maintenance_source::DirectoryPin::open(&parent)?;
    let workspace_pin = crate::maintenance_source::DirectoryPin::open(workspace)?;
    let id = Uuid::new_v4().to_string();
    let (inventory, projected_sources) = selected_inventory(profile, workspace, upload, &id, job)?;
    let encoded = serde_json::to_vec(&inventory).map_err(|_| fail())?;
    if encoded.len() as u64 > MANIFEST_LIMIT {
        return Err(fail());
    }
    let logical = 8 + encoded.len() as u64 + inventory.total_bytes;
    space(&parent, logical * 2 + 1024 * 1024)?;
    let mut prefix = (encoded.len() as u64).to_be_bytes().to_vec();
    prefix.extend_from_slice(&encoded);
    let temporary = parent.join(format!(".selected-{id}.gcm.pending"));
    let stage = parent.join(format!(".selected-{id}.extract.pending"));
    let mut output = private_new(&temporary)?;
    let mut body = Body {
        prefix: Cursor::new(prefix),
        projected_sources,
        root: profile.into(),
        items: inventory.items.clone(),
        references: BTreeMap::new(),
        next: 0,
        current: None,
    };
    if crate::maintenance_stream::seal_compact(
        &mut body,
        &mut output,
        &key.0,
        SELECTED_CONTEXT,
        SELECTED_LIMIT + MANIFEST_LIMIT + 8,
    )? != logical
    {
        return Err(fail());
    }
    output.sync_all().map_err(|_| fail())?;
    drop(output);
    let mut ciphertext = file(&temporary)?;
    let ciphertext_meta = ciphertext.metadata().map_err(|_| fail())?;
    installations::new_directory(&stage)?;
    let recovered_content = stage.join("selected-content");
    let mut sink = stream_extraction::Extraction::with_validator(
        profile,
        &recovered_content,
        validate_selected,
    );
    if crate::maintenance_stream::open(
        &mut ciphertext,
        &mut sink,
        &key.0,
        SELECTED_CONTEXT,
        SELECTED_LIMIT + MANIFEST_LIMIT + 8,
    )? != logical
    {
        return Err(fail());
    }
    let (reconstructed, readback) = sink.finish()?;
    if readback != encoded {
        return Err(fail());
    }
    for item in reconstructed.items.iter().rev() {
        let path = recovered_content.join(&item.path);
        fs::set_permissions(&path, fs::Permissions::from_mode(item.mode)).map_err(|_| fail())?;
        if item.kind == "directory" {
            sync(&path)?;
        } else {
            file(&path)?.sync_all().map_err(|_| fail())?;
            let (bytes, sha256) = hash(&path)?;
            if item.bytes != bytes || item.sha256.as_deref() != Some(sha256.as_str()) {
                return Err(fail());
            }
        }
    }
    let (after, _) = selected_inventory(profile, workspace, upload, &id, job)?;
    if serde_json::to_vec(&after).map_err(|_| fail())? != encoded
        || !unchanged(
            &ciphertext_meta,
            &ciphertext.metadata().map_err(|_| fail())?,
        )
        || !unchanged(
            &ciphertext_meta,
            &fs::symlink_metadata(&temporary).map_err(|_| fail())?,
        )
    {
        return Err(fail());
    }
    workspace_pin.check()?;
    parent_pin.check()?;
    let mut verified = serde_json::to_value(receipt(
        &inventory,
        "selected-personal-checkpoint-verified",
        &encoded,
    ))
    .map_err(|_| fail())?;
    verified["scope"] = "selected-personal-checkpoint".into();
    verified["uploadJobId"] = job.into();
    write_new(
        &stage.join("verified.json"),
        &serde_json::to_vec(&verified).map_err(|_| fail())?,
    )?;
    write_new(&stage.join("manifest.json"), &encoded)?;
    sync(&recovered_content)?;
    sync(&stage)?;
    // Both outputs are new and private. A failed publication preserves the
    // exact archive/extraction candidate and does not touch the source.
    publish_directory(&temporary, archive)?;
    publish_directory(&stage, recovered)?;
    sync(&parent)
}

struct PrivateKey(Vec<u8>);
impl Drop for PrivateKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, os::unix::fs::MetadataExt, sync::Arc};

    #[test]
    fn selected_checkpoint_rejects_workspace_outside_managed_profile() {
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("selected-checkpoint-boundary-{}", Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let profile = parent.join("profile");
        let workspace = parent.join("override-workspace");
        installations::new_directory(&profile).unwrap();
        installations::new_directory(&workspace).unwrap();
        let archive = workspace.join("selected.gcm");
        let recovered = workspace.join("verified-selected");
        let key = parent.join("key");
        write_new(&key, &[1; 32]).unwrap();
        let job = Uuid::new_v4().to_string();
        assert_eq!(
            protect_selected(
                &profile,
                &workspace,
                &profile.join("upload.json"),
                &job,
                &key,
                &archive,
                &recovered
            )
            .unwrap_err()
            .code,
            "HOST_CHECKPOINT_INVALID"
        );
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
    }

    #[test]
    #[ignore = "Requires exact local source payload and heavy resource admission"]
    fn selected_checkpoint_protects_data_and_pending_upload_without_touching_oex_links() {
        use crate::installations::{InstallationController, native_ui::Request};
        use crate::native_runtime::Session;
        let payload_path =
            PathBuf::from(std::env::var_os("EXHIBITOS_TEST_SOURCE_PAYLOAD").unwrap());
        let payload = Arc::new(crate::native_payload::Payload::open(&payload_path).unwrap());
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("selected-checkpoint-{}", Uuid::new_v4()));
        installations::new_directory(&parent).unwrap();
        let profile = parent.join("profile");
        let controller = InstallationController::new(profile.clone(), None).unwrap();
        let selection = controller.context().unwrap();
        let token = &selection.selection_token;
        let root = PathBuf::from(&selection.installations[0].path);
        let service = LifecycleService::bound_existing(root.clone()).unwrap();
        let listeners: Vec<_> = (0..3)
            .map(|_| TcpListener::bind(("127.0.0.1", 0)).unwrap())
            .collect();
        let port = |i: usize| listeners[i].local_addr().unwrap().port();
        let ports = crate::native_installation::Ports {
            web: port(0),
            api: port(1),
            database: port(2),
        };
        drop(listeners);
        service.prepare_native_personal(&payload, ports).unwrap();
        let mut session = Session::new(&service, payload, ports).unwrap();
        assert_eq!(session.start().unwrap().state, "ready");
        assert_eq!(session.stop().unwrap().state, "stopped");
        drop(session);

        let mut transfer = controller.native_upload(token).unwrap();
        let job = Uuid::new_v4().to_string();
        transfer.job_id = Some(job.clone());
        transfer.intent = Some(crate::installations::native_upload::Intent {
            artwork_id: Uuid::new_v4().to_string(),
            idempotency_key: Uuid::new_v4().to_string(),
            mime: "image/png".into(),
            sha256: digest(b"synthetic upload"),
            bytes: 16,
            scale_meters: 1.0,
            rights: serde_json::json!({}),
            derived: None,
            model_derivation: None,
            model_preparation: None,
            coarse_preparation: None,
            hayes_original: None,
        });
        controller.save_native_upload(token, transfer).unwrap();
        let upload_path = profile.join(format!(
            "native-upload-{}.json",
            digest(root.as_os_str().as_encoded_bytes())
        ));
        let upload_bytes = fs::read(&upload_path).unwrap();
        let journal = root.join("native-oex-journal-synthetic");
        installations::new_directory(&journal).unwrap();
        let reservation = journal.join("reservation.json");
        let alias = journal.join("reservation.alias");
        write_new(&reservation, b"linked OEX record").unwrap();
        fs::hard_link(&reservation, &alias).unwrap();
        let link_state = || {
            let a = fs::symlink_metadata(&reservation).unwrap();
            let b = fs::symlink_metadata(&alias).unwrap();
            (
                a.ino(),
                b.ino(),
                a.nlink(),
                b.nlink(),
                hash_raw(&reservation),
                hash_raw(&alias),
            )
        };
        let before_links = link_state();
        assert_eq!(before_links.0, before_links.1);
        assert_eq!((before_links.2, before_links.3), (2, 2));

        let key = parent.join("protection.key");
        let full = parent.join("full.gcm");
        let archive = parent.join("selected.gcm");
        let recovered = parent.join("verified-selected");
        controller.create_backup_key(token, &key).unwrap();
        drop(controller);
        assert_eq!(
            checkpoint_host(&profile, &key, &full, true, true)
                .unwrap_err()
                .code,
            "HOST_CHECKPOINT_INVALID"
        );
        assert!(!full.exists());

        let controller = Arc::new(InstallationController::new(profile.clone(), None).unwrap());
        let token = controller.context().unwrap().selection_token;
        let mut host = crate::native_host::Host::new(controller, payload_path.clone());
        let request: Request = serde_json::from_value(serde_json::json!({
            "version":1,"id":1,"command":"native_maintenance","selection_token":token,
            "maintenance":{"action":"local_personal_transition","acknowledged":true,
                "transition":{"phase":"protect","sourcePayload":payload_path,"archive":archive,
                    "keyFile":key,"recovered":recovered,"uploadJobId":job}},
        }))
        .unwrap();
        host.handle(request).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
        loop {
            let status = host.status(&token).unwrap();
            if !status.busy {
                assert_eq!(status.error_code, None);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        host.shutdown();
        assert_eq!(link_state(), before_links);
        assert_eq!(fs::read(&upload_path).unwrap(), upload_bytes);
        let content = recovered.join("selected-content");
        assert_eq!(fs::read(content.join(UPLOAD)).unwrap(), upload_bytes);
        assert!(
            !content
                .join("workspace/native-oex-journal-synthetic")
                .exists()
        );
        assert!(!content.join("workspace/runtime.env").exists());
        assert!(!content.join("workspace/native-personal.json").exists());
        let receipt: Value =
            serde_json::from_slice(&fs::read(recovered.join("verified.json")).unwrap()).unwrap();
        assert_eq!(receipt["scope"], "selected-personal-checkpoint");
        assert_eq!(receipt["uploadJobId"], job);
        assert_eq!(
            digest(&fs::read(recovered.join("manifest.json")).unwrap()),
            receipt["manifestSha256"].as_str().unwrap()
        );
        assert_eq!(archive.metadata().unwrap().mode() & 0o777, 0o600);
        let key = PrivateKey(external_key(&profile, &key).unwrap());
        assert!(
            crate::maintenance_stream::open(
                &mut File::open(&archive).unwrap(),
                &mut std::io::sink(),
                &key.0,
                CONTEXT,
                SELECTED_LIMIT + MANIFEST_LIMIT + 8
            )
            .is_err()
        );
        assert!(
            crate::maintenance_stream::open(
                &mut File::open(&archive).unwrap(),
                &mut std::io::sink(),
                &[0; 32],
                SELECTED_CONTEXT,
                SELECTED_LIMIT + MANIFEST_LIMIT + 8
            )
            .is_err()
        );
    }

    fn hash_raw(path: &Path) -> String {
        digest(&fs::read(path).unwrap())
    }
}
