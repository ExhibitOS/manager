// SPDX-License-Identifier: Apache-2.0
//! Closed wire contract for the selected Hayes pre-OPEN terminal operation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{digest, err, hash_valid, Result};

fn unconfirmed() -> crate::LifecycleError {
    err("HAYES_PREOPEN_TERMINAL_UNCONFIRMED")
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    HayesPreopenTerminalChallenge,
    HayesPreopenTerminalRecorded,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Challenge {
    version: u8,
    kind: Kind,
    #[serde(deserialize_with = "deserialize_id")]
    job_id: Uuid,
    proof_sha256: String,
    #[serde(deserialize_with = "deserialize_id")]
    challenge: Uuid,
    #[serde(deserialize_with = "deserialize_id")]
    native_owner_id: Uuid,
    #[serde(deserialize_with = "deserialize_id")]
    runtime_generation: Uuid,
    runtime_manifest_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    PreopenCancelled,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recorded {
    version: u8,
    kind: Kind,
    #[serde(deserialize_with = "deserialize_id")]
    challenge: Uuid,
    #[serde(deserialize_with = "deserialize_id")]
    job_id: Uuid,
    #[serde(deserialize_with = "deserialize_id")]
    attempt_id: Uuid,
    proof_sha256: String,
    outcome: Outcome,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
    #[serde(deserialize_with = "deserialize_id")]
    job_id: Uuid,
    #[serde(deserialize_with = "deserialize_id")]
    attempt_id: Uuid,
    proof_sha256: String,
    outcome: Outcome,
}

pub(super) struct Proof<'a> {
    observation: &'a Value,
    pub(super) job_id: Uuid,
    attempt_id: Uuid,
    pub(super) sha256: &'a str,
}

impl<'a> Proof<'a> {
    /// The caller must supply the authenticated read-only observation and the
    /// digest sealed by admission, never an arbitrary client-authored value.
    pub(super) fn checked(observation: &'a Value, sha256: &'a str) -> Result<Self> {
        let mut canonical = observation.clone();
        canonical.sort_all_objects();
        let canonical = serde_json::to_vec(&canonical).map_err(|_| unconfirmed())?;
        if canonical.len() > 4096 || digest(&canonical) != sha256
            || observation["version"] != 1
            || observation["kind"] != "hayes-preopen-claim-observation" {
            return Err(unconfirmed());
        }
        Ok(Self {
            observation,
            job_id: observation["jobId"].as_str().and_then(canonical_id).ok_or_else(unconfirmed)?,
            attempt_id: observation["claim"]["attemptId"].as_str().and_then(canonical_id).ok_or_else(unconfirmed)?,
            sha256,
        })
    }

    pub(super) fn answer(&self, bytes: &[u8], owner: Uuid, generation: Uuid, manifest: &str) -> Result<(Uuid, Vec<u8>)> {
        let challenge: Challenge = parse(bytes)?;
        if challenge.version != 1 || challenge.kind != Kind::HayesPreopenTerminalChallenge
            || challenge.job_id != self.job_id || challenge.proof_sha256 != self.sha256
            || challenge.native_owner_id != owner || challenge.runtime_generation != generation
            || challenge.runtime_manifest_sha256 != manifest || !hash_valid(manifest)
        {
            return Err(unconfirmed());
        }
        let response = serde_json::to_vec(&serde_json::json!({
            "version": 1, "kind": "hayes-preopen-terminal-proof",
            "jobId": self.job_id, "proofSha256": self.sha256,
            "challenge": challenge.challenge, "nativeOwnerId": owner,
            "runtimeGeneration": generation, "runtimeManifestSha256": manifest,
            "observation": self.observation,
        })).map_err(|_| unconfirmed())?;
        Ok((challenge.challenge, response))
    }

    pub(super) fn recorded(&self, bytes: &[u8], challenge: Uuid) -> Result<Receipt> {
        let recorded: Recorded = parse(bytes)?;
        if recorded.version != 1 || recorded.kind != Kind::HayesPreopenTerminalRecorded
            || recorded.challenge != challenge {
            return Err(unconfirmed());
        }
        self.match_receipt(Receipt { job_id: recorded.job_id, attempt_id: recorded.attempt_id,
            proof_sha256: recorded.proof_sha256, outcome: recorded.outcome })
    }

    pub(super) fn readback(&self, bytes: &[u8]) -> Result<Receipt> {
        self.match_receipt(parse(bytes)?)
    }

    fn match_receipt(&self, receipt: Receipt) -> Result<Receipt> {
        if receipt.job_id != self.job_id || receipt.attempt_id != self.attempt_id
            || receipt.proof_sha256 != self.sha256 {
            return Err(unconfirmed());
        }
        Ok(receipt)
    }
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > 8192 { return Err(unconfirmed()); }
    serde_json::from_slice(bytes).map_err(|_| unconfirmed())
}

fn canonical_id(raw: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(raw).ok()?;
    (id.get_variant() == uuid::Variant::RFC4122 && (1..=8).contains(&id.get_version_num())
        && id.to_string() == raw).then_some(id)
}

fn deserialize_id<'de, D: serde::Deserializer<'de>>(input: D) -> std::result::Result<Uuid, D::Error> {
    let raw = String::deserialize(input)?;
    canonical_id(&raw).ok_or_else(|| serde::de::Error::custom("invalid canonical UUID"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_one_selected_challenge_and_both_receipt_surfaces() {
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let challenge = Uuid::new_v4();
        let manifest = "a".repeat(64);
        let observation = json!({"version":1,"kind":"hayes-preopen-claim-observation",
            "jobId":job,"claim":{"attemptId":attempt}});
        let mut canonical = observation.clone();
        canonical.sort_all_objects();
        let sha = digest(&serde_json::to_vec(&canonical).unwrap());
        let proof = Proof::checked(&observation, &sha).unwrap();
        assert!(Proof::checked(&observation, &manifest).is_err());
        let frame = json!({"version":1,"kind":"hayes-preopen-terminal-challenge",
            "jobId":job,"proofSha256":sha,"challenge":challenge,"nativeOwnerId":owner,
            "runtimeGeneration":generation,"runtimeManifestSha256":manifest});
        let raw = serde_json::to_vec(&frame).unwrap();
        let (observed, response) = proof.answer(&raw, owner, generation, &manifest).unwrap();
        assert_eq!(observed, challenge);
        let response: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(response["kind"], "hayes-preopen-terminal-proof");
        assert_eq!(response["observation"], observation);
        assert!(proof.answer(&raw, Uuid::new_v4(), generation, &manifest).is_err());
        assert!(proof.answer(&raw, owner, Uuid::new_v4(), &manifest).is_err());
        let mut noncanonical = frame;
        noncanonical["challenge"] = json!(challenge.simple().to_string());
        assert!(proof.answer(&serde_json::to_vec(&noncanonical).unwrap(), owner, generation, &manifest).is_err());

        let mut receipt = json!({"jobId":job,"attemptId":attempt,"proofSha256":sha,
            "outcome":"preopen-cancelled"});
        let readback = proof.readback(&serde_json::to_vec(&receipt).unwrap()).unwrap();
        let mut ack = receipt.clone();
        ack["version"] = json!(1);
        ack["kind"] = json!("hayes-preopen-terminal-recorded");
        ack["challenge"] = json!(challenge);
        assert_eq!(proof.recorded(&serde_json::to_vec(&ack).unwrap(), challenge).unwrap(), readback);
        assert!(proof.recorded(&serde_json::to_vec(&ack).unwrap(), Uuid::new_v4()).is_err());
        receipt["attemptId"] = json!(Uuid::new_v4());
        assert!(proof.readback(&serde_json::to_vec(&receipt).unwrap()).is_err());
        receipt["attemptId"] = json!(attempt);
        receipt["extra"] = json!(true);
        assert!(proof.readback(&serde_json::to_vec(&receipt).unwrap()).is_err());
    }
}
