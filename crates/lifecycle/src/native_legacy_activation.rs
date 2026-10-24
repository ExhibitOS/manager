// SPDX-License-Identifier: Apache-2.0
//! Conditional activation of a positively identified, stopped legacy owner.
//! No missing daemon/source, operator boolean or journal can issue this proof.
use super::*;

fn owner_required() -> LifecycleError {
    err("NATIVE_LEGACY_ORIGINAL_WRITER_REQUIRED")
}
fn positive_stopped(status: &crate::Status, expected: &[String]) -> bool {
    status.installed
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
/// This proof is local/cooperative ownership, not a claim that unrelated external
/// programs cannot write. The existing original-root lock stays held through
/// activation. An unavailable conditional legacy adapter never becomes success.
struct OriginalWriter {
    service: LifecycleService,
    _guard: crate::OperationGuard,
    root: DirectoryPin,
    pins: Vec<Pin>,
    services: Vec<String>,
}
impl OriginalWriter {
    fn capture(candidate: &Candidate) -> Result<Self> {
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
            ("config/freeze-signing-key.json", "freeze-signing-key.json"),
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
            let pin = if matches!(current, "runtime.env" | "config/freeze-signing-key.json") {
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
        let value = Self {
            service,
            _guard: guard,
            root,
            pins,
            services: manifest.services,
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
        let original = OriginalWriter::capture(candidate)?;
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
        let result = (|| {
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
        original.check()?;
        candidate.check()?;
        key_pin.check()?;
        crate::write_json(&candidate.job, "activated.json", &receipt)?;
        sync(&candidate.job)?;
        // Keep the pending marker on every preceding error/ambiguity/cancel.
        let (marker, marker_pin) = read::<Pending>(&candidate.service.root.join(MARKER))?;
        if marker.format != 1 || marker.id != candidate.intent.id {
            return Err(changed());
        }
        marker_pin.check()?;
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
