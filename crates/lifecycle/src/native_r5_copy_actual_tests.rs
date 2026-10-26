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
    let relative="platform/apps/web/dist/index.html"; let html=target_root.join(relative);
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
    let authenticated=controller.capture_native_update_source(&selected.selection_token,&mut source_session,crate::installations::native_backup::Input {key_file,destination:root.join("authenticated-backup")}).unwrap();
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
    let store=Store::provision(&profile,"default",policy,10).unwrap();
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
    let prepared=store.prepare_native_target_copy(&mut admitted,&mut source,&mut source_session,&mut target_session,&target_id,&operation,20).unwrap();
    let receipt=serde_json::to_value(prepared.receipt()).unwrap(); assert_eq!(receipt["state"],"PreparedInactive");
    for flag in ["configurationVerified","preflightVerified","executionPermitted"] { assert_eq!(receipt[flag],false); assert_eq!(receipt["observation"][flag],false); }
    assert_eq!(crate::installations::load(&profile).unwrap().unwrap().0.active_id,source_id);
    let intent: serde_json::Value=serde_json::from_slice(&fs::read(Path::new(&candidate).join(format!("native-target-preparation-{operation}/intent.json"))).unwrap()).unwrap();
    assert_eq!(intent["recipe"],"authenticated-native-r5-copy-v1"); assert_eq!(intent["state"],"PreparedInactive");
    // Persist only public outcome metadata; no keys, DB credentials, raw source
    // configuration or decrypted archive contents cross this boundary.
    let result=serde_json::json!({"scope":"REAL_SIGNED_TEST_ADMISSION_AUTHENTICATED_R5_INACTIVE_DATA_COPY_ONLY","receipt":receipt,"sourceSelectionPreserved":true,"ordinaryActivationPermitted":false,"sourceOriginalsNotUsedAsRuntimeRoots":true});
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    target_session.stop().unwrap(); source_session.stop().unwrap();
    assert_eq!(target_session.start().err().expect("Inactive marker must block service start").code,"NATIVE_RESTORE_ACTIVATION_REQUIRED");
    assert!(!Path::new(&candidate).join("native-database/postmaster.pid").exists());
    assert!(!Path::new(&source_service_root).join("native-database/postmaster.pid").exists());
}
