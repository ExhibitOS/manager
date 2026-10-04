# Persisted update preparation

`exhibitos-update prepare-update` binds a verified signed release and actual local
artifact to an exact update plan, then commits the prepared intent and accepted
sequence/time floors together in the [external trust journal](release-trust.md).
There is no crash gap where a newly accepted generation loses its intended source,
target or backup identity. This is preparation only: it does not execute/import an
image, prove a backup or migration, apply an update, observe health or roll back.

```sh
./target/release/exhibitos-update prepare-update \
  --profile '<absolute logical profile>' --installation default \
  --release '<signed envelope>' --artifact '<exact signed basename>' \
  --plan '<private update plan.json>' --apps-closed
./target/release/exhibitos-update update-intent \
  --profile '<same profile>' --installation default --apps-closed
```

Plan JSON is the closed `update::Plan` schema in [update safety](update-safety.md):
operationId, distinct sourceInstance/targetInstance, distinct sourceImage/targetImage,
sourceSchema/targetSchema, backupId/backupManifest, sourceInventory and positive
requiredFreeBytes. The signature/artifact verifier requires exact pinned source
schema and signed target image/schema. Private plan ownership/write/link/size guards
apply. UUID/profile scoping is not proof of actual installation membership. A plan
fingerprint is not evidence that a real backup, source or migration was checked.

Records store the exact public signed envelope and bound update core.
They contain no signing key or fabricated compatibility/backup/source/space flags.
The diagnostic result has `executed:false`, stage `prepared` and null preflight;
its trust receipt remains `activated:false`. The pure core still refuses applying
without independently verified compatibility/restoration/source/space observations.

An exact envelope already accepted by the current stored policy can be prepared
without lowering the persistent sequence floor. It must match that accepted
payload/key/sequence/issued-time/artifact identity; signature, current keys/schema,
time/expiry and actual bytes are checked again. A different envelope with the same
sequence cannot use this recovery path. Ordinary `accept` still refuses replay.
Missing acceptance, changed/revoked policy or expired metadata cannot be repaired
by resetting floors. A pending intent blocks another preparation and standalone
acceptance. All prior records/envelopes/artifacts remain; no automatic discard,
activation, cancellation or runtime restart occurs. Explicit preparation reconciliation and the durable callback APIs below are
implemented; actual engine, health and restoration adapters remain executor work.

The intent is outside profile recovery bytes with the security journal and survives
profile absence/replacement. Security journal consistent backup/latest-state
recovery remains required; never restore lower floors/older revocations. The
journal bound is now96KiB per record to hold signed envelope plus plan/policy.
Existing records without intent remain readable. Older binaries reject new intent
fields or larger records, and must not accept releases from the extended journal.
No public format contract or native GUI/package/Windows/powerloss acceptance is
implied. The OS account is trusted; whole journal/suffix rollback is not resisted.

Local checks: lifecycle tests/strict Clippy plus independent Node/OpenSSL actual CLI
scripts `test-signed-release.mjs`, `test-release-trust.mjs` and
`test-update-intent.mjs`. Synthetic artifacts are not deployable OCI bundles.
Required next connection: actual verified
pre-update restoration and source/space/compatibility observations, fenced engine
candidate/update/health/rollback, latest security-state recovery and original
T08-02 OS/GUI/full restoration acceptance. Preparation is not that completion.

## Durable application boundary

The trusted Rust executor API `Store::begin_update` rechecks the stored envelope
against current policy, revocation, source schema, time and an actual artifact
verification receipt. It binds only signature/artifact flags; compatibility, full
backup restoration, current source inventory and space observations must come
from trusted adapters and match the exact plan. No CLI accepts these flags.

The API durably appends an exact `begin` event and Applying core before returning
to an executor. The Store retains its exclusive profile and trust fences. If the
write is uncertain, callers must not perform an engine mutation. On subsequent
Store open any in-flight record (Applying, AwaitingHealth, Restoring or
AwaitingRollbackHealth) appends `interrupted` and RecoveryRequired under
those fences before returning. Reopening that recovered state is idempotent.
Even diagnostic commands can therefore write a recovery record; clock rollback,
capacity exhaustion and write failure refuse to return an apparently usable store.

