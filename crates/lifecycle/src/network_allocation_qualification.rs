// SPDX-License-Identifier: Apache-2.0
//! Developer qualification only; production restoration does not yet use this allocator.
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
#[ignore = "explicit macOS local Docker empty private-network allocation qualification; production restoration unchanged"]
fn actual_bounded_network_create_inspect_remove() {
    let ids=run("docker",&["network".into(),"ls".into(),"--no-trunc".into(),"--format".into(),"{{.ID}}".into()],None,30).unwrap();
    let ids=std::str::from_utf8(&ids).unwrap().lines().map(str::to_owned).collect::<Vec<_>>();
    assert!(!ids.is_empty() && ids.len()<=4096 && ids.iter().all(|id|hash_valid(id)));
    let mut args=vec!["network".into(),"inspect".into()];args.extend(ids);
    let inventory:Value=serde_json::from_slice(&run("docker",&args,None,30).unwrap()).unwrap();
    let mut blocked=engine_ranges(&inventory).unwrap();
    blocked.extend(mac_routes(&run("/usr/sbin/netstat",&["-rn".into(),"-f".into(),"inet".into()],None,30).unwrap()).unwrap());
    let subnet=select(&blocked).unwrap();
    let name=format!("exhibitos-private-allocation-probe-{}",Uuid::new_v4());
    let output=run("docker",&["network".into(),"create".into(),"--driver".into(),"bridge".into(),"--subnet".into(),subnet.clone(),"--label".into(),"com.exhibitos.probe=private-allocation".into(),name.clone()],None,30).unwrap();
    let id=std::str::from_utf8(&output).unwrap().trim();assert!(hash_valid(id));
    let net=backup_creation::inspected("docker",&["network".into(),"inspect".into(),id.into()]).unwrap();
    // Only delete this newly created, exactly identified empty probe. Never delete any preexisting network.
    assert_eq!(net["Name"],name);assert_eq!(net["Id"],id);assert_eq!(net["Driver"],"bridge");
    assert_eq!(net["Labels"]["com.exhibitos.probe"],"private-allocation");
    assert!(net["Containers"].as_object().unwrap().is_empty());
    run("docker",&["network".into(),"rm".into(),id.into()],None,30).unwrap();
    assert_eq!(net["IPAM"]["Config"][0]["Subnet"],subnet);
    let after: Value=serde_json::from_slice(&run("docker",&args,None,30).unwrap()).unwrap();
    assert_eq!(inventory,after,"preexisting network metadata changed during qualification");
    println!("PASS_BOUNDED_PRIVATE_NETWORK_CREATE_INSPECT_REMOVE_NO_EXISTING_NETWORK_MUTATION");
}
