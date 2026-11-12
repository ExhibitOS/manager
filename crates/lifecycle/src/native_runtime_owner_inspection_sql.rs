// SPDX-License-Identifier: Apache-2.0
//! Fixed SQL45 inspection transaction and bounded diagnostic projection.
//! This module grants no lifecycle admission and starts no process. The caller
//! must retain the authenticated physical lease and one owned PG connection.
use super::*;

pub(super) const ERROR: &str = "NATIVE_OWNER_INSPECTION_INVALID";
const KINDS: [&str; 4] = ["furnishing-intake", "model-preparation", "image-preparation", "material-intake"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Owner {
    pub workspace_id: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub owner_uid: u32,
}
impl Owner {
    fn check(&self) -> Result<()> {
        if self.owner_uid == 0 || [&self.workspace_id, &self.tenant_id, &self.principal_id]
            .iter().any(|id| !installations::uuid(id) || id.to_lowercase() != **id) {
            return Err(err(ERROR));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Database {
    pub system_identifier: String,
    pub database_oid: String,
}
impl Database {
    fn check(&self) -> Result<()> {
        fn positive(value: &str) -> bool {
            value.parse::<u64>().is_ok_and(|n| n > 0 && n.to_string() == value)
        }
        if !positive(&self.system_identifier) || !positive(&self.database_oid)
            || self.database_oid.parse::<u32>().is_err() { return Err(err(ERROR)); }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Outstanding {
    pub kind: String,
    /// None is the explicit redacted/invalid/unbound generation, never an empty set.
    pub generation: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Observation {
    version: u32,
    owner: Owner,
    database: Database,
    owner_matches: bool,
    membership_valid: bool,
    catalogue_exact: bool,
    pub global_idle: bool,
    inspected_kinds: Vec<String>,
    hayes_inspected: bool,
    pub outstanding: Vec<Outstanding>,
    pub truncated: bool,
}
impl Observation {
    pub(super) fn parse(bytes: &[u8], owner: &Owner, database: &Database) -> Result<Self> {
        owner.check()?; database.check()?;
        if bytes.is_empty() || bytes.len() > 16384 { return Err(err(ERROR)); }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| err(ERROR))?;
        let claims = raw.get("outstanding").and_then(Value::as_array).ok_or_else(|| err(ERROR))?;
        for claim in claims {
            let fields = claim.as_object().ok_or_else(|| err(ERROR))?;
            if fields.len() != 2 || !fields.contains_key("kind") || !fields.contains_key("generation") {
                return Err(err(ERROR));
            }
        }
        let value: Self = serde_json::from_value(raw).map_err(|_| err(ERROR))?;
        if value.version != 1 || value.owner != *owner || value.database != *database
            || !value.owner_matches || !value.membership_valid || !value.catalogue_exact
            || value.inspected_kinds != KINDS || value.hayes_inspected || value.outstanding.len() > 20
            || (value.truncated && value.outstanding.len() != 20)
            || (value.global_idle && (!value.outstanding.is_empty() || value.truncated)) { return Err(err(ERROR)); }
        let mut unique = std::collections::BTreeSet::new();
        for claim in &value.outstanding {
            if !KINDS.contains(&claim.kind.as_str())
                || claim.generation.as_ref().is_some_and(|id| !installations::uuid(id) || id.to_lowercase() != *id)
                || !unique.insert((claim.kind.clone(), claim.generation.clone())) { return Err(err(ERROR)); }
        }
        // globalIdle=false is a valid diagnostic, not a failed owner guard.
        // No field can assert a global committed generation or recovery readiness.
        Ok(value)
    }
}

/// One fixed stdin script for one `psql --no-psqlrc --quiet --tuples-only
/// --no-align --set=ON_ERROR_STOP=1 --file=-` connection. No caller SQL is accepted.
/// A guard failure closes the connection (implicit rollback); successful reads
/// explicitly ROLLBACK. This must never be run through a journal-mutating helper.
pub(super) fn transaction(owner: &Owner, database: &Database, ledger: &str) -> Result<String> {
    owner.check()?; database.check()?;
    let rows: Vec<_> = ledger.lines().map(|line| line.split_once('|').ok_or_else(|| err(ERROR)))
        .collect::<Result<_>>()?;
    if rows.len() != 45 || rows.iter().enumerate().any(|(i, (name, hash))| {
        !name.starts_with(&format!("{:03}_", i + 1)) || !name.ends_with(".sql")
            || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
            || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) { return Err(err(ERROR)); }
    let expected: Vec<_> = rows.iter().map(|(name, hash)| serde_json::json!({"name":name,"sha256":hash})).collect();
    let expected = serde_json::to_string(&expected).map_err(|_| err(ERROR))?;
    let owner_json = serde_json::to_string(owner).map_err(|_| err(ERROR))?;
    let kinds = serde_json::to_string(&KINDS).map_err(|_| err(ERROR))?;
    let owned = OWNED_GENERATIONS.replace("$1", &format!("'{}'", owner.workspace_id))
        .replace("$2", &format!("'{}'", owner.tenant_id)).replace("$3", &format!("'{}'", owner.principal_id));
    Ok(format!(r#"\set ON_ERROR_STOP on
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout='3000ms';
SET LOCAL lock_timeout='500ms';
SET LOCAL idle_in_transaction_session_timeout='8000ms';
SELECT pg_try_advisory_xact_lock(82003) AS acquired \gset
\if :acquired
\else
\quit 3
\endif
SELECT pg_try_advisory_xact_lock(82002) AS acquired \gset
\if :acquired
\else
\quit 3
\endif
SELECT (SELECT system_identifier::text FROM pg_control_system()) AS system_identifier,
 (SELECT oid::text FROM pg_database WHERE datname=current_database()) AS database_oid \gset
SELECT :'system_identifier'='{system}' AND :'database_oid'='{oid}' AS verified \gset
\if :verified
\else
\quit 3
\endif
SELECT count(*)=1 AS verified FROM local_workspaces
 WHERE id='{workspace}'::uuid AND tenant_id='{tenant}'::uuid AND user_id='{principal}'::uuid AND mac_uid={uid}::bigint \gset
\if :verified
\else
\quit 3
\endif
SELECT EXISTS (SELECT 1 FROM memberships m JOIN users u ON u.id=m.user_id JOIN tenants t ON t.id=m.tenant_id
 WHERE m.tenant_id='{tenant}'::uuid AND m.user_id='{principal}'::uuid AND m.role='admin' AND NOT u.disabled AND t.deleted_at IS NULL) AS verified \gset
\if :verified
\else
\quit 3
\endif
SELECT COALESCE(jsonb_agg(to_jsonb(ledger_rows) ORDER BY name),'[]'::jsonb)='{expected}'::jsonb AS verified
 FROM (SELECT name,sha256 FROM public.schema_migrations ORDER BY name LIMIT 46) ledger_rows \gset
\if :verified
\else
\quit 3
\endif
{idle} \gset
-- idle=false permits only the following bounded diagnostic SELECT, never admission.
WITH claims AS ({owned})
SELECT jsonb_build_object('version',1,'owner','{owner_json}'::jsonb,
 'database',jsonb_build_object('systemIdentifier',:'system_identifier','databaseOid',:'database_oid'),
 'ownerMatches',true,'membershipValid',true,'catalogueExact',true,'globalIdle',:'idle'::boolean,
 'inspectedKinds','{kinds}'::jsonb,'hayesInspected',false,
 'outstanding',COALESCE((SELECT jsonb_agg(jsonb_build_object('kind',kind,'generation',NULLIF(generation,'unbound')) ORDER BY kind,generation)
 FROM (SELECT kind,generation FROM claims ORDER BY kind,generation LIMIT 20) bounded),'[]'::jsonb),
 'truncated',(SELECT count(*)>20 FROM claims));
ROLLBACK;
"#, system=database.system_identifier, oid=database.database_oid, workspace=owner.workspace_id,
 tenant=owner.tenant_id, principal=owner.principal_id, uid=owner.owner_uid,
 idle=GLOBAL_IDLE, expected=expected, owner_json=owner_json, kinds=kinds, owned=owned))
}

/// A separate diagnostic mode: an observed OID is never an independent prior.
/// Strict transaction()/Observation::parse() remain unchanged and never fall back.
pub(super) fn observed_transaction(owner: &Owner, system: &str, nonce: &str, ledger: &str) -> Result<String> {
    if !installations::uuid(nonce) { return Err(err(ERROR)); }
    let placeholder = Database { system_identifier: system.into(), database_oid: "1".into() };
    let sql = transaction(owner, &placeholder, ledger)?;
    let strict = format!("SELECT :'system_identifier'='{system}' AND :'database_oid'='1' AS verified");
    let observed = format!("SELECT :'system_identifier'='{system}' AND current_setting('exhibitos.instance',true)='{nonce}' AND current_database()='exhibitos' AS verified");
    if sql.matches(&strict).count()!=1 { return Err(err(ERROR)); }
    Ok(sql.replacen(&strict,&observed,1))
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ClusterObservation {
    identity_mode: &'static str,
    cluster_system_id_matched: bool,
    expected_database_oid: Option<String>,
    observed_database_oid: String,
    database_oid_comparison: &'static str,
    logical_database_identity_qualified: bool,
    scope: &'static str,
    recovery_eligible: bool,
    admission_eligible: bool,
    generations_state: &'static str,
    hayes_inspection: &'static str,
    observation: Observation,
}
impl ClusterObservation {
    pub(super) fn parse(bytes: &[u8], owner: &Owner, system: &str) -> Result<Self> {
        if bytes.len()>16384 { return Err(err(ERROR)); }
        let raw: Value=serde_json::from_slice(bytes).map_err(|_|err(ERROR))?;
        let database: Database=serde_json::from_value(raw.get("database").cloned().ok_or_else(||err(ERROR))?).map_err(|_|err(ERROR))?;
        database.check()?;
        if database.system_identifier!=system {return Err(err(ERROR));}
        // Deny unknown/contradictory labels via the strict inner projection. The
        // observed OID here is used only for schema checking, never as a prior.
        let observation=Observation::parse(bytes,owner,&database)?;
        Ok(Self {identity_mode:"cluster-owner-observation",cluster_system_id_matched:true,
            expected_database_oid:None,observed_database_oid:database.database_oid,
            database_oid_comparison:"not-performed-no-independent-prior",logical_database_identity_qualified:false,
            scope:"observed current endpoint/database only; no continuity claim with prior profile database",
            recovery_eligible:false,admission_eligible:false,generations_state:"observed",hayes_inspection:"not-inspected",observation})
    }
}

const GLOBAL_IDLE: &str = r#"SELECT NOT EXISTS (SELECT 1 FROM workspace_furnishing_intakes WHERE worker_token IS NOT NULL OR phase IN ('uploading','validating','cancelling') UNION ALL SELECT 1 FROM workspace_material_intakes WHERE worker_token IS NOT NULL OR phase IN ('uploading','validating','cancelling') UNION ALL SELECT 1 FROM import_jobs WHERE state IN ('uploading','queued','processing') OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cardinality(png_worker_tokens)>0 OR model_preparation ? 'claim' OR derived ? 'claim' UNION ALL SELECT 1 FROM oex_import_jobs WHERE state IN ('uploading','queued','processing') OR lease_id IS NOT NULL OR lease_until IS NOT NULL OR cleanup_pending) AS idle"#;

const OWNED_GENERATIONS: &str = r#"WITH owned_claims(kind,generation) AS (SELECT 'furnishing-intake',worker_generation::text FROM workspace_furnishing_intakes WHERE workspace_id=$1::uuid AND tenant_id=$2::uuid AND user_id=$3::uuid AND worker_token IS NOT NULL UNION ALL SELECT 'model-preparation',CASE WHEN model_preparation#>>'{claim,workspaceId}'=$1::text AND model_preparation#>>'{claim,tenantId}'=$2::text AND model_preparation#>>'{claim,principalId}'=$3::text THEN model_preparation#>>'{claim,runtimeGeneration}' ELSE NULL END FROM import_jobs WHERE tenant_id=$2::uuid AND user_id=$3::uuid AND model_preparation ? 'claim' UNION ALL SELECT 'image-preparation',CASE WHEN derived#>>'{claim,workspaceId}'=$1::text AND derived#>>'{claim,tenantId}'=$2::text AND derived#>>'{claim,principalId}'=$3::text THEN derived#>>'{claim,runtimeGeneration}' ELSE NULL END FROM import_jobs WHERE tenant_id=$2::uuid AND user_id=$3::uuid AND derived ? 'claim' AND derived->>'workspaceId'=$1::text AND derived->>'principalId'=$3::text UNION ALL SELECT 'material-intake',NULL FROM workspace_material_intakes WHERE workspace_id=$1::uuid AND tenant_id=$2::uuid AND user_id=$3::uuid AND worker_token IS NOT NULL) SELECT DISTINCT kind,CASE WHEN generation ~ '^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$' THEN generation ELSE 'unbound' END AS generation FROM owned_claims ORDER BY kind,generation LIMIT 21"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> Owner {
        Owner { workspace_id: "11111111-1111-4111-8111-111111111111".into(),
            tenant_id: "22222222-2222-4222-8222-222222222222".into(),
            principal_id: "33333333-3333-4333-8333-333333333333".into(), owner_uid: 501 }
    }
    fn database() -> Database { Database { system_identifier: "7706486847612491309".into(), database_oid: "16384".into() } }
    fn ledger() -> String { (1..=45).map(|n| format!("{n:03}_synthetic.sql|{}", "a".repeat(64))).collect::<Vec<_>>().join("\n") }
    fn observation() -> Value {
        serde_json::json!({"version":1,"owner":owner(),"database":database(),"ownerMatches":true,
            "membershipValid":true,"catalogueExact":true,"globalIdle":true,
            "inspectedKinds":KINDS,"hayesInspected":false,"outstanding":[],"truncated":false})
    }
    fn parse(value: &Value) -> Result<Observation> {
        Observation::parse(&serde_json::to_vec(value).unwrap(), &owner(), &database())
    }

    #[test]
    fn owner_inspection_empty_four_kind_set_is_valid_without_inventing_global_generation() {
        let value = parse(&observation()).unwrap();
        assert!(value.global_idle && value.outstanding.is_empty() && !value.truncated);
        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(encoded["hayesInspected"], false);
        assert!(encoded.get("runtimeGeneration").is_none());
        assert!(encoded.get("recoveryReady").is_none());
        let mut forged = observation(); forged["runtimeGeneration"] = owner().workspace_id.into();
        assert!(parse(&forged).is_err());
    }

    #[test]
    fn owner_inspection_idle_false_retains_bounded_unbound_diagnostic_without_authority() {
        let mut value = observation(); value["globalIdle"] = false.into();
        value["outstanding"] = serde_json::json!([{"kind":"model-preparation","generation":null}]);
        let parsed = parse(&value).unwrap();
        assert!(!parsed.global_idle);
        assert_eq!(parsed.outstanding.len(), 1);
        assert!(parsed.outstanding[0].generation.is_none());
        value.as_object_mut().unwrap().remove("outstanding");
        assert!(parse(&value).is_err(), "Uninspected cannot become an empty set");
    }

    #[test]
    fn owner_inspection_refuses_wrong_owner_database_guard_and_unrecognized_projection() {
        for field in ["ownerMatches", "membershipValid", "catalogueExact"] {
            let mut value = observation(); value[field] = false.into(); assert!(parse(&value).is_err());
        }
        for field in ["workspaceId", "tenantId", "principalId"] {
            let mut value = observation(); value["owner"][field] = "44444444-4444-4444-8444-444444444444".into();
            assert!(parse(&value).is_err());
        }
        for field in ["systemIdentifier", "databaseOid"] {
            let mut value = observation(); value["database"][field] = "9".into(); assert!(parse(&value).is_err());
        }
        let mut value = observation(); value["owner"]["ownerUid"] = 502.into(); assert!(parse(&value).is_err());
        let mut value = observation(); value["hayesInspected"] = true.into(); assert!(parse(&value).is_err());
        let mut value = observation(); value["inspectedKinds"] = serde_json::json!([]); assert!(parse(&value).is_err());
        assert!(Observation::parse(&vec![b' '; 16385], &owner(), &database()).is_err());
    }

    #[test]
    fn owner_inspection_bounds_and_validates_each_projected_generation() {
        let mut value = observation(); value["globalIdle"] = false.into();
        for claim in [serde_json::json!({"kind":"model-preparation"}),
                      serde_json::json!({"kind":"hayes","generation":null}),
                      serde_json::json!({"kind":"model-preparation","generation":"not-a-uuid"}),
                      serde_json::json!({"kind":"model-preparation","generation":null,"token":"secret"})] {
            value["outstanding"] = serde_json::json!([claim]); assert!(parse(&value).is_err());
        }
        value["outstanding"] = serde_json::json!([{"kind":"model-preparation","generation":null},
            {"kind":"model-preparation","generation":null}]); assert!(parse(&value).is_err());
        let claims: Vec<_> = (1..=21).map(|n| serde_json::json!({"kind":"model-preparation",
            "generation":format!("44444444-4444-4444-8444-{n:012x}")})).collect();
        value["outstanding"] = claims.clone().into(); value["truncated"] = true.into(); assert!(parse(&value).is_err());
        value["outstanding"] = claims[..20].to_vec().into(); assert!(parse(&value).unwrap().truncated);
        value["outstanding"] = claims[..19].to_vec().into(); assert!(parse(&value).is_err());
    }

    #[test]
    fn owner_inspection_refuses_idle_true_with_outstanding_claims() {
        let mut value = observation();
        value["outstanding"] = serde_json::json!([{"kind":"model-preparation","generation":null}]);
        assert!(parse(&value).is_err());
    }

    #[test]
    fn owner_inspection_transaction_rejects_sql_injection_and_nonexact_input_ledger() {
        assert!(transaction(&owner(), &database(), &ledger()).is_ok());
        let mut malicious = owner(); malicious.workspace_id = "'; DELETE FROM users; --".into();
        assert!(transaction(&malicious, &database(), &ledger()).is_err());
        let mut root = owner(); root.owner_uid = 0; assert!(transaction(&root, &database(), &ledger()).is_err());
        let mut bad_database = database(); bad_database.database_oid = "016384".into();
        assert!(transaction(&owner(), &bad_database, &ledger()).is_err());
        let short = ledger().lines().take(44).collect::<Vec<_>>().join("\n");
        assert!(transaction(&owner(), &database(), &short).is_err());
        assert!(transaction(&owner(), &database(), &format!("{}\n046_extra.sql|{}",ledger(),"a".repeat(64))).is_err());
        assert!(transaction(&owner(), &database(), &ledger().replace("001_synthetic", "001_synthetic'" )).is_err());
        assert!(transaction(&owner(), &database(), &ledger().replace("045_", "044_")).is_err());
    }
    #[test]
    fn owner_inspection_observed_mode_is_explicit_and_rejects_prior_identity_labels() {
        let sql=observed_transaction(&owner(),&database().system_identifier,"55555555-5555-4555-8555-555555555555",&ledger()).unwrap();
        assert!(sql.contains("current_database()='exhibitos'"));
        assert!(sql.contains("current_setting('exhibitos.instance',true)='55555555-5555-4555-8555-555555555555'"));
        assert!(!sql.contains("AND :'database_oid'='1'"));
        let strict=transaction(&owner(),&database(),&ledger()).unwrap();
        assert!(strict.contains("AND :'database_oid'='16384'"));
        let observed=ClusterObservation::parse(&serde_json::to_vec(&observation()).unwrap(),&owner(),&database().system_identifier).unwrap();
        let receipt=serde_json::to_value(observed).unwrap();
        assert_eq!(receipt["expectedDatabaseOid"],Value::Null);
        assert_eq!(receipt["logicalDatabaseIdentityQualified"],false);
        assert_eq!(receipt["recoveryEligible"],false);
        for (key,value) in [("expectedDatabaseOid",serde_json::json!("16384")),("logicalDatabaseIdentityQualified",serde_json::json!(true)),("identityMode",serde_json::json!("strict-prior-database-identity")),("recoveryEligible",serde_json::json!(true))] {
            let mut raw=observation();raw[key]=value;
            assert!(ClusterObservation::parse(&serde_json::to_vec(&raw).unwrap(),&owner(),&database().system_identifier).is_err());
        }
        assert!(observed_transaction(&owner(),&database().system_identifier,"';select 1",&ledger()).is_err());
        assert!(ClusterObservation::parse(&serde_json::to_vec(&observation()).unwrap(),&owner(),"123").is_err());
    }

}