Chain validation recomputes each event from the preceding core and forbids
changes to the signed envelope, plan, policy, revocations or accepted floors in
that transition. Existing event-free records remain readable; old binaries reject
the new event field or unknown event variants. All transition APIs below append
validated events; current pointers can be cleared only in explicitly permitted stages.

RecoveryRequired does not prove helpers are stopped, that any mutation occurred,
or that data was restored. It never automatically restarts or replays an engine.
Real executor quiescence, current source locks, verified restoration, image import
and current native/OS qualification remain required. Test observations
are explicitly synthetic storage fixtures and do not qualify those adapters.

## Trusted executor callback order

These Rust APIs persist observations; they do not perform their prerequisite engine
actions. Pass the exact `operationId` and `TrustReceipt.generation` observed by that
executor. `UPDATE_OPERATION_STALE` rejects stale generations; a foreign operation
refuses without writing. One Store holds the exclusive fences throughout work.

| Callback | Durable result | Required actual observation/action |
| --- | --- | --- |
| `application_finished` | AwaitingHealth | Application step finished; success is still unproven |
| `observe_health` | Updated | Target readiness and exact instance/image/schema; current target signature policy and time rechecked |
| `update_failed` | RecoveryRequired | Application or target health failure |
| `request_rollback` | RecoveryRequired | Explicit request after Updated |
| `begin_restore` | Restoring | New separate candidate ID; publication must precede any restore mutation |
| `restore_finished` | AwaitingRollbackHealth | Exact backup/manifest/inventory/schema restored and verified at that candidate |
| `observe_health` | RolledBack | Readiness of that restored candidate with original image/schema |
| `image_only_rollback` | AwaitingRollbackHealth | Equal schema AND explicit data compatibility/no incompatible writes; unknown refuses |
| `recovery_failed` | RecoveryRequired | Restore or rollback health failure; old restore proof cleared |
| `release_completed` | No active intent | Only Updated/RolledBack; actual helper/runtime ownership must be reconciled first |

Expired/revoked target metadata cannot be declared Updated. Failure and restoration
callbacks remain available, including observed original-image rollback health.
Readiness alone is insufficient: every receipt must match the original exact plan.
No command accepts boolean health/preflight/restore evidence from a tenant, feed or
network. Adapters must authenticate observations, detect races, keep writers quiescent
and reserve/recheck space. IDs and hashes do not prove any of those actions happened.

Every operation, target and restore candidate ID remains reserved in the validated
full history after failure, interruption, discard or completion. A current source may
legitimately be a previous target/candidate; a new target/candidate must be fresh.
`UPDATE_IDENTITY_REUSED` refuses before publication and on chain reload. Scope is
one profile+installation journal, not global cross-installation identity attestation.
Existing managed Controllers acquire a shared ProfileSession/anchor, whereas this
Store holds the anchor exclusively. The future engine adapter needs an internal
validated owned-fence capability for composing those operations; constructing an
unmodified Controller under this Store conflicts with the fence. Do not release
the Store lock or bypass helper/source checks to make an update run. No such
executor composition or actual engine action is implemented in these callbacks.

The existing 4096-record bound also bounds the ID sets; no automatic deletion or
compaction is introduced. Consistent security-state recovery must preserve full
history, highest generation/floors/revocations AND these identity reservations.

## Cancel an unexecuted preparation

```sh
./target/release/exhibitos-update discard-update-intent \
  --profile '<same absolute logical profile>' --installation default \
  --operation-id '<exact prepared operation>' --expected-generation '<journal generation>' \
  --preserve-data --apps-closed
```

This appends DiscardPrepared and clears only a Prepared active pointer. Original
records, profiles, failed candidates, artifacts and accepted security floors remain.
The acknowledgment does not request cleanup. It cannot discard Applying, recovery
or terminal states. After discard, a new preparation requires fresh operation and
target IDs and fresh signature/artifact validation; no old ID may be reused. The
diagnostic `executed:false` means that CLI command executes no engine, including
when it reads a trusted executor's Updated/RolledBack record. Trust receipts remain
`activated:false` because they are journal receipts, not runtime activation proof.

Tests use explicitly synthetic trusted preflight/restore/health observations for
real local journal/API and separate CLI-process checks. They do not qualify actual
engine updates, migrations, complete backup restoration, power loss, native GUI,
Windows/Podman or coherent security-state+host+volume recovery.
