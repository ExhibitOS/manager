// SPDX-License-Identifier: Apache-2.0
//! Inactive selected-import binding encoding and system MAC primitive. No
//! live secret handling, MAC verification, stage seal or authority exists here.
use super::*;

const DOMAIN:&[u8]=b"ExhibitOS/OEX-import/manager-binding/v1\0";

#[allow(dead_code)]
#[derive(Clone,Copy)]
pub(super) struct Fields<'a> {
    pub workspace:&'a str, pub tenant:&'a str, pub principal:&'a str, pub runtime_generation:&'a str,
    pub job:&'a str, pub attempt:&'a str, pub generation:&'a str, pub stage:&'a str,
    pub reservation:&'a str, pub reservation_generation:&'a str,
    /// Digest of the exact persisted preclaim bytes, treated as opaque bytes;
    /// do not reproduce Node's canonical JSON serialization in Rust.
    pub claim_digest:&'a str,
    /// Digest of the actual independently verified provider receipt.
    pub receipt_digest:&'a str,
}

fn decode_hash(value:&str)->Result<[u8;32]> {
    if !hash_valid(value) {return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));}
    let bytes=value.as_bytes();let mut result=[0u8;32];
    let nibble=|b:u8|->u8 {if b<=b'9' {b-b'0'} else {b-b'a'+10}};
    for i in 0..32 {result[i]=(nibble(bytes[2*i])<<4)|nibble(bytes[2*i+1]);}
    Ok(result)
}

#[allow(dead_code)]
impl Fields<'_> {
    /// Encode public correlation inputs only. The caller must independently
    /// verify their live source and maintain the same held fence; the stage
    /// device/inode is separately correlated later under that same fence.
    pub(super) fn encode(&self)->Result<Vec<u8>> {
        let mut bytes=Vec::with_capacity(DOMAIN.len()+160+64);bytes.extend_from_slice(DOMAIN);
        for value in [self.workspace,self.tenant,self.principal,self.runtime_generation,self.job,
            self.attempt,self.generation,self.stage,self.reservation,self.reservation_generation] {
            let id=Uuid::parse_str(value).map_err(|_|err("NATIVE_OEX_ATTEMPT_INVALID"))?;
            if id.to_string()!=value || id.get_variant()!=uuid::Variant::RFC4122 || !(1..=8).contains(&id.get_version_num()) {
                return Err(err("NATIVE_OEX_ATTEMPT_INVALID"));
            }
            bytes.extend_from_slice(id.as_bytes());
        }
        bytes.extend_from_slice(&decode_hash(self.claim_digest)?);
        bytes.extend_from_slice(&decode_hash(self.receipt_digest)?);
        Ok(bytes)
    }
}

/// macOS system HMAC for this fixed domain and typed field set only. The
/// caller owns the ephemeral raw key and must preserve the live Session and
/// socket; this value is correlation, not authority. No logging or erasure
/// guarantee is made here. This helper is inactive and not a verifier.
#[cfg(target_os="macos")]
#[allow(dead_code)]
pub(super) fn correlation_mac(key:&[u8;32],fields:&Fields<'_>)->Result<[u8;32]> {
    #[link(name="System")]
    unsafe extern "C" {
        fn CCHmac(algorithm:u32,key:*const libc::c_void,key_length:usize,
            data:*const libc::c_void,data_length:usize,out:*mut libc::c_void);
    }
    let message=fields.encode()?;let mut output=[0u8;32];
    // CommonCrypto kCCHmacAlgSHA256=2; all input and output buffers remain
    // valid for the duration of this synchronous system-library call.
    unsafe {CCHmac(2,key.as_ptr().cast(),key.len(),message.as_ptr().cast(),message.len(),output.as_mut_ptr().cast());}
    Ok(output)
}

#[cfg(not(target_os="macos"))]
#[allow(dead_code)]
pub(super) fn correlation_mac(_key:&[u8;32],_fields:&Fields<'_>)->Result<[u8;32]> {
    Err(err("NATIVE_OEX_WORKER_UNAVAILABLE"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_network_order_public_message_vector() {
        let ids:Vec<String>=(1..=10).map(|i|format!("10000000-0000-4000-8000-{i:012}")).collect();
        let fields=Fields{workspace:&ids[0],tenant:&ids[1],principal:&ids[2],runtime_generation:&ids[3],
            job:&ids[4],attempt:&ids[5],generation:&ids[6],stage:&ids[7],reservation:&ids[8],reservation_generation:&ids[9],
            claim_digest:"202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f",
            receipt_digest:"404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f"};
        let message=fields.encode().unwrap();assert_eq!(message.len(),264);
        assert_eq!(digest(&message),"637abbfa8a464fea4738b1b1e2268a3dc251190d51fff85995d5b002a984d41c");
        #[cfg(target_os="macos")]
        {
            let key=std::array::from_fn(|i|i as u8);let mac=correlation_mac(&key,&fields).unwrap();
            assert_eq!(mac.iter().map(|v|format!("{v:02x}")).collect::<String>(),"9619007a47a688c544840f87c8ea25f683c3c16cafcb5990d22ff3623ade513a");
            let mut changed=key;changed[0]^=1;assert_ne!(correlation_mac(&changed,&fields).unwrap(),mac);
        }
        let swapped=Fields{workspace:&ids[1],tenant:&ids[0],..fields};assert_ne!(digest(&swapped.encode().unwrap()),digest(&message));
        let invalid=Fields{claim_digest:"a",..fields};assert!(invalid.encode().is_err());
        let uppercase="A".repeat(64);let invalid=Fields{claim_digest:&uppercase,..fields};assert!(invalid.encode().is_err());
        let invalid=Fields{workspace:"10000000-0000-4000-8000-00000000000A",..fields};assert!(invalid.encode().is_err());
    }
}
