// SPDX-License-Identifier: Apache-2.0
//! Closed wire for the dedicated local model adoption socket.

use crate::{Result, err, hash_valid};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) const MAX_FRAME: usize = 8192;
pub(crate) const MAX_AGE_MS: u64 = 15 * 60 * 1000;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const MAX_ORIGINAL: u64 = 32 * 1024 * 1024;
const MAX_COMPACT: u64 = 8 * 1024 * 1024;

pub(crate) fn unavailable() -> crate::LifecycleError {
    err("NATIVE_MODEL_ADOPTION_UNAVAILABLE")
}

pub(crate) fn valid_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| {
        id.get_variant() == uuid::Variant::RFC4122
            && (1..=8).contains(&id.get_version_num())
            && id.to_string() == value
    })
}

pub(crate) fn refused(code: &str) -> crate::LifecycleError {
    match code {
        "FORBIDDEN"
        | "RIGHTS_DENIED"
        | "REMOTE_CONFLICT"
        | "REQUEST_CONFLICT"
        | "MODEL_ADOPTION_CANCELLED"
        | "MODEL_ADOPTION_TARGET_CHANGED"
        | "MODEL_ADOPTION_SOURCE_CHANGED" => err(code),
        _ => unavailable(),
    }
}

pub(crate) fn etag(value: &str) -> Option<u32> {
    let (revision, hash) = value
        .strip_prefix("\"studio-r")?
        .strip_suffix('"')?
        .split_once('-')?;
    let number = revision.parse::<u32>().ok()?;
    (number > 0 && number.to_string() == revision && hash_valid(hash)).then_some(number)
}

pub(crate) fn now_ms() -> Result<u64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable())?
        .as_millis();
    u64::try_from(now)
        .ok()
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(unavailable)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub(crate) workspace_id: String,
    pub(crate) tenant_id: String,
    pub(crate) principal_id: String,
    pub(crate) artwork_id: String,
    pub(crate) artwork_revision: u32,
    pub(crate) exhibition_id: String,
    pub(crate) placement_id: String,
    pub(crate) etag: String,
}

impl Target {
    pub(crate) fn valid(&self) -> bool {
        [
            &self.workspace_id,
            &self.tenant_id,
            &self.principal_id,
            &self.artwork_id,
            &self.exhibition_id,
            &self.placement_id,
        ]
        .into_iter()
        .all(|value| valid_id(value))
            && (1..=i32::MAX as u32).contains(&self.artwork_revision)
            && etag(&self.etag).is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) workspace_id: String,
    pub(crate) tenant_id: String,
    pub(crate) principal_id: String,
    pub(crate) artwork_id: String,
    pub(crate) artwork_revision: u32,
    pub(crate) artwork_revision_id: String,
}

