// SPDX-License-Identifier: Apache-2.0
//! Direct owned Docker state observation, never an external-writer isolation proof.
use crate::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStoppedReceipt {
    pub source_instance: String,
    pub bundle_id: String,
    pub platform_container: String,
    pub database_container: String,
    pub runtime_image_sha256: String,
    pub observed_at: u64,
}
fn stopped(value: &Value) -> bool {
    let s = &value["State"];
    s["Running"] == false
        && s["Paused"] == false
        && s["Restarting"] == false
        && s["Dead"] == false
        && s["Pid"].as_u64() == Some(0)
        && matches!(s["Status"].as_str(), Some("exited" | "created"))
}
pub(super) fn observe(
    source: &LifecycleService,
    plan: &crate::update::Plan,
) -> Result<SourceStoppedReceipt> {
    if cfg!(windows) {
        return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
    }
    if !source.root.join("installed.json").exists() {
        return Err(err("UPDATE_SOURCE_NOT_INSTALLED"));
    }
    let m = source.manifest()?;
    let installed: BundleManifest = read_json(&checked_path(&source.root, "installed.json")?)?;
    if serde_json::to_value(&installed).map_err(|_| err("STATE_INVALID"))?
        != serde_json::to_value(&m).map_err(|_| err("STATE_INVALID"))?
    {
        return Err(err("UPDATE_SOURCE_CHANGED"));
    }
    if m.services.len() != 2
        || !m.services.iter().any(|s| s == "platform")
        || !m.services.iter().any(|s| s == "database")
    {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    let engine = source.engine(&m, false)?;
    if engine != "docker" {
        return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
    }
    source.ensure_backup_helpers_idle(&engine)?;
    source.validate_compose(&m, &engine)?;
    source.validate_ownership(&m, &engine)?;
    source.validate_volumes(&m, &engine)?;
    let (platform_container, app) =
        crate::backup_creation::one_container(source, &m, &engine, "platform")?;
    let (database_container, db) =
        crate::backup_creation::one_container(source, &m, &engine, "database")?;
    if !stopped(&app) || !stopped(&db) {
        return Err(err("UPDATE_SOURCE_NOT_STOPPED"));
    }
    let expected = format!("sha256:{}", plan.source_image);
    if !hash_valid(&plan.source_image) || app["Image"] != expected {
        return Err(err("UPDATE_SOURCE_IMAGE_MISMATCH"));
    }
    Ok(SourceStoppedReceipt {
        source_instance: plan.source_instance.clone(),
        bundle_id: m.bundle_id,
        platform_container,
        database_container,
        runtime_image_sha256: plan.source_image.clone(),
        observed_at: now(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"State":{"Running":false,"Paused":false,"Restarting":false,"Dead":false,"Pid":0,"Status":"exited"}})
    }
    #[test]
    fn only_complete_observed_stopped_state_is_accepted() {
        assert!(stopped(&fixture()));
        let mut created = fixture();
        created["State"]["Status"] = "created".into();
        assert!(stopped(&created));
        for field in ["Running", "Paused", "Restarting", "Dead"] {
            let mut v = fixture();
            v["State"][field] = true.into();
            assert!(!stopped(&v));
            v["State"].as_object_mut().unwrap().remove(field);
            assert!(!stopped(&v));
        }
        for status in ["running", "restarting", "paused", "dead", "removing", ""] {
            let mut v = fixture();
            v["State"]["Status"] = status.into();
            assert!(!stopped(&v));
        }
        let mut v = fixture();
        v["State"]["Pid"] = 1.into();
        assert!(!stopped(&v));
        v["State"].as_object_mut().unwrap().remove("Pid");
        assert!(!stopped(&v));
        assert!(!stopped(&serde_json::json!({})));
    }
}
