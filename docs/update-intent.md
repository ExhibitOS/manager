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
activation, cancellation or runtime restart occurs. Explicit pending-intent
reconciliation and health/rollback persistence remain executor work.

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
Store open an Applying record appends `interrupted` and RecoveryRequired under
those fences before returning. Reopening that recovered state is idempotent.
Even diagnostic commands can therefore write a recovery record; clock rollback,
capacity exhaustion and write failure refuse to return an apparently usable store.

Chain validation recomputes each event from the preceding core and forbids
changes to the signed envelope, plan, policy, revocations or accepted floors in
that transition. Existing event-free records remain readable; old binaries reject
the new event field. This version persists only begin/interruption. It has no
application-finished, health, rollback, discard or completion journal API.

RecoveryRequired does not prove helpers are stopped, that any mutation occurred,
or that data was restored. It never automatically restarts or replays an engine.
Real executor quiescence, current source locks, verified restoration, image import
and all success/rollback receipt transitions remain required. Test observations
are explicitly synthetic storage fixtures and do not qualify those adapters.
