// SPDX-License-Identifier: Apache-2.0
//! Explicit opt-in local IPAM. Does not mutate daemon settings or existing bundles.
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplicitLocalNetwork {
    pub mode: String,
    pub subnet: String,
    pub gateway: String,
}
impl ExplicitLocalNetwork {
    pub fn validate(&self) -> Result<()> {
        if self.mode != "explicit-rfc1918-v1"
            || crate::restoration_network::policy_subnet(&self.subnet).is_err()
            || crate::restoration_network::policy_gateway(&self.subnet)? != self.gateway
        {
            return Err(err("BUNDLE_INVALID"));
        }
        Ok(())
    }
}
const DOCKER_SELECTORS: [&str; 4] = [
    "DOCKER_CONTEXT",
    "DOCKER_TLS",
    "DOCKER_TLS_VERIFY",
    "DOCKER_CERT_PATH",
];
#[cfg(target_os = "macos")]
static NATIVE_DOCKER_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
static NATIVE_DOCKER_ENDPOINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
#[cfg(target_os = "macos")]
static NATIVE_DOCKER_PIN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn validate_transport(host: &str, selectors: bool) -> Result<()> {
    if host.len() > 1024
        || host
            .bytes()
            .any(|b| b <= 32 || b == 127 || b"?#".contains(&b))
        || !(host.starts_with("unix:///") && host.len() > 8
            || host.strip_prefix("npipe:////./pipe/").is_some_and(|p| {
                !p.is_empty()
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            }))
        || selectors
    {
        return Err(err("ENGINE_LOCALITY_UNVERIFIED"));
    }
    Ok(())
}
#[cfg(any(target_os = "macos", test))]
fn native_endpoint(value: &Value) -> Result<String> {
    let host = value["Host"]
        .as_str()
        .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?;
    validate_transport(host, false)?;
    if !host.starts_with("unix:///") || value["SkipTLSVerify"] != Value::Bool(false) {
        return Err(err("ENGINE_LOCALITY_UNVERIFIED"));
    }
    Ok(host.into())
}
/// Enables lazy native-only admission without doing I/O during application setup.
#[cfg(target_os = "macos")]
pub fn enable_native_docker_transport() {
    NATIVE_DOCKER_ENABLED.store(true, std::sync::atomic::Ordering::Release);
}
/// Called lazily only when an off-main operation actually needs Docker. CLI callers
/// do not enable this path and retain their explicit environment-pin admission.
#[cfg(target_os = "macos")]
fn prepare_native_docker_transport() -> Result<()> {
    if NATIVE_DOCKER_ENDPOINT.get().is_some() {
        return Ok(());
    }
    let _guard = NATIVE_DOCKER_PIN_LOCK.lock().map_err(|_| err("BUSY"))?;
    if NATIVE_DOCKER_ENDPOINT.get().is_some() {
        return Ok(());
    }
    if super::engine_executable("docker").is_none() {
        return Ok(());
    }
    if DOCKER_SELECTORS
        .iter()
        .any(|k| std::env::var_os(k).is_some())
    {
        return Err(err("ENGINE_LOCALITY_UNVERIFIED"));
    }
    let endpoint = if let Some(value) = std::env::var_os("DOCKER_HOST") {
        let host = value
            .into_string()
            .map_err(|_| err("ENGINE_LOCALITY_UNVERIFIED"))?;
        validate_transport(&host, false)?;
        if !host.starts_with("unix:///") {
            return Err(err("ENGINE_LOCALITY_UNVERIFIED"));
        }
        host
    } else {
        // An absolute resolved executable uses the existing bounded process backend
        // without re-entering the symbolic Docker pin hook. Arguments are fixed/read-only.
        let executable =
            super::engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
        let bytes = super::run(
            executable
                .to_str()
                .ok_or_else(|| err("ENGINE_LOCALITY_UNVERIFIED"))?,
            &[
                "context".into(),
                "inspect".into(),
                "--format".into(),
                "{{json .Endpoints.docker}}".into(),
            ],
            None,
            15,
        )?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| err("ENGINE_LOCALITY_UNVERIFIED"))?;
        native_endpoint(&value)?
    };
    NATIVE_DOCKER_ENDPOINT
        .set(endpoint)
        .map_err(|_| err("ENGINE_LOCALITY_UNVERIFIED"))?;
    Ok(())
}
#[cfg(any(target_os = "macos", test))]
fn command_endpoint(
    command: &mut std::process::Command,
    host: &str,
    selectors: bool,
) -> Result<()> {
    validate_transport(host, selectors)?;
    command.env("DOCKER_HOST", host);
    Ok(())
}
pub(super) fn pin_command(command: &mut std::process::Command) -> Result<()> {
    #[cfg(target_os = "macos")]
    if NATIVE_DOCKER_ENABLED.load(std::sync::atomic::Ordering::Acquire) {
        prepare_native_docker_transport()?;
    }
    #[cfg(target_os = "macos")]
    if let Some(host) = NATIVE_DOCKER_ENDPOINT.get() {
        command_endpoint(
            command,
            host,
            DOCKER_SELECTORS
                .iter()
                .any(|k| std::env::var_os(k).is_some()),
        )?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = command;
    Ok(())
}
fn local_transport() -> Result<()> {
    #[cfg(target_os = "macos")]
    if NATIVE_DOCKER_ENABLED.load(std::sync::atomic::Ordering::Acquire) {
        prepare_native_docker_transport()?;
    }
    #[cfg(target_os = "macos")]
    if let Some(host) = NATIVE_DOCKER_ENDPOINT.get() {
        return validate_transport(
            host,
            DOCKER_SELECTORS
                .iter()
                .any(|k| std::env::var_os(k).is_some()),
        );
    }
    let host = std::env::var("DOCKER_HOST").map_err(|_| err("ENGINE_LOCALITY_UNVERIFIED"))?;
    validate_transport(
        &host,
        DOCKER_SELECTORS
            .iter()
            .any(|k| std::env::var_os(k).is_some()),
    )
}
/// A read-only observation, not a reservation or permission. Host routes include VPN routes.
pub fn choose_explicit_local_network() -> Result<ExplicitLocalNetwork> {
    local_transport()?;
    let subnet = crate::restoration_network::choose()?;
    Ok(ExplicitLocalNetwork {
        mode: "explicit-rfc1918-v1".into(),
        gateway: crate::restoration_network::policy_gateway(&subnet)?,
        subnet,
    })
}
#[derive(Deserialize)]
struct ComposeNetworkBinding {
    networks: ClosedNetworks,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedNetworks {
    default: ClosedNetwork,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedNetwork {
    #[serde(default, deserialize_with = "present_network_name")]
    name: Option<String>,
    labels: ClosedLabels,
    ipam: ClosedIPAM,
}
// Missing is the original raw wire format; present must be an actual string,
// so explicit null cannot silently become the legacy absent case.
fn present_network_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedLabels {
    #[serde(rename = "com.exhibitos.bundle")]
    bundle: String,
    #[serde(rename = "com.exhibitos.project")]
    project: String,
    #[serde(rename = "com.exhibitos.schema")]
    schema: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedIPAM {
    config: Vec<ClosedConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedConfig {
    subnet: String,
    gateway: String,
}
pub(crate) fn bind_compose(m: &BundleManifest, bytes: &[u8]) -> Result<()> {
    let Some(p) = &m.explicit_local_network else {
        return Ok(());
    };
    p.validate()?;
    if m.preferred_engine.as_deref() != Some("docker") {
        return Err(err("BUNDLE_INVALID"));
    }
    let value: ComposeNetworkBinding =
        serde_json::from_slice(bytes).map_err(|_| err("BUNDLE_INVALID"))?;
    let n = value.networks.default;
    if n.name
        .as_deref()
        .is_some_and(|name| name != format!("{}_default", m.project_name))
        || n.labels.bundle != m.bundle_id
        || n.labels.project != m.project_name
        || n.labels.schema != m.schema_version
        || n.ipam.config.len() != 1
        || n.ipam.config[0].subnet != p.subnet
        || n.ipam.config[0].gateway != p.gateway
    {
        return Err(err("BUNDLE_INVALID"));
    }
    Ok(())
}
/// Runs inside the existing operation lock. Only this exact owned network may be excluded.
pub(crate) fn admit(m: &BundleManifest, engine: &str) -> Result<()> {
    let Some(p) = &m.explicit_local_network else {
        return Ok(());
    };
    p.validate()?;
    local_transport()?;
    if engine != "docker" {
        return Err(err("BUNDLE_INVALID"));
    }
    crate::restoration_network::recheck_owned(m, &p.subnet, &p.gateway)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_network_name_is_optional_but_exact_and_closed() {
        let mut m = super::super::tests::manifest_for_detection();
        m.preferred_engine = Some("docker".into());
        m.explicit_local_network = Some(ExplicitLocalNetwork {
            mode: "explicit-rfc1918-v1".into(),
            subnet: "10.240.0.0/28".into(),
            gateway: "10.240.0.1".into(),
        });
        let raw = serde_json::json!({"networks":{"default":{"labels":{"com.exhibitos.bundle":m.bundle_id,"com.exhibitos.project":m.project_name,"com.exhibitos.schema":m.schema_version},"ipam":{"config":[{"subnet":"10.240.0.0/28","gateway":"10.240.0.1"}]}}}});
        assert!(bind_compose(&m, &serde_json::to_vec(&raw).unwrap()).is_ok());
        let mut normalized = raw;
        normalized["networks"]["default"]["name"] =
            serde_json::json!(format!("{}_default", m.project_name));
        assert!(bind_compose(&m, &serde_json::to_vec(&normalized).unwrap()).is_ok());
        for name in [
            serde_json::json!("foreign_default"),
            serde_json::json!(""),
            Value::Null,
            Value::Bool(false),
        ] {
            let mut bad = normalized.clone();
            bad["networks"]["default"]["name"] = name;
            assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut bad = normalized.clone();
        bad["networks"]["default"]["external"] = Value::Bool(false);
        assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = normalized.clone();
        bad["networks"]["extra"] = normalized["networks"]["default"].clone();
        assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = normalized.clone();
        bad["networks"]["default"]["ipam"]["config"][0]["subnet"] =
            serde_json::json!("10.240.0.16/28");
        assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = normalized;
        bad["networks"]["default"]["ipam"]["driver"] = serde_json::json!("default");
        assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[test]
    fn local_network_policy_closed_gateway_and_legacy_wire() {
        let mut m = super::super::tests::manifest_for_detection();
        let old = serde_json::to_value(&m).unwrap();
        assert!(old.get("explicitLocalNetwork").is_none());
        assert!(
            serde_json::from_value::<BundleManifest>(old.clone())
                .unwrap()
                .explicit_local_network
                .is_none()
        );
        let p = ExplicitLocalNetwork {
            mode: "explicit-rfc1918-v1".into(),
            subnet: "10.240.0.0/28".into(),
            gateway: "10.240.0.1".into(),
        };
        assert!(p.validate().is_ok());
        for bad in [
            serde_json::json!({"mode":"explicit-rfc1918-v1","subnet":"10.240.0.0/28","gateway":"10.240.0.1","extra":true}),
            serde_json::json!({"mode":"explicit-rfc1918-v1","subnet":"10.240.0.0/28"}),
        ] {
            assert!(serde_json::from_value::<ExplicitLocalNetwork>(bad).is_err());
        }
        for (subnet, gateway) in [
            ("10.240.0.1/28", "10.240.0.1"),
            ("10.240.0.0/24", "10.240.0.1"),
            ("10.240.0.0/28", "10.240.0.2"),
            ("192.168.1.0/28", "192.168.1.1"),
        ] {
            let b = ExplicitLocalNetwork {
                mode: p.mode.clone(),
                subnet: subnet.into(),
                gateway: gateway.into(),
            };
            assert!(b.validate().is_err());
        }
        m.preferred_engine = Some("docker".into());
        m.explicit_local_network = Some(p.clone());
        let v = serde_json::json!({"networks":{"default":{"labels":{"com.exhibitos.bundle":m.bundle_id,"com.exhibitos.project":m.project_name,"com.exhibitos.schema":m.schema_version},"ipam":{"config":[{"subnet":p.subnet,"gateway":p.gateway}]}}}});
        assert!(bind_compose(&m, &serde_json::to_vec(&v).unwrap()).is_ok());
        let bytes = serde_json::to_string(&v).unwrap();
        let duplicate = bytes.replace(
            "\"subnet\":\"10.240.0.0/28\"",
            "\"subnet\":\"10.240.0.16/28\",\"subnet\":\"10.240.0.0/28\"",
        );
        assert!(bind_compose(&m, duplicate.as_bytes()).is_err());
        let mut bad = v.clone();
        bad["networks"]["default"]["external"] = Value::Bool(true);
        assert!(bind_compose(&m, &serde_json::to_vec(&bad).unwrap()).is_err());
        let mut b = v;
        b["networks"]["default"]["ipam"]["config"][0]["gateway"] = serde_json::json!("10.240.0.2");
        assert!(bind_compose(&m, &serde_json::to_vec(&b).unwrap()).is_err());
    }
}
#[cfg(test)]
mod remap_tests {
    use super::*;
    #[test]
    fn restored_explicit_policy_is_rebound_while_legacy_manifest_stays_omitted() {
        let mut original = super::super::tests::manifest_for_detection();
        let ids = [
            format!("sha256:{}", "a".repeat(64)),
            format!("sha256:{}", "b".repeat(64)),
        ];
        let images: Vec<_> = ids
            .iter()
            .map(|id| crate::restoration::PreservedImage {
                reference: id.clone(),
                content_id: id.clone(),
                archive: "synthetic.tar".into(),
                bytes: 1,
                sha256: "c".repeat(64),
            })
            .collect();
        // Detection deliberately has a one-image fixture; restoration needs
        // the actual two runtime image identities, before optional maintenance.
        let new_id = "70000000-0000-4000-8000-000000000010";
        assert!(crate::restoration::remapped_bundle(
            &original, &images, new_id, 13201, &ids[0], &ids[1], Some("10.240.0.16/28")
        ).is_err());
        original.images = ids.iter().map(|reference| crate::Image {
            reference: reference.clone(), archive: None,
        }).collect();
        let (legacy, _) = crate::restoration::remapped_bundle(
            &original,
            &images,
            new_id,
            13201,
            &ids[0],
            &ids[1],
            Some("10.240.0.16/28"),
        )
        .unwrap();
        assert!(legacy.explicit_local_network.is_none());
        original.explicit_local_network = Some(ExplicitLocalNetwork {
            mode: "explicit-rfc1918-v1".into(),
            subnet: "10.240.0.0/28".into(),
            gateway: "10.240.0.1".into(),
        });
        assert!(
            crate::restoration::remapped_bundle(
                &original, &images, new_id, 13201, &ids[0], &ids[1], None
            )
            .is_err()
        );
        let (m, c) = crate::restoration::remapped_bundle(
            &original,
            &images,
            new_id,
            13201,
            &ids[0],
            &ids[1],
            Some("10.240.0.16/28"),
        )
        .unwrap();
        let p = m.explicit_local_network.as_ref().unwrap();
        assert_eq!(p.subnet, "10.240.0.16/28");
        assert_eq!(p.gateway, "10.240.0.17");
        assert_eq!(m.compose_sha256, digest(&c));
        assert!(bind_compose(&m, &c).is_ok());
    }
}

#[cfg(test)]
mod native_transport_tests {
    use super::*;
    #[test]
    fn endpoint_validation_preserves_explicit_cli_and_refuses_remote_or_selectors() {
        assert!(validate_transport("unix:///tmp/synthetic-docker.sock", false).is_ok());
        assert!(validate_transport("npipe:////./pipe/docker_engine", false).is_ok());
        for host in [
            "tcp://127.0.0.1:2375",
            "ssh://example.invalid",
            "unix:///tmp/socket?x",
            "unix:///tmp/with space",
            "unix://",
        ] {
            assert!(validate_transport(host, false).is_err());
        }
        assert!(validate_transport("unix:///tmp/synthetic-docker.sock", true).is_err());
    }
    #[test]
    fn native_context_requires_explicit_unencrypted_local_unix_endpoint() {
        assert_eq!(native_endpoint(&serde_json::json!({"Host":"unix:///tmp/synthetic-docker.sock","SkipTLSVerify":false})).unwrap(),"unix:///tmp/synthetic-docker.sock");
        for value in [
            serde_json::json!({"Host":"tcp://localhost:2375","SkipTLSVerify":false}),
            serde_json::json!({"Host":"ssh://example.invalid","SkipTLSVerify":false}),
            serde_json::json!({"Host":"unix:///tmp/socket","SkipTLSVerify":true}),
            serde_json::json!({"Host":"unix:///tmp/socket"}),
            serde_json::json!({"Host":"npipe:////./pipe/docker_engine","SkipTLSVerify":false}),
        ] {
            assert!(native_endpoint(&value).is_err());
        }
    }
    #[test]
    fn pinned_command_uses_exact_endpoint_and_never_erases_selectors() {
        let mut command = std::process::Command::new("docker");
        command.env("DOCKER_CONTEXT", "existing-user-selector");
        assert!(command_endpoint(&mut command, "unix:///tmp/pin.sock", true).is_err());
        assert!(command.get_envs().any(|(k, v)| k == "DOCKER_CONTEXT"
            && v == Some(std::ffi::OsStr::new("existing-user-selector"))));
        let mut clean = std::process::Command::new("docker");
        command_endpoint(&mut clean, "unix:///tmp/pin.sock", false).unwrap();
        assert!(
            clean.get_envs().any(|(k, v)| k == "DOCKER_HOST"
                && v == Some(std::ffi::OsStr::new("unix:///tmp/pin.sock")))
        );
    }
    #[test]
    fn native_endpoint_lifetime_cannot_switch_to_a_later_context() {
        let pin = std::sync::OnceLock::new();
        let first = native_endpoint(
            &serde_json::json!({"Host":"unix:///tmp/first.sock","SkipTLSVerify":false}),
        )
        .unwrap();
        pin.set(first).unwrap();
        assert!(pin.set("unix:///tmp/changed.sock".to_string()).is_err());
        assert_eq!(pin.get().unwrap(), "unix:///tmp/first.sock");
    }
}