impl Source {
    fn valid(&self) -> bool {
        [
            &self.workspace_id,
            &self.tenant_id,
            &self.principal_id,
            &self.artwork_id,
            &self.artwork_revision_id,
        ]
        .into_iter()
        .all(|value| valid_id(value))
            && (1..=i32::MAX as u32).contains(&self.artwork_revision)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

impl Blob {
    pub(crate) fn valid(&self) -> bool {
        hash_valid(&self.sha256) && (28..=MAX_ORIGINAL).contains(&self.bytes)
    }

    pub(crate) fn identity(&self) -> (u64, &str) {
        (self.bytes, &self.sha256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Grant {
    version: u8,
    pub(crate) operation_id: String,
    issued_at: u64,
    pub(crate) expires_at: u64,
    pub(crate) source: Source,
    pub(crate) target: Target,
    receipt_sha256: String,
    rights_sha256: String,
    rights_policy_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_credit_line: Option<String>,
    pub(crate) original: Blob,
    pub(crate) display: Blob,
}

impl Grant {
    pub(crate) fn check(&self, now: u64) -> Result<()> {
        if self.version != 1
            || !valid_id(&self.operation_id)
            || self.expires_at > MAX_SAFE_INTEGER
            || self.issued_at > now
            || self.expires_at <= now
            || self.expires_at - self.issued_at > MAX_AGE_MS
            || !self.source.valid()
            || !self.target.valid()
            || self.source.workspace_id == self.target.workspace_id
            || ![
                &self.receipt_sha256,
                &self.rights_sha256,
                &self.rights_policy_sha256,
            ]
            .into_iter()
            .all(|value| hash_valid(value))
            || self.source_credit_line.as_deref().is_some_and(|credit| {
                // Match JavaScript trim without altering the authenticated bytes.
                credit.len() > 2048
                    || credit.bytes().any(|byte| byte.is_ascii_control())
                    || credit.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}').is_empty()
            })
            || !self.original.valid()
            || !self.display.valid()
            || if self.original.sha256 == self.display.sha256 {
                self.original.bytes != self.display.bytes
            } else {
                self.display.bytes > MAX_COMPACT
            }
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(crate) fn has_source_credit(&self) -> bool {
        self.source_credit_line.is_some()
    }

    pub(crate) fn digest(&self) -> Result<String> {
        // Sort keys recursively to match Platform's canonicalModelValue.
        // Attribution is hashed as its exact UTF-8 content.
        let mut canonical = serde_json::to_value(self).map_err(|_| unavailable())?;
        canonical.sort_all_objects();
        let bytes = serde_json::to_vec(&canonical).map_err(|_| unavailable())?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Outbound<'a, T> {
    version: u8,
    kind: &'static str,
    operation_id: &'a str,
    runtime_generation: &'a str,
    #[serde(flatten)]
    fields: T,
}

pub(crate) trait Request: Serialize {
    const KIND: &'static str;
}

pub(crate) fn request<T: Request>(
    operation_id: &str,
    runtime_generation: &str,
    fields: T,
) -> Result<Vec<u8>> {
    if !valid_id(operation_id) || !valid_id(runtime_generation) {
        return Err(unavailable());
    }
    let bytes = serde_json::to_vec(&Outbound {
        version: 1,
        kind: T::KIND,
        operation_id,
        runtime_generation,
        fields,
    })
    .map_err(|_| unavailable())?;
    if bytes.len() > MAX_FRAME {
        return Err(unavailable());
    }
    Ok(bytes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageSource<'a> {
    pub(crate) source_artwork_id: &'a str,
    pub(crate) source_artwork_revision: u32,
    pub(crate) target: &'a Target,
    pub(crate) expires_at: u64,
}
impl Request for StageSource<'_> {
    const KIND: &'static str = "adoption-stage-source";
}

#[derive(Serialize)]
pub(crate) struct PreviewTarget<'a> {
    pub(crate) grant: &'a Grant,
}
impl Request for PreviewTarget<'_> {
    const KIND: &'static str = "adoption-preview-target";
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommitTarget<'a> {
    pub(crate) grant_sha256: &'a str,
    pub(crate) grant: &'a Grant,
}
impl Request for CommitTarget<'_> {
    const KIND: &'static str = "adoption-commit-target";
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusTarget<'a> {
    pub(crate) grant_sha256: &'a str,
}
impl Request for StatusTarget<'_> {
    const KIND: &'static str = "adoption-status-target";
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisposeTarget<'a> {
    pub(crate) grant_sha256: &'a str,
    pub(crate) exhibition_id: &'a str,
}
impl Request for DisposeTarget<'_> {
    const KIND: &'static str = "adoption-dispose-target";
}

pub(crate) trait Reply: Serialize + DeserializeOwned {
    const KIND: &'static str;
    fn header(&self) -> (u8, &str, &str, &str);
}

macro_rules! reply {
    ($name:ident, $kind:literal, $($field:ident: $type:ty),* $(,)?) => {
        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub(crate) struct $name {
            version: u8,
            kind: String,
            operation_id: String,
            runtime_generation: String,
            $(pub(crate) $field: $type,)*
        }

        impl Reply for $name {
            const KIND: &'static str = $kind;
            fn header(&self) -> (u8, &str, &str, &str) {
                (self.version, &self.kind, &self.operation_id, &self.runtime_generation)
            }
        }
    };
}

reply!(SourceStaged, "adoption-source-staged", grant: Grant);
reply!(TargetPreview, "adoption-target-preview", grant_sha256: String, preview: Preview);
reply!(TargetCommitted, "adoption-target-committed", grant_sha256: String, revision: u32, etag: String);
reply!(TargetStatus, "adoption-target-status", grant_sha256: String, outcome: Outcome, revision: Option<u32>, etag: Option<String>);
reply!(TargetDisposition, "adoption-target-disposition", grant_sha256: String, exhibition_id: String, outcome: Outcome, revision: Option<u32>, etag: Option<String>);
reply!(Refused, "adoption-refused", code: String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Preview {
    pub(crate) artwork_id: String,
    pub(crate) artwork_revision: u32,
    pub(crate) exhibition_id: String,
    pub(crate) placement_id: String,
    pub(crate) etag: String,
    pub(crate) original: Blob,
    pub(crate) display: Blob,
}

impl Preview {
    pub(crate) fn matches(&self, grant: &Grant) -> bool {
        self.artwork_id == grant.target.artwork_id
            && self.artwork_revision == grant.target.artwork_revision
            && self.exhibition_id == grant.target.exhibition_id
            && self.placement_id == grant.target.placement_id
            && self.etag == grant.target.etag
            && self.original == grant.original
            && self.display == grant.display
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Outcome {
    Committed,
    Cancelled,
    Unresolved,
}

pub(crate) fn reply<T: Reply>(bytes: &[u8], operation: &str, generation: &str) -> Result<T> {
    let value: T = serde_json::from_slice(bytes).map_err(|_| unavailable())?;
    if value.header() != (1, T::KIND, operation, generation)
        || serde_json::to_vec(&value).map_err(|_| unavailable())? != bytes
    {
        return Err(unavailable());
    }
    Ok(value)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn fixture_id(value: u32) -> String {
        format!("00000000-0000-4000-8000-{value:012}")
    }

    pub(crate) fn fixture_grant(issued_at: u64, expires_at: u64) -> Grant {
        let id = fixture_id;
        serde_json::from_value(json!({
            "version":1,"operationId":id(1),"issuedAt":issued_at,"expiresAt":expires_at,
            "source":{"workspaceId":id(9),"tenantId":id(10),"principalId":id(11),"artworkId":id(12),
                "artworkRevision":1,"artworkRevisionId":id(13)},
            "target":{"workspaceId":id(2),"tenantId":id(3),"principalId":id(4),"artworkId":id(5),
                "artworkRevision":6,"exhibitionId":id(7),"placementId":id(8),"etag":format!("\"studio-r7-{}\"", "e".repeat(64))},
            "receiptSha256":"a".repeat(64),"rightsSha256":"b".repeat(64),"rightsPolicySha256":"c".repeat(64),
            "original":{"sha256":"d".repeat(64),"bytes":32},"display":{"sha256":"e".repeat(64),"bytes":48}
        })).unwrap()
    }

    #[test]
    fn matches_platform_canonical_digest_and_closed_grant() {
        let grant = fixture_grant(1000, 2000);
        assert!(grant.check(1500).is_ok());
        assert!(grant.check(2000).is_err());
        assert_eq!(
            grant.digest().unwrap(),
            "6aa6f66d8dad05d67088ee424af545be93bffe42f8241a8914c8ae7cd8c2697d"
        );
        let mut missing = serde_json::to_value(&grant).unwrap();
        missing
            .as_object_mut()
            .unwrap()
            .remove("rightsPolicySha256");
        assert!(serde_json::from_value::<Grant>(missing).is_err());
        let mut changed = grant;
        changed.rights_policy_sha256 = "f".repeat(64);
        assert_ne!(
            changed.digest().unwrap(),
            "6aa6f66d8dad05d67088ee424af545be93bffe42f8241a8914c8ae7cd8c2697d"
        );
    }

    #[test]
    fn source_credit_preserves_platform_utf8_digest_and_refuses_invalid_text() {
        let mut grant = fixture_grant(1000, 2000);
        let credit = "  Épreuve • Source  ";
        grant.source_credit_line = Some(credit.into());
        assert!(grant.check(1500).is_ok());
        assert_eq!(serde_json::to_value(&grant).unwrap()["sourceCreditLine"], credit);
        assert_eq!(grant.digest().unwrap(),
            "4e6ef2880555f607b2b5cf8962cd121caeaf3600acb3dd3bde58b644ab7a7721");
        for invalid in [" \u{feff}".into(), "credit\nline".into(), "\u{7f}".into(), "é".repeat(1025)] {
            grant.source_credit_line = Some(invalid);
            assert!(grant.check(1500).is_err());
        }
    }

    #[test]
    fn frames_require_exact_order_and_generation() {
        let grant = fixture_grant(1000, 2000);
        let generation = fixture_id(21);
        let digest = grant.digest().unwrap();
        let commit = request(
            &grant.operation_id,
            &generation,
            CommitTarget {
                grant_sha256: &digest,
                grant: &grant,
            },
        )
        .unwrap();
        let text = String::from_utf8(commit).unwrap();
        assert!(text.starts_with(&format!(
            "{{\"version\":1,\"kind\":\"adoption-commit-target\",\"operationId\":\"{}\",\"runtimeGeneration\":\"{generation}\",\"grantSha256\":\"{digest}\",\"grant\":",
            grant.operation_id
        )));
        let response = TargetStatus {
            version: 1,
            kind: "adoption-target-status".into(),
            operation_id: grant.operation_id.clone(),
            runtime_generation: generation.clone(),
            grant_sha256: digest,
            outcome: Outcome::Unresolved,
            revision: None,
            etag: None,
        };
        let raw = serde_json::to_vec(&response).unwrap();
        assert!(reply::<TargetStatus>(&raw, &grant.operation_id, &generation).is_ok());
        assert!(reply::<TargetStatus>(&raw, &grant.operation_id, &fixture_id(22)).is_err());
        let reordered = serde_json::to_vec(&serde_json::to_value(&response).unwrap()).unwrap();
        assert!(reply::<TargetStatus>(&reordered, &grant.operation_id, &generation).is_err());
    }
}
