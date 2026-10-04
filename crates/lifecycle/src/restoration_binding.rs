// SPDX-License-Identifier: Apache-2.0
//! Source-backup binding for real fresh candidate restoration, not update activation.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestorationProof {
    pub inventory_sha256: String,
    pub schema_sha256: String,
    pub runtime_image_sha256: String,
}
impl RestorationProof {
    pub(crate) fn valid(&self) -> bool {
        hash_valid(&self.inventory_sha256)
            && hash_valid(&self.schema_sha256)
            && hash_valid(&self.runtime_image_sha256)
    }
}
pub(crate) struct RestorationBinding {
    backup_id: String,
    manifest_sha256: String,
    required_free_bytes: u64,
    proof: RestorationProof,
}
impl RestorationBinding {
    pub(crate) fn from_plan(plan: &crate::update::Plan) -> Result<Self> {
        let b = Self {
            backup_id: plan.backup_id.clone(),
            manifest_sha256: plan.backup_manifest.clone(),
            required_free_bytes: plan.required_free_bytes,
            proof: RestorationProof {
                inventory_sha256: plan.source_inventory.clone(),
                schema_sha256: plan.source_schema.clone(),
                runtime_image_sha256: plan.source_image.clone(),
            },
        };
        b.validate()?;
        Ok(b)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if !super::super::installations::uuid(&self.backup_id)
            || !hash_valid(&self.manifest_sha256)
            || self.required_free_bytes == 0
            || !self.proof.valid()
        {
            return Err(err("UPDATE_RESTORE_BINDING_INVALID"));
        }
        Ok(())
    }
    pub(crate) fn required_free_bytes(&self) -> u64 {
        self.required_free_bytes
    }
    pub(crate) fn proof(&self) -> &RestorationProof {
        &self.proof
    }
    pub(crate) fn authenticated(
        &self,
        backup_id: &str,
        manifest_hash: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.validate()?;
        if backup_id != self.backup_id
            || manifest_hash != self.manifest_sha256
            || digest(bytes) != self.manifest_sha256
        {
            return Err(err("UPDATE_RESTORE_BINDING_MISMATCH"));
        }
        let manifest: Value =
            serde_json::from_slice(bytes).map_err(|_| err("RESTORE_RESULT_INVALID"))?;
        let inventory = &manifest["inventory"];
        if manifest["id"] != self.backup_id
            || !inventory.is_object()
            || schema_fingerprint(inventory)? != self.proof.schema_sha256
            || inventory_fingerprint(inventory)? != self.proof.inventory_sha256
        {
            return Err(err("UPDATE_RESTORE_BINDING_MISMATCH"));
        }
        Ok(())
    }
    pub(crate) fn runtime_image(&self, image: &str) -> Result<()> {
        if image.strip_prefix("sha256:") != Some(self.proof.runtime_image_sha256.as_str()) {
            return Err(err("UPDATE_RESTORE_BINDING_MISMATCH"));
        }
        Ok(())
    }
}
/// Physical SQL schema plus every ordered migration name/checksum, including
/// data-only migrations whose physical schema digest does not change.
fn schema_fingerprint(inventory: &Value) -> Result<String> {
    let physical = inventory["schemaDigest"]
        .as_str()
        .filter(|s| hash_valid(s))
        .ok_or_else(|| err("UPDATE_RESTORE_INVENTORY_INVALID"))?;
    let version = inventory["schemaVersion"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        .ok_or_else(|| err("UPDATE_RESTORE_INVENTORY_INVALID"))?;
    let migrations = inventory["migrations"]
        .as_array()
        .ok_or_else(|| err("UPDATE_RESTORE_INVENTORY_INVALID"))?;
    for migration in migrations {
        let object = migration
            .as_object()
            .filter(|o| o.len() == 2)
            .ok_or_else(|| err("UPDATE_RESTORE_INVENTORY_INVALID"))?;
        if !object
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
            || !object
                .get("sha256")
                .and_then(Value::as_str)
                .is_some_and(hash_valid)
        {
            return Err(err("UPDATE_RESTORE_INVENTORY_INVALID"));
        }
    }
    inventory_fingerprint(
        &serde_json::json!({"schemaDigest":physical,"schemaVersion":version,"migrations":migrations}),
    )
}
/// SHA-256 over compact JSON with recursively UTF-8-byte-sorted object keys,
/// preserved array order and safe integer numbers. Timestamp is retained: this
/// identifies the exact archived inventory, not a newly captured source snapshot.
fn inventory_fingerprint(value: &Value) -> Result<String> {
    fn safe(v: &Value) -> bool {
        match v {
            Value::Number(n) => {
                n.as_i64()
                    .is_some_and(|x| x.unsigned_abs() <= 9_007_199_254_740_991)
                    || n.as_u64().is_some_and(|x| x <= 9_007_199_254_740_991)
            }
            Value::Array(a) => a.iter().all(safe),
            Value::Object(o) => o.values().all(safe),
            _ => true,
        }
    }
    if !safe(value) {
        return Err(err("UPDATE_RESTORE_INVENTORY_INVALID"));
    }
    fn encode(value: &Value, bytes: &mut Vec<u8>) -> Result<()> {
        match value {
            Value::Object(object) => {
                bytes.push(b'{');
                let mut keys: Vec<_> = object.keys().collect();
                keys.sort_unstable();
                for (index, key) in keys.iter().enumerate() {
                    if index != 0 {
                        bytes.push(b',');
                    }
                    serde_json::to_writer(&mut *bytes, key)
                        .map_err(|_| err("UPDATE_RESTORE_INVENTORY_INVALID"))?;
                    bytes.push(b':');
                    encode(&object[*key], bytes)?;
                }
                bytes.push(b'}');
            }
            Value::Array(array) => {
                bytes.push(b'[');
                for (index, item) in array.iter().enumerate() {
                    if index != 0 {
                        bytes.push(b',');
                    }
                    encode(item, bytes)?;
                }
                bytes.push(b']');
            }
            _ => serde_json::to_writer(&mut *bytes, value)
                .map_err(|_| err("UPDATE_RESTORE_INVENTORY_INVALID"))?,
        }
        Ok(())
    }
    // Explicit sorting also works with serde_json/preserve_order in a native graph.
    let mut bytes = Vec::new();
    encode(value, &mut bytes)?;
    Ok(digest(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (RestorationBinding, Vec<u8>) {
        let id = "45ec39e9-5c19-47e6-9aaf-176978521a73";
        let inventory = serde_json::json!({"tables":[{"rows":1,"digest":"c".repeat(64)}],"schemaDigest":"a".repeat(64),"schemaVersion":"1.0.0-draft.1","migrations":[{"name":"001.sql","sha256":"d".repeat(64)}],"createdAt":"synthetic"});
        let bytes =
            serde_json::to_vec(&serde_json::json!({"id":id,"inventory":inventory})).unwrap();
        (
            RestorationBinding {
                backup_id: id.into(),
                manifest_sha256: digest(&bytes),
                required_free_bytes: 1,
                proof: RestorationProof {
                    inventory_sha256: inventory_fingerprint(&inventory).unwrap(),
                    schema_sha256: schema_fingerprint(&inventory).unwrap(),
                    runtime_image_sha256: "b".repeat(64),
                },
            },
            bytes,
        )
    }
    #[test]
    fn exact_authenticated_backup_inventory_schema_and_runtime_are_required() {
        let (b, bytes) = fixture();
        b.authenticated(&b.backup_id, &b.manifest_sha256, &bytes)
            .unwrap();
        b.runtime_image(&format!("sha256:{}", "b".repeat(64)))
            .unwrap();
        for field in 0..5 {
            let (mut b, bytes) = fixture();
            match field {
                0 => b.backup_id = "60308ef4-d47e-43d4-958d-68b250a2a465".into(),
                1 => b.manifest_sha256 = "0".repeat(64),
                2 => b.proof.inventory_sha256 = "0".repeat(64),
                3 => b.proof.schema_sha256 = "0".repeat(64),
                _ => b.proof.runtime_image_sha256 = "0".repeat(64),
            }
            let (original, _) = fixture();
            if field == 4 {
                assert_eq!(
                    b.runtime_image(&format!("sha256:{}", "b".repeat(64)))
                        .unwrap_err()
                        .code,
                    "UPDATE_RESTORE_BINDING_MISMATCH"
                );
            } else {
                assert_eq!(
                    b.authenticated(&original.backup_id, &original.manifest_sha256, &bytes)
                        .unwrap_err()
                        .code,
                    "UPDATE_RESTORE_BINDING_MISMATCH"
                );
            }
        }
    }
    #[test]
    fn raw_manifest_change_and_unsafe_numbers_cannot_authorize_restore() {
        let (b, mut bytes) = fixture();
        bytes.push(b' ');
        assert_eq!(
            b.authenticated(&b.backup_id, &b.manifest_sha256, &bytes)
                .unwrap_err()
                .code,
            "UPDATE_RESTORE_BINDING_MISMATCH"
        );
        for text in ["1.25", "9007199254740992", "-9007199254740992"] {
            let v: Value = serde_json::from_str(text).unwrap();
            assert!(inventory_fingerprint(&v).is_err());
        }
        let a: Value = serde_json::from_str(r#"{"z":1,"nested":{"b":2,"a":1},"a":0}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":0,"nested":{"a":1,"b":2},"z":1}"#).unwrap();
        assert_eq!(
            inventory_fingerprint(&a).unwrap(),
            inventory_fingerprint(&b).unwrap()
        );
        assert_eq!(
            inventory_fingerprint(&a).unwrap(),
            digest(br#"{"a":0,"nested":{"a":1,"b":2},"z":1}"#)
        );
    }
    #[test]
    fn data_only_migration_checksum_is_part_of_schema_identity() {
        let (b, bytes) = fixture();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        let mut changed = original["inventory"].clone();
        changed["migrations"][0]["sha256"] = Value::String("e".repeat(64));
        assert_eq!(
            changed["schemaDigest"],
            original["inventory"]["schemaDigest"]
        );
        assert_ne!(schema_fingerprint(&changed).unwrap(), b.proof.schema_sha256);
        changed["migrations"][0]["extra"] = Value::Bool(true);
        assert!(schema_fingerprint(&changed).is_err());
    }
}
