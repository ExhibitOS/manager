# Update safety decision model v1

`exhibitos_lifecycle::update` is a pure Rust decision core. It performs no download,
cryptographic verification, engine update, database migration, backup/restore,
filesystem persistence or native UI operation. It consumes trusted observations
from future adapters. It does not complete the desktop update/rollback feature.

## Evidence boundary

A plan binds a unique operation ID, distinct source/target runtime instance IDs,
immutable SHA-256 image digests (64 lowercase hex, no prefix), complete schema and
migration fingerprints, backup identity, verified backup manifest SHA-256, full
source inventory SHA-256 and the positive required free-space budget. IDs are
nonsecret ASCII letters/numbers/underscore/hyphen, 1–128 bytes. Digest values identify
content; this library does not hash or authenticate the underlying content.

The trusted preflight producer must actually verify the release signature and
artifact, image/schema compatibility, separately tested full service restoration,
current source inventory matching that backup and space for images plus candidate
DB/blob/configuration. Signature/backup booleans are verifier results, not proof
created by this module. Every preflight result is bound to the exact plan. Unknown
or missing evidence must not be turned into `true`. Keep the source quiescent and
reserve/recheck resources between observations and executor mutation; the core
cannot detect a race with external writers or exhausted disk.

`imageOnlyRollbackVerified` is an additional, explicit trusted attestation of old
runtime data compatibility: no migration, backfill, or target writes may invalidate
old-runtime reads. Equal schema fingerprints alone are insufficient. Use `false`
when unknown. A changed schema always requires data restore even if this flag is
true. All migration changes, including data-only migrations, must be represented in
the schema/migration fingerprint. Adapter compatibility policies must also cover
runtime configuration and data-format changes; this core cannot infer them.

A restore receipt binds operation, backup identity/manifest, original full inventory,
source schema and the exact separate candidate ID persisted before restore. An
executor must restore into that separate inactive candidate, compare all inventories
and preserve the original environment, then activate that candidate. Healthy
rollback requires readiness plus observed original image/schema **at that candidate
ID**; health from the retained original instance cannot substitute. Image-only
rollback instead requires observed readiness at the original source instance.
Update success requires observed target image/schema at the target instance.
Instances must be unique per attempt; IDs alone do not attest real processes.

## Integration order and restart

1. `Update::new(plan)` creates Prepared. `begin_update(preflight)` refuses signature,
   artifact, compatibility, backup, source or space failures without mutation.
2. Persist Applying atomically and durably **before** invoking any mutation. Taking
   this step immediately means schema/data may have been touched if the process
   disappears, even without a migration completion report.
3. Executor completion calls `application_finished`; AwaitingHealth is not success.
   Only a valid observed `HealthReceipt` reaches Updated. Executor/health failure
   calls `update_failed` and requires explicit recovery.
4. Recovery can call `image_only_rollback` only with unchanged schema plus explicit
   data-compatibility evidence. Otherwise call `begin_restore(candidate_id)`, persist
   Restoring, invoke separate-candidate restore and supply the validated receipt to
   `restore_finished`. AwaitingRollbackHealth is still not success. Failed restore
   or rollback health calls `recovery_failed`; previous restore proof is discarded.
5. Read at most `MAX_RECORD_BYTES` (16 KiB) **before allocating/loading a file** and
   call `Update::from_json`. It rejects unknown fields/versions and inconsistent
   records and automatically converts all in-flight stages into RecoveryRequired
   with Interrupted. Persist that result before further work. Restore evidence and
   candidate binding are cleared; the operator must reconcile retained candidates
   before issuing a new unique candidate ID. No automatic replay occurs.

`to_json` validates and bounds output, but does not write it. The executor must use
private atomic durable storage, cross-process locking, unique operation/candidate
IDs, evidence authentication and bounded reads. Direct serde deserialization is
supported for integrations but requires `recover_after_restart` before any work;
public transitions validate structural consistency but do not imply durable-write
completion. A fabricated but structurally consistent receipt cannot be detected by
this pure core. Never accept these objects from an untrusted tenant or network.

Failure enum values are bounded diagnostic categories, with no raw logs, paths,
credentials or exception messages. Failed observations leave state unchanged until
the caller records failure explicitly. Prepared plans can be discarded without
mutation; once execution begins do not implement cancel by claiming rollback.
Terminal records retain observed receipt evidence. Production retry/activation,
backup secrets/key recovery, rollback retention and actual OS tests remain adapter
work; no real data or previous backup is deleted by this module.

## Validation

`cargo test -p exhibitos-lifecycle --locked` exercises preflight refusal, exact
receipt binding, successful observations, migration and equal-schema safety,
update/restore/health failures, every in-flight restart stage, malformed persisted
records and forbidden transitions with synthetic identities only. It does not
execute a database or engine update, perform cryptography, restore actual data or
qualify Windows/native UI.
