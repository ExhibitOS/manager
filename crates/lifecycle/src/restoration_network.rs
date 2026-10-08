// SPDX-License-Identifier: Apache-2.0
//! Bounded private restoration networks; no daemon configuration changes or resource removal.
use super::*;
use std::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range { start: u32, end: u32 }
impl Range {
    fn cidr(value: &str, abbreviated: bool) -> Result<Self> {
        let (address, prefix) = match value.split_once('/') {
            Some((a, p)) => (a, p.parse::<u8>().map_err(|_| err("ENGINE_OUTPUT_INVALID"))?),
            None => (value, if abbreviated { (value.split('.').count() * 8) as u8 } else { 32 }),
        };
        if prefix > 32 { return Err(err("ENGINE_OUTPUT_INVALID")); }
        let mut parts: Vec<&str> = address.split('.').collect();
        if abbreviated && !parts.is_empty() && parts.len() <= 4 { while parts.len() < 4 { parts.push("0"); } }
        let address: Ipv4Addr = parts.join(".").parse().map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        let mask = if prefix == 0 { 0 } else { u32::MAX << (32-prefix) };
        let start = u32::from(address) & mask;
        Ok(Self {start,end:start | !mask})
    }
    fn overlaps(self, other: Self) -> bool { self.start <= other.end && other.start <= self.end }
}
fn select(blocked: &[Range]) -> Result<String> {
    // Bounded RFC1918 policy: 4096 /28 candidates, at most 14 usable addresses each.
    for slot in 0..4096u32 {
        let start = u32::from(Ipv4Addr::new(10,240,0,0)) + slot*16;
        let range = Range {start,end:start+15};
        if !blocked.iter().any(|other| range.overlaps(*other)) { return Ok(format!("{}/28",Ipv4Addr::from(start))); }
    }
    Err(err("ENGINE_NETWORK_CAPACITY"))
}
#[cfg(any(target_os="macos",test))]
fn mac_routes(bytes: &[u8]) -> Result<Vec<Range>> {
    let text = std::str::from_utf8(bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let mut active = false;
    let mut output = Vec::new();
    for line in text.lines() {
        if line.starts_with("Destination") { active = true; continue; }
        if !active || line.trim().is_empty() { continue; }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 4 { return Err(err("ENGINE_OUTPUT_INVALID")); }
        // Default route is the uplink, not an occupied local allocation. More specific VPN routes remain exclusions.
        if fields[0] == "default" { continue; }
        output.push(Range::cidr(fields[0],true)?);
    }
    if !active || output.is_empty() { return Err(err("ENGINE_OUTPUT_INVALID")); }
    Ok(output)
}
fn engine_ranges(value: &Value) -> Result<Vec<Range>> {
    let networks = value.as_array().ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    if networks.is_empty() || networks.len() > 4096 { return Err(err("ENGINE_OUTPUT_INVALID")); }
    let mut ranges = Vec::new();
    for network in networks {
        if !network["IPAM"].is_object() || network["IPAM"].get("Config").is_none() { return Err(err("ENGINE_OUTPUT_INVALID")); }
        let configs = network["IPAM"]["Config"].as_array();
        if configs.is_none() && !network["IPAM"]["Config"].is_null() { return Err(err("ENGINE_OUTPUT_INVALID")); }
        for config in configs.into_iter().flatten() {
            let subnet = config["Subnet"].as_str().ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
            if subnet.contains(':') {
                let (addr,prefix)=subnet.split_once('/').ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
                addr.parse::<std::net::Ipv6Addr>().map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
                if prefix.parse::<u8>().map_err(|_| err("ENGINE_OUTPUT_INVALID"))? >128 {return Err(err("ENGINE_OUTPUT_INVALID"));}
                continue;
            }
            ranges.push(Range::cidr(subnet,false)?);
        }
    }
    Ok(ranges)
}

#[cfg(any(target_os="linux",test))]
fn linux_routes(bytes: &[u8]) -> Result<Vec<Range>> {
    let value: Value=serde_json::from_slice(bytes).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?;
    let routes=value.as_array().ok_or_else(||err("ENGINE_OUTPUT_INVALID"))?;
    if routes.is_empty() || routes.len()>16384 {return Err(err("ENGINE_OUTPUT_INVALID"));}
    let mut ranges=Vec::new();
    for route in routes {
        let destination=route["dst"].as_str().ok_or_else(||err("ENGINE_OUTPUT_INVALID"))?;
        if destination!="default" {ranges.push(Range::cidr(destination,false)?);}
    }
    Ok(ranges)
}
fn host_routes() -> Result<Vec<Range>> {
    #[cfg(target_os="macos")]
    return mac_routes(&run("/usr/sbin/netstat",&["-rn".into(),"-f".into(),"inet".into()],None,30)?);
    #[cfg(target_os="linux")]
    return linux_routes(&run("ip",&["-j".into(),"-4".into(),"route".into(),"show".into(),"table".into(),"all".into()],None,30)?);
    #[cfg(not(any(target_os="macos",target_os="linux")))]
    Err(err("BACKUP_PLATFORM_UNVERIFIED"))
}
fn engine_inventory() -> Result<Value> {
    let raw=run("docker",&["network".into(),"ls".into(),"--no-trunc".into(),"--format".into(),"{{.ID}}".into()],None,30)?;
    let ids=std::str::from_utf8(&raw).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?.lines().map(str::to_owned).collect::<Vec<_>>();
    if ids.is_empty() || ids.len()>4096 || ids.iter().any(|id|!hash_valid(id)) {return Err(err("ENGINE_OUTPUT_INVALID"));}
    let mut args=vec!["network".into(),"inspect".into()];args.extend(ids);
    serde_json::from_slice(&run("docker",&args,None,30)?).map_err(|_|err("ENGINE_OUTPUT_INVALID"))
}
pub(crate) fn choose() -> Result<String> {
    let mut blocked=engine_ranges(&engine_inventory()?)?;
    blocked.extend(host_routes()?);
    select(&blocked)
}
pub(crate) fn policy_subnet(value: &str) -> Result<()> {
    let range=Range::cidr(value,false)?;
    let first=u32::from(Ipv4Addr::new(10,240,0,0));
    if range.start<first || range.end>=first+65536 || range.end-range.start!=15 || format!("{}/28",Ipv4Addr::from(range.start))!=value {return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));}
    Ok(())
}
pub(crate) fn policy_gateway(subnet: &str) -> Result<String> {
    policy_subnet(subnet)?;
    let range = Range::cidr(subnet, false)?;
    Ok(Ipv4Addr::from(range.start + 1).to_string())
}
/// Legacy receipts omit the gateway; new receipts bind the canonical gateway explicitly.
pub(crate) fn compose_gateway(bytes: &[u8]) -> Result<Option<String>> {
    let subnet = compose_subnet(bytes)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    let Some(gateway) = value["networks"]["default"]["ipam"]["config"][0].get("gateway") else { return Ok(None); };
    let subnet = subnet.ok_or_else(|| err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    let expected = policy_gateway(&subnet)?;
    if gateway != &Value::String(expected.clone()) { return Err(err("RESTORE_LAYOUT_UNSUPPORTED")); }
    Ok(Some(expected))
}
pub(crate) fn recheck(value: &str) -> Result<()> {
    policy_subnet(value)?;
    let range=Range::cidr(value,false)?;
    let mut blocked=engine_ranges(&engine_inventory()?)?;blocked.extend(host_routes()?);
    if blocked.iter().any(|r|range.overlaps(*r)) {return Err(err("ENGINE_NETWORK_CAPACITY"));}
    Ok(())
}
/// Rebuild the exact trusted Compose later; this only extracts a bounded policy value.
pub(crate) fn compose_subnet(bytes: &[u8]) -> Result<Option<String>> {
    let value: Value=serde_json::from_slice(bytes).map_err(|_|err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    let network=&value["networks"]["default"];
    let Some(ipam)=network.get("ipam") else {return Ok(None)};
    let configs=ipam["config"].as_array().ok_or_else(||err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    if configs.len()!=1 {return Err(err("RESTORE_LAYOUT_UNSUPPORTED"));}
    let subnet=configs[0]["subnet"].as_str().ok_or_else(||err("RESTORE_LAYOUT_UNSUPPORTED"))?;
    policy_subnet(subnet)?;Ok(Some(subnet.into()))
}
pub(crate) fn receipt_compose_subnet(bytes: &[u8], recorded: Option<&str>) -> Result<Option<String>> {
    let observed=compose_subnet(bytes)?;
    if observed.as_deref()!=recorded {return Err(err("UPDATE_SOURCE_CONFIGURATION_MISMATCH"));}
    Ok(observed)
}
pub(crate) fn verify_observed(network: &Value, subnet: &str) -> Result<()> {
    policy_subnet(subnet)?;
    let configs=network["IPAM"]["Config"].as_array().ok_or_else(||err("OWNERSHIP_CONFLICT"))?;
    if configs.len()!=1 || configs[0]["Subnet"]!=subnet || network["EnableIPv6"]!=false || configs[0].get("IPRange").is_some_and(|v|v!="") {return Err(err("OWNERSHIP_CONFLICT"));}
    let gateway=configs[0]["Gateway"].as_str().ok_or_else(||err("OWNERSHIP_CONFLICT"))?;
    let gateway=Range::cidr(gateway,false)?;let range=Range::cidr(subnet,false)?;
    if gateway.start<=range.start || gateway.end>=range.end {return Err(err("OWNERSHIP_CONFLICT"));}
    Ok(())
}
#[cfg(test)]
mod tests {
use super::*;
#[test]
fn explicit_gateway_is_policy_bound_while_legacy_and_observed_gates_remain_strict() {
    assert_eq!(policy_gateway("10.240.0.0/28").unwrap(),"10.240.0.1");
    assert_eq!(policy_gateway("10.240.255.240/28").unwrap(),"10.240.255.241");
    assert!(policy_gateway("10.240.0.1/28").is_err());
    let legacy=br#"{"networks":{"default":{"ipam":{"config":[{"subnet":"10.240.0.0/28"}]}}}}"#;
    assert_eq!(compose_gateway(legacy).unwrap(),None);
    let mut value:Value=serde_json::from_slice(legacy).unwrap();
    value["networks"]["default"]["ipam"]["config"][0]["gateway"]=serde_json::json!("10.240.0.1");
    assert_eq!(compose_gateway(&serde_json::to_vec(&value).unwrap()).unwrap().as_deref(),Some("10.240.0.1"));
    for wrong in [Value::Null,serde_json::json!("10.240.0.0"),serde_json::json!("10.240.0.16"),serde_json::json!("10.240.0.2")] {
        value["networks"]["default"]["ipam"]["config"][0]["gateway"]=wrong;
        assert!(compose_gateway(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let observed=serde_json::json!({"EnableIPv6":false,"IPAM":{"Config":[{"Subnet":"10.240.0.0/28"}]}});
    assert_eq!(verify_observed(&observed,"10.240.0.0/28").unwrap_err().code,"OWNERSHIP_CONFLICT");
}

#[test]
fn allocation_excludes_engine_routes_and_nested_addresses() {
    let blocked = [Range::cidr("10.240.0.0/28",false).unwrap(),Range::cidr("10.240.0.21",false).unwrap()];
    assert_eq!(select(&blocked).unwrap(),"10.240.0.32/28");
    assert_eq!(select(&[Range::cidr("10.0.0.0/8",false).unwrap()]).unwrap_err().code,"ENGINE_NETWORK_CAPACITY");
    assert_eq!(select(&[Range::cidr("0.0.0.0/0",false).unwrap()]).unwrap_err().code,"ENGINE_NETWORK_CAPACITY");
}
#[test]
fn route_parser_retains_vpn_routes_and_refuses_ambiguous_input() {
    let routes=mac_routes(b"Routing tables\nInternet:\nDestination Gateway Flags Netif\ndefault 192.168.1.1 UGScg en0\n10/8 link#1 UCS utun0\n192.168.68/22 link#1 UCS en0\n").unwrap();
    assert_eq!(select(&routes).unwrap_err().code,"ENGINE_NETWORK_CAPACITY");
    assert!(mac_routes(b"Destination Gateway Flags Netif\nmalformed link#1 UCS en0\n").is_err());
    assert!(Range::cidr("10.240.0.0/33",false).is_err());
    assert!(mac_routes(b"truncated").is_err());
}
#[test]
fn engine_inventory_validates_dual_stack_and_refuses_bad_ranges() {
    let value=serde_json::json!([{"IPAM":{"Config":[{"Subnet":"10.240.0.0/27"},{"Subnet":"fd00::/64"}]}}]);
    assert_eq!(select(&engine_ranges(&value).unwrap()).unwrap(),"10.240.0.32/28");
    for value in [serde_json::json!([]),serde_json::json!([{}]),serde_json::json!([{"IPAM":{"Config":[{}]}}]),serde_json::json!([{"IPAM":{"Config":[{"Subnet":"fd00::/129"}]}}])] {assert!(engine_ranges(&value).is_err());}
}
#[cfg(target_os="macos")]
#[test]
#[ignore = "explicit macOS local Docker empty private-network allocation qualification; no service or data volume changes"]
fn actual_bounded_network_create_inspect_remove() {
    let record=std::env::temp_dir().join(format!("exhibitos-network-native-proof-{}",Uuid::new_v4()));
    fs::create_dir(&record).unwrap();
    println!("NETWORK_PROOF_RECORD={}",record.display());
    let ids=run("docker",&["network".into(),"ls".into(),"--no-trunc".into(),"--format".into(),"{{.ID}}".into()],None,30).unwrap();
    let ids=std::str::from_utf8(&ids).unwrap().lines().map(str::to_owned).collect::<Vec<_>>();
    assert!(!ids.is_empty() && ids.len()<=4096 && ids.iter().all(|id|hash_valid(id)));
    let mut args=vec!["network".into(),"inspect".into()];args.extend(ids);
    let inventory:Value=serde_json::from_slice(&run("docker",&args,None,30).unwrap()).unwrap();
    crate::restoration::private_bytes(&record.join("before.json"),&serde_json::to_vec(&inventory).unwrap()).unwrap();
    let subnet=choose().unwrap();recheck(&subnet).unwrap();
    let name=format!("exhibitos-private-allocation-probe-{}",Uuid::new_v4());
    let output=run("docker",&["network".into(),"create".into(),"--driver".into(),"bridge".into(),"--subnet".into(),subnet.clone(),"--label".into(),"com.exhibitos.probe=private-allocation".into(),name.clone()],None,30).unwrap();
    let id=std::str::from_utf8(&output).unwrap().trim();assert!(hash_valid(id));
    crate::restoration::private_bytes(&record.join("probe.json"),&serde_json::to_vec(&serde_json::json!({"id":id,"name":name,"subnet":subnet,"removed":false})).unwrap()).unwrap();
    let net=backup_creation::inspected("docker",&["network".into(),"inspect".into(),id.into()]).unwrap();
    // Only delete this newly created, exactly identified empty probe. Never delete any preexisting network.
    assert_eq!(net["Name"],name);assert_eq!(net["Id"],id);assert_eq!(net["Driver"],"bridge");
    assert_eq!(net["Labels"]["com.exhibitos.probe"],"private-allocation");
    assert!(net["Containers"].as_object().unwrap().is_empty());
    run("docker",&["network".into(),"rm".into(),id.into()],None,30).unwrap();
    crate::restoration::private_bytes(&record.join("removed.json"),&serde_json::to_vec(&serde_json::json!({"id":id,"removed":true})).unwrap()).unwrap();
    verify_observed(&net,&subnet).unwrap();
    let after: Value=serde_json::from_slice(&run("docker",&args,None,30).unwrap()).unwrap();
    crate::restoration::private_bytes(&record.join("after.json"),&serde_json::to_vec(&after).unwrap()).unwrap();
    assert_eq!(inventory,after,"preexisting network metadata changed during qualification");
    println!("PASS_BOUNDED_PRIVATE_NETWORK_CREATE_INSPECT_REMOVE_NO_EXISTING_NETWORK_MUTATION");
}

#[test]
fn native_policy_and_linux_routes_reject_ambiguous_or_foreign_allocations() {
    let ranges=linux_routes(br#"[{"dst":"default","gateway":"192.168.1.1"},{"dst":"10.240.0.0/24"},{"dst":"127.0.0.0/8"}]"#).unwrap();
    assert_eq!(select(&ranges).unwrap(),"10.240.1.0/28");
    assert!(linux_routes(br#"[{"gateway":"missing destination"}]"#).is_err());
    for bad in ["10.240.0.1/28","10.241.0.0/28","172.16.0.0/28","10.240.0.0/24"] {assert!(policy_subnet(bad).is_err());}
    let good=serde_json::json!({"IPAM":{"Config":[{"Subnet":"10.240.0.0/28","Gateway":"10.240.0.1"}]},"EnableIPv6":false});
    verify_observed(&good,"10.240.0.0/28").unwrap();
    for field in ["Subnet","Gateway"] {
        let mut changed=good.clone();changed["IPAM"]["Config"][0][field]="10.241.0.1".into();assert!(verify_observed(&changed,"10.240.0.0/28").is_err());
    }
}
#[test]
fn subnet_receipt_binding_preserves_legacy_and_refuses_changed_configuration() {
    let legacy=br#"{"networks":{"default":{"labels":{}}}}"#;
    assert_eq!(receipt_compose_subnet(legacy,None).unwrap(),None);
    let fresh=br#"{"networks":{"default":{"ipam":{"config":[{"subnet":"10.240.0.0/28"}]}}}}"#;
    assert_eq!(receipt_compose_subnet(fresh,Some("10.240.0.0/28")).unwrap().as_deref(),Some("10.240.0.0/28"));
    assert!(receipt_compose_subnet(fresh,None).is_err());
    assert!(receipt_compose_subnet(fresh,Some("10.240.0.16/28")).is_err());
    assert!(receipt_compose_subnet(legacy,Some("10.240.0.0/28")).is_err());
}

}

/// Exclude only a strictly bound owned network; retain every other route/network exclusion.
pub(crate) fn recheck_owned(m: &BundleManifest, subnet: &str, gateway: &str) -> Result<()> {
    let inventory = engine_inventory()?;
    admit_owned_inventory(m, subnet, gateway, &inventory, &host_routes_for_owned(m,subnet,&inventory)?)
}
fn admit_owned_inventory(
    m: &BundleManifest,
    subnet: &str,
    gateway: &str,
    inventory: &Value,
    routes: &[Range],
) -> Result<()> {
    policy_subnet(subnet)?;
    if policy_gateway(subnet)? != gateway {
        return Err(err("BUNDLE_INVALID"));
    }
    let networks = inventory
        .as_array()
        .ok_or_else(|| err("ENGINE_OUTPUT_INVALID"))?;
    if networks.is_empty() || networks.len() > 4096 {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let expected = format!("{}_default", m.project_name);
    let mut others = Vec::new();
    let mut own = 0;
    for network in networks {
        if network["Name"] == expected {
            own += 1;
            if own > 1
                || network["Labels"]["com.exhibitos.bundle"] != m.bundle_id
                || network["Labels"]["com.exhibitos.project"] != m.project_name
                || network["Labels"]["com.exhibitos.schema"] != m.schema_version
                || network["Driver"] != "bridge"
                || network["IPAM"]["Driver"] != "default"
                || !(network["IPAM"]["Options"].is_null() || network["IPAM"]["Options"].as_object().is_some_and(|o|o.is_empty()))
                || network["Internal"] != false
                || network["Scope"] != "local"
                || network["Options"].as_object().is_none_or(|o|!o.is_empty())
                || !network["Id"].as_str().is_some_and(hash_valid)
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            verify_observed(network, subnet)?;
            if network["IPAM"]["Config"][0]["Gateway"] != gateway {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
        } else {
            others.push(network.clone());
        }
    }
    let mut blocked = if others.is_empty() {
        Vec::new()
    } else {
        engine_ranges(&Value::Array(others))?
    };
    blocked.extend_from_slice(routes);
    let candidate = Range::cidr(subnet, false)?;
    if blocked.iter().any(|r| candidate.overlaps(*r)) {
        return Err(err("ENGINE_NETWORK_CAPACITY"));
    }
    Ok(())
}
#[cfg(test)]
mod owned_policy_tests {
    use super::*;
    #[test]
    fn explicit_network_excludes_only_exact_owned_identity_and_preserves_host_routes() {
        let m = super::super::tests::manifest_for_detection();
        let subnet = "10.240.0.0/28";
        let gateway = "10.240.0.1";
        let own = serde_json::json!({"Id":"a".repeat(64),"Name":format!("{}_default",m.project_name),"Driver":"bridge","Internal":false,"Scope":"local","Options":{},"EnableIPv6":false,"Labels":{"com.exhibitos.bundle":m.bundle_id,"com.exhibitos.project":m.project_name,"com.exhibitos.schema":m.schema_version},"IPAM":{"Driver":"default","Options":null,"Config":[{"Subnet":subnet,"Gateway":gateway}]}});
        assert!(
            admit_owned_inventory(&m, subnet, gateway, &serde_json::json!([own.clone()]), &[])
                .is_ok()
        );
        assert_eq!(
            admit_owned_inventory(
                &m,
                subnet,
                gateway,
                &serde_json::json!([own.clone()]),
                &[Range::cidr("10/8", true).unwrap()]
            )
            .unwrap_err()
            .code,
            "ENGINE_NETWORK_CAPACITY"
        );
        for field in ["Id", "Driver", "Internal"] {
            let mut v = own.clone();
            v[field] = Value::Null;
            assert!(
                admit_owned_inventory(&m, subnet, gateway, &serde_json::json!([v]), &[]).is_err()
            );
        }
        let mut v = own.clone();
        v["Labels"]["com.exhibitos.bundle"] = serde_json::json!("foreign");
        assert_eq!(
            admit_owned_inventory(&m, subnet, gateway, &serde_json::json!([v]), &[])
                .unwrap_err()
                .code,
            "OWNERSHIP_CONFLICT"
        );
        let mut v = own.clone();
        v["Name"] = serde_json::json!("foreign");
        assert_eq!(
            admit_owned_inventory(&m, subnet, gateway, &serde_json::json!([own, v]), &[])
                .unwrap_err()
                .code,
            "ENGINE_NETWORK_CAPACITY"
        );
    }
}

#[cfg(any(target_os="linux",test))]
fn linux_owned_routes(bytes:&[u8],subnet:&str,own_id:Option<&str>)->Result<Vec<Range>> {
 policy_subnet(subnet)?;
 let value:Value=serde_json::from_slice(bytes).map_err(|_|err("ENGINE_OUTPUT_INVALID"))?;let routes=value.as_array().ok_or_else(||err("ENGINE_OUTPUT_INVALID"))?;
 if routes.is_empty()||routes.len()>16384{return Err(err("ENGINE_OUTPUT_INVALID"));}
 let dev=own_id.filter(|id|hash_valid(id)).map(|id|format!("br-{}",&id[..12]));let mut out=Vec::new();let range=Range::cidr(subnet,false)?;
 let network=Ipv4Addr::from(range.start).to_string();let broadcast=Ipv4Addr::from(range.end).to_string();let gateway=policy_gateway(subnet)?;
 let address_is=|dst:&str,expected:&str|dst==expected||dst==format!("{expected}/32");
 for route in routes {
  let dst=route["dst"].as_str().ok_or_else(||err("ENGINE_OUTPUT_INVALID"))?;if dst=="default"{continue;}
  let own_kernel=dev.as_ref().is_some_and(|d|route["dev"]==*d)&&route["protocol"]=="kernel";
  let main=route.get("table").is_none()||route["table"]=="main"||route["table"]==254;
  let local=route["table"]=="local"||route["table"]==255;
  let unicast=route.get("type").is_none()||route["type"]=="unicast";
  let subnet_route=dst==subnet&&main&&unicast&&route["scope"]=="link";
  let gateway_route=address_is(dst,&gateway)&&local&&route["type"]=="local"&&route["scope"]=="host";
  let broadcast_route=(address_is(dst,&network)||address_is(dst,&broadcast))&&local&&route["type"]=="broadcast"&&route["scope"]=="link";
  if own_kernel&&(subnet_route||gateway_route||broadcast_route){continue;}
  out.push(Range::cidr(dst,false)?);
 }Ok(out)
}
fn host_routes_for_owned(m:&BundleManifest,subnet:&str,inventory:&Value)->Result<Vec<Range>> {
 #[cfg(target_os="linux")] {
  let name=format!("{}_default",m.project_name);let own=inventory.as_array().and_then(|v|v.iter().find(|n|n["Name"]==name)).and_then(|n|n["Id"].as_str());
  let bytes=run("ip",&["-j".into(),"-4".into(),"route".into(),"show".into(),"table".into(),"all".into()],None,30)?;return linux_owned_routes(&bytes,subnet,own);
 }
 #[cfg(not(target_os="linux"))] {let _=(m,subnet,inventory);host_routes()}
}
#[cfg(test)]mod own_host_routes_tests {
 use super::*;
 #[test]fn exact_kernel_own_bridge_route_only_is_excluded(){
  let id="a".repeat(64);let v=serde_json::json!([{ "dst":"10.240.0.0/28","dev":"br-aaaaaaaaaaaa","protocol":"kernel","scope":"link"}]);let bytes=serde_json::to_vec(&v).unwrap();assert!(linux_owned_routes(&bytes,"10.240.0.0/28",Some(&id)).unwrap().is_empty());
  assert_eq!(linux_owned_routes(&bytes,"10.240.0.0/28",None).unwrap().len(),1);
  for (key,val) in [("dev","vpn0"),("protocol","static"),("scope","global")]{let mut b=v.clone();b[0][key]=serde_json::json!(val);assert_eq!(linux_owned_routes(&serde_json::to_vec(&b).unwrap(),"10.240.0.0/28",Some(&id)).unwrap().len(),1);}
 }
}

#[cfg(test)]mod local_table_route_tests {
 use super::*;
 #[test]fn owned_kernel_local_and_broadcast_are_exact_bounded_exemptions(){
  let id="a".repeat(64);let routes=serde_json::json!([
   {"dst":"10.240.0.0/28","dev":"br-aaaaaaaaaaaa","protocol":"kernel","scope":"link"},
   {"dst":"10.240.0.1","dev":"br-aaaaaaaaaaaa","protocol":"kernel","scope":"host","type":"local","table":"local"},
   {"dst":"10.240.0.0","dev":"br-aaaaaaaaaaaa","protocol":"kernel","scope":"link","type":"broadcast","table":"local"},
   {"dst":"10.240.0.15/32","dev":"br-aaaaaaaaaaaa","protocol":"kernel","scope":"link","type":"broadcast","table":255}]);
  assert!(linux_owned_routes(&serde_json::to_vec(&routes).unwrap(),"10.240.0.0/28",Some(&id)).unwrap().is_empty());
  for (index,key,value) in [(1,"dst",serde_json::json!("10.240.0.2")),(1,"table",serde_json::json!("main")),(1,"scope",serde_json::json!("link")),(1,"type",serde_json::json!("unicast")),(2,"dev",serde_json::json!("vpn0")),(3,"protocol",serde_json::json!("static")),(3,"dst",serde_json::json!("10.240.0.14"))] {let mut v=routes.clone();v[index][key]=value;assert_eq!(linux_owned_routes(&serde_json::to_vec(&v).unwrap(),"10.240.0.0/28",Some(&id)).unwrap().len(),1);}
  assert_eq!(linux_owned_routes(&serde_json::to_vec(&routes).unwrap(),"10.240.0.0/28",None).unwrap().len(),4);
 }
}
