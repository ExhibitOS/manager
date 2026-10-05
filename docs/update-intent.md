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

## Owned registered-source adapters

`Store::execution()` borrows the exclusive Store for the lifetime of an execution
session, duplicates its existing anchor descriptor without changing the lock,
and acquires the legacy exclusive profile-session fence. It loads an existing
private installation registry; no controller bootstrap, missing directory creation,
arbitrary root override or selection change is allowed. The scoped default entry
or explicit registered UUID must equal `plan.sourceInstance`. Canonical private
profile/source inode identities and the exact registry bytes are checked before
and after every adapter call, including failed calls. Missing/corrupt/aliased or
replaced roots refuse execution. This is local namespace membership, not global
runtime identity or plan image/schema/inventory attestation.

```sh
./target/release/exhibitos-update execution-status \
  --profile '<same logical profile>' --installation default --apps-closed
./target/release/exhibitos-update verify-update-backup \
  --profile '<same logical profile>' --installation default \
  --maintenance-image 'sha256:<trusted local immutable maintenance image>' \
  --key '<external private 32-byte key file>' \
  --archive '<private encrypted archive directory>' --apps-closed
```

Status uses the existing root operation lock and actual lifecycle status adapter.
Archive verification uses the existing operation lock, private path/key guards,
immutable local image inspection and network-isolated authenticated decryption
adapter. Its plaintext manifest hash must equal `plan.backupManifest`; a different
authenticated archive yields `UPDATE_BACKUP_MISMATCH`. A mismatch or failed helper
retains its private candidate for inspection. Neither command changes the update
stage or supplies preflight/health/restore receipts. Outputs explicitly report
`updateExecuted:false`; authentication also reports `restoreVerified:false`.
Normal source containers/data are not stopped, restored, migrated or activated.

No new journal schema, package dependency or public format change. The engine
commands are additive CLI functionality; existing journal-only diagnostics still
work when the profile is absent. Keep the app closed throughout these commands.
Same-UID hostile filesystem mutation and whole security-chain rollback remain
outside the trusted-account boundary. Actual update/import/migration, full restore
inventory and source quiescence/compatibility/space verification remain required.

Reproduce using only retained synthetic fixtures:

```sh
node scripts/test-owned-execution.mjs \
  ./target/release/exhibitos-update '<synthetic installed root>' \
  'sha256:<maintenance image>' '<private synthetic key file>' \
  '<synthetic archive>' '<authenticated plaintext manifest sha256>'
```

The script creates separate private registered profiles, copies only the five
installed deployment files, authenticates actual ciphertext, checks wrong keys,
foreign source/manifest and legacy process locking, and asserts original deployment
file/archive/key hashes and persistent container states remain unchanged. Test
artifacts and candidates are retained; the signing key is transient in memory.
The copied profile references an existing synthetic Docker project, so this does
not qualify multi-instance/global runtime identity, a real OCI update or full
restore. Native GUI, Windows/Podman and production acceptance remain separate.

## Actual source-version candidate preparation

`prepare-update-candidate` executes a real fresh restoration into the registered
recovery entry whose UUID is exactly `plan.targetInstance`. The signed Prepared
journal already reserves that target identity durably. The source and target must
be distinct existing canonical private roots in the same unchanged registry.
Create the new recovery space using the existing Manager installation selector,
return to the intended source, close the app, and prepare the exact source/target
plan before running this command. The command neither registers missing spaces nor
changes the active selection. Existing/failed targets are never erased or reused.

```sh
./target/release/exhibitos-update prepare-update-candidate \
  --profile '<same logical profile>' --installation default \
  --maintenance-image 'sha256:<trusted immutable local maintenance image>' \
  --key '<external private 32-byte key file>' \
  --archive '<private Manager-produced draft2 service archive>' \
  --port '<unused loopback port above 1023>' \
  --fresh-candidate --external-writers-quiesced --apps-closed
```

