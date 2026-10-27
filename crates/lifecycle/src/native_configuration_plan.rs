// SPDX-License-Identifier: Apache-2.0
//! In-memory native configuration rebinding only. No installer or executable
//! restore plan is produced; secret bytes never enter a serializable receipt.
use super::*;
use crate::hosted_runtime_file::Pin;
use crate::maintenance_source::DirectoryPin;

struct ConfigurationFile {
    _profile: bool,
    _name: String,
    _bytes: Secret,
}
pub(crate) struct ReboundConfigurationPlan {
    files: Vec<ConfigurationFile>,
    pins: Vec<Pin>,
    directories: Vec<DirectoryPin>,
    candidate_configuration: crate::installations::native_backup::CandidateConfiguration,
    absent: Vec<(PathBuf, String)>,
}
impl ReboundConfigurationPlan {
    pub(crate) fn file_count(&self) -> usize {
        self.files.len()
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        for directory in &self.directories {
            directory.check()?;
        }
        for pin in &self.pins {
            pin.check()?;
        }
        self.candidate_configuration.check()?;
        for (root, name) in &self.absent {
            if exists(root, name)? {
                return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
            }
        }
        Ok(())
    }
}
fn pinned_secret(root: &Path, name: &str, limit: u64, pins: &mut Vec<Pin>) -> Result<Secret> {
    let private = Secret(installation_backup::source_bytes(root, name, limit, true)?);
    let (bytes, pin) = crate::hosted_runtime_file::bounded(&root.join(name), limit)?;
    let bytes = Secret(bytes);
    if private.0 != bytes.0 {
        return Err(bad());
    }
    pin.check()?;
    pins.push(pin);
    Ok(bytes)
}
fn authenticated_file(
    manifest: &Value,
    role: &str,
    name: &str,
    root: &Path,
    logical: &str,
    limit: u64,
    pins: &mut Vec<Pin>,
) -> Result<Secret> {
    let entries = manifest["files"].as_array().ok_or_else(bad)?;
    let matches: Vec<_> = entries
        .iter()
        .filter(|v| v["role"] == role && v["name"] == name)
        .collect();
    if matches.len() != 1 {
        return Err(bad());
    }
    let bytes = pinned_secret(root, logical, limit, pins)?;
    if matches[0]["bytes"].as_u64() != Some(bytes.0.len() as u64)
        || matches[0]["sha256"].as_str() != Some(digest(&bytes.0).as_str())
    {
        return Err(bad());
    }
    Ok(bytes)
}
/// Called only by the authenticated same-catalog target adapter, under its
/// profile lock and both native session fences. This function reads/pins the
/// verified archive and generated candidate configuration; it writes nothing.
pub(crate) fn plan_rebound_configuration(
    verified: &Path,
    authenticated_manifest: &[u8],
    source_root: &Path,
    source_payload: &Payload,
    target: &mut Session,
    profile: &Path,
) -> Result<ReboundConfigurationPlan> {
    let root = target.target_observation_root().to_owned();
    let source = LifecycleService::bound_existing(source_root.to_owned())?;
    let candidate = LifecycleService::bound_existing(root.clone())?;
    let old = source.native_prepared()?.ok_or_else(bad)?;
    let new = candidate.native_prepared()?.ok_or_else(bad)?;
    let target_payload = target.maintenance_payload();
    if old.kind != new.kind
        || old.manifest_sha256 != source_payload.context().manifest_sha256
        || new.manifest_sha256 != target_payload.context().manifest_sha256
        || old.manifest_sha256 == new.manifest_sha256
    {
        return Err(bad());
    }
    let owner = owner_name(&old.kind)?;
    let configuration = verified.join("configuration");
    let mut directories = vec![
        DirectoryPin::open(verified)?,
        DirectoryPin::open(&configuration)?,
        DirectoryPin::open(source_root)?,
        DirectoryPin::open(&root)?,
        DirectoryPin::open(profile)?,
    ];
    let mut pins = Vec::new();
    let candidate_configuration =
        crate::installations::native_backup::CandidateConfiguration::capture(&root.join("config"))?;
    let mut absent = Vec::new();
    let manifest_bytes = pinned_secret(verified, "manifest.json", 16 * 1024 * 1024, &mut pins)?;
    if manifest_bytes.0 != authenticated_manifest {
        return Err(bad());
    }
    let manifest: Value = serde_json::from_slice(authenticated_manifest).map_err(|_| bad())?;
    let expected: BTreeSet<String> = [
        "native-source.json",
        "native-runtime.env",
        owner,
        "freeze-signing-key.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    names(&configuration, &expected)?;
    let config_names: Vec<_> = manifest["files"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .filter(|v| v["role"] == "configuration")
        .map(|v| v["name"].as_str().ok_or_else(bad))
        .collect::<Result<_>>()?;
    if config_names.len() != expected.len()
        || config_names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
            != expected
    {
        return Err(bad());
    }
    let map_bytes = authenticated_file(
        &manifest,
        "configuration",
        "native-source.json",
        &configuration,
        "native-source.json",
        65536,
        &mut pins,
    )?;
    let map: SourceMap = serde_json::from_slice(&map_bytes.0).map_err(|_| bad())?;
    if map.format != 1
        || map.source_root != source_root
        || map.owner_kind != old.kind
        || map.manifest_sha256 != old.manifest_sha256
        || map.journal_names.len() > 4
    {
        return Err(bad());
    }
    for (logical, name) in &map.journal_names {
        if journal(source_root, logical)? != *name || exists(profile, &journal(&root, logical)?)? {
            return Err(err("NATIVE_TARGET_CONFIGURATION_CHANGED"));
        }
        absent.push((profile.to_owned(), journal(&root, logical)?));
    }
    let original_env = authenticated_file(
        &manifest,
        "configuration",
        "native-runtime.env",
        &configuration,
        "native-runtime.env",
        8192,
        &mut pins,
    )?;
    let original_owner = authenticated_file(
        &manifest,
        "configuration",
        owner,
        &configuration,
        owner,
        4096,
        &mut pins,
    )?;
    let signing_key = authenticated_file(
        &manifest,
        "configuration",
        "freeze-signing-key.json",
        &configuration,
        "freeze-signing-key.json",
        65536,
        &mut pins,
    )?;
    // Bind parsed identities to the exact authenticated source bytes, and pin
    // BOTH candidate files while its validated DB credentials/ports are read.
    if original_env.0 != pinned_secret(source_root, "runtime.env", 8192, &mut pins)?.0
        || original_owner.0 != pinned_secret(source_root, owner, 4096, &mut pins)?.0
    {
        return Err(bad());
    }
    let _target_env = pinned_secret(&root, "runtime.env", 8192, &mut pins)?;
    let _target_owner = pinned_secret(&root, owner, 4096, &mut pins)?;
    let original_fields = Fields(source.native_start_environment(source_payload, old.ports)?);
    let target_fields = Fields(candidate.native_start_environment(&target_payload, new.ports)?);
    target.maintenance_owner(&old.kind, &original_fields.0)?;
    let environment = rebound_environment(&original_env.0, &target_fields.0)?;
    let mut proposed = Fields(original_fields.0.clone());
    for key in [
        "POSTGRES_PASSWORD",
        "DATABASE_URL",
        "EXHIBITOS_PORT",
        "EXHIBITOS_API_PORT",
        "NATIVE_RUNTIME_MANIFEST_SHA256",
    ] {
        proposed.0.insert(
            key.into(),
            target_fields.0.get(key).ok_or_else(bad)?.clone(),
        );
    }
    rebound_identity(&old.kind, &original_fields, &proposed)?;
    let mut record: Value = serde_json::from_slice(&original_owner.0).map_err(|_| bad())?;
    record["ports"] = serde_json::to_value(new.ports).map_err(|_| bad())?;
    record["manifestSha256"] = Value::String(new.manifest_sha256);
    record["environmentSha256"] = Value::String(digest(&environment.0));
    let mut files = vec![
        ConfigurationFile {
            _profile: false,
            _name: "runtime.env".into(),
            _bytes: environment,
        },
        ConfigurationFile {
            _profile: false,
            _name: owner.into(),
            _bytes: Secret(serde_json::to_vec(&record).map_err(|_| bad())?),
        },
        ConfigurationFile {
            _profile: false,
            _name: "config/freeze-signing-key.json".into(),
            _bytes: signing_key,
        },
    ];
    let journal_root = verified.join("deployment/native-authoring");
    let deployment_names: Vec<_> = manifest["files"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .filter(|v| v["role"] == "deployment")
        .map(|v| {
            v["name"]
                .as_str()
                .and_then(|s| s.strip_prefix("native-authoring/"))
                .ok_or_else(bad)
        })
        .collect::<Result<_>>()?;
    if deployment_names.len() != map.journal_names.len()
        || deployment_names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
            != map.journal_names.keys().cloned().collect()
    {
        return Err(bad());
    }
    if !map.journal_names.is_empty() {
        directories.push(DirectoryPin::open(&journal_root)?);
        names(&journal_root, &map.journal_names.keys().cloned().collect())?;
    }
    for logical in map.journal_names.keys() {
        let bytes = authenticated_file(
            &manifest,
            "deployment",
            &format!("native-authoring/{logical}"),
            &journal_root,
            logical,
            2_200_000,
            &mut pins,
        )?;
        let rebound = Secret(rebound_journal(logical, &bytes.0, &original_fields)?);
        files.push(ConfigurationFile {
            _profile: true,
            _name: journal(&root, logical)?,
            _bytes: rebound,
        });
    }
    let plan = ReboundConfigurationPlan {
        files,
        pins,
        directories,
        candidate_configuration,
        absent,
    };
    plan.recheck()?;
    Ok(plan)
}
