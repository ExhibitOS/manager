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
fn local_transport() -> Result<()> {
    let host = std::env::var("DOCKER_HOST").map_err(|_| err("ENGINE_LOCALITY_UNVERIFIED"))?;
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
        || [
            "DOCKER_CONTEXT",
            "DOCKER_TLS",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
        ]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
    {
        return Err(err("ENGINE_LOCALITY_UNVERIFIED"));
    }
    Ok(())
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
pub(crate) fn bind_compose(m: &BundleManifest, bytes: &[u8]) -> Result<()> {
    let Some(p) = &m.explicit_local_network else {
        return Ok(());
    };
    p.validate()?;
    if m.preferred_engine.as_deref() != Some("docker") {
        return Err(err("BUNDLE_INVALID"));
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| err("BUNDLE_INVALID"))?;
    let n = &v["networks"]["default"];
    let expected = serde_json::json!({"labels":{"com.exhibitos.bundle":m.bundle_id,"com.exhibitos.project":m.project_name,"com.exhibitos.schema":m.schema_version},"ipam":{"config":[{"subnet":p.subnet,"gateway":p.gateway}]}});
    if v["networks"].as_object().is_none_or(|o| o.len() != 1) || n != &expected {
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
        let new_id = "70000000-0000-4000-8000-000000000010";
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