The command requires Prepared, explicit fresh-candidate acknowledgement, both
exclusive profile fences and the original source operation lock. The target
operation lock guards its empty-root test and full restore. Both the plan's free
space budget and existing 2GiB restore minimum apply; neither is an estimate of
available provider quota. Source/target identities and registry bytes are checked
again after execution, including failure. A replaced namespace cannot return
success. The command directly inspects both owned source Docker containers before and after
restoration and requires exited/created state, PID zero, and Running/Paused/Restarting/Dead
false, with the Platform image equal to plan.sourceImage. Container IDs must remain
equal. It never stops or starts source services. --external-writers-quiesced is an
additional explicit operator acknowledgement; these observations do not isolate
external DB/blob/config writers or prove a current source snapshot match.

Before image import or creating the candidate's installed bundle/services, the
existing isolated helper authenticates the archive. Rust then checks its exact
plaintext bytes against `plan.backupManifest`, its UUID against `plan.backupId`,
its inventory fingerprint against `plan.sourceInventory`, and its complete
schema/migration fingerprint against `plan.sourceSchema`. The configured Platform
image is resolved through the authenticated preserved image inventory and must
match `plan.sourceImage`. Existing image archives are hash/content-ID verified
through actual Docker load and inspection; no mutable image tag or registry pull
can supply these proofs. Binding mismatches remain private failed candidates.

Fingerprint definitions for this source-backup adapter:

- `sourceInventory`: SHA-256 of compact JSON for the complete authenticated
  `manifest.inventory`, with recursively UTF-8-byte-sorted object keys, preserved
  array order and integer numbers bounded to JavaScript safe integer range.
  `createdAt` is retained; this identifies the exact archived snapshot.
- `sourceSchema`: the same canonical SHA-256 algorithm over
  `{schemaDigest, schemaVersion, migrations}` from that inventory. Every ordered
  migration's closed `{name, sha256}` entry is included, so a data-only migration
  checksum changes this identity even when the physical SQL schema stays equal.
- `sourceImage`: the Platform's immutable preserved content ID without the
  `sha256:` prefix. The PostgreSQL image cannot substitute for the Platform image.

A physical SQL hash alone cannot qualify a sourceSchema plan for this adapter.
Existing journal-only diagnostics and unbound restoration remain unchanged;
regenerate an authorized exact plan/policy/release with the full fingerprint,
rather than lowering security floors or editing a prepared intent.

The normal restoration adapter restores and verifies the actual PostgreSQL/blob
inventory into new named volumes, preserves signing/configuration credentials,
starts the restored **source-version** candidate on a new loopback port and checks
Runtime readiness. Success adds optional `sourceVerification` to its private
receipt (`inventorySha256`, full `schemaSha256`, `runtimeImageSha256`). It is absent
for existing unbound restore calls/receipts; older closed readers reject a new
bound receipt rather than silently consuming its additional proof. No public
format or dependencies changed. New receipt readers validate proof shapes.

Output reports `candidatePrepared:true`, `updateExecuted:false` and
`preflightVerified:false`. The update journal stays Prepared with null preflight.
Actual source-version restoration is one required preflight component; current
source equality/quiescence, migration/data compatibility, resource reservation,
actual signed target OCI import/application and observed target image/schema/health
remain necessary before Applying. This command is not completed update or rollback.
The candidate's separate restoration journal provides failure/interruption/helper
reconciliation; no automatic replay occurs. Explicit Prepared intent discard does
not stop/delete its candidate services, volumes, journals or registry entry, and
history retains its target ID reservation. Preserve and explicitly inspect/stop
owned candidates when reconciling or abandoning an attempt.

The real synthetic regression is `scripts/test-bound-candidate.mjs <update CLI>
<manager CLI> <retained synthetic creation fixture>`. It verifies actual foreign
manifest refusal before target installation, separate full DB/blob/config/image
restoration, schema+migration/inventory/image bindings, actual witness/blob/signing
key, administrator HTTP login/web, repeat refusal, original hashes/container
states and unchanged active selection/Prepared journal. It stops only its new
completed synthetic candidate and retains all new data/volumes/failure candidates.
It does not move the original path or export a private signing key. Cached Docker
images, native GUI, cold-engine/full frozen corpus, Windows/Podman, production
roots, signed target update and whole security-state recovery remain separate.

The no-overwrite fresh-root guard runs under the target lock before a new-operation
space budget, so a repeat attempt still reports an existing/failed target when
available disk has fallen. Successful candidates can consume substantial temporary
space (authenticated/restored plaintext plus imported deployment copies); preserve
those candidates and originals rather than weaken the 2GiB floor.

