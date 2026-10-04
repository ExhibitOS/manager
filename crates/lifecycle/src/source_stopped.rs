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
    pub blob_volume: String,
    pub configuration_volume: String,
    pub database_volume: String,
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
// A direct Engine mount census narrows the operator-acknowledged writer gap.
// It cannot fence host processes or a writer starting after this observation.
fn source_volumes(app: &Value, db: &Value) -> Result<std::collections::BTreeSet<String>> {
    let mut names = std::collections::BTreeSet::new();
    for container in [app, db] {
        for mount in container["Mounts"]
            .as_array()
            .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?
        {
            let kind = mount["Type"]
                .as_str()
                .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
            if kind == "volume" {
                let name = mount["Name"]
                    .as_str()
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
                names.insert(name.to_owned());
            }
        }
    }
    if names.is_empty() {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    Ok(names)
}
fn check_volume_users(
    rows: &[Value],
    names: &std::collections::BTreeSet<String>,
    owned: &[&str],
) -> Result<()> {
    for row in rows {
        let id = row["Id"]
            .as_str()
            .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
        if owned.contains(&id) {
            if !stopped(row) {
                return Err(err("UPDATE_SOURCE_NOT_STOPPED"));
            }
            continue;
        }
        for mount in row["Mounts"]
            .as_array()
            .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?
        {
            let kind = mount["Type"]
                .as_str()
                .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
            if kind == "volume" {
                let name = mount["Name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
                let writable = mount["RW"]
                    .as_bool()
                    .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
                if names.contains(name) && writable && !stopped(row) {
                    return Err(err("UPDATE_SOURCE_VOLUME_WRITER"));
                }
            }
        }
    }
    Ok(())
}
fn observe_volume_users(engine: &str, app: &Value, db: &Value) -> Result<()> {
    let names = source_volumes(app, db)?;
    let mut ids = std::collections::BTreeSet::new();
    for name in &names {
        let output = run(
            engine,
            &[
                "ps".into(),
                "--all".into(),
                "--no-trunc".into(),
                "--filter".into(),
                format!("volume={name}"),
                "--format".into(),
                "{{.ID}}".into(),
            ],
            None,
            15,
        )?;
        for id in std::str::from_utf8(&output)
            .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
            .lines()
        {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
            ids.insert(id.to_owned());
        }
        if ids.len() > 1024 {
            return Err(err("ENGINE_OUTPUT_INVALID"));
        }
    }
    let app_id = app["Id"]
        .as_str()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    let db_id = db["Id"]
        .as_str()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    // No missing or silently truncated census is accepted.
    if !ids.contains(db_id) {
        return Err(err("UPDATE_SOURCE_CHANGED"));
    }
    let mut args = vec!["inspect".into()];
    args.extend(ids.iter().cloned());
    let data: Value = serde_json::from_slice(&run(engine, &args, None, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let rows = data
        .as_array()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    let returned = rows
        .iter()
        .map(|r| {
            r["Id"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))
        })
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    if rows.len() != ids.len() || returned != ids {
        return Err(err("UPDATE_SOURCE_CHANGED"));
    }
    check_volume_users(rows, &names, &[app_id, db_id])
}
fn check_mount_layout(app: &Value, db: &Value) -> Result<()> {
    for (container, expected) in [
        (app, &["/data/blobs", "/data/config"][..]),
        (db, &["/var/lib/postgresql"][..]),
    ] {
        let mounts = container["Mounts"]
            .as_array()
            .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
        if mounts.len() != expected.len() {
            return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
        }
        let mut names = std::collections::BTreeSet::new();
        for target in expected {
            let matches: Vec<_> = mounts
                .iter()
                .filter(|m| m["Destination"] == *target)
                .collect();
            if matches.len() != 1 {
                return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
            }
            let mount = matches[0];
            let name = mount["Name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
            if mount["Type"] != "volume" || mount["RW"] != true || !names.insert(name) {
                return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
            }
        }
    }
    Ok(())
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
    check_mount_layout(&app, &db)?;
    let config: Value = serde_json::from_slice(&run(
        &engine,
        &compose_args(&m, &["config", "--format", "json"]),
        Some(&source.root.join("bundle")),
        30,
    )?)
    .map_err(|_| err("BACKUP_LAYOUT_UNSUPPORTED"))?;
    let blobs =
        crate::backup_creation::volume_for(&engine, &m, &config, "platform", "/data/blobs", &app)?;
    let configuration =
        crate::backup_creation::volume_for(&engine, &m, &config, "platform", "/data/config", &app)?;
    let database = crate::backup_creation::volume_for(
        &engine,
        &m,
        &config,
        "database",
        "/var/lib/postgresql",
        &db,
    )?;
    if blobs == configuration || blobs == database || configuration == database {
        return Err(err("BACKUP_LAYOUT_UNSUPPORTED"));
    }
    observe_volume_users(&engine, &app, &db)?;
    Ok(SourceStoppedReceipt {
        source_instance: plan.source_instance.clone(),
        bundle_id: m.bundle_id,
        platform_container,
        database_container,
        runtime_image_sha256: plan.source_image.clone(),
        blob_volume: blobs,
        configuration_volume: configuration,
        database_volume: database,
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
    #[test]
    fn foreign_volume_writers_are_rejected_without_trusting_labels() {
        let names = std::collections::BTreeSet::from(["owned-db".to_owned()]);
        let mut foreign = fixture();
        foreign["Id"] = "foreign".into();
        foreign["Mounts"] = serde_json::json!([{"Type":"volume","Name":"owned-db","RW":true}]);
        assert!(check_volume_users(&[foreign.clone()], &names, &["app", "db"]).is_ok());
        for field in ["Running", "Paused", "Restarting", "Dead"] {
            let mut writer = foreign.clone();
            writer["State"][field] = true.into();
            assert_eq!(
                check_volume_users(&[writer], &names, &["app", "db"])
                    .unwrap_err()
                    .code,
                "UPDATE_SOURCE_VOLUME_WRITER"
            );
        }
        foreign["State"]["Running"] = true.into();
        foreign["Config"]["Labels"]["com.exhibitos.bundle"] = "pretend-owner".into();
        assert_eq!(
            check_volume_users(&[foreign.clone()], &names, &["app", "db"])
                .unwrap_err()
                .code,
            "UPDATE_SOURCE_VOLUME_WRITER"
        );
        foreign["Mounts"][0]["RW"] = false.into();
        assert!(check_volume_users(&[foreign.clone()], &names, &["app", "db"]).is_ok());
        foreign["Mounts"][0]["RW"] = true.into();
        foreign["Mounts"][0]["Name"] = "unrelated".into();
        assert!(check_volume_users(&[foreign], &names, &["app", "db"]).is_ok());
    }
    #[test]
    fn malformed_writer_state_or_mount_access_is_not_accepted() {
        let names = std::collections::BTreeSet::from(["owned-db".to_owned()]);
        let mut foreign = serde_json::json!({"Id":"foreign","Mounts":[{"Type":"volume","Name":"owned-db","RW":true}]});
        assert_eq!(
            check_volume_users(&[foreign.clone()], &names, &[])
                .unwrap_err()
                .code,
            "UPDATE_SOURCE_VOLUME_WRITER"
        );
        foreign["Mounts"][0].as_object_mut().unwrap().remove("RW");
        assert_eq!(
            check_volume_users(&[foreign], &names, &[])
                .unwrap_err()
                .code,
            "ENGINE_OUTPUT_INVALID"
        );
        assert!(source_volumes(&serde_json::json!({}), &fixture()).is_err());
    }
    #[test]
    fn owned_source_that_starts_during_census_is_refused() {
        let names = std::collections::BTreeSet::from(["owned-db".to_owned()]);
        let mut owned = fixture();
        owned["Id"] = "db".into();
        owned["State"]["Running"] = true.into();
        assert_eq!(
            check_volume_users(&[owned], &names, &["app", "db"])
                .unwrap_err()
                .code,
            "UPDATE_SOURCE_NOT_STOPPED"
        );
    }
    #[test]
    fn stopped_source_mount_layout_rejects_aliases_binds_and_missing_mounts() {
        let app = serde_json::json!({"Mounts":[{"Type":"volume","Name":"blobs","Destination":"/data/blobs","RW":true},{"Type":"volume","Name":"config","Destination":"/data/config","RW":true}]});
        let db = serde_json::json!({"Mounts":[{"Type":"volume","Name":"db","Destination":"/var/lib/postgresql","RW":true}]});
        assert!(check_mount_layout(&app, &db).is_ok());
        for field in ["Name", "Type", "Destination", "RW"] {
            let mut bad = app.clone();
            bad["Mounts"][0].as_object_mut().unwrap().remove(field);
            assert!(check_mount_layout(&bad, &db).is_err());
        }
        let mut bad = app.clone();
        bad["Mounts"][1]["Name"] = "blobs".into();
        assert!(check_mount_layout(&bad, &db).is_err());
        let mut bad = app.clone();
        bad["Mounts"][0]["Type"] = "bind".into();
        assert!(check_mount_layout(&bad, &db).is_err());
        let mut bad = app.clone();
        bad["Mounts"][0]["RW"] = false.into();
        assert!(check_mount_layout(&bad, &db).is_err());
        let mut bad = db.clone();
        bad["Mounts"]
            .as_array_mut()
            .unwrap()
            .push(db["Mounts"][0].clone());
        assert!(check_mount_layout(&app, &bad).is_err());
    }
    #[test]
    #[ignore = "requires local Docker and the qualified existing maintenance image"]
    fn actual_engine_source_volume_identity() {
        let project = format!("exhibitos-identities-{}", uuid::Uuid::new_v4());
        let m: BundleManifest = serde_json::from_value(serde_json::json!({
            "schemaVersion":"org.exhibitos.runtime-bundle/v1","bundleId":uuid::Uuid::new_v4().to_string(),
            "version":"synthetic","protocolVersion":"1","composeSha256":"0".repeat(64),
            "projectName":project,"services":["platform","database"],"images":[],"ports":[],
            "openUrl":"http://127.0.0.1:1","readinessUrl":"http://127.0.0.1:1","minimumFreeBytes":0
        })).unwrap();
        let image = "sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06";
        let call = |args: &[String]| run("docker", args, None, 30).unwrap();
        let names: Vec<_> = ["blobs", "config", "db"]
            .iter()
            .map(|suffix| format!("{project}_{suffix}"))
            .collect();
        for name in &names {
            call(&[
                "volume".into(),
                "create".into(),
                "--label".into(),
                format!("com.exhibitos.bundle={}", m.bundle_id),
                "--label".into(),
                format!("com.exhibitos.project={project}"),
                "--label".into(),
                format!("com.exhibitos.schema={}", m.schema_version),
                name.clone(),
            ]);
        }
        let config = serde_json::json!({"services":{"platform":{"volumes":[{"target":"/data/blobs","type":"volume","source":"blobs"},{"target":"/data/config","type":"volume","source":"config"}]},"database":{"volumes":[{"target":"/var/lib/postgresql","type":"volume","source":"db"}]}},"volumes":{"blobs":{"name":names[0]},"config":{"name":names[1]},"db":{"name":names[2]}}});
        let mut ids = Vec::new();
        for (index, mounts) in [
            vec![
                format!("type=volume,source={},target=/data/blobs", names[0]),
                format!("type=volume,source={},target=/data/config", names[1]),
            ],
            vec![format!(
                "type=volume,source={},target=/var/lib/postgresql",
                names[2]
            )],
        ]
        .into_iter()
        .enumerate()
        {
            let mut args = vec![
                "create".into(),
                "--network".into(),
                "none".into(),
                "--entrypoint".into(),
                "sleep".into(),
            ];
            for mount in mounts {
                args.extend(["--mount".into(), mount]);
            }
            let selected_image = if index == 0 {
                "sha256:335f8f2c1437841266c41e79912b1160b03ce500511acc94afa338c4c8f6215b"
            } else {
                image
            };
            args.extend([selected_image.into(), "300".into()]);
            ids.push(String::from_utf8(call(&args)).unwrap().trim().to_owned());
        }
        let inspected = |id: &str| -> Value {
            serde_json::from_slice::<Value>(&call(&["inspect".into(), id.into()])).unwrap()[0]
                .clone()
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let app = inspected(&ids[0]);
            let db = inspected(&ids[1]);
            check_mount_layout(&app, &db).unwrap();
            for (service, target, actual, name) in [
                ("platform", "/data/blobs", &app, &names[0]),
                ("platform", "/data/config", &app, &names[1]),
                ("database", "/var/lib/postgresql", &db, &names[2]),
            ] {
                assert_eq!(
                    crate::backup_creation::volume_for(
                        "docker", &m, &config, service, target, actual
                    )
                    .unwrap(),
                    *name
                );
            }
            let mut foreign = m.clone();
            foreign.bundle_id = uuid::Uuid::new_v4().to_string();
            assert_eq!(
                crate::backup_creation::volume_for(
                    "docker",
                    &foreign,
                    &config,
                    "platform",
                    "/data/blobs",
                    &app
                )
                .unwrap_err()
                .code,
                "OWNERSHIP_CONFLICT"
            );
            let mut wrong = config.clone();
            wrong["volumes"]["blobs"]["name"] = names[1].clone().into();
            assert_eq!(
                crate::backup_creation::volume_for(
                    "docker",
                    &m,
                    &wrong,
                    "platform",
                    "/data/blobs",
                    &app
                )
                .unwrap_err()
                .code,
                "OWNERSHIP_CONFLICT"
            );
        }));
        // These helpers were never started; these volumes contain no application data.
        for id in &ids {
            call(&["rm".into(), id.clone()]);
        }
        for name in &names {
            call(&["volume".into(), "rm".into(), name.clone()]);
        }
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
    #[test]
    #[ignore = "requires local Docker and the qualified existing maintenance image"]
    fn actual_engine_volume_writer_census() {
        let name = format!("exhibitos-writer-census-{}", uuid::Uuid::new_v4());
        let image = "sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06";
        let call = |args: &[&str]| {
            run(
                "docker",
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                None,
                30,
            )
            .unwrap()
        };
        call(&["volume", "create", &name]);
        let mut ids = Vec::new();
        for readonly in [false, false, true] {
            let mount = format!(
                "type=volume,source={name},target=/synthetic{}",
                if readonly { ",readonly" } else { "" }
            );
            let id = String::from_utf8(call(&[
                "create",
                "--network",
                "none",
                "--mount",
                &mount,
                "--entrypoint",
                "sleep",
                image,
                "300",
            ]))
            .unwrap()
            .trim()
            .to_owned();
            ids.push(id);
        }
        let inspect = |id: &str| -> Value {
            serde_json::from_slice::<Value>(&call(&["inspect", id])).unwrap()[0].clone()
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let owned = inspect(&ids[0]);
            assert!(observe_volume_users("docker", &owned, &owned).is_ok());
            call(&["start", &ids[1]]);
            assert_eq!(
                observe_volume_users("docker", &owned, &owned)
                    .unwrap_err()
                    .code,
                "UPDATE_SOURCE_VOLUME_WRITER"
            );
            call(&["stop", &ids[1]]);
            call(&["start", &ids[2]]);
            assert!(observe_volume_users("docker", &owned, &owned).is_ok());
            call(&["stop", &ids[2]]);
        }));
        // Only this test's never-data-bearing helpers and empty named volume.
        for id in &ids {
            let _ = run("docker", &["stop".into(), id.clone()], None, 30);
            call(&["rm", id]);
        }
        call(&["volume", "rm", &name]);
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}
