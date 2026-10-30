// SPDX-License-Identifier: Apache-2.0
//! Opt-in REAL native entry in the existing signed-release TEST target. Inputs
//! are fresh, verified cold copies prepared under the owner's resource guards.
//! No production authority, caller-supplied SQL, observer or capability fixture.
use super::*;
use crate::{installations::InstallationController, native_installation::Ports, native_runtime::Session};
use sha2::{Digest, Sha256};
use std::{io::Read, net::TcpListener, process::Command};

fn copied(from: &Path, to: &Path) {
    assert!(Command::new("/bin/cp").arg("-cR").arg(from).arg(to).status().unwrap().success());
}
fn file_hash(path: &Path) -> String {
    let mut file=fs::File::open(path).unwrap(); let mut hash=Sha256::new(); let mut bytes=[0u8;65536];
    loop { let n=file.read(&mut bytes).unwrap(); if n==0 { break; } hash.update(&bytes[..n]); }
    format!("{:x}",hash.finalize())
}
fn seal_native(key: &ed25519_dalek::SigningKey, release: &native::NativeRelease) -> Vec<u8> {
    let payload=serde_json::to_string(release).unwrap();
    serde_json::to_vec(&Envelope {format:1,algorithm:"ed25519".into(),key_id:hash(key.verifying_key().as_bytes()),signature:key.sign(&message(&payload)).to_bytes().iter().map(|b|format!("{b:02x}")).collect(),payload}).unwrap()
}
fn release_for(key: &ed25519_dalek::SigningKey, store: &Store, payload: &Arc<Payload>, root: &Path, name: &str, schema: &str) -> (Vec<u8>, native::StagedNativeArtifact) {
    let archive=root.join(name);
    assert!(!archive.exists());
    assert!(Command::new("/usr/bin/tar").arg("-cf").arg(&archive).arg("-C").arg(payload.node().parent().unwrap().parent().unwrap()).arg(".").status().unwrap().success());
    fs::set_permissions(&archive,fs::Permissions::from_mode(0o600)).unwrap();
    let release=native::NativeRelease {format:2,kind:"native-payload".into(),product:"ExhibitOS/runtime".into(),channel:"development".into(),target:"macos-arm64".into(),version:"0.2.0-dev.1".into(),sequence:2,issued_at:10,expires_at:100,protocol_version:1,source_schemas:vec![schema.into()],artifact:native::NativeArtifact {name:name.into(),bytes:fs::metadata(&archive).unwrap().len(),sha256:file_hash(&archive),native_manifest_sha256:payload.context().manifest_sha256,schema_sha256:schema.into()}};
    assert!(release.artifact.bytes>1_000_000,"Real retained tools, never seven fixture bytes");
    let envelope=seal_native(key,&release); let mut verified=store.verify_native(&envelope,20).unwrap();
    let staged=native::stage(&mut verified,&archive,root,20).unwrap(); (envelope,staged)
}
fn free_ports() -> Ports {
    let sockets: Vec<_>=(0..3).map(|_|TcpListener::bind(("127.0.0.1",0)).unwrap()).collect();
    Ports {web:sockets[0].local_addr().unwrap().port(),api:sockets[1].local_addr().unwrap().port(),database:sockets[2].local_addr().unwrap().port()}
}

#[test]
#[ignore = "Explicit isolated R5 cold-copy fixture + global app/DB/heavy guards required"]
fn actual_r5_signed_admission_authenticated_inactive_copy() {
    exercise_r5(0);
}