The regression script persists a private source baseline before a new restore.
After a terminal test assertion failure, it can observe the same completed owned
candidate without replaying restoration using `--resume-existing <private test
workspace> --archive-baseline <independent retained same-backup ciphertext copy>
--producer-record <private original CLI/hash record>`. Original deployment files
are compared to the registered source copy made before restoration and ciphertext
to the independent copy. Its report distinguishes the original producer binary,
current verification binary and resume-time key/container-state baseline. This
mode is test observation, not automatic product recovery or update preflight.

## Direct source stop observation

`verify-update-source-stopped --profile <absolute profile> --installation default|UUID
--external-writers-quiesced --apps-closed` performs the same owned source check under
the borrowed exclusive profile fence and source operation lock, with helper-idle,
Compose/ownership/volume validation. It requires both Platform and database stopped,
exact registered source and plan Runtime content ID. Running, paused, restarting,
dead, missing or ambiguous states refuse. It does not stop anything or write update
events; output includes sourceStopped:true, preflightVerified:false, updateExecuted:false.
The receipt is a momentary observation, not a persisted authorization token.

Candidate preparation now additionally requires this explicit acknowledgement and
successful observations before and after restoration. Existing-target no-overwrite
refusal remains first. Source state change after restoration refuses success and
retains the candidate and its restoration journal; do not replay automatically.
Source snapshot/inventory equality, external writer isolation, resource reservation
and compatibility remain necessary for Applying. Older candidate CLI invocations
must add --external-writers-quiesced; public Spec and journal formats are unchanged.

## Current source host deployment comparison

`verify-update-source-deployment --profile <absolute profile> --installation default|UUID
--external-writers-quiesced --apps-closed` requires Prepared and its exact already
registered recovery target with a completed bound restoration receipt. Both source
and target operation locks and exclusive profile fences remain held. The original
candidate root identity/registry is rechecked even on failure; no arbitrary manifest,
root, receipt, UUID override or supplied verification flag is accepted.

The adapter checks the receipt backup UUID/manifest hash and complete inventory,
schema+migration and Runtime proof against the stored plan, then reads that candidate's
private authenticated manifest and independently rechecks its exact raw bytes and
all plan bindings. Unbound/failed/interrupted candidates cannot supply this proof.
It compares current source bytes/lengths for these five uniquely named configuration
records: manager-bundle-manifest.json -> bundle/manifest.json, manager-compose.yaml ->
bundle/compose.yaml, manager-runtime.env -> runtime.env, manager-installed.json ->
installed.json and manager-engine.json -> engine.json. Missing/duplicate records,
wrong roles, malformed hashes/sizes, private mode/ownership/alias/hardlink failures
and changed bytes refuse. File reads are bounded and check file identity before/after.
Current files are compared twice with direct source-stopped/image observations;
source container IDs must remain unchanged. Output contains paths/lengths/hashes,
never environment values or credentials.

Success reports hostDeploymentVerified:true, dataInventoryVerified:false,
preflightVerified:false, updateExecuted:false. It does not advance the Prepared
journal or qualify full source equality: PostgreSQL rows/sequences/schema/migrations,
all object/reference bytes and the live configuration volume/freeze signing key
still require independent current observations. manager-image-inventory.json is a
backup-generated image mapping, not one of the five current source host files.
External/noncooperating writers remain operator-acknowledged rather than isolated.
The completed candidate receipt is historical restoration evidence, not a fresh
candidate DB/blob inventory or current health attestation. Actual signed target
application/migrations/rollback and coherent security+data recovery remain required.

Synthetic regression: scripts/test-source-deployment.mjs <update CLI> <retained
synthetic bound-candidate workspace>. It observes the retained completed candidate,
temporarily changes only owned synthetic source/manifest/receipt/registry copies
and restores their exact bytes in finally blocks, checking each refusal, repeat
proof, original source files, container states and unchanged Prepared intent.
It performs no new restoration or source start/stop. Native GUI, Windows/Podman,
cold/full corpus and global instance isolation remain separate qualifications.


