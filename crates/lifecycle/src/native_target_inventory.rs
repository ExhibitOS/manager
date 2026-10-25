// SPDX-License-Identifier: Apache-2.0
//! Candidate schema/data observation issued only inside authenticated source
//! backup and native session/profile fences. No restoration or update occurs.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeTargetEvidence {
    pub(crate) operation: String,
    pub(crate) backup_id: String,
    pub(crate) authenticated_manifest_sha256: String,
    pub(crate) source_inventory_sha256: String,
    pub(crate) source_schema_sha256: String,
    pub(crate) target_schema_sha256: String,
    pub(crate) target_migrations_sha256: String,
    pub(crate) target_system_identifier: String,
    original_data_preserved: bool,
    configuration_verified: bool,
    preflight_verified: bool,
    update_executed: bool,
}
fn checked_evidence(
    value: Value,
    source: &NativeSourceEvidence,
    target_schema: &str,
) -> Result<NativeTargetEvidence> {
    let value: NativeTargetEvidence =
        serde_json::from_value(value).map_err(|_| err("NATIVE_TARGET_RESULT_INVALID"))?;
    let system = &value.target_system_identifier;
    if value.operation != "native-target-inventory-observed"
        || value.backup_id != source.backup_id
        || value.authenticated_manifest_sha256 != source.manifest_sha256
        || value.source_inventory_sha256 != source.inventory_sha256
        || value.source_schema_sha256 != source.schema_sha256
        || value.target_schema_sha256 != target_schema
        || !hash_valid(&value.target_migrations_sha256)
        || system == &source.system_identifier
        || system.starts_with('0')
        || system.parse::<u64>().is_err()
        || system.is_empty()
        || system.len() > 20
        || !system.bytes().all(|v| v.is_ascii_digit())
        || !value.original_data_preserved
        || value.configuration_verified
        || value.preflight_verified
        || value.update_executed
    {
        return Err(err("NATIVE_TARGET_RESULT_INVALID"));
    }
    Ok(value)
}
impl AuthenticatedNativeBackup {
    /// The registry checker runs on BOTH sides of the fixed target observation,
    /// while the same profile operation guard and both native sessions remain held.
    pub(crate) fn observe_target(
        &self,
        source: &mut Session,
        target: &mut Session,
        target_schema: &str,
        mut check_selection: impl FnMut(&Path, &str, &Path, &Path) -> Result<()>,
    ) -> Result<NativeTargetEvidence> {
        let profile = LifecycleService::bound_existing(self.profile_root.clone())?;
        let _profile = profile_lock(&profile)?;
        let target_root = target.target_observation_root().to_owned();
        if [
            &self.source_root,
            &self.archive,
            &self.verified,
            &self.scratch,
        ]
        .iter()
        .any(|path| target_root.starts_with(path.as_path()) || path.starts_with(&target_root))
        {
            return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        source.check_native_observation_fence()?;
        target.check_native_observation_fence()?;
        let target_pin = crate::maintenance_source::DirectoryPin::open(&target_root)?;
        check_selection(
            &self.profile_root,
            &self.installation_id,
            &self.source_root,
            &target_root,
        )?;
        let before = self.observe_locked(source, &mut |profile, id, root| {
            check_selection(profile, id, root, &target_root)
        })?;
        let observed = target.observe_target_inventory(
            &self.manifest,
            &self.evidence.manifest_sha256,
            target_schema,
        )?;
        let after = self.observe_locked(source, &mut |profile, id, root| {
            check_selection(profile, id, root, &target_root)
        })?;
        source.check_native_observation_fence()?;
        target.check_native_observation_fence()?;
        target_pin.check()?;
        if before != after {
            return Err(err("NATIVE_SOURCE_OBSERVATION_STALE"));
        }
        checked_evidence(observed, &before, target_schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> NativeSourceEvidence {
        NativeSourceEvidence {
            backup_id: uuid::Uuid::new_v4().to_string(),
            manifest_sha256: "a".repeat(64),
            inventory_sha256: "b".repeat(64),
            schema_sha256: "c".repeat(64),
            system_identifier: "12345".into(),
        }
    }
    fn output(source: &NativeSourceEvidence) -> Value {
        serde_json::json!({"operation":"native-target-inventory-observed","backupId":source.backup_id,"authenticatedManifestSha256":source.manifest_sha256,"sourceInventorySha256":source.inventory_sha256,"sourceSchemaSha256":source.schema_sha256,"targetSchemaSha256":"d".repeat(64),"targetMigrationsSha256":"e".repeat(64),"targetSystemIdentifier":"12346","originalDataPreserved":true,"configurationVerified":false,"preflightVerified":false,"updateExecuted":false})
    }
    #[test]
    fn target_result_binds_authenticated_source_and_independent_candidate_without_execution_claims()
    {
        let source = source();
        let value = output(&source);
        let observed = checked_evidence(value.clone(), &source, &"d".repeat(64)).unwrap();
        assert_eq!(observed.backup_id, source.backup_id);
        for key in [
            "backupId",
            "authenticatedManifestSha256",
            "sourceInventorySha256",
            "sourceSchemaSha256",
            "targetSchemaSha256",
        ] {
            let mut wrong = value.clone();
            wrong[key] = Value::String("f".repeat(64));
            assert!(
                checked_evidence(wrong, &source, &"d".repeat(64)).is_err(),
                "{key}"
            );
        }
        for key in [
            "configurationVerified",
            "preflightVerified",
            "updateExecuted",
        ] {
            let mut forged = value.clone();
            forged[key] = Value::Bool(true);
            assert!(checked_evidence(forged, &source, &"d".repeat(64)).is_err());
        }
        let mut lost = value.clone();
        lost["originalDataPreserved"] = Value::Bool(false);
        assert!(checked_evidence(lost, &source, &"d".repeat(64)).is_err());
        let mut future = value;
        future["executionPermit"] = Value::Bool(true);
        assert!(checked_evidence(future, &source, &"d".repeat(64)).is_err());
    }
    #[test]
    fn target_result_refuses_original_cluster_aliases_invalid_systems_and_migration_pins() {
        let source = source();
        let value = output(&source);
        for system in [
            "12345",
            "012346",
            "0",
            "",
            "18446744073709551616",
            "12346/foreign",
        ] {
            let mut wrong = value.clone();
            wrong["targetSystemIdentifier"] = Value::String(system.into());
            assert!(checked_evidence(wrong, &source, &"d".repeat(64)).is_err());
        }
        let mut wrong = value;
        wrong["targetMigrationsSha256"] = Value::String("not-observed".into());
        assert!(checked_evidence(wrong, &source, &"d".repeat(64)).is_err());
    }
}
