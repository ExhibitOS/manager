// SPDX-License-Identifier: Apache-2.0
//! Conditional activation of a positively identified, stopped legacy owner.
//! No missing daemon/source, operator boolean or journal can issue this proof.
use super::*;

fn owner_required() -> LifecycleError {
    err("NATIVE_LEGACY_ORIGINAL_WRITER_REQUIRED")
}
fn positive_stopped(status: &crate::Status, expected: &[String]) -> bool {
    !expected.is_empty()
        && status.installed
        && status.state == "stopped"
        && !status.readiness.ready
        && status.active_job.is_none()
        && status.services.len() == expected.len()
        && expected.iter().all(|name| {
            status
                .services
                .iter()
                .filter(|s| &s.name == name && matches!(s.state.as_str(), "exited" | "dead"))
                .count()
                == 1
        })
}
// Select only images already authenticated in the archive and still declared by
// the byte-pinned installed compose. Never accept an operator-supplied helper.
fn bound_images(
    manifest: &BundleManifest,
    images: &[crate::restoration::PreservedImage],
    compose: &serde_json::Value,
) -> Result<(String, String)> {
    let runtime = compose["services"]["platform"]["image"]
        .as_str()
        .ok_or_else(owner_required)?;
    let runtime_image = images
        .iter()
        .filter(|i| i.reference == runtime)
        .collect::<Vec<_>>();
    let maintenance = manifest
        .images
        .iter()
        .filter(|i| {
            i.archive
                .as_ref()
                .is_some_and(|a| a.path == "maintenance-image.tar")
        })
        .collect::<Vec<_>>();
    if runtime_image.len() != 1 || maintenance.len() != 1 {
        return Err(owner_required());
    }
    let maintenance = &maintenance[0].reference;
    if !maintenance.strip_prefix("sha256:").is_some_and(hash_valid)
        || images
            .iter()
            .filter(|i| &i.reference == maintenance && &i.content_id == maintenance)
            .count()
            != 1
    {
        return Err(owner_required());
    }
    let runtime = runtime_image[0]
        .content_id
        .strip_prefix("sha256:")
        .filter(|v| hash_valid(v))
        .ok_or_else(owner_required)?;
    Ok((runtime.into(), maintenance.clone()))
}
fn same_source(
    a: &crate::signed_release::trust::SourceStoppedReceipt,
    b: &crate::signed_release::trust::SourceStoppedReceipt,
) -> bool {
    a.source_instance == b.source_instance
        && a.bundle_id == b.bundle_id
        && a.platform_container == b.platform_container
        && a.database_container == b.database_container
        && a.runtime_image_sha256 == b.runtime_image_sha256
        && a.blob_volume == b.blob_volume
        && a.configuration_volume == b.configuration_volume
        && a.database_volume == b.database_volume
}
/// This proof is local/cooperative ownership, not a claim that unrelated external
/// programs cannot write. The existing original-root lock stays held through
/// activation. An unavailable conditional legacy adapter never becomes success.
struct OriginalWriter<'a> {
    service: LifecycleService,
    _guard: crate::OperationGuard,
    root: DirectoryPin,
    pins: Vec<Pin>,
    services: Vec<String>,
    runtime_image: String,
    maintenance_image: String,
    signing_record: &'a crate::signed_release::trust::LegacyConfigurationProof,
    identity: crate::signed_release::trust::SourceStoppedReceipt,
    volume: serde_json::Value,
}
impl<'a> OriginalWriter<'a> {
    fn capture(candidate: &'a Candidate) -> Result<Self> {
        candidate.check()?;
        let original = candidate
            .source
            .evidence
            .original_root
            .as_ref()
            .ok_or_else(owner_required)?;
        private_directory(original).map_err(|_| owner_required())?;
        let root = DirectoryPin::open(original)?;
        let service = LifecycleService::bound_existing(original.clone())?;
        // This codec admits an existing Docker legacy source only. Never install
        // an engine, probe another source kind or make it a native prerequisite.
        let lock_path = original.join("operation.lock");
        if fs::canonicalize(&lock_path).map_err(|_| owner_required())? != lock_path {
            return Err(owner_required());
        }
        // Lock the exact existing inode; do not create a plausible new lock in
        // an unrelated/missing source and mistake that for an owner handoff.
        let (_, lock_pin) =
            crate::hosted_runtime_file::bounded(&lock_path, 0).map_err(|_| owner_required())?;
        let lock = lock_pin.duplicate_file()?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| owner_required())?;
        let guard = crate::OperationGuard(lock);
        lock_pin.check()?;
        let mut pins = vec![lock_pin];
        for (current, archived) in [
            ("bundle/manifest.json", "manager-bundle-manifest.json"),
            ("bundle/compose.yaml", "manager-compose.yaml"),
            ("installed.json", "manager-installed.json"),
            ("engine.json", "manager-engine.json"),
            ("runtime.env", "manager-runtime.env"),
        ] {
            let path = original.join(current);
            if fs::canonicalize(&path).map_err(|_| owner_required())? != path {
                return Err(owner_required());
            }
            let expected = candidate
                .source
                .configuration
                .configuration_sha256
                .get(archived)
                .ok_or_else(owner_required)?;
            let pin = if current == "runtime.env" {
                let (bytes, pin) = super::super::super::codec::private_read(&path, 1024 * 1024)
                    .map_err(|_| owner_required())?;
                if &crate::digest(&bytes.0) != expected {
                    return Err(owner_required());
                }
                pin
            } else {
                let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 1024 * 1024)
                    .map_err(|_| owner_required())?;
                if &crate::digest(&bytes) != expected {
                    return Err(owner_required());
                }
                pin
            };
            pins.push(pin);
        }
        let manifest = service.manifest().map_err(|_| owner_required())?;
        if serde_json::to_value(&manifest).ok()
            != serde_json::to_value(&candidate.source.configuration.manifest).ok()
        {
            return Err(owner_required());
        }
        service
            .validate_compose(&manifest, "docker")
            .map_err(|_| owner_required())?;
        let compose: serde_json::Value = serde_json::from_slice(&crate::run(
            "docker",
            &crate::compose_args(&manifest, &["config", "--format", "json"]),
            Some(&original.join("bundle")),
            30,
        )?)
        .map_err(|_| owner_required())?;
        let (runtime_image, maintenance_image) =
            bound_images(&manifest, &candidate.source.configuration.images, &compose)?;
        let identity = crate::signed_release::trust::legacy_stopped_observe(
            &service,
            &manifest.bundle_id,
            &runtime_image,
        )?;
        let volume = crate::backup_creation::inspected(
            "docker",
            &[
                "volume".into(),
                "inspect".into(),
                identity.configuration_volume.clone(),
            ],
        )?;
        let value = Self {
            service,
            _guard: guard,
            root,
            pins,
            services: manifest.services,
            runtime_image,
            maintenance_image,
            signing_record: &candidate.source.configuration.signing_record,
            identity,
            volume,
        };
        value.check()?;
        candidate.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        self.root.check()?;
        for pin in &self.pins {
            pin.check()?;
        }
        // status already validates the fixed installation/container ownership.
        // runtime_unavailable/not_installed/empty output are all refusals.
        if !positive_stopped(
            &self.service.status().map_err(|_| owner_required())?,
            &self.services,
        ) {
            return Err(owner_required());
        }
        let before = crate::signed_release::trust::legacy_stopped_observe(
            &self.service,
            &self.identity.source_instance,
            &self.runtime_image,
        )?;
        if !same_source(&self.identity, &before) {
            return Err(owner_required());
        }
        let volume = || {
            crate::backup_creation::inspected(
                "docker",
                &[
                    "volume".into(),
                    "inspect".into(),
                    self.identity.configuration_volume.clone(),
                ],
            )
        };
        if volume()? != self.volume {
            return Err(owner_required());
        }
        // Existing helper: read-only volume mount, fixed local image, private
        // stable file metadata/hash, bounded output and owned normal cleanup.
        crate::signed_release::trust::legacy_configuration_observe(
            &self.maintenance_image,
            &self.identity.configuration_volume,
            self.signing_record,
        )?;
        let after = crate::signed_release::trust::legacy_stopped_observe(
            &self.service,
            &self.identity.source_instance,
            &self.runtime_image,
        )?;
        if !same_source(&self.identity, &after) || volume()? != self.volume {
            return Err(owner_required());
        }
        self.root.check()?;
        for pin in &self.pins {
            pin.check()?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivationReceipt {
    candidate_id: String,
    backup_id: String,
    state: String,
    system_identifier: String,
    freeze_rows_verified: u64,
    nonzero_freeze_rows_verified: bool,
    origin: String,
    key_id: String,
}
impl AppliedSchema<'_> {
    /// The only marker-removal path consumes an in-memory, freshly authenticated
    /// schema capability and an original-root proof which cannot be deserialized.
    pub(crate) fn activate(
        self,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<ActivationReceipt> {
        self.summary()?;
        let candidate = self.candidate;
        let (staged, staged_pin) = read::<Staged>(&candidate.job.join("staged.json"))?;
        let (database, database_pin) = read::<Database>(&candidate.job.join("database.json"))?;
        let signing = crate::digest(&candidate.source.configuration.freeze_signing_key.0);
        if !staged.matches(&candidate.intent, &self.frame.system_identifier, &signing)
            || !database.matches(&candidate.intent.id, &self.frame.system_identifier)
        {
            return Err(changed());
        }
        let restored = candidate.job.join(format!("restored-{}", staged.attempt));
        let configuration_pins = candidate.restored_configuration(&restored)?;
        let signing_path = candidate.job.join("activation/freeze-signing-key.json");
        let (key_bytes, key_pin) = super::super::super::codec::private_read(&signing_path, 8192)?;
        if key_bytes.0 != candidate.source.configuration.freeze_signing_key.0 {
            return Err(changed());
        }
        let mut session = Session::new(
            &candidate.service,
            candidate.payload.clone(),
            candidate.intent.ports,
        )?;
        let cancellation = session.cancellation();
        publish_cancel(cancellation.clone());
        // Publish cancellation before the conditional Docker observer can run a
        // bounded helper. Its owned cleanup still completes normally on cancel.
        let original = if cancellation.load(Ordering::SeqCst) {
            Err(err("NATIVE_CANCELLED"))
        } else {
            OriginalWriter::capture(candidate)
        };
        let result = (|| {
            let original = original.as_ref().map_err(Clone::clone)?;
            if cancellation.load(Ordering::SeqCst) {
                return Err(err("NATIVE_CANCELLED"));
            }
            original.check()?;
            session.start_database_only_cancellable()?;
            let system = session.maintenance_system_identifier()?;
            if !database.matches(&candidate.intent.id, &system) {
                return Err(changed());
            }
            candidate.check()?;
            let binding = serde_json::json!({"candidateId":candidate.intent.id,"backupId":candidate.intent.backup_id,"manifestPath":restored.join("manifest.json"),"manifestSha256":candidate.intent.manifest_sha256,"payloadSha256":candidate.intent.payload_sha256,"systemIdentifier":system});
            let mut reconcile = binding.clone();
            reconcile["operation"] = serde_json::json!("reconcile");
            let current: SchemaFrame = serde_json::from_value(
                session
                    .apply_legacy_schema(&serde_json::to_vec(&reconcile).map_err(|_| changed())?)?,
            )
            .map_err(|_| changed())?;
            if current.operation != "legacy-schema-applied"
                || serde_json::to_value(&current).ok() != serde_json::to_value(&self.frame).ok()
            {
                return Err(changed());
            }
            session.maintenance_owner(
                "legacy_account",
                &BTreeMap::from([
                    (
                        "TENANT_ID".into(),
                        candidate.source.configuration.credentials.tenant_id.clone(),
                    ),
                    (
                        "ADMIN_SUBJECT".into(),
                        candidate.source.configuration.credentials.subject.clone(),
                    ),
                ]),
            )?;
            let freeze = session
                .check_legacy_activation(&serde_json::to_vec(&binding).map_err(|_| changed())?)?;
            let count = freeze["freezeRowsVerified"]
                .as_u64()
                .filter(|v| *v <= 10000)
                .ok_or_else(changed)?;
            if freeze["format"] != 1
                || freeze["operation"] != "legacy-activation-integrity-checked"
                || freeze["candidateId"] != candidate.intent.id
                || freeze["backupId"] != candidate.intent.backup_id
                || freeze["manifestSha256"] != candidate.intent.manifest_sha256
                || freeze["payloadSha256"] != candidate.intent.payload_sha256
                || freeze["systemIdentifier"] != system
                || freeze["origin"] != candidate.intent.original_origin
                || freeze["nonzeroFreezeRowsVerified"] != (count > 0)
                || freeze["rightsOrGrantRenewed"] != false
                || !freeze["keyId"].as_str().is_some_and(hash_valid)
            {
                return Err(changed());
            }
            Ok(ActivationReceipt {
                candidate_id: candidate.intent.id.clone(),
                backup_id: candidate.intent.backup_id.clone(),
                state: "activated_same_legacy_owner".into(),
                system_identifier: system,
                freeze_rows_verified: count,
                nonzero_freeze_rows_verified: count > 0,
                origin: candidate.intent.original_origin.clone(),
                key_id: freeze["keyId"].as_str().ok_or_else(changed)?.into(),
            })
        })();
        loop {
            let stopped = session.stop()?;
            if stopped.postgres_pid.is_none()
                && stopped.node_pid.is_none()
                && stopped.auxiliary_pid.is_none()
            {
                break;
            }
        }
        let receipt = result?;
        let original = original?;
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        let _guard = candidate.service.lock()?;
        original.check()?;
        self.summary()?;
        for pin in configuration_pins {
            pin.check()?;
        }
        staged_pin.check()?;
        database_pin.check()?;
        key_pin.check()?;
        let config = candidate.service.root.join("config");
        if exists(&config)? {
            private_directory(&config)?;
        } else {
            new_directory(&config)?;
        }
        let target_key = config.join("freeze-signing-key.json");
        if exists(&target_key)? {
            let (bytes, pin) = super::super::super::codec::private_read(&target_key, 8192)?;
            if bytes.0 != key_bytes.0 {
                return Err(changed());
            }
            pin.check()?;
        } else {
            crate::write_private_new(&config, "freeze-signing-key.json", &key_bytes.0, 8192)?;
            sync(&config)?;
        }
        let (installed_key, installed_key_pin) =
            super::super::super::codec::private_read(&target_key, 8192)?;
        if installed_key.0 != key_bytes.0 {
            return Err(changed());
        }
        original.check()?;
        candidate.check()?;
        key_pin.check()?;
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        crate::write_json(&candidate.job, "activated.json", &receipt)?;
        sync(&candidate.job)?;
        // Keep the pending marker on every preceding error/ambiguity/cancel.
        let (marker, marker_pin) = read::<Pending>(&candidate.service.root.join(MARKER))?;
        if marker.format != 1 || marker.id != candidate.intent.id {
            return Err(changed());
        }
        marker_pin.check()?;
        installed_key_pin.check()?;
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        fs::remove_file(candidate.service.root.join(MARKER)).map_err(|_| changed())?;
        sync(&candidate.service.root)?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn status() -> crate::Status {
        crate::Status {
            installed: true,
            bundle_id: None,
            version: None,
            state: "stopped".into(),
            services: vec![
                crate::ServiceState {
                    name: "database".into(),
                    state: "exited".into(),
                    health: None,
                },
                crate::ServiceState {
                    name: "platform".into(),
                    state: "exited".into(),
                    health: None,
                },
            ],
            readiness: crate::Readiness::default(),
            storage: crate::StorageState {
                used_bytes: None,
                free_bytes: 0,
                quota_bytes: 0,
                minimum_free_bytes: 0,
            },
            active_job: None,
        }
    }
    #[test]
    #[ignore = "Requires the retained real archive, authenticated same016 candidate, fixed capsule and exclusive runtime slot; never applies schema or activates"]
    fn actual_authenticated_same_candidate_cancel_and_original_owner_probe() {
        let input = |name| {
            let path = PathBuf::from(std::env::var_os(name).expect("explicit owned fixture input"));
            assert!(path.is_absolute() && fs::canonicalize(&path).unwrap() == path);
            path
        };
        let payload = Arc::new(Payload::open(&input("EXHIBITOS_LEGACY_TEST_PAYLOAD")).unwrap());
        let workspace = input("EXHIBITOS_LEGACY_TEST_WORKSPACE");
        private_directory(&workspace).unwrap();
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
        let target = input("EXHIBITOS_LEGACY_INSPECTION_TEST_TARGET");
        let marker_before = fs::read(target.join(MARKER)).unwrap();
        assert!(!target.join("config/freeze-signing-key.json").exists());
        let source = crate::native_legacy_migration::authenticate(
            &payload,
            &input("EXHIBITOS_LEGACY_TEST_ARCHIVE"),
            &input("EXHIBITOS_LEGACY_TEST_KEY"),
            &workspace,
            Some(&input("EXHIBITOS_LEGACY_TEST_ORIGINAL")),
            &AtomicBool::new(false),
        )
        .unwrap();
        let candidate = source.resume_candidate(payload, target.clone()).unwrap();
        let staged_before = fs::read(candidate.job.join("staged.json")).unwrap();
        let database_before = fs::read(candidate.job.join("database.json")).unwrap();
        let environment_before = candidate.intent.environment_sha256.clone();
        // Reuse the already-applied016 candidate. There is deliberately no
        // inspect/apply/restore call or journal-issued capability in this test.
        let applied = candidate
            .recover_schema(|_| {})
            .unwrap()
            .expect("existing actual016 candidate must reconcile");
        let schema = applied.summary().unwrap();
        let cancelled = applied
            .activate(|flag| flag.store(true, Ordering::SeqCst))
            .err()
            .expect("published cancellation must refuse before owner observation");
        assert_eq!(cancelled.code, "NATIVE_CANCELLED");
        assert!(fs::read(target.join(MARKER)).unwrap() == marker_before);
        assert!(!target.join("config/freeze-signing-key.json").exists());
        // Observe the real conditional legacy owner only. Even an unexpectedly
        // positive proof is dropped; it must never activate a zero-grant fixture.
        let owner = match OriginalWriter::capture(&candidate) {
            Ok(proof) => {
                proof.check().unwrap();
                drop(proof);
                serde_json::json!({"state":"positive-readonly-proof-dropped","activationExecuted":false})
            }
            Err(error) => {
                serde_json::json!({"state":"refused","code":error.code,"activationExecuted":false})
            }
        };
        candidate.check().unwrap();
        assert!(fs::read(candidate.job.join("staged.json")).unwrap() == staged_before);
        assert!(fs::read(candidate.job.join("database.json")).unwrap() == database_before);
        assert!(candidate.intent.environment_sha256 == environment_before);
        assert!(fs::read(target.join(MARKER)).unwrap() == marker_before);
        assert!(!target.join("config/freeze-signing-key.json").exists());
        crate::write_json(
            &workspace,
            "authority-cancel-owner-probe.json",
            &serde_json::json!({"scope":"ACTUAL_SAME016_CANCEL_AND_READONLY_OWNER_PROBE_NOT_ACTIVATION_OR_FULL_RECOVERY","schema":schema,"cancelCode":cancelled.code,"owner":owner,"markerUnchanged":true,"activeSigningNotInstalled":true,"stagedAttemptDatabaseAndEnvironmentUnchanged":true,"schemaApplyExecuted":false,"activationExecuted":false,"nonzeroFreezeAcceptance":false}),
        )
        .unwrap();
    }
    #[test]
    fn original_configuration_images_require_authenticated_exact_bindings() {
        let runtime = format!("sha256:{}", "a".repeat(64));
        let helper = format!("sha256:{}", "b".repeat(64));
        let mut manifest: BundleManifest = serde_json::from_value(serde_json::json!({
            "schemaVersion":"1.0.0-draft.1", "bundleId":"synthetic", "version":"test",
            "protocolVersion":"1", "composeSha256":"c".repeat(64), "projectName":"synthetic",
            "services":["platform","database"], "images":[
                {"reference":runtime,"archive":null},
                {"reference":helper,"archive":{"path":"maintenance-image.tar","sha256":"d".repeat(64),"bytes":1}}
            ], "ports":[13200], "openUrl":"http://127.0.0.1:13200", "readinessUrl":"http://127.0.0.1:13200/api/v1/readiness", "minimumFreeBytes":1
        })).unwrap();
        let mut images = vec![
            crate::restoration::PreservedImage {
                reference: runtime.clone(),
                content_id: runtime.clone(),
                archive: "images/runtime.tar".into(),
                bytes: 1,
                sha256: "e".repeat(64),
            },
            crate::restoration::PreservedImage {
                reference: helper.clone(),
                content_id: helper.clone(),
                archive: "images/helper.tar".into(),
                bytes: 1,
                sha256: "f".repeat(64),
            },
        ];
        let compose = serde_json::json!({"services":{"platform":{"image":runtime}}});
        assert_eq!(
            bound_images(&manifest, &images, &compose).unwrap(),
            ("a".repeat(64), helper)
        );
        assert!(bound_images(&manifest, &images, &serde_json::json!({})).is_err());
        images[1].content_id = format!("sha256:{}", "0".repeat(64));
        assert!(bound_images(&manifest, &images, &compose).is_err());
        images[1].content_id = images[1].reference.clone();
        manifest.images[1].archive = None;
        assert!(bound_images(&manifest, &images, &compose).is_err());
    }
    #[test]
    fn original_configuration_identity_cannot_switch_volume_container_or_image() {
        let make = || crate::signed_release::trust::SourceStoppedReceipt {
            source_instance: "original".into(),
            bundle_id: "bundle".into(),
            platform_container: "app".into(),
            database_container: "db".into(),
            runtime_image_sha256: "a".repeat(64),
            blob_volume: "blobs".into(),
            configuration_volume: "config".into(),
            database_volume: "database".into(),
            observed_at: 1,
        };
        let original = make();
        let mut observed = make();
        observed.observed_at = 2;
        assert!(same_source(&original, &observed));
        for field in [
            "configuration_volume",
            "platform_container",
            "runtime_image_sha256",
            "bundle_id",
        ] {
            let mut observed = make();
            match field {
                "configuration_volume" => observed.configuration_volume = "other".into(),
                "platform_container" => observed.platform_container = "other".into(),
                "runtime_image_sha256" => observed.runtime_image_sha256 = "b".repeat(64),
                _ => observed.bundle_id = "other".into(),
            }
            assert!(!same_source(&original, &observed));
        }
    }
    #[test]
    fn unavailable_or_incomplete_original_is_never_stopped_authority() {
        let expected = vec!["database".into(), "platform".into()];
        let mut value = status();
        assert!(positive_stopped(&value, &expected));
        for state in [
            "runtime_unavailable",
            "not_installed",
            "running",
            "degraded",
        ] {
            value.state = state.into();
            assert!(!positive_stopped(&value, &expected));
        }
        value = status();
        value.services.clear();
        assert!(!positive_stopped(&value, &expected));
        assert!(!positive_stopped(&value, &[]));
        value = status();
        value.services[1].state = "running".into();
        assert!(!positive_stopped(&value, &expected));
        value = status();
        value.services[1].name = "database".into();
        assert!(!positive_stopped(&value, &expected));
        value = status();
        value.installed = false;
        assert!(!positive_stopped(&value, &expected));
    }
}