Successful fresh candidate preparation now performs this same authenticated
five-file comparison before reporting candidatePrepared:true. If the source
host bytes changed since the backup, restoration may already have completed into
a new candidate; the command refuses success and preserves its services/data,
restoration journal and Prepared intent. Inspect/stop the owned candidate explicitly.
This does not silently discard a candidate, replay restoration, or qualify current
DB/blob/config-volume equality. A full fresh restoration with this additional
post-restore guard remains a separate acceptance test.

## Observed shared-volume writers

Source-stopped observations now query Docker for all containers attached to each
currently mounted named source volume, regardless of name or ownership label. A
foreign writable mount with running, paused, restarting, dead or ambiguous state
refuses with `UPDATE_SOURCE_VOLUME_WRITER`. Missing/malformed mount access or an
incomplete Engine census refuses. Stopped foreign users and actual read-only
mounts are accepted; unrelated volumes do not block the source. No foreign
container is stopped, removed or modified. Candidate preparation and source host
deployment comparison use this guard through their existing source observations.

This remains a momentary observation under cooperative Manager locks. It does
not prevent an external container starting afterward, host access to volume files,
remote/database clients or writers through bind mounts. Operator acknowledgement,
independent current data/config observations and the full update/restore gates
remain required; this check does not authorize Applying. Journal/Spec formats
and existing acknowledgement requirements are unchanged.

The opt-in Rust test `actual_engine_volume_writer_census` uses the already-local
qualified maintenance image and a unique empty synthetic volume to inspect an
actual running writable foreign container (refused), stopped writable foreign
container (accepted), and running read-only container (accepted). Its three
helpers and empty volume are scoped to that invocation and removed afterward.
Run `cargo test -p exhibitos-lifecycle --lib actual_engine_volume_writer_census
--locked -- --ignored --nocapture` only with the documented local image available.

## Current source native volume bindings

Source observations now additionally require exactly two writable named Platform
mounts at `/data/blobs` and `/data/config`, and one writable named database mount
at `/var/lib/postgresql`. Each actual mount must match the resolved Compose volume
name, the bundle/project/schema ownership labels, local driver and supported
volume options. The three names must be distinct. Bind substitutions, extra/missing
mounts, read-only substitutions and aliases refuse. Existing backup producer
volume validation is shared with this observer instead of accepting arbitrary names.

The source receipt adds `blobVolume`, `configurationVolume` and `databaseVolume`.
Candidate preparation and host deployment comparison require those names to remain
equal before/after their existing work, alongside source container IDs. CLI output
is additive; public Spec and durable update journals are unchanged. These are
observed bindings, not volume-content equality or immutable Docker volume identities:
a privileged external actor can recreate a volume with the same name and labels.
Native configuration/blob byte checks and stopped DB snapshot observation remain
necessary before applying an update. The product does not start or mutate source
services/volumes in this observation.

`cargo test -p exhibitos-lifecycle --lib actual_engine_source_volume_identity
--locked -- --ignored --nocapture` verifies real local Docker volume ownership and
actual container mounts using empty synthetic fixtures and existing immutable
Platform335f8f2 and maintenance8f0e7b0 images. An initial fixture used the maintenance
image for the app: its declared database volume created an unexpected extra mount,
which the strict layout check correctly rejected. The corrected fixture uses the
Platform image for the app. Known test helpers and three empty named volumes are
removed; the failed fixture's automatically generated anonymous volume is retained
for later separately scoped review. This test does not execute the full profile/CLI
source comparison or certify current DB/blob/config content.

## Current native freeze signing configuration

`verify-update-source-configuration --profile <absolute profile> --installation
default|UUID --image sha256:<trusted local maintenance content ID>
--external-writers-quiesced --apps-closed` holds the exclusive profile/source/target
fences and requires the exact registered completed recovery candidate of Prepared.
It authenticates that candidate's raw manifest against the stored plan, validates
its restoration bindings and current five host files, and derives the unique
freeze-signing-key configuration record. An extracted historical key is never
used as the current reader. The current source native volume is derived from
owned actual Docker mounts, not an arbitrary path or caller success flag.

