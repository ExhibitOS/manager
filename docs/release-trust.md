# Local release trust journal v1

The app-closed `exhibitos-update` development commands persist trusted policy,
revoked key IDs, accepted release sequence/issued-time floors and the last observed
OS time. They perform **no runtime update, activation, migration or backup restore**.
The existing read-only `verify --policy` command remains independent and never
claims persistent replay protection. Use the persisted store for release acceptance.

## Provision separately from a release feed

A trusted deployment administrator provisions a public-key policy through a private
local file. Do not obtain trust roots or policy instructions from an untrusted feed.
No production signing key/root is supplied. Policy file ownership/current UID,
no group/world write, regular-file identity, no symlinks/hardlinks, bounded reads and
closed JSON schemas follow the [signed-release verifier](signed-releases.md).

Close all Manager apps and CLI operations for the profile before these commands.
The CLI requires the explicit `--apps-closed` acknowledgement and also obtains the
stable profile pathname fence exclusively. Concurrent cooperative controllers and
recovery commands refuse; the acknowledgement alone is not evidence of quiescence.
Use the same absolute logical profile pathname and `default` or registered
installation UUID on every run. UUID syntax is checked; the development CLI does
not prove that a supplied UUID is registered or that a source schema matches the
actual database. Installation routing and executor binding remain required.

```sh
./target/release/exhibitos-update trust-provision \
  --profile '<absolute logical profile>' --installation default \
  --policy '<separately trusted policy.json>' --apps-closed
./target/release/exhibitos-update trust-status \
  --profile '<same profile>' --installation default --apps-closed
./target/release/exhibitos-update accept \
  --profile '<same profile>' --installation default \
  --release '<signed envelope.json>' --artifact '<exact signed basename>' \
  --apps-closed
```

`trust-provision` exclusively creates a new store and refuses an existing one.
`accept` verifies against its stored policy, reads/hashes the actual artifact,
rechecks file identity, policy/expiry/time and exact prior verifier proof, then
commits the new minimum sequence/time floor. Missing/bad signatures, artifact
failures, expiry or clock regression never advance the floor. Successful acceptance
consumes that sequence even if a later update fails; it is not installation success.
A future retry must use a durable operation record bound to this accepted payload,
not reset security floors or blindly accept the old envelope again. That retry and
mutation-time trust recheck are not yet connected to the update executor.

The receipt includes `verification` and `trust`; `activated:false` and
`backupRestoreVerified:false` remain explicit. A store status/policy receipt is not
an artifact verification receipt. Last observed time never decreases. OS clock
regression refuses; this does not authenticate real wall-clock time or protect
against an administrator setting the clock forward.

## Authorized policy replacement and revocation

```sh
./target/release/exhibitos-update trust-policy \
  --profile '<same profile>' --installation default \
  --policy '<administrator-provisioned replacement policy.json>' \
  --expected-generation '<policyGeneration from trust-status>' --apps-closed
```

The expected policy generation must match the current stored generation; stale
administrator instructions refuse. Channel, container target and protocol stay fixed; sequence/time floors cannot
lower. Removed key IDs remain revoked and cannot be reintroduced in this store.
The pinned source schema can change through an explicit trusted administrator
policy action, including a deliberate data rollback; this action is not a verified
migration. No policy is accepted from release metadata. Remote signed rotation,
root expiration, recovery authorization, a production publisher and deployed
roots remain unimplemented. The OS account administering this store is trusted;
these commands do not provide a separate server role boundary.

## Persistence, recovery and limits

A deterministic scope hash includes the canonical logical profile pathname and
installation identity. The private700 store is a sibling directory named
`.exhibitos-release-trust-<scope hash>`, **outside** replaceable/archived profile
bytes. Its private600 lock and exclusive profile anchor stay held throughout each
operation. Old profile restore, absence or replacement does not recreate/reset the
store. A missing or corrupt store refuses open/accept and never automatically
bootstraps from an archive or feed. Parent-folder relocation changes the namespace
and requires a separately authorized security-state migration; it is not automatic.

Each immutable, bounded96KiB record contains the policy, monotonically increasing
journal/policy generations, observed time, revoked IDs, acceptance identity and
previous record hash. On open, the full contiguous chain is checked, including
monotonic policy transitions and scope. New records use fresh private staging,
file fsync, OS atomic no-replace rename and directory fsync. Publication collisions
never overwrite existing records. Partial writes remain private `pending-UUID.json`
files for diagnosis, without advancing floors. A write/sync error reports
`UPDATE_TRUST_WRITE_UNCERTAIN`; that live Store refuses further verification/writes
and marks diagnostic receipts `writeUncertain:true`. Close and reopen before any retry, inspect the
persisted state, and never assume the previous floor still applies. No automatic
record, pending file or user-data deletion occurs.

Current bounds:4096 committed records (96KiB maximum per record),256 permanently revoked IDs, at most65
additional directory entries including the lock/pending records. Exceeding bounds
fails closed. Automatic compaction/rotation is absent and must preserve the highest
security floors and revocations. Record hashes detect chain corruption/gaps; they
are not a signature/MAC against the trusted OS account. Deleting the whole store,
removing its final suffix or replaying a complete old store can evade these local
checks. Hardware-backed/remote monotonic recovery protection is not implemented.
Do not restore an old security journal as part of a profile/data rollback.

These files are Git-excluded application security configuration. Existing profile
and host checkpoints deliberately exclude them. A complete installation recovery
must back them up consistently with its update operation records and preserve the
**highest** policy/floors/revocations against any older snapshot. Until a verified
security-state recovery protocol exists, retain the original external journal and
anchor during host recovery. Git bundle backups do not include this state.

macOS atomic publication and filesystem behavior are locally tested. Linux uses
`renameat2(RENAME_NOREPLACE)` but requires actual OS qualification; unsupported OS
paths, including Windows ACL/publication, refuse. Actual power-loss/disk-failure qualification remains required. No native app package/UI build or
production OS signing qualification is implied by CLI tests. Old binaries unaware
of this journal must not be used for release acceptance.

The [persisted update preparation](update-intent.md) binds actual signed bytes and
plan to the same atomic acceptance record; it does not execute an update.
