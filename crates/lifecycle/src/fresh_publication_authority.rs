// SPDX-License-Identifier: Apache-2.0
//! Test-only fresh native qualification bindings, never public admission.
use crate::update::{Plan, Stage, Update};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FreshAuthority {
    pub generation: u64,
    pub head: String,
    pub scope: String,
    pub plan: Plan,
    pub envelope_sha256: String,
    pub artifact_sha256: String,
    pub catalog_sha256: String,
    pub original_selection_sha256: String,
    pub public_key: String,
    pub manager_source_commit: String,
    pub binding_sha256: String,
    pub host_archive_sha256: String,
    pub trust_archive_sha256: String,
    pub publication_phase: String,
}
// Deserialize known authority fields directly from the raw input before Value
// normalization: duplicate fields, including nested Plan fields, must refuse.
pub(super) fn parse_input(bytes: &[u8]) -> Result<FreshAuthority, serde_json::Error> {
    #[derive(Deserialize)]
    struct Input {
        #[serde(rename = "freshAuthority")]
        authority: FreshAuthority,
        #[serde(flatten)]
        _legacy: std::collections::HashMap<String, serde_json::Value>,
    }
    serde_json::from_slice::<Input>(bytes).map(|input| input.authority)
}

// These operations belong to consumed historical qualification baselines. They
// cannot become fresh just because a caller edits a generation in JSON.
const CONSUMED: [&str; 3] = [
    "c5b5becd-4f70-435f-909e-73031e5def37",
    "2a517fd1-e818-4f90-8e4f-73c9fe7b976b",
    "a9ccf7b0-7e3f-4bae-bcf1-3f2c53e15c49",
];
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) struct Observation<'a> {
    pub generation: u64,
    pub head: &'a str,
    pub scope: &'a str,
    pub plan: &'a Plan,
    pub stage: Stage,
    pub public_keys: &'a [String],
    pub manager_source_commit: &'a str,
    pub dirty_source: bool,
}
impl FreshAuthority {
    pub fn check(&self, actual: &Observation<'_>) -> Result<(), &'static str> {
        if self.generation <= 43 || CONSUMED.contains(&self.plan.operation_id.as_str()) {
            return Err("PUBLICATION_AUTHORITY_CONSUMED");
        }
        if Update::new(self.plan.clone()).is_err()
            || self.plan.source_schema == self.plan.target_schema
            || !matches!(
                self.publication_phase.as_str(),
                "pending-synced" | "selection-renamed" | "selection-dir-synced"
            )
            || [
                &self.head,
                &self.scope,
                &self.envelope_sha256,
                &self.artifact_sha256,
                &self.catalog_sha256,
                &self.original_selection_sha256,
                &self.public_key,
                &self.binding_sha256,
                &self.host_archive_sha256,
                &self.trust_archive_sha256,
            ]
            .iter()
            .any(|s| !hex(s, 64))
            || !hex(&self.manager_source_commit, 40)
        {
            return Err("PUBLICATION_AUTHORITY_INVALID");
        }
        if actual.stage != Stage::Prepared
            || actual.generation != self.generation
            || actual.head != self.head
            || actual.scope != self.scope
            || actual.plan != &self.plan
            || actual.public_keys != [self.public_key.clone()]
            || actual.manager_source_commit != self.manager_source_commit
            || actual.dirty_source
        {
            return Err("PUBLICATION_AUTHORITY_CHANGED");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> FreshAuthority {
        FreshAuthority {
            generation: 46,
            head: "a".repeat(64),
            scope: "b".repeat(64),
            plan: Plan {
                operation_id: "fresh-operation".into(),
                source_instance: "original".into(),
                target_instance: "fresh-target".into(),
                source_image: "c".repeat(64),
                target_image: "d".repeat(64),
                source_schema: "e".repeat(64),
                target_schema: "f".repeat(64),
                backup_id: "backup".into(),
                backup_manifest: "1".repeat(64),
                source_inventory: "2".repeat(64),
                required_free_bytes: 1,
            },
            envelope_sha256: "3".repeat(64),
            artifact_sha256: "4".repeat(64),
            catalog_sha256: "5".repeat(64),
            original_selection_sha256: "6".repeat(64),
            public_key: "7".repeat(64),
            manager_source_commit: "8".repeat(40),
            binding_sha256: "9".repeat(64),
            host_archive_sha256: "a".repeat(64),
            trust_archive_sha256: "b".repeat(64),
            publication_phase: "pending-synced".into(),
        }
    }
    fn check(
        expected: &FreshAuthority,
        actual: &FreshAuthority,
        stage: Stage,
        dirty: bool,
    ) -> Result<(), &'static str> {
        expected.check(&Observation {
            generation: actual.generation,
            head: &actual.head,
            scope: &actual.scope,
            plan: &actual.plan,
            stage,
            public_keys: &[actual.public_key.clone()],
            manager_source_commit: &actual.manager_source_commit,
            dirty_source: dirty,
        })
    }
    #[test]
    fn fresh_publication_authority_requires_exact_current_prepared_binding() {
        let expected = fixture();
        assert_eq!(check(&expected, &expected, Stage::Prepared, false), Ok(()));
        for stage in [
            Stage::Applying,
            Stage::RecoveryRequired,
            Stage::Updated,
            Stage::RolledBack,
        ] {
            assert_eq!(
                check(&expected, &expected, stage, false),
                Err("PUBLICATION_AUTHORITY_CHANGED")
            );
        }
        assert_eq!(
            check(&expected, &expected, Stage::Prepared, true),
            Err("PUBLICATION_AUTHORITY_CHANGED")
        );
    }
    #[test]
    fn fresh_publication_authority_never_reinterprets_consumed_operations() {
        for generation in [21, 29, 37, 43] {
            let mut old = fixture();
            old.generation = generation;
            assert_eq!(
                check(&old, &old, Stage::Prepared, false),
                Err("PUBLICATION_AUTHORITY_CONSUMED")
            );
        }
        for operation in CONSUMED {
            let mut old = fixture();
            old.plan.operation_id = operation.into();
            assert_eq!(
                check(&old, &old, Stage::Prepared, false),
                Err("PUBLICATION_AUTHORITY_CONSUMED")
            );
        }
    }
    #[test]
    fn fresh_publication_authority_rejects_mismatched_generation_head_scope_plan_key_source() {
        let expected = fixture();
        for field in 0..6 {
            let mut actual = fixture();
            match field {
                0 => actual.generation += 1,
                1 => actual.head = "0".repeat(64),
                2 => actual.scope = "0".repeat(64),
                3 => actual.plan.backup_id = "foreign".into(),
                4 => actual.public_key = "0".repeat(64),
                _ => actual.manager_source_commit = "0".repeat(40),
            };
            assert_eq!(
                check(&expected, &actual, Stage::Prepared, false),
                Err("PUBLICATION_AUTHORITY_CHANGED")
            );
        }
    }
    #[test]
    fn fresh_publication_authority_closed_shape_and_invalid_plan_refuse() {
        let expected = fixture();
        let mut authority = serde_json::to_value(&expected).unwrap();
        authority["allowConsumed"] = true.into();
        assert!(serde_json::from_value::<FreshAuthority>(authority).is_err());
        let mut authority = serde_json::to_value(&expected).unwrap();
        authority
            .as_object_mut()
            .unwrap()
            .remove("hostArchiveSha256");
        assert!(serde_json::from_value::<FreshAuthority>(authority).is_err());
        let mut value = serde_json::to_value(&expected.plan).unwrap();
        value["extra"] = true.into();
        assert!(serde_json::from_value::<Plan>(value).is_err());
        let mut bad = fixture();
        bad.plan.target_instance = bad.plan.source_instance.clone();
        assert_eq!(
            check(&bad, &bad, Stage::Prepared, false),
            Err("PUBLICATION_AUTHORITY_INVALID")
        );
        bad = fixture();
        bad.publication_phase = "completed".into();
        assert_eq!(
            check(&bad, &bad, Stage::Prepared, false),
            Err("PUBLICATION_AUTHORITY_INVALID")
        );
    }
    #[test]
    fn fresh_publication_authority_raw_duplicate_fields_refuse() {
        let raw = serde_json::to_string(&fixture()).unwrap();
        let input = format!("{{\"freshAuthority\":{raw},\"root\":\"synthetic\"}}");
        assert!(parse_input(input.as_bytes()).is_ok());
        let duplicate = input.replacen(
            "\"generation\":46",
            "\"generation\":46,\"generation\":47",
            1,
        );
        assert_ne!(duplicate, input);
        assert!(parse_input(duplicate.as_bytes()).is_err());
        let duplicate = input.replacen(
            "\"requiredFreeBytes\":1",
            "\"requiredFreeBytes\":1,\"requiredFreeBytes\":2",
            1,
        );
        assert_ne!(duplicate, input);
        assert!(parse_input(duplicate.as_bytes()).is_err());
        let duplicate = format!("{{\"freshAuthority\":{raw},\"freshAuthority\":{raw}}}");
        assert!(parse_input(duplicate.as_bytes()).is_err());
    }
}