#[test]
#[ignore = "Explicit isolated R5 cold-copy fixture + global app/DB/heavy guards required"]
fn actual_r5_begin_inactive_configuration_publication() {
    exercise_r5(1);
}
#[test]
#[ignore = "Explicit isolated R5 cold-copy fixture + global app/DB/heavy guards required"]
fn actual_r5_torn_configuration_offline_repair() {
    exercise_r5(2);
}
#[test]
#[ignore = "Explicit isolated R5 cold-copy fixture + global app/DB/heavy guards required"]
fn actual_r5_configuration_publish_and_offline_repair() {
    exercise_r5(3);
}
fn exercise_r5(mode: u8) {
    let root=fs::canonicalize(std::env::var_os("EXHIBITOS_NATIVE_R5_COPY_ROOT").expect("Owned fixture root required")).unwrap();
    crate::installations::private_directory(&root).unwrap();
    let input: serde_json::Value=serde_json::from_slice(&fs::read(root.join("fixture-input.json")).unwrap()).unwrap();
    assert_eq!(input["kind"],"verified-cold-r5-copy-v1");
    assert_eq!(input["sourceRuntimeManifestSha256"],"53de38d941f76439fb2f5947a8f698298fcb04e862e523c9eea581cea3d94588");
    let cold=root.join("cold-profile"); crate::installations::private_directory(&cold).unwrap();
    assert!(!cold.join("local-runtime/native-database/postmaster.pid").exists());
    let source_payload=Arc::new(Payload::open(&root.join("source-runtime")).unwrap());
    assert_eq!(source_payload.context().manifest_sha256,input["sourceRuntimeManifestSha256"].as_str().unwrap());
    let source_recipe=source_payload.native_copy_resources().unwrap(); assert_eq!(source_recipe.recipe_name,"authenticated-native-r5-copy-v1");
    let target_root=root.join("target-runtime"); assert!(!target_root.exists()); copied(&root.join("source-runtime"),&target_root);
    // Distinct independently admitted target: a harmless HTML comment in this
    // NEW test copy only. No product version or executable codec/tool changes.
    // The frozen runtime binds index.html. The unused native viewport page is
    // outside that inventory, so this distinct test target remains coherent.
    let relative="platform/apps/web/dist/native-viewport.html"; let html=target_root.join(relative);
    let mut bytes=fs::read(&html).unwrap(); bytes.extend_from_slice(b"\n<!-- isolated R5 data-copy admission fixture -->\n"); fs::write(&html,&bytes).unwrap();
    let mut manifest: serde_json::Value=serde_json::from_slice(&fs::read(target_root.join("manifest.json")).unwrap()).unwrap();
    let entry=manifest["files"].as_array_mut().unwrap().iter_mut().find(|v|v["path"]==relative).unwrap();
    entry["bytes"]=serde_json::json!(bytes.len()); entry["sha256"]=serde_json::json!(hash(&bytes));
    fs::write(target_root.join("manifest.json"),serde_json::to_vec(&manifest).unwrap()).unwrap();
    let target_payload=Arc::new(Payload::open(&target_root).unwrap());
    assert_ne!(target_payload.context().manifest_sha256,source_payload.context().manifest_sha256);
    assert_eq!(source_recipe.recipe_sha256,target_payload.native_copy_resources().unwrap().recipe_sha256);
    let (profile,key,mut policy,_unused_oci_release)=super::super::tests::fixture_in(root.join("test-store"));
    copied(&cold.join("."),&profile);
    let controller=InstallationController::new(profile.clone(),None).unwrap(); let initial=controller.context().unwrap();
    let source_id=initial.active_id.clone(); let source_service_root=initial.installations.iter().find(|i|i.id==source_id).unwrap().path.clone();
    // Register and prepare the empty inactive candidate BEFORE capture of any
    // SourceAuthority, then restore the source selection. No activation later.
    let created=controller.create(&initial.selection_token,true).unwrap(); let target_id=created.active_id.clone();
    let candidate=created.installations.iter().find(|i|i.id==target_id).unwrap().path.clone(); let target_ports=free_ports();
    controller.with_current(&created.selection_token,|service|service.prepare_native_personal(&target_payload,target_ports)).unwrap();
    let selected=controller.select(&created.selection_token,&source_id,true).unwrap();
    let key_directory=root.join("test-keys"); fs::create_dir(&key_directory).unwrap(); fs::set_permissions(&key_directory,fs::Permissions::from_mode(0o700)).unwrap();
    let key_file=key_directory.join("backup.key"); controller.create_backup_key(&selected.selection_token,&key_file).unwrap();
    let source_ports=controller.with_current(&selected.selection_token,|service|Ok(service.native_prepared()?.ok_or_else(||crate::err("NATIVE_NOT_READY"))?.ports)).unwrap();
    let source_service=crate::LifecycleService::bound_existing(source_service_root.clone().into()).unwrap();
    let target_service=crate::LifecycleService::bound_existing(candidate.clone().into()).unwrap();
    let mut source_session=Session::new(&source_service,source_payload.clone(),source_ports).unwrap();
    let mut target_session=Session::new(&target_service,target_payload.clone(),target_ports).unwrap();
    eprintln!("R5_COPY_STEP=database-only-owned-sessions");
    source_session.start_database_only().unwrap(); target_session.start_database_only().unwrap();
    assert!(target_session.maintenance_empty_target().unwrap());
    eprintln!("R5_COPY_STEP=authenticated-synthetic-source");
    let authenticated=controller.capture_native_update_source(&selected.selection_token,&mut source_session,crate::installations::native_backup::Input {key_file:key_file.clone(),destination:root.join("authenticated-backup")}).unwrap();
    // Measure through the existing fixed authenticated observer to establish
    // the TEST feed's real schema declaration, then re-observe under authority.
    let measured=authenticated.observe(&mut source_session,|p,id,r| {
        let registry=crate::installations::load(p)?.ok_or_else(||crate::err("UPDATE_SOURCE_UNREGISTERED"))?.0;
        if p!=profile || id!=source_id || r!=Path::new(&source_service_root) || registry.active_id!=source_id { return Err(crate::err("NATIVE_SOURCE_OBSERVATION_STALE")); } Ok(())
    }).unwrap();
    policy.format=2; policy.target="macos-arm64".into(); policy.source_schema_sha256=measured.schema_sha256.clone();
    // Transfer the profile anchor from Controller to Store under the existing
    // profile operation fence. Both DB Session/root fences remain held. Store
    // requires an exclusive anchor; a live Controller owns a conflicting one.
    let profile_service=crate::LifecycleService::bound_existing(profile.clone()).unwrap();
    let handoff=profile_service.lock().unwrap();
    drop(controller);
    let mut store=Store::provision(&profile,"default",policy,10).unwrap();
    drop(handoff);
    let (source_envelope,source_staged)=release_for(&key,&store,&source_payload,&root,"source-native-runtime.tar",&measured.schema_sha256);
    let mut source_admitted=store.admit_native_payload(&source_envelope,source_staged,source_payload,20).unwrap();
    let (target_envelope,target_staged)=release_for(&key,&store,&target_payload,&root,"target-native-runtime.tar",&measured.schema_sha256);
    let mut admitted=store.admit_native_payload(&target_envelope,target_staged,target_payload,20).unwrap();
    store.recheck_native_admission(&mut source_admitted,20).unwrap();
    eprintln!("R5_COPY_STEP=real-signed-TEST-feed-admission");
    let mut source=store.observe_native_source(&mut admitted,authenticated,&mut source_session,20).unwrap();
    let operation=uuid::Uuid::new_v4().to_string();
    eprintln!("R5_COPY_STEP=production-target-copy-path");
    let mut prepared=store.prepare_native_target_copy(&mut admitted,&mut source,&mut source_session,&mut target_session,&target_id,&operation,20).unwrap();
    let receipt=serde_json::to_value(prepared.receipt()).unwrap(); assert_eq!(receipt["state"],"PreparedInactive");
    for flag in ["configurationVerified","preflightVerified","executionPermitted"] { assert_eq!(receipt[flag],false); assert_eq!(receipt["observation"][flag],false); }
    assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
    let intent: serde_json::Value=serde_json::from_slice(&fs::read(Path::new(&candidate).join(format!("native-target-preparation-{operation}/intent.json"))).unwrap()).unwrap();
    assert_eq!(intent["recipe"],"authenticated-native-r5-copy-v1"); assert_eq!(intent["state"],"PreparedInactive");
    // Invoke the REAL planner while the adapter-issued capability and both
    // owned database sessions are still alive. Never reconstruct it from JSON.
    let source_root=Path::new(&source_service_root); let candidate_root=Path::new(&candidate);
    let source_suffix=hash(source_service_root.as_bytes()); let target_suffix=hash(candidate.as_bytes());
    let mut preserved=Vec::new();
    for runtime in [source_root,candidate_root] {
        for name in ["runtime.env","native-personal.json","config/freeze-signing-key.json"] {
            let path=runtime.join(name); if path.is_file() { preserved.push((path.clone(),file_hash(&path))); }
        }
    }
    let prefixes=["native-document-","native-upload-","native-publication-","native-static-export-"];
    for prefix in prefixes {
        let path=profile.join(format!("{prefix}{source_suffix}.json"));
        if path.is_file() { preserved.push((path.clone(),file_hash(&path))); }
        assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists());
    }
    let config_names=|| { let mut names:Vec<_>=fs::read_dir(candidate_root.join("config")).unwrap().map(|v|v.unwrap().file_name()).collect(); names.sort(); names };
    let config_before=config_names();
    eprintln!("R5_COPY_STEP=actual-in-memory-configuration-planner");
    let planned=store.plan_native_target_configuration(&mut admitted,&mut source,&mut prepared,&mut source_session,&mut target_session,20).unwrap();
    let planning_receipt=serde_json::to_value(planned.receipt()).unwrap();
    assert_eq!(planning_receipt["state"],"ConfigurationPlannedInactive"); assert_eq!(planning_receipt["configurationPlanned"],true);
    assert_eq!(planning_receipt["operationId"],operation);
    assert_eq!(planning_receipt["configurationFileCount"].as_u64().unwrap(),3+preserved.iter().filter(|(p,_)|p.parent()==Some(profile.as_path())).count() as u64);
    for flag in ["configurationVerified","preflightVerified","executionPermitted"] { assert_eq!(planning_receipt[flag],false); }
    assert_eq!(config_names(),config_before);
    for (path,before) in &preserved { assert_eq!(&file_hash(path),before); }
    for prefix in prefixes { assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists()); }
    assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
    if mode > 0 {
        eprintln!("R5_COPY_STEP=actual-inactive-publication-begin");
        let permit=store.begin_native_configuration_publication_test_clock(&mut admitted,&mut source,&mut prepared,planned,&mut source_session,&mut target_session,20).unwrap();
        let begin_receipt=serde_json::to_value(permit.receipt()).unwrap();
        assert_eq!(begin_receipt["state"],"PublicationIntentInactive");
        for flag in ["configurationPublished","preflightVerified","executionPermitted"] { assert_eq!(begin_receipt[flag],false); }
        assert_eq!(config_names(),config_before);
        for (path,before) in &preserved { assert_eq!(&file_hash(path),before); }
        for prefix in prefixes { assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists()); }
        assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
        if mode == 3 {
            let held=store.publish_native_configuration_held_test_clock(&mut admitted,&mut source,&mut prepared,permit,&mut source_session,&mut target_session,20).unwrap();
            let published=serde_json::to_value(held.receipt()).unwrap();
            assert_eq!(published["state"],"PublishedInactive");
            assert_eq!(published["executionPermitted"],false); assert_eq!(published["preflightVerified"],false);
            let observation=store.observe_published_native_configuration_test_clock(&mut admitted,&source,&prepared,&held,&source_session,&target_session,20).unwrap();
            let observation=serde_json::to_value(observation).unwrap();
            assert_eq!(observation["configurationVerified"],true); assert_eq!(observation["targetInventoryReobserved"],false);
            assert_eq!(observation["preflightVerified"],false); assert_eq!(observation["executionPermitted"],false);
            drop(held);
            target_session.stop().unwrap(); source_session.stop().unwrap();
            drop(target_session); drop(source_session);
            let repair=store.repair_native_configuration_publication(&key_file).unwrap();
            let repair_receipt=serde_json::to_value(repair).unwrap();
            assert_eq!(repair_receipt["state"],"RestoredGeneratedInactive");
            assert_eq!(repair_receipt["executionPermitted"],false);
            for (path,before) in &preserved { assert_eq!(&file_hash(path),before); }
            assert_eq!(config_names(),config_before);
            for prefix in prefixes { assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists()); }
            assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
            assert!(candidate_root.join("native-target-preparation-current.json").exists());
            fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"REAL_SIGNED_TEST_R5_INACTIVE_PUBLICATION_CONFIG_OBSERVATION_AND_OFFLINE_REPAIR_ONLY","beginReceipt":begin_receipt,"publishedReceipt":published,"observationReceipt":observation,"repairReceipt":repair_receipt,"configurationAndSourceFilesUnchangedAfterRepair":true,"sourceSelectionPreserved":true,"targetJournalsNotInstalled":true,"inactiveMarkerRetained":true})).unwrap()).unwrap();
            return;
        }
        target_session.stop().unwrap(); source_session.stop().unwrap();
        assert_eq!(target_session.start().err().expect("Inactive marker blocks start").code,"NATIVE_RESTORE_ACTIVATION_REQUIRED");
        assert!(!candidate_root.join("native-database/postmaster.pid").exists());
        assert!(!source_root.join("native-database/postmaster.pid").exists());
        if mode == 2 {
            drop(target_session); drop(source_session);
            // A NEW fixture only: install just the first generated afterimage to
            // emulate interruption between env and owner, then use raw guards.
            permit.simulate_partial_for_test(1).unwrap();
            drop(permit);
            let repair=store.repair_native_configuration_publication(&key_file).unwrap();
            let repair_receipt=serde_json::to_value(repair).unwrap();
            assert_eq!(repair_receipt["state"],"RestoredGeneratedInactive");
            assert_eq!(repair_receipt["executionPermitted"],false);
            for (path,before) in &preserved { assert_eq!(&file_hash(path),before); }
            assert_eq!(config_names(),config_before);
            for prefix in prefixes { assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists()); }
            assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
            assert!(candidate_root.join("native-target-preparation-current.json").exists());
            fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"REAL_SIGNED_TEST_R5_TORN_OFFLINE_REPAIR_ONLY","beginReceipt":begin_receipt,"repairReceipt":repair_receipt,"configurationAndSourceFilesUnchanged":true,"sourceSelectionPreserved":true,"targetJournalsNotInstalled":true,"inactiveMarkerRetained":true})).unwrap()).unwrap();
        } else {
            drop(permit);
            fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"REAL_SIGNED_TEST_R5_INACTIVE_PUBLICATION_BEGIN_ONLY","beginReceipt":begin_receipt,"configurationAndSourceFilesUnchanged":true,"sourceSelectionPreserved":true,"targetJournalsNotInstalled":true})).unwrap()).unwrap();
        }
        return;
    }
    // Self-consistent disk tampering still cannot replace the held attempt.
    // Only these two control files in this NEW isolated fixture are changed;
    // retain them as failed evidence, never clear/replay an inactive marker.
    let marker_path=candidate_root.join("native-target-preparation-current.json");
    let intent_path=candidate_root.join(format!("native-target-preparation-{operation}/intent.json"));
    let mut marker:serde_json::Value=serde_json::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
    let mut changed_intent=intent.clone(); let changed_attempt=uuid::Uuid::new_v4().to_string();
    marker["attemptId"]=serde_json::json!(changed_attempt); changed_intent["attemptId"]=marker["attemptId"].clone();
    fs::write(&marker_path,serde_json::to_vec(&marker).unwrap()).unwrap(); fs::write(&intent_path,serde_json::to_vec(&changed_intent).unwrap()).unwrap();
    let rejected=store.plan_native_target_configuration(&mut admitted,&mut source,&mut prepared,&mut source_session,&mut target_session,20).err().expect("Changed disk attempt must not replace the held adapter capability");
    assert_eq!(rejected.code,"NATIVE_TARGET_CONFIGURATION_CHANGED");
    assert_eq!(config_names(),config_before);
    for (path,before) in &preserved { assert_eq!(&file_hash(path),before); }
    for prefix in prefixes { assert!(!profile.join(format!("{prefix}{target_suffix}.json")).exists()); }
    assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
    // Persist only public outcome metadata; no keys, DB credentials, raw source
    // configuration or decrypted archive contents cross this boundary.
    let result=serde_json::json!({"scope":"REAL_SIGNED_TEST_ADMISSION_AUTHENTICATED_R5_INACTIVE_DATA_COPY_AND_CONFIGURATION_PLAN_ONLY","receipt":receipt,"planningReceipt":planning_receipt,"heldAttemptTamperRejected":true,"configurationAndSourceFilesUnchanged":true,"targetJournalsNotInstalled":true,"tamperedFixtureControlFilesRetained":true,"sourceSelectionPreserved":true,"ordinaryActivationPermitted":false,"sourceOriginalsNotUsedAsRuntimeRoots":true});
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    target_session.stop().unwrap(); source_session.stop().unwrap();
    assert_eq!(target_session.start().err().expect("Inactive marker must block service start").code,"NATIVE_RESTORE_ACTIVATION_REQUIRED");
    assert!(!Path::new(&candidate).join("native-database/postmaster.pid").exists());
    assert!(!Path::new(&source_service_root).join("native-database/postmaster.pid").exists());
}
