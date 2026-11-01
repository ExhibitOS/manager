// SPDX-License-Identifier: Apache-2.0
//! Mac-user-owned personal workspace; no user-created CMS password. Legacy
//! installations are never adopted. Database credentials stay private as before.
use super::*;
use crate::native_installation::{Context, Ports, Prepared};
use crate::native_payload::Payload;
use std::collections::BTreeMap;
const RECORD: &str = "native-personal.json";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Personal {
    format: u32,
    workspace_id: String,
    tenant_id: String,
    principal_id: String,
    owner_uid: u32,
    ports: Ports,
    manifest_sha256: String,
    environment_sha256: String,
}

#[cfg(test)]
mod selected_guard_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn guard_is_bound_to_exact_private_root() {
        let parent = std::env::temp_dir().join(format!("exhibitos-selected-guard-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let first = parent.join("first");
        let second = parent.join("second");
        for path in [&first, &second] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let a = LifecycleService::bound_existing(first.clone()).unwrap();
        let b = LifecycleService::bound_existing(second.clone()).unwrap();
        let guard_a = a.lock().unwrap();
        let guard_b = b.lock().unwrap();
        assert!(a.check_prepare_guard(&guard_a).is_ok());
        assert!(b.check_prepare_guard(&guard_b).is_ok());
        assert!(a.check_prepare_guard(&guard_b).is_err());
        assert!(b.check_prepare_guard(&guard_a).is_err());
        assert!(fs::read_dir(first).unwrap().all(|entry| entry.unwrap().file_name() == "operation.lock"));
        assert!(fs::read_dir(second).unwrap().all(|entry| entry.unwrap().file_name() == "operation.lock"));
    }
}
struct Bytes(Vec<u8>);
impl Drop for Bytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
fn present(root: &Path, name: &str) -> Result<bool> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(err("NATIVE_CONFIGURATION_UNAVAILABLE")),
    }
}
fn read(service: &LifecycleService) -> Result<Personal> {
    let record: Personal = serde_json::from_slice(&installation_backup::source_bytes(
        &service.root,
        RECORD,
        4096,
        true,
    )?)
    .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?;
    if record.format != 1
        || record.owner_uid == 0
        || record.owner_uid != unsafe { libc::geteuid() }
        || !installations::uuid(&record.workspace_id)
        || !installations::uuid(&record.tenant_id)
        || !installations::uuid(&record.principal_id)
        || !hash_valid(&record.manifest_sha256)
        || !hash_valid(&record.environment_sha256)
    {
        return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH"));
    }
    if [record.ports.web, record.ports.api, record.ports.database]
        .iter()
        .any(|p| *p < 1024)
        || record.ports.web == record.ports.api
        || record.ports.web == record.ports.database
        || record.ports.api == record.ports.database
    {
        return Err(err("NATIVE_PORTS_INVALID"));
    }
    Ok(record)
}
impl LifecycleService {
    pub(crate) fn personal_identity(&self) -> Result<(String, String, String)> {
        let r = read(self)?;
        Ok((r.workspace_id, r.tenant_id, r.principal_id))
    }