A network-disabled, read-only, capability-free UID1000 helper reads only the
source configuration volume. The supported native layout contains exactly
`freeze-signing-key.json`. Canonical regular single-link0600/UID1000 files under
1MiB are read twice with file identity/timestamp and measured shared-filesystem
backend guards. Buffers are zeroed; output contains only name/size/digest. Changed
bytes, public permissions, foreign ownership and additional/alias files refuse.
The adapter compares the result with the authenticated record, rechecks current
host files and source container/volume bindings, and retains the Prepared intent.
The receipt binds source/target IDs, backup/raw-manifest hash, volume name, immutable
helper image and observation time. Helpers are stopped/observed and removed by
exact ID/ownership without force or volume deletion; uncertain creation/cleanup
refuses and retains the helper for review. No archive key is required.

Success reports nativeFreezeKeyVerified:true, configurationInventoryVerified:false,
preflightVerified:false, updateExecuted:false. The backup-generated image mapping
still needs independent fresh Engine evidence, and full DB/blob/current security
history and actual application/rollback remain necessary. This is a momentary
observation, not external-writer isolation, atomic configuration snapshot or an
Applying authorization. UID1000/native Docker is the supported initial scope;
Windows/Podman/other ownership and the full profile/CLI synthetic path remain
unqualified. Real-Engine Rust regression `actual_native_configuration_content_and_metadata`
checks a separately owned synthetic Linux volume's matching/changed bytes, public
mode, ownership and additional/alias refusals, then restores its original bytes,
0600/1000 metadata and one-file scope. That synthetic volume is retained privately.

## Fresh current Engine image mappings

`verify-update-source-images --profile <absolute profile> --installation default|UUID
--external-writers-quiesced --apps-closed` observes both original pinned image
references directly from Docker under the existing profile/source/target fences.
Prepared's exact completed registered recovery candidate and raw manifest binding
are required. The private extracted manager-image-inventory.json must match its
unique authenticated configuration record; its two references/order, bounded
archive sizes/hashes and exact image-0/image-1 deployment records must agree with
the original installed bundle and authenticated manifest.

The historical map supplies expected values only. Fresh current Engine lookups
run twice and must resolve every pinned reference to the expected content ID; both
currently installed service containers must also use the matching resolved Compose
image. Current host/source/container/volume bindings and raw configuration/manifest
copies are rechecked around the observations. No image is pulled, loaded, tagged,
saved or removed, and no source service starts. Receipt includes source/target,
backup/manifest, observed references/content IDs and time.

Success reports currentImageMappingsVerified:true, imageBytesVerified:false,
configurationInventoryVerified:false, preflightVerified:false, updateExecuted:false.
Engine metadata and a content-ID reference do not independently inspect stored
image layers or regenerate tar bytes. DB/blob snapshots, complete configuration
evidence, full owned profile/CLI flow, signed application/rollback and coherent
current security/data recovery remain required. Privileged external Engine changes
after observation remain possible; acknowledgement is not isolation. Public Spec
and durable journals are unchanged. The opt-in actual_engine_current_image_mappings
Rust test inspects existing immutable local images and refuses an incorrect expected
content ID without any Engine mutation; it does not execute the full profile/CLI
container-binding path.


### Prepared artifact under the execution fence

`exhibitos-update stage-prepared-artifact --profile <absolute private profile> --installation default --artifact <absolute runtime.tar> --staging-parent <absolute external private directory> --apps-closed` stages the artifact from the current Prepared intent. It takes no caller policy, envelope, plan, or verified flags. The existing Store/profile/source fence stays held while current policy, exact intent binding, signature, actual bytes, time and retained read-only file identity are checked. The staging parent must be outside the profile/security journal. Partial files are retained on failure.

The opaque retained object is intended for the update executor; it must be reverified under the same session before use. The CLI receipt is diagnostic and cannot authorize Applying. OCI internals/import, restored backup, compatibility, current data, resources, coherent security/data recovery and actual update/rollback remain separate requirements. Windows private filesystem staging is still unqualified.


### OCI preparation under the owned execution session

The development `qualify-prepared-oci` command stages the current intent's artifact and feeds the retained read-only file as seekable child stdin to the compiled-in bounded OCI qualifier. A second retained descriptor supplies the backup-authenticated manifest with an independently checked digest, without reopening its path in the child. Only the child clears close-on-exec for this exact descriptor; parent flags stay unchanged. It derives the source manifest from the current registered target's completed authenticated restoration and compares the original deployment files before and after. No caller source manifest/policy/plan or success flags are accepted. Current trust, exact intent, wall-clock validity and retained artifact identity are rechecked. Both source and target operation locks remain held.

