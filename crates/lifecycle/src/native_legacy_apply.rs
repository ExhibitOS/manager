// SPDX-License-Identifier: Apache-2.0
//! Fixed additive schema application. Neither a journal nor a caller-supplied
//! target hash can issue this capability, and it does not activate services.
use super::*;
#[path = "native_legacy_activation.rs"]
mod activation;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SchemaFrame {
    format: u32,
    operation: String,
    candidate_id: String,
    backup_id: String,
    manifest_sha256: String,
    payload_sha256: String,
    system_identifier: String,
    source_inventory_sha256: String,
    source_schema_sha256: String,
    target_schema_sha256: String,
    target_migrations: Vec<InspectionMigration>,
    original_data_preserved: bool,
}
/// This is deliberately not serde-constructible. It borrows the same freshly
/// authenticated, fenced candidate and retains every source/configuration pin.
pub(crate) struct AppliedSchema<'a> {
    candidate: &'a Candidate,
    frame: SchemaFrame,
    pins: Vec<Pin>,
}
impl AppliedSchema<'_> {
    pub(crate) fn summary(&self) -> Result<serde_json::Value> {
        self.candidate.check()?;
        self.candidate.payload.recheck()?;
        for pin in &self.pins {
            pin.check()?;
        }
        serde_json::to_value(&self.frame).map_err(|_| changed())
    }
}
impl<'a> SchemaInspection<'a> {
    /// Consume the nonserializable inspection once. A failure after the commit
    /// request leaves the candidate blocked and requires authenticated recovery.
    pub(crate) fn apply(
        self,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<AppliedSchema<'a>> {
        self.summary()?;
        let (frame, mut pins) = self.candidate.schema_operation(true, publish_cancel)?;
        if frame.operation != "legacy-schema-applied"
            || frame.source_inventory_sha256 != self.frame.source_inventory_sha256
            || frame.source_schema_sha256 != self.frame.source_schema_sha256
            || frame.target_schema_sha256 != self.frame.target_schema_sha256
            || serde_json::to_value(&frame.target_migrations).ok()
                != serde_json::to_value(&self.frame.target_migrations).ok()
        {
            return Err(changed());
        }
        pins.extend(self.pins);
        Ok(AppliedSchema {
            candidate: self.candidate,
            frame,
            pins,
        })
    }
}
impl Candidate {
    /// Fresh source authentication is required to construct Candidate on reopen.
    /// A proven 015 state returns None, never an automatic replay of the intent.
    pub(crate) fn recover_schema(
        &self,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<Option<AppliedSchema<'_>>> {
        let (frame, pins) = self.schema_operation(false, publish_cancel)?;
        Ok(
            (frame.operation == "legacy-schema-applied").then_some(AppliedSchema {
                candidate: self,
                frame,
                pins,
            }),
        )
    }
    fn schema_operation(
        &self,
        apply: bool,
        publish_cancel: impl FnOnce(Arc<AtomicBool>),
    ) -> Result<(SchemaFrame, Vec<Pin>)> {
        self.check()?;
        self.payload.recheck()?;
        let (staged, staged_pin) = read::<Staged>(&self.job.join("staged.json"))?;
        let (database, database_pin) = read::<Database>(&self.job.join("database.json"))?;
        let signing = crate::digest(&self.source.configuration.freeze_signing_key.0);
        if !staged.matches(&self.intent, &database.system_identifier, &signing)
            || !database.matches(&self.intent.id, &staged.system_identifier)
        {
            return Err(changed());
        }
        let restored = self.job.join(format!("restored-{}", staged.attempt));
        let mut pins = self.restored_configuration(&restored)?;
        let intent = serde_json::json!({"format":1,"candidateId":self.intent.id,"systemIdentifier":database.system_identifier,"manifestSha256":self.intent.manifest_sha256,"payloadSha256":self.intent.payload_sha256});
        if apply {
            let _guard = self.service.lock()?;
            self.check()?;
            let path = self.job.join("schema-apply-intent.json");
            if exists(&path)? {
                let (prior, pin) = read::<serde_json::Value>(&path)?;
                if prior != intent {
                    return Err(changed());
                }
                pin.check()?;
            } else {
                write_new(&self.job, "schema-apply-intent.json", &intent)?;
            }
        }
        let mut session = Session::new(&self.service, self.payload.clone(), self.intent.ports)?;
        let cancellation = session.cancellation();
        publish_cancel(cancellation.clone());
        let result = (|| {
            session.start_database_only_cancellable()?;
            let system = session.maintenance_system_identifier()?;
            if !database.matches(&self.intent.id, &system) {
                return Err(changed());
            }
            self.check()?;
            let request = serde_json::to_vec(&serde_json::json!({
                "operation":if apply {"apply"} else {"reconcile"},
                "candidateId":self.intent.id,"backupId":self.intent.backup_id,
                "manifestPath":restored.join("manifest.json"),"manifestSha256":self.intent.manifest_sha256,
                "payloadSha256":self.intent.payload_sha256,"systemIdentifier":system,
            })).map_err(|_| changed())?;
            let frame: SchemaFrame = serde_json::from_value(session.apply_legacy_schema(&request)?)
                .map_err(|_| changed())?;
            if frame.format != 1
                || !matches!(
                    frame.operation.as_str(),
                    "legacy-schema-applied" | "legacy-schema-unapplied"
                )
                || frame.candidate_id != self.intent.id
                || frame.backup_id != self.intent.backup_id
                || frame.manifest_sha256 != self.intent.manifest_sha256
                || frame.payload_sha256 != self.intent.payload_sha256
                || frame.system_identifier != system
                || !frame.original_data_preserved
                || !hash_valid(&frame.source_inventory_sha256)
                || !hash_valid(&frame.source_schema_sha256)
                || !hash_valid(&frame.target_schema_sha256)
                || frame.source_schema_sha256 == frame.target_schema_sha256
                || frame.target_migrations.len() != 16
                || frame.target_migrations.last().is_none_or(|m| {
                    m.name != "016_local_workspaces.sql"
                        || m.sha256
                            != "cef351fc481576cac99f8b9653e6343b505b38f079d0f0789bf00367d2d69fab"
                })
                || frame
                    .target_migrations
                    .iter()
                    .any(|m| !hash_valid(&m.sha256))
            {
                return Err(changed());
            }
            session.maintenance_owner(
                "legacy_account",
                &BTreeMap::from([
                    (
                        "TENANT_ID".into(),
                        self.source.configuration.credentials.tenant_id.clone(),
                    ),
                    (
                        "ADMIN_SUBJECT".into(),
                        self.source.configuration.credentials.subject.clone(),
                    ),
                ]),
            )?;
            Ok(frame)
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
        let frame = result?;
        if cancellation.load(Ordering::SeqCst) {
            return Err(err("NATIVE_CANCELLED"));
        }
        for pin in &pins {
            pin.check()?;
        }
        staged_pin.check()?;
        database_pin.check()?;
        let _guard = self.service.lock()?;
        self.check()?;
        self.payload.recheck()?;
        // This journal is diagnostic only; recovery always re-derives authority.
        crate::write_json(&self.job, "schema-application.json", &frame)?;
        sync(&self.job)?;
        pins.extend([staged_pin, database_pin]);
        Ok((frame, pins))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires reviewed schema code, real retained synthetic backup authentication, the same stopped staged candidate, updated fixed capsule and an exclusive heavy slot"]
    fn actual_legacy_schema_apply_and_fresh_reconcile_same_candidate() {
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
        let authenticate_at = |workspace: &Path| {
            super::super::super::authenticate(
                &payload,
                &input("EXHIBITOS_LEGACY_TEST_ARCHIVE"),
                &input("EXHIBITOS_LEGACY_TEST_KEY"),
                workspace,
                Some(&input("EXHIBITOS_LEGACY_TEST_ORIGINAL")),
                &AtomicBool::new(false),
            )
            .unwrap()
        };
        let candidate = authenticate_at(&workspace)
            .resume_candidate(payload.clone(), target.clone())
            .unwrap();
        let original_secret = candidate.intent.environment_sha256.clone();
        let (staged, _) = read::<Staged>(&candidate.job.join("staged.json")).unwrap();
        // Recovery on proven original state must not replay an apply intent.
        assert!(candidate.recover_schema(|_| {}).unwrap().is_none());
        let cancelled = candidate
            .inspect_schema(|_| {})
            .unwrap()
            .apply(|flag| flag.store(true, Ordering::SeqCst))
            .err()
            .expect("cancel before PG/application");
        assert_eq!(cancelled.code, "NATIVE_CANCELLED");
        let applied = candidate
            .inspect_schema(|_| {})
            .unwrap()
            .apply(|_| {})
            .unwrap();
        let first = applied.summary().unwrap();
        assert_eq!(first["operation"], "legacy-schema-applied");
        assert_eq!(first["systemIdentifier"], staged.system_identifier);
        assert_eq!(first["originalDataPreserved"], true);
        drop(applied);
        // Preserve the observed result, then model an unavailable/untrusted
        // completion journal. Fresh recovery must derive actual DB state only.
        fs::copy(
            candidate.job.join("schema-application.json"),
            candidate
                .job
                .join("schema-application-before-reconcile.json"),
        )
        .unwrap();
        crate::write_json(&candidate.job, "schema-application.json", &serde_json::json!({"format":1,"operation":"untrusted-result","targetSchemaSha256":"0".repeat(64)})).unwrap();
        drop(candidate);
        let again = workspace.join("fresh-authentication");
        new_directory(&again).unwrap();
        let candidate = authenticate_at(&again)
            .resume_candidate(payload, target.clone())
            .unwrap();
        let recovered = candidate
            .recover_schema(|_| {})
            .unwrap()
            .expect("fresh actual016 observation");
        let second = recovered.summary().unwrap();
        assert_eq!(first, second);
        assert_eq!(candidate.intent.environment_sha256, original_secret);
        let (after_stage, _) = read::<Staged>(&candidate.job.join("staged.json")).unwrap();
        assert_eq!(after_stage.attempt, staged.attempt);
        assert_eq!(after_stage.system_identifier, staged.system_identifier);
        assert!(target.join(MARKER).exists());
        assert!(!target.join("config/freeze-signing-key.json").exists());
        crate::write_json(&workspace, "schema-apply-result.json", &second).unwrap();
    }
}
