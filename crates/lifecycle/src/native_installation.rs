// SPDX-License-Identifier: Apache-2.0
//! Native first-account preparation using the existing private runtime.env
//! contract. Never adopts a Docker installation or changes an existing account.
use super::initial_account::{InitialAccountInput, encode_password};
use super::native_payload::Payload;
use super::*;
use std::collections::BTreeMap;

const RECORD: &str = "native-installation.json";
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ports {
    pub web: u16,
    pub api: u16,
    pub database: u16,
}
impl Default for Ports {
    fn default() -> Self {
        Self {
            web: 13200,
            api: 13201,
            database: 15432,
        }
    }
}
impl Ports {
    pub(crate) fn validate(&self) -> Result<()> {
        if [self.web, self.api, self.database]
            .iter()
            .any(|p| *p < 1024)
            || self.web == self.api
            || self.web == self.database
            || self.api == self.database
        {
            return Err(err("NATIVE_PORTS_INVALID"));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    format: u32,
    manifest_sha256: String,
    environment_sha256: String,
    ports: Ports,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub kind: String,
    pub tenant_id: String,
    pub subject: String,
    pub ports: Ports,
    pub manifest_sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub kind: String,
    pub state: String,
    pub ports: Ports,
    pub tenant_id: String,
    pub subject: String,
    pub manifest_sha256: String,
}
struct PrivateBytes(Vec<u8>);
impl Drop for PrivateBytes {
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
fn native_environment(bytes: &[u8], pin: &str, ports: Ports) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?;
        if fields.insert(key, value).is_some() {
            return Err(err("NATIVE_CONFIGURATION_INVALID"));
        }
    }
    let expected = [
        ("EXHIBITOS_RUNTIME_MODE", "native".into()),
        ("NATIVE_RUNTIME_MANIFEST_SHA256", pin.into()),
        ("EXHIBITOS_PORT", ports.web.to_string()),
        ("EXHIBITOS_API_PORT", ports.api.to_string()),
    ];
    if expected
        .iter()
        .any(|(key, value)| fields.get(key).copied() != Some(value.as_str()))
        || fields.len() != 9
    {
        return Err(err("NATIVE_CONFIGURATION_INVALID"));
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
    let database_url = format!(
        "postgresql://exhibitos:{password}@127.0.0.1:{}/exhibitos",
        ports.database
    );
    if fields.get("DATABASE_URL").copied() != Some(database_url.as_str()) {
        return Err(err("NATIVE_CONFIGURATION_INVALID"));
    }
    Ok(())
}
impl LifecycleService {
    /// Metadata only. This is configuration presence, never live readiness or
    /// signed payload qualification; Start revalidates the complete binding.
    pub fn native_prepared(&self) -> Result<Option<Prepared>> {
        if self.has_native_personal()? {
            return self.personal_prepared().map(Some);
        }
        if present(&self.root, "installed.json")? || present(&self.root, "bundle")? {
            return Err(err("NATIVE_MIGRATION_REQUIRED"));
        }
        if !present(&self.root, RECORD)? {
            return Ok(None);
        }
        let record: Record = serde_json::from_slice(&installation_backup::source_bytes(
            &self.root, RECORD, 4096, true,
        )?)
        .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?;
        record.ports.validate()?;
        if record.format != 1
            || !hash_valid(&record.manifest_sha256)
            || !hash_valid(&record.environment_sha256)
        {
            return Err(err("NATIVE_CONFIGURATION_INVALID"));
        }
        let account = self.initial_account_context()?;
        Ok(Some(Prepared {
            kind: "legacy_account".into(),
            tenant_id: account
                .tenant_id
                .ok_or_else(|| err("INITIAL_ACCOUNT_UNAVAILABLE"))?,
            subject: account
                .subject
                .ok_or_else(|| err("INITIAL_ACCOUNT_UNAVAILABLE"))?,
            ports: record.ports,
            manifest_sha256: record.manifest_sha256,
        }))
    }
    pub fn native_account_required(&self) -> Result<bool> {
        Ok(!self.has_native_personal()? && present(&self.root, "runtime.env")?)
    }
    /// Caller holds this installation's operation lock. Secrets stay in memory
    /// and go only to the pinned local service processes, never IPC responses.
    pub(crate) fn native_start_environment(
        &self,
        payload: &Payload,
        ports: Ports,
    ) -> Result<BTreeMap<String, String>> {
        ports.validate()?;
        if self.has_native_personal()? {
            return self.personal_environment(payload, ports);
        }
        if present(&self.root, "installed.json")? || present(&self.root, "bundle")? {
            return Err(err("NATIVE_MIGRATION_REQUIRED"));
        }
        let pin = payload.context().manifest_sha256;
        let bytes = PrivateBytes(installation_backup::source_bytes(
            &self.root,
            "runtime.env",
            8192,
            true,
        )?);
        native_environment(&bytes.0, &pin, ports)?;
        let record: Record = serde_json::from_slice(&installation_backup::source_bytes(
            &self.root, RECORD, 4096, true,
        )?)
        .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?;
        if record.format != 1
            || record.manifest_sha256 != pin
            || record.environment_sha256 != digest(&bytes.0)
            || record.ports != ports
        {
            return Err(err("NATIVE_CONFIGURATION_CHANGED"));
        }
        let credentials =
            publication_source::read(&self.root, format!("http://127.0.0.1:{}", ports.web))?;
        let mut fields = BTreeMap::new();
        for line in std::str::from_utf8(&bytes.0)
            .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?
            .lines()
        {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| err("NATIVE_CONFIGURATION_INVALID"))?;
            fields.insert(key.into(), value.into());
        }
        fields.insert(
            "ADMIN_PASSWORD".into(),
            String::from_utf8(credentials.password().to_vec())
                .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?,
        );
        Ok(fields)
    }
    /// Configure only; schema/bootstrap/service readiness belong to explicit Start.
    pub fn prepare_native_installation(
        &self,
        payload: &Payload,
        ports: Ports,
        input: Option<InitialAccountInput>,
    ) -> Result<Context> {
        let _lock = self.lock()?;
        signed_release::trust::lifecycle_writer::admit(&self.root, &Action::Install)?;
        ports.validate()?;
        if present(&self.root, "installed.json")? || present(&self.root, "bundle")? {
            return Err(err("NATIVE_MIGRATION_REQUIRED"));
        }
        let pin = payload.context().manifest_sha256;
        if !present(&self.root, "runtime.env")? {
            for name in [RECORD, "native-database", "blobs", "config"] {
                if present(&self.root, name)? {
                    return Err(err("NATIVE_MIGRATION_REQUIRED"));
                }
            }
            let input = input.ok_or_else(|| err("INITIAL_ACCOUNT_REQUIRED"))?;
            input.validate()?;
            let password = format!(
                "{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            );
            let mut bytes = format!("EXHIBITOS_RUNTIME_MODE=native\nNATIVE_RUNTIME_MANIFEST_SHA256={pin}\nEXHIBITOS_PORT={}\nEXHIBITOS_API_PORT={}\nPOSTGRES_PASSWORD={password}\nDATABASE_URL=postgresql://exhibitos:{password}@127.0.0.1:{}/exhibitos\nADMIN_SUBJECT={}\nADMIN_PASSWORD={}\nTENANT_ID={}\n",ports.web,ports.api,ports.database,input.subject,encode_password(&input.password),Uuid::new_v4()).into_bytes();
            let written = write_private_new(&self.root, "runtime.env", &bytes, 8192);
            bytes.fill(0);
            written?;
        } else if input.is_some() {
            return Err(err("INITIAL_ACCOUNT_EXISTS"));
        }
        let bytes = PrivateBytes(installation_backup::source_bytes(
            &self.root,
            "runtime.env",
            8192,
            true,
        )?);
        native_environment(&bytes.0, &pin, ports)?;
        let credentials =
            publication_source::read(&self.root, format!("http://127.0.0.1:{}", ports.web))?;
        let environment_sha256 = digest(&bytes.0);
        if present(&self.root, RECORD)? {
            let record: Record = serde_json::from_slice(&installation_backup::source_bytes(
                &self.root, RECORD, 4096, true,
            )?)
            .map_err(|_| err("NATIVE_CONFIGURATION_INVALID"))?;
            if record.format != 1
                || record.manifest_sha256 != pin
                || record.environment_sha256 != environment_sha256
                || record.ports != ports
            {
                return Err(err("NATIVE_CONFIGURATION_CHANGED"));
            }
        } else {
            let record = Record {
                format: 1,
                manifest_sha256: pin.clone(),
                environment_sha256,
                ports,
            };
            write_private_new(
                &self.root,
                RECORD,
                &serde_json::to_vec(&record).map_err(|_| err("STATE_INVALID"))?,
                4096,
            )?;
        }
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| err("STATE_UNAVAILABLE"))?;
        Ok(Context {
            kind: "legacy_account".into(),
            state: "configured".into(),
            ports,
            tenant_id: credentials.tenant_id.clone(),
            subject: credentials.subject.clone(),
            manifest_sha256: pin,
        })
    }
}