```sh
exhibitos-update qualify-prepared-oci --profile <absolute private profile> --installation default --artifact <absolute runtime.tar> --staging-parent <external private directory> --python <canonical absolute Python3.11-or-newer executable> --source-commit <40-character development revision> --development-cache-import --apps-closed
```

This explicit development path currently qualifies Linux ARM64 artifacts only. Python is an external development prerequisite; the verifier source is included in the compiled binary and isolated Python ignores environment injection. The expected revision is a label expectation, not independently authenticated source provenance. The qualifier verifies the tagless graph/blob/layer/config/migration declarations against the authenticated backup manifest and signed release. It does not execute layer contents.

The same retained handle then supplies local Docker image load. Complete observed container/mount/status, volume name and tag snapshots must stay unchanged; existing cached images are preserved and the loaded image/rootfs identity is checked. The current image may already be cached, so success is not cold Engine evidence. Cache import failures/timeouts may leave daemon work uncertain: retain the cache, artifacts and diagnostics; never auto-delete or claim no import happened. `preflightVerified` and `updateExecuted` remain false. No installed candidate image/registry, DB/schema, active selection or trust journal is changed. Actual Applying, compatibility, combined trusted preflight, coherent security/data recovery and full rollback are still separate requirements. Partial private staging is retained.


## Explicit transient configuration observations

`exhibitos-update verify-update-configuration-transient --profile <absolute-profile> --installation <default-or-UUID> --image <qualified-maintenance-image-id> --external-writers-quiesced --apps-closed` performs the same complete current seven-file configuration and fresh image-byte verification as `verify-update-configuration-inventory`. Only after repeated source/target/root checks pass, it verifies each fresh export's exact hash/size/private owner/mode/single-link identity and retires that invocation's images through a retained directory handle. `verified-images.json` is synced before retirement; the returned observation sets `imageArchivesRetained=false`. The small receipt contains hashes and identities of expected images, not secret contents or a recovery archive. Re-export exact content IDs and recheck authenticated sizes/hashes to repeat this observation.

The existing inventory command and final checkpoint-stage image export retain their complete image files. Transient verification does not remove historical observations, source/runtime bundles, service archives, keys, image caches or volumes. Failed verification candidates remain. A retirement failure reports uncertainty and preserves remaining candidates; inspect the receipt and remaining files rather than claiming complete cleanup. This Unix path is locally qualified separately from Windows, which refuses unqualified retirement. Transient observations do not authorize Applying, attest DB/blob recovery, restore live authority, or replace the coherent recovery baseline.

## Missing host namespace with retained authority

`exhibitos-update restore-missing-host --profile <original-absolute-profile> --installation <default-or-UUID> --host-archive <host.bin> --trust-archive <trust.bin> --key <external-private-key-file> --absent-original-profile --apps-closed` restores a missing host namespace under the Store's existing external anchor/trust lock. It refuses any existing destination, including a dangling symlink. The independent latest journal must remain present and valid: this command never provisions or replaces trust authority. Its encrypted trust checkpoint must match the entire current history/head/floors/intent; old or corrupt trust archives refuse before large host extraction.

Host extraction authenticates every frame and original profile namespace, then the candidate is rechecked against its authenticated manifest: all file bytes/hash/modes, complete directory inventory and registry. Publication uses the supported OS no-replace rename and sync. The original encrypted archives/key stay intact, a private publication intent is synced before publication, and a separate completion receipt follows successful publication. On uncertainty preserve the published profile, remaining staging, archives and external journal; never overwrite/replay publication. Ordinary controllers may reopen only after the Store and its anchor are dropped. Successful extraction retires its fresh plaintext intermediate using the existing policy.

This feature restores host bytes while preserving independently retained trust. It does not restore lost trust authority or prove that separately supplied host/data archives constitute the full coherent point, restore external DB/blob volumes, start a runtime, authorize Applying or attest runtime health. Those gates need the full coherent recovery verifier/executor. Supported macOS publication is exercised locally; Linux/Windows/crash qualification is separate. Apps and other host writers must remain closed; these cooperative locks are not privileged-writer isolation. Space floors, large-file coverage and original backup criteria are unchanged.