    pub(crate) fn has_native_personal(&self) -> Result<bool> {
        present(&self.root, RECORD)
    }
    pub(crate) fn personal_prepared(&self) -> Result<Prepared> {
        let record = read(self)?;
        Ok(Prepared {
            kind: "personal".into(),
            tenant_id: record.tenant_id,
            subject: String::new(),
            ports: record.ports,
            manifest_sha256: record.manifest_sha256,
        })
    }
    pub(crate) fn personal_environment(
        &self,
        payload: &Payload,
        ports: Ports,
    ) -> Result<BTreeMap<String, String>> {
        let record = read(self)?;
        if present(&self.root, "native-installation.json")?
            || present(&self.root, "installed.json")?
            || present(&self.root, "bundle")?
        {
            return Err(err("NATIVE_MIGRATION_REQUIRED"));
        }
        if record.manifest_sha256 != payload.context().manifest_sha256 || record.ports != ports {
            return Err(err("NATIVE_CONFIGURATION_CHANGED"));
        }
        let bytes = Bytes(installation_backup::source_bytes(
            &self.root,
            "runtime.env",
            8192,
            true,
        )?);
        if digest(&bytes.0) != record.environment_sha256 {
            return Err(err("NATIVE_CONFIGURATION_CHANGED"));
        }
        let mut fields = BTreeMap::new();
        for line in std::str::from_utf8(&bytes.0)
            .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?
            .lines()
        {
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?;
            if fields.insert(k.to_string(), v.to_string()).is_some() {
                return Err(err("NATIVE_CONFIGURATION_INVALID"));
            }
        }
        let password = fields
            .get("POSTGRES_PASSWORD")
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?;
        if password.len() != 96
            || !password
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(err("NATIVE_CONFIGURATION_INVALID"));
        }
        let required = [
            ("EXHIBITOS_RUNTIME_MODE", "native".into()),
            ("EXHIBITOS_AUTHORING_MODE", "personal".into()),
            ("NATIVE_RUNTIME_MANIFEST_SHA256", record.manifest_sha256),
            ("EXHIBITOS_PORT", ports.web.to_string()),
            ("EXHIBITOS_API_PORT", ports.api.to_string()),
            ("TENANT_ID", record.tenant_id),
            (
                "DATABASE_URL",
                format!(
                    "postgresql://exhibitos:{password}@127.0.0.1:{}/exhibitos",
                    ports.database
                ),
            ),
        ];
        if fields.len() != 8 || required.iter().any(|(k, v)| fields.get(*k) != Some(v)) {
            return Err(err("NATIVE_CONFIGURATION_INVALID"));
        }
        fields.insert("WORKSPACE_ID".into(), record.workspace_id);
        fields.insert("PRINCIPAL_ID".into(), record.principal_id);
        fields.insert("OWNER_UID".into(), record.owner_uid.to_string());
        Ok(fields)
    }
    pub fn prepare_native_personal(&self, payload: &Payload, ports: Ports) -> Result<Context> {
        let _lock = self.lock()?;
        self.prepare_native_personal_impl(payload, ports)
    }
    /// Caller retains the existing root guard throughout preparation.
    pub(super) fn prepare_native_personal_locked(&self, payload: &Payload, ports: Ports, guard: &OperationGuard) -> Result<Context> {
        self.check_prepare_guard(guard)?;
        self.prepare_native_personal_impl(payload, ports)
    }
    fn check_prepare_guard(&self, guard: &OperationGuard) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        installations::private_directory(&self.root)?;
        let held = guard.metadata().map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        let named = fs::symlink_metadata(self.root.join("operation.lock"))
            .map_err(|_| err("NATIVE_TARGET_SELECTION_INVALID"))?;
        if !held.is_file() || named.is_symlink() || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o777 != 0o600 || held.nlink() != 1
            || (held.dev(), held.ino()) != (named.dev(), named.ino()) {
            return Err(err("NATIVE_TARGET_SELECTION_INVALID"));
        }
        Ok(())
    }
    fn prepare_native_personal_impl(&self, payload: &Payload, ports: Ports) -> Result<Context> {
        signed_release::trust::lifecycle_writer::admit(&self.root, &Action::Install)?;
        if [ports.web, ports.api, ports.database]
            .iter()
            .any(|p| *p < 1024)
            || ports.web == ports.api
            || ports.web == ports.database
            || ports.api == ports.database
        {
            return Err(err("NATIVE_PORTS_INVALID"));
        }
        let uid = unsafe { libc::geteuid() };
        if uid == 0 {
            return Err(err("NATIVE_PERSONAL_OWNER_MISMATCH"));
        }
        if !self.has_native_personal()? {
            for name in [
                "native-installation.json",
                "installed.json",
                "bundle",
                "runtime.env",
                "native-database",
                "blobs",
                "config",
            ] {
                if present(&self.root, name)? {
                    return Err(err("NATIVE_MIGRATION_REQUIRED"));
                }
            }
            let pin = payload.context().manifest_sha256;
            let tenant = Uuid::new_v4().to_string();
            let password = format!(
                "{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            );
            let mut env=format!("EXHIBITOS_RUNTIME_MODE=native\nEXHIBITOS_AUTHORING_MODE=personal\nNATIVE_RUNTIME_MANIFEST_SHA256={pin}\nEXHIBITOS_PORT={}\nEXHIBITOS_API_PORT={}\nPOSTGRES_PASSWORD={password}\nDATABASE_URL=postgresql://exhibitos:{password}@127.0.0.1:{}/exhibitos\nTENANT_ID={tenant}\n",ports.web,ports.api,ports.database).into_bytes();
            let record = Personal {
                format: 1,
                workspace_id: Uuid::new_v4().to_string(),
                tenant_id: tenant,
                principal_id: Uuid::new_v4().to_string(),
                owner_uid: uid,
                ports,
                manifest_sha256: pin,
                environment_sha256: digest(&env),
            };
            let result = write_private_new(&self.root, "runtime.env", &env, 8192);
            env.fill(0);
            result?;
            write_private_new(
                &self.root,
                RECORD,
                &serde_json::to_vec(&record).map_err(|_| err("STATE_INVALID"))?,
                4096,
            )?;
            File::open(&self.root)
                .and_then(|f| f.sync_all())
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        let mut fields = self.personal_environment(payload, ports)?;
        for value in fields.values_mut() {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
        let record = read(self)?;
        Ok(Context {
            kind: "personal".into(),
            state: "configured".into(),
            ports,
            tenant_id: record.tenant_id,
            subject: String::new(),
            manifest_sha256: record.manifest_sha256,
        })
    }
}
