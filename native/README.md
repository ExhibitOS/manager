# Native macOS application (in progress)

Responsibility and build boundaries are defined once in the [Platform architecture](https://github.com/ExhibitOS/platform/blob/main/docs/architecture.md#repository-roles-build-boundaries). This page is for developers, not end-user installation prerequisites. The current native development slice has bounded synthetic save/cold-reopen/OEX and existing-publication-to-static-output evidence. Native settings GUI/accessibility, real hosted delivery, legacy migration and final signed installation remain separate. Dated coverage below records its original checkpoint and must not override later scoped results.

The SwiftUI window is connected to the existing Rust installation controller by
the bundled `exhibitos-native` helper. The helper has no Tauri dependency. Both
presentations share the same selected installation and existing durable jobs.
Mode, current screen and100–200% text preference are revision-checked and saved
in the private managed profile; changing modes never changes installation or
claims a workflow step complete. Corrupt state is preserved and refused.

The application uses system navigation, menus, settings, semantic colors and
scaled text. Preview configurations cover light, dark and a narrow200% layout.
These previews are source configurations, not accessibility acceptance evidence.
The `docs/decisions/native-authoring-macos.md` decision describes unified native
Studio/CMS controls and reuse of the existing shared web3D viewport/API.

Development with the pinned Rust toolchain and Xcode:

```sh
cargo test -p exhibitos-lifecycle --locked --test native_ui
cargo clippy -p exhibitos-lifecycle --locked --bin exhibitos-native --test native_ui -- -D warnings
EXHIBITOS_NATIVE_TEST_HELPER=/absolute/path/to/target/debug/exhibitos-native swift test --package-path native
```

`scripts/package-native-preview.py` assembles existing built outputs into a new
development app/profile; it never replaces an installation, builds or signs an
artifact, or accesses credentials. Supply the debug Swift executable and built
Rust helper explicitly. The validation-profile Info key is recognized only in
debug builds. Use an isolated profile for validation and retain original apps.
Release delivery/signing is a separate existing distribution pipeline.

## Ordinary furnishing workspace lifecycle

The 037 ordinary path requires one qualified local runtime manifest shared by
the native helper and Swift authoring. It uses a fresh empty installation,
a clean reopen on the same physical root and lock, or an authenticated EXB
restore activation into a separate physical installation. Restore preserves
the logical workspace and owner IDs. Payload changes require a separately
qualified transition.

The current-run record keeps the writer generation and physical binding when
database-only maintenance advances the diagnostic run ID. Before ordinary
startup, the helper durably consumes the prior clean or restored completion
and records the new active generation. Normal stop requires authenticated
completion from the owned Node process, its successful exit, the exact
ledger and zero worker claims while PostgreSQL is still available, and
successful PostgreSQL shutdown under the same writer fence. Interrupted or
incomplete transitions preserve the active state and refuse ordinary reopen.
An EXB source requires either this session's verified writer drain or an
unconsumed clean completion for the same physical root and lock. A database-only
restore may initialize a new empty target or resume the exact pending restore
intent bound to the target lock and PostgreSQL system identifier. The restored
database identifier is checked on the first start and later clean reopens.

If restore preparation fails before a pending restore appears, reopen Backup
and Restore and choose Restore to a New Workspace (새 작업 공간에 복원). This
creates a fresh target and preserves the earlier candidate, original workspace,
and backup. Once a pending restore appears, use Resume Restore with the same
backup and the correct key.

The Manager implementation and Platform wire must be qualified together before
adding a manifest pin. Source regression checks do not qualify actual startup,
owner-loss recovery or EXB restoration.

The publication catalog (038) reuses the ordinary owner exchange with its own
exact payload pin, publication capability and 38-row ledger. Start with an
empty workspace. A clean reopen requires the same payload and physical
installation and checks the complete ledger before launching services.

Create the EXB with the qualified 038 payload and restore it with the same
payload through Restore to a New Workspace (새 작업 공간에 복원). Activation
preserves the workspace, tenant and principal IDs in the new physical
installation. The first reopen checks the activation receipt and restored
PostgreSQL system identifier before starting services. Backup and restore
require writer quiescence and the exact 038 ledger. The Manager and Swift
authoring pin lists stay empty pending qualification of the exact artifact.

For a separately qualified 038 package with the selected-import marker, the
debug v7 entry can import one bounded draft.12 archive in a new workspace.
The writer lease stays live through selected save and settlement. After the
Manager records the completed save, Stop drains the runtime and publishes the
clean proof for an ordinary 038 reopen. An interrupted selected run keeps the
active proof and its import records for inspection.

Workspace material publication (039) requires its own qualified payload and
complete 39-row ledger, including the 038 publication migration. Start from
a new workspace. The debug v7 entry requires the 039 selected-import marker
to import draft.12. After settlement and normal stop, reopen with the same
039 payload. Use that exact payload for backups and restores of a 039
workspace.

Workspace material multimap (040) has its own payload and 40-row ledger.
Initialize a separate workspace for this catalog. The selected debug v7
entry verifies the 040 draft.12 marker and its distinct writer lease mode.
After the selected run settles, reopen with the same qualified 040 payload.
On the local-validation 040 payload with manifest
`72a13c526fb422c106f8eefaa7e277e5ab003f3c2085ebd09b2814c0a19ef904`,
the debug Manager and automation CLI can connect registered floor and wall
materials with color, optional normal and optional roughness maps. Both
require an authenticated owner session and a clean saved draft. Apply the
binding, then save the exhibition. The native viewport uses the same
owner-scoped read routes for these maps.

## Private demo painting preparation

The standalone debug tool accepts only the three pinned official JPEGs. It
verifies source bytes, dimensions, orientation and the exact embedded ICC
profile or its absence. Run one case at a time in a private work directory.

```sh
xcrun swiftc -D DEBUG -O native/Sources/ManagerCore/NativeImagePreparation.swift \
  scripts/prepare-private-painting.swift -o "$PRIVATE_TOOL"
"$PRIVATE_TOOL" met-437394 "$SOURCE_JPEG" "$NEW_OUTPUT_DIRECTORY"
```

Use canonical absolute paths and a new output directory under a separate
mode-0700 parent. Select `met-10158`, `met-437394` or `met-436951` to obtain
1925 × 2634, 2419 × 2552 or 2135 × 1361 respectively. The tool publishes
`full.png` and `receipt.json` together after checking the RGB8 readback. The
encoded limit is 8 MiB.

The Aristotle profile uses one bounded adaptive PNG filter pass and zlib's
best-compression level. The receipt's optional `pngEncoding` object identifies
that encoding and records RGB8 hashes before encoding and after decoding. The
PNG SHA and byte count are in `image`. The separate pixel recipe still uses
nearest-centre sampling and floor rounding. Each output needs its own exact
review before registration or publication.

## Saved workspace runtime transition

The macOS debug helper built with `--features workspace-v5-owner-validation`
supports the reviewed local-validation transition from manifest
`40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415` to
`cfe1faba63dc6d41e2451ad8bf8870d197a48e1b6d234850bdbad16c23083b59`.
Use absolute canonical paths and a stopped, existing managed profile with a
saved Studio draft. Quit the profile owner and verify that Postgres, Node and
the auxiliary process have exited cleanly. Resolve any pending import, upload,
publication, document edit, archive or restore through its owner before staging.
Use a private directory owned by the current user for each external input and
keep the profile, staging and rollback data on the same local APFS volume.
Start with an empty mode-0700 staging directory outside the entire profile and
an unused key path in another private directory outside the profile and staging.
Allow space for two copies of the protected data and 256 MiB plus the manifest
budget.

```sh
exhibitos-native --profile "$PROFILE" --maintain-workspace-v5-transition "$TARGET_RUNTIME"
```

Send version-1 JSON lines on stdin and read one reply per request ID. Replace
the placeholders with actual absolute paths and the `selectionToken` returned
by the context request. The key command creates a 32-byte private key file.
Use a new request ID for each poll.

```jsonl
{"version":1,"id":1,"command":"context"}
{"version":1,"id":2,"command":"native_maintenance","selection_token":"TOKEN","maintenance":{"action":"create_key","path":"/absolute/private-key-dir/checkpoint.key"}}
{"version":1,"id":3,"command":"native_maintenance","selection_token":"TOKEN","maintenance":{"action":"local_personal_transition","acknowledged":true,"transition":{"phase":"stage_saved","sourcePayload":"/absolute/source-runtime","staging":"/absolute/private-staging","keyFile":"/absolute/private-key-dir/checkpoint.key"}}}
{"version":1,"id":4,"command":"native_status","selection_token":"TOKEN"}
{"version":1,"id":5,"command":"native_maintenance","selection_token":"TOKEN","maintenance":{"action":"local_personal_transition","acknowledged":true,"transition":{"phase":"activate","staging":"/absolute/private-staging","keyFile":"/absolute/private-key-dir/checkpoint.key"}}}
```

Wait for `native_status` to report `busy: false` and no `errorCode` after each
phase. Staging prepares and rereads the five protected members
(`native-database`, `blobs`, `config`, `native-personal.before` and
`runtime-env.before`), seals the inventory as `physical-checkpoint.gcm`,
and fences ordinary startup. The staged before-images contain private
plaintext data. Keep the staging directory and key in their original locations
through activation or recovery. If copying stops before the seal is written,
retain the incomplete directory and retry staging with a new empty directory.

Activation starts the target under the fence. It verifies the workspace and
account binding, the saved journal digest, the complete Studio draft and
HTTP/body ETag, and unchanged blob and config inventories after the trial stops.
This covers the saved draft's placements, floor bindings and counts. A rendered
preview, export and other exhibitions require their own qualification.
On a staged or trial error, wait for the processes to drain and send
`phase: "recover"` with the same `staging` and `keyFile` in a transition request.
Recovery requires the pending fence. It verifies the authenticated before-images
and restores the source inventory before ordinary source reopen is possible.
An unconfirmed runtime stop requires owner diagnostics before recovery.
After successful activation, close the helper with EOF and reopen the profile
against the exact target with `--reopen-workspace-v5-validation`.
The completed transition clears the fence. This command set has no post-commit
rollback path.

The isolated V5 wall preview validation also admits a directed saved-profile
transition from the exact `cfe1` manifest to the reviewed `6be2` manifest.
It uses the same authenticated checkpoint and saved-draft trial. The new
payload is admitted for ordinary reopen after the transition.

### Private floor rendering

The reviewed `cfe1` local-validation runtime also admits the existing Quiet
Oak catalogue and its three pinned floor maps. When the authenticated local
session advertises `workspace-material-intake-v1` and the runtime matches a
debug workspace material pin, the native viewport permits the same-origin,
same-tenant material version and color-map routes. The server handles GET,
ownership and display rights. The renderer verifies the descriptor and map
before using it.

In expert mode, open **3D 연결 상세** and select **표시 상태 확인** to
inspect the loaded map status. Check the entrance and the adjacent floor in
**걸어보기**. A `fallback` status cannot qualify the intended material.
The readout reports requests and loaded maps. Visual scale and direction need
observation in the rendered view.

Scene snapshots are returned in-process by the existing live native exhibition
model's `captureSceneSnapshot(camera:placementId:)` method, before the optional
GUI save panel. It requires that model's authenticated, clean saved document and
exact acknowledged WK frame; it is not a renderer-free server or CLI endpoint.
The original `scene-snapshot-v1` saved-viewpoint request keeps its same-room
requirement. The separate `scene-snapshot-inspection-v1` capability accepts a
temporary room-local camera for an existing placement without editing the saved
viewing plan or selection. Shared Viewer geometry validates and resolves that
camera; the native reply binds its exact local pose and saved/frame identities.
Both paths retain the fixed 640×360 PNG, size, rights and single-operation limits.
The opt-in `ManagerHostTests` integration requires a normal foreground
application-hosted runner and an explicitly owned, prepared isolated fixture
(`EXHIBITOS_NATIVE_SNAPSHOT_HOST_FIXTURE`). A standalone `swift test` CLI skips
actual host execution, including when a fixture is supplied: a CLI process may
remain inactive and occluded despite an activation request. Compilation or a
skipped test does not qualify live WK capture, restoration or artwork framing;
these need an actual visible application host and its returned PNG.

`--legal-resources /absolute/prepared-directory` copies an existing license/source
index and its files to `Contents/Resources/NativeRuntime-Legal`. The Help menu's
“라이선스 및 소스 보기” command opens that local `index.html`; it is disabled when
the index is absent. This companion stays outside the runtime inventory. Its
actual corresponding-source/notices contents and owning-app resource seal must
be reviewed and recorded with the delivered app; copying it does not establish
redistribution qualification or change runtime admission.

The native packager copies the approved Manager icon and integrated Studio/CMS
marks from `native/Resources/Brand` into the app's existing Resources directory.
`CFBundleIconFile` names `manager.icns`; the existing screen header uses the
Manager PNG and Studio/CMS SVGs at 32 points or larger through AppKit. The small
system sidebar symbols remain available. Attribution and license copies accompany
these resources, including the rounded MDI floor-plan modification notice. No
external font or brand-generation tool is required by the app. Existing bundles
acquire these assets only when a subsequent package includes them; source changes
alone do not establish package, GUI or distribution qualification.

The native OEX client keeps the existing64MiB export when the authenticated
session does not advertise `nativeOexExportProfile: "bounded-file-v1"`. A
supporting personal runtime can receive up to512MiB into a private snapshot,
with a new request UUID and exact saved ETag. The client verifies the fixed
length, profile, operation ID and SHA256, then waits for that same session's
producer/response/known-stage terminal receipt before exposing a file. An
explicit retry first checks a retained uncertain UUID; it never reposts that
UUID. The receiver's intent is in memory, and runtime loss is not a completed
owner return. The server's unresolved-stage refusal remains authoritative.
Bounded output uses a new destination and never replaces an existing file.
These are implementation boundaries; actual runtime/workspace qualification
and native GUI output evidence must match the delivered payload separately.

Fresh-workspace OEX import preserves the existing026/027 draft.9 mode. A
separately admitted028/v4 payload supports explicit draft.9 and draft.10 under
the same512MiB bounded-file profile. Before saving a new intent, the fixed bundled
helper inspects the already sealed private snapshot through a read-only inherited
FD using Spec's size index. That provisional format is not archive validation or
import authority: selection ACK, job/lease, actual hashed preflight, worker control
and decoded manifest must agree. The028 catalog row/hash is checked independently
before the unchanged catalog ACK. Old capabilities/schemas and failed intents stay
unchanged; draft.10 cannot enter a legacy selected runtime. The additive028 schema
is applied only to a newly selected workspace, and ordinary startup does not
silently upgrade an existing catalog. Native execution and output acceptance
remain specific to the validated source/payload/profile.

The separately admitted Quiet Oak floor supplement retains normal028/v4 and its
existing capability bytes. Its exact additional declaration and029 schema are
required only for the new local payload tuple; the helper independently checks
the owned DB's029 checksum before the unchanged catalogue ACK. Fresh selected
workspaces apply the addition, while ordinary reopen of an older028 catalogue
does not silently apply029. Historical migrations and payload pins remain intact.

`ManagerCore.NativeWorkspaceOEXTerminalProof` verifies Draft 12 terminal
evidence for the bounded workspace material import. Supply the exact UTF-8
proof (up to 8 KiB), exact result (up to 4 MiB), independently held import scope
and selected save offer, held Studio revision 1, and the independently reopened
Studio revision 2 after the save. The verifier checks the fixed proof encoding
and the raw result length and SHA-256 before decoding the native draft. It
rejects duplicate result object keys at any depth, requires the same complete
draft in both Studio revisions, and checks the
remapped color material bindings against matching floor and wall surfaces. The returned value contains proof
and result digests and their validated scope. Owner settlement, transport,
durable local recording and acknowledgement are the caller's responsibilities.
The caller also binds the reopened revision to the exact pending save request
and receipt. The terminal wire data carries the import request ID.
The GUI and CLI import flows have yet to connect to this verifier. A matching
Platform contract must admit wall and mixed imports before those archives can
enter the selected import flow.

The focused Swift tests use synthetic inputs. Set
`EXHIBITOS_DRAFT12_TERMINAL_TEST_FIXTURE` to the exact Platform PR #347 synthetic
fixture at `apps/api/test-fixtures/workspace-oex-terminal-native-v1.json` when
running `swift test --filter NativeWorkspaceOEXTerminalProofTests` to check the
producer's original proof and result bytes. Set
`EXHIBITOS_DRAFT12_WALL_TERMINAL_FIXTURES` to the directory containing the
pinned Platform wall and mixed JSON and raw UTF-8 proof/result files. These
fixtures exercise the producer encoding and the Manager verifier using
synthetic results. Runtime authority and database commits require separate
evidence.

The Manager Rust owner has a Draft 12 recovery witness boundary for the
selected owner protocol. After matching the exact settlement receipt, it checks
the held worker reaping results and retired stage, binds the prior claim and
provider records to the pinned directories, and creates
`native-oex-recovery-<jobId>.json` under the private workspace root. It
syncs the file and directory before sending the settlement receipt ACK. The
32 KiB v1 JSON witness records the attempt and claim, prior generation and
journal/provider identity, database owner, source SHA-256, exact receipt
SHA-256 and r2 save identity. Hash its original bytes for comparison.

The restart check uses an independently held pending transfer and expected
attempt, generation, claim digest and r2 save. It pins the prior generation
under the existing root operation guard before publishing a new run pointer.
Missing, partial or conflicting evidence, a remaining resource anchor, or
replaced directories prevent recovery. Platform must separately verify ACK
acceptance, committed finalization, decoder drain, prior Node termination and
current owner and database fences. The recovery witness is retained for the
future authenticated restart transport.

The debug automation configuration accepts `workspaceV6ValidationRuntime` for
a fresh selected Draft 12 profile once the exact Platform source, producer and
payload manifest are admitted. `import-oex12` writes its completion receipt
after the live save and settlement, normal runtime stop and a fresh helper's
Studio readback. The receipt records both runtime generations and the verified
Studio revision. A failed reopen returns an error and leaves the requested
completion receipt absent. Keep the profile for inspection. The Draft 9 and 10
automation paths use their existing formats and receipt behavior.

The separate `workspaceV7ValidationRuntime` debug setting selects the V7
Draft 12 wall and mixed import protocol once its complete Platform payload is
admitted. Use a fresh profile for the selected import. V7 requires the exact
V6 parent marker, V7 marker and schema 035. The owned PostgreSQL ledger must
match 026–035 for V7 or 026–034 with 035 absent for V6. After settlement,
the existing stop and reopen flow uses the same payload and checks the ledger
before starting services. The reopened helper uses the ordinary session.
Existing profiles keep their original payload binding. The macOS debug helper
built with `workspace-v5-owner-validation` admits one isolated V7 payload. It
requires Platform source `d823896613e27c49d7a3aaeee570a527b2142350`, its
producer SHA-256 `809ae0bd70b5702b325e0607324d53423474b453242e72a222a22fc08e970f34`,
and manifest SHA-256 `aad7ddf920dc64b02b916166e829d9638fca11b585cd0cb9157bc49a0f09e0d9`.
Use this payload in a fresh synthetic validation profile. After ordinary reopen,
the debug authoring client can list and read workspace catalog metadata when the
authenticated native session advertises `workspace-material-intake-v1`.
Intake, map preview and surface editing retain their existing admission checks.
The V6 and release admission tables are unchanged.

Local publication uses its own affirmative `nativePublicationProfile:
"bounded-local-v1"` session advertisement and an explicit per-call profile
header. Ordinary readiness/publish requests retain their existing behavior.
A negotiated pending request commits the selected profile in its existing plan
binding, keeping the same snapshot, ETag and publish request ID on cold retry.
This does not grant watermark support or enlarge the ordinary draft-create limit.

An optional `--runtime-payload /absolute/newly-produced-runtime` embeds the
Platform producer's local validation payload under `Contents/Resources/native-runtime`.
The helper's closed `runtime_payload` query resolves that fixed app-relative path,
verifies its complete declared file set and internal links, and returns byte
identity only. Self-declared release flags never establish signed distribution
admission. The optional legacy installation API retains first-account validation and its
private `runtime.env` contract. New personal workspaces instead use the
Mac-user binding described below; both paths preserve existing account data and
refuse automatic Docker/account installation adoption or damaged settings. Configuration
is distinct from initialized database, service readiness and release acceptance.

`native_runtime::Session` now owns the prepared installation's PostgreSQL and
Node processes under the existing operation fence. It uses fixed payload paths,
a minimal environment, bounded private per-run logs and durable run records.
The PostgreSQL connection must return this run's instance nonce before any
database creation; Node must report its own successful listener startup before
the existing five-component readiness contract is accepted. Stop sends SIGTERM
only to retained child handles, keeps ownership while termination is pending,
and preserves the database/configuration for reopen or explicit retry. Occupied
ports and unknown live PIDs refuse; there is no automatic PID adoption or kill.
The fixed helper now routes `native_prepare`, `native_start`, `native_status` and
`native_stop` through one asynchronous owner. Preparation/start/stop return a
background state; presentation settings remain available while they run. A stop
arriving before startup publishes its cancellation handle is retained. EOF and
SIGTERM/SIGINT trigger owned shutdown before releasing the profile. Oversized or
truncated input also drains owned services before exiting with a protocol error.
The existing core checks are supplemented by actual helper pipe cancellation,
EOF/SIGTERM termination, durable terminal exit records and same-profile reopen.

SwiftUI now provides first-account secure entry, explicit preparation/start,
progress and stop controls, and live native readiness. Successful preparation
starts services only as part of the explicit “prepare and start” action; reopening
never starts automatically. Closing a window keeps the app alive; quitting while
native work is active asks to stop and quit, and completion waits for actual child
termination. Internet-published exhibitions are separate from local shutdown.
The app delegate and button flows compile but require actual GUI qualification;
headless helper checks alone do not prove them.

At the initial lifecycle checkpoint, coverage included the native window, bounded pipe connection, persistent
presentation preferences and actual helper-owned preparation/start/stop. Native
button/quit GUI acceptance, backup/update/recovery parity, integrated artwork/exhibition editing, shared3D
viewport, publication, data migration, complete keyboard/VoiceOver interaction
and final signed delivery remained unfinished. The then-current create/preview/publish
destinations describe missing preconditions and perform no publication. This is
not a Docker-free release or a qualified replacement for the existing Manager.


## Native artwork management (partial authoring)

The Create destination now contains a personal workspace connection, optional
existing-account login, artist creation, artwork
search/pagination and metadata forms, using the existing CMS API. The selected native configuration supplies workspace identity. New personal
workspaces use a private helper-issued session, while optional legacy passwords
remain transient SecureField inputs. Sessions use a private ephemeral cookie jar.
Requests stay on the selected IPv4 loopback origin, refuse redirects, use the
existing CSRF token and bound tenant, limit JSON responses and never log secrets.
Authentication changes are serialized against in-flight requests.

The native editor preserves metadata/rights/provenance fields, sends the existing
revision on PATCH and retains edits on errors. Changing artwork/new-document
requires an explicit discard when dirty; quitting also warns about unsaved
artwork input. Mode/navigation changes share the same in-memory editor. A login
with a different account cannot reuse another account's dirty draft. Durable
unsaved-draft resume, Keychain session persistence and actual interaction
acceptance remain pending.

The actual isolated Swift/API integration creates a synthetic artist/artwork,
updates/reopens its metadata and rights, observes stale-revision rejection,
searches and logs out/in against bundled native PG/Node. This proves the Swift
HTTP boundary for that admin fixture, not native GUI acceptance, every RBAC role,
GLB import/upload/job recovery, server comparison UI, immutable approval,
shared3D/exhibition editing or OEX/publication completion. Those remain part of
the same first release scope.


## Personal workspace without a local CMS account

The default “이 Mac에서 시작하기” prepares a private, durable workspace bound to
its Mac UID, workspace/tenant and internal owner IDs. It creates no CMS password
credential and asks for no username or password. The Rust helper supplies random
per-run session/CSRF values to its owned Node child over bounded inherited stdin;
they are never status/journal fields, environment tokens or command arguments.
Only an explicit private helper request may return the active loopback session to
Swift. Foundation adopts it in an ephemeral cookie jar, then confirms the server
session's tenant/principal through the same authenticated API used elsewhere.
Origin, CSRF, RBAC and revision checks stay enabled. There is no anonymous HTTP
bootstrap or administrator route. Existing installations remain legacy data and
require an explicit connection/migration; they are not silently converted.

The native helper+Swift integration now demonstrates a new personal workspace,
authenticated authoring without `/auth/login`, metadata save, normal service
stop/restart, changed per-run token and same-owner data reopen. Native ownership
mismatch/legacy-file preservation and actual server origin/CSRF/legacy-account
refusal are separate passing checks. This is metadata-boundary evidence only.
Native GUI acceptance, actual GLB/textures/exhibition/OEX/static publication,
durable unsaved drafts, optional migration/backup compatibility and seamless
renewal after the current eight-hour server session are still pending.


## Native GLB file transfer and recovery

The native editor now offers an AppKit file panel and bounded, read-only GLB/PNG
snapshots (1–32 MiB). It refuses nonregular files and final symlinks, detects
source changes during reading, and holds immutable bytes only in Swift memory.
Before creating a server import, the private helper durably saves the exact
intent, hash, length, scale, rights and idempotency key. The record is bound to
the selected personal workspace and principal, revision checked, and contains
neither source paths nor session secrets. Recovery reuses that exact intent and
job. If the original upload already reached the server, the existing completion
API rechecks its durable quarantine bytes and resumes validation without local
file reselection. Missing bytes request the same original; integrity errors stay
visible. No new request, derivative, retry or asset is silently invented.

The native transfer owns and drains its local task across intent persistence,
create, upload and completion. Scope/generation checks reject stale results;
service stop and quit cancel and await the owned transfer and source reader.
A server cancellation response is not proof its worker has drained. Registration
still belongs to the existing server validation/lease/rights transaction, and
artwork approval remains a separate explicit action. This supports the same
uploaded GLB/PNG bytes; original custody for future derived formats and a native
conversion producer remain open. No side vault or backup format is added.

Existing authenticated import APIs perform binary upload, validation, status,
retry/cancel and revision-bound artwork approval. A separate acknowledgement
confirms the saved metadata, asset and rights. The actual isolated Swift/helper/
PG/Node test imported a synthetic 1,516-byte GLB, recovered the same job after a
lost response and service restart, verified approved disk bytes, approved the
artwork, and reopened it. A malformed GLB failed validation, retried and cancelled
without replacing the earlier approved file. Unicode/space bundle paths are
covered. Source-file refusal and transfer-state ownership/corruption/symlink/CAS
checks passed separately. Run `ImportFileTests` for the bounded reader; the
explicit-fixture `GLBImportTests` requires an isolated payload/helper/profile.

Actual beginner GUI also selected the synthetic GLB through an owning-window
file sheet, saved its metadata/rights/provenance, completed validation and explicit
artwork approval, then reopened the same file/rights/import request after app
relaunch. Both GUI-owned service runs stopped normally with Node/PostgreSQL exit0.
The first standalone file-panel candidate was not accessible to the control tool;
that candidate/data were preserved and the panel was changed to a window sheet.

This supersedes the earlier GLB client-only gap in the tested slice. Textures, shared3D viewport,
exhibition editing, OEX/static publication, complete accessibility and operational
migration/backup/update acceptance remain pending. It does not qualify a release.


## Native exhibition documents and retained save requests

The exhibition destination now lists accessible Studio documents and edits title,
room name/dimensions and approved CMS artwork placement using SwiftUI controls.
The document retains opaque extension fields, original approved artwork transforms,
rights and inventory. Every added placement gets the same plain-text accessibility
alternative as the web editor. Platform's complete OES/Studio validators remain
in force; the initial missing-alternative failure was retained and fixed in the
native construction, without relaxing server admission.

One current valid draft, base ETag/saved edit version and exact pending save are
stored privately by the selected Mac workspace's Rust controller. The state is
principal/workspace bound, revision checked, owner checked, size bounded and
flushed before a POST/PUT; it contains no credential, source file path or asset
bytes. The inherited IPC frame bound is now3MiB (document store2.2MB), while
upload intent128KiB and JSON response2MiB bounds remain separate. Corrupt or linked
records are refused and preserved. A changed or unacknowledged request is never
silently assigned another ID. Known conflicts preserve the native input; a saved
version is read for comparison and applied only by explicit selection/confirmation.

Actual Swift/helper/PG/Node integration saved and recovered an unacknowledged
POST after relaunch, placed a real synthetic GLB, saved/reopened revision2,
refused stale If-Match and changed request payloads, and restored an unsent valid
local draft separately from the saved server version. Actual beginner GUI restored
that draft, changed title/room/position, saved revision3, compared and reopened it.
The GUI test's list row center only focused its Button row; the latest source uses
native List selection binding instead (compiled, not requalified in that candidate).
All owned native service runs stopped with Node/PostgreSQL exit0.

The current editor is partial: multiple-room/surface/lighting controls, shared3D
viewport/direct manipulation/undo, OEX/static publication, durable raw invalid
form input, full keyboard/VoiceOver/expert and migration/backup/update qualification
remain unfinished. Locally retaining valid input is an explicit button, not a
promise that every in-progress keystroke survives an unexpected exit. The server
list's query-admission tests passed; full cross-role/tenant HTTP acceptance remains
separate from the tested personal-admin flow. This is not release qualification.

The first native light inspector source adds list-based selection, existing
point/spot/directional/area inputs, room-local position, linear-RGB color,
renderer intensity, full beam angle in degrees and area size. General mode
disclosures expose the same position/color fields as Expert mode. Text stays in
the shared exhibition model when the inspector hides or the mode changes.
Preview requires the optional `light-inspector-v1` request and the existing
shared full validator; the old v1 frame selection and geometry action remain
closed. Exact reply/base/input/account binding precedes local journal Apply and
the existing Undo/Redo, server save and export paths. Opening unsupported data
refuses editing rather than replacing its values with defaults. Units follow
the existing producer; changing between different units uses its default input
of100 rather than a photometric conversion. Fixture shapes, direct3D movement,
aiming, Kelvin, calibrated illuminance and full lighting release acceptance are
separate. The fixture selector now uses the already optional
`light-fixture-aim-v1` shared validator for none, spot can, disc and panel.
It preserves existing aim, Kelvin intent, range, softness, other bindings and
opaque base-light fields, and uses the same preview/Apply/Cancel/Undo/save paths.
Only the shared validator admits generated geometry and room clearance. Missing
capability or unsupported selected metadata keeps the existing fixture unchanged.
This slice does not add aim/K editing or certify optics. Source and synthetic unit coverage do not establish actual GUI,
save/reopen, OEX/Viewer/static consumer or distribution qualification.

The native bench producer uses the optional `bench-inspector-v1` bridge with
list-based selection independent of the strict v1 frame selection/action kinds.
Slab/backed benches expose room-local position, yaw in degrees, seat dimensions,
and the existing InstallationMaterial (sRGB hex and PBR factors). General mode
disclosures expose every Expert numeric field from the same model buffer. A new
bench can atomically create one fresh standard Presentation viewpoint even in an
empty exhibition. Editing a camera creates a fresh viewpoint and relinks the
bench; existing/shared viewpoints are preserved. Removal retains the camera.
Full shared validation and exact source/result/account/input binding precede
Preview, explicit Apply and one existing journal/history entry. Cancel discards
only this form's uncommitted preview. Save, reopen and outputs use the existing
exhibition paths; native code does not remap public/OEX identities or implement
seated physics independently. The shared Viewer owns explicit Sit/Return and
runtime clearance. Source/unit verification does not establish actual native
GUI, saved-output consumption, seated operation or release acceptance.

The native plaque producer references existing exhibition title/introduction,
room name or a same-room artwork placement. It stores no copied prose, fonts,
URIs or assets. List selection is independent of the strict v1 frame kinds.
One model buffer holds source, visibility, size, position and quaternion text;
General disclosures expose all Expert numeric fields and the same four facing
presets. Hidden records stay validated and retained. The optional
`plaque-inspector-v1` bridge uses full shared source/result validation and exact
binding/delta checks before transient Preview and explicit Apply. One existing
journal/history entry carries the change through normal save, reopen and
exports. Removal changes only the plaque namespace and the existing template
marker; original source text, artwork and unrelated rows/extensions are kept.
Native GUI and saved-output/host continuity remain separate qualification.


### Shared 3D viewport development slice (2026-10-23)

The SwiftUI editor embeds only Platform's dedicated shared GeometryPreview viewport,
with a nonpersistent WK cookie store and per-view private session. Navigation and
content rules pin the loopback origin, native viewport page, compiled assets and
scoped approved-artwork projection/preview paths. Nonce/sequence/owner checks reject
stale callbacks. Native proposals retain all nongeometry fields and persist through
the existing local document CAS; they do not publish or bypass save validation.

Actual development GUI has displayed one approved synthetic GLB with zero failed
assets. Initial module readiness is awaited before the first frame. Frames travel as JSON
text to avoid Foundation container identity affecting empty JS collections. Strict CSP is
preserved; Platform precompiles its existing AJV schemas instead of adding runtime
eval permission. This is partial evidence: native white-cube generation, undo/redo, light addition
and server save revision2 passed in actual GUI. The original Foundation object bridge
aliased empty collections (actual WK regression: [1,1,1,1]); JSON transport preserves
[1,0,0,0] and fixed the geometry rejection without relaxing schema admission. Read-only
preview loaded the approved GLB, but the closed cube occludes it from the current
outside camera in that candidate. The camera/transparency gap is superseded by
the following scoped development acceptance; full accessibility and recovery remain. It is not
release qualification or a substitute for OEX, publication, accessibility and recovery.


The dedicated viewport frame now includes a bounded camera request sequence and a
space-transparency flag (12 fields, packaged Manager/Platform pair). Repeated view
buttons issue actual camera actions; native transparency defaults on and only
changes rendering, preserving saved materials and candidate bytes. The dedicated
page memoizes its material lookup by document, so camera/selection frames retain
the renderer instead of resetting it. Existing web/Viewer consumers retain their
uncontrolled camera and transparency behavior.

Actual r21 beginner GUI passed top/isometric/front camera changes, a placement gizmo
drag reflected in native X and durable draft (2.0538089134331527m), white-cube6,
opaque/transparent switching, save revision2 and visible read-only front preview
of the displaced approved GLB. Loaded1/failed0. Expert mode persisted, but Computer
Use disconnected at selection and stayed unavailable after rebind/reset, so expert
rendering/reverse transition is unqualified. With the document saved/no pending
request, exact owned-app normal SIGTERM led to helper EOF and Node/PostgreSQL exit0.
At that checkpoint OEX/static publication, comprehensive selection/rotation/dimensions/keyboard/
VoiceOver/mode/relaunch UX, durable raw input/history/view preferences, session renewal,
and operational/distribution compatibility remain incomplete.


### Native OEX export development slice (2026-10-23)

The exhibition editor and preview now offer an owning-window NSSavePanel for OEX.
Uncommitted native input or an uncertain save disables export with save-first guidance.
The private Foundation client sends the existing authenticated, Origin/CSRF-protected
POST with the exact saved ETag, and never refreshes a conflicting ETag automatically.
The existing Platform service still verifies current approvals, export rights, source
bytes and complete OEX profile. No anonymous API, parallel archive format or web UI
wrapper is introduced.

Binary response admission rejects redirects, wrong MIME/encoding, declared or streamed
sizes over64MiB, changed session generation and truncated declared content. The client
checks ZIP transport magic only; Platform/public-contract validation remains authoritative
for the archive. Transfer/resource timeout is bounded120s; ordinary JSON request idle
limit stays15s and JSON response bound2MiB. User-selected output is written atomically on a
worker task after validation; canceling the native save sheet keeps the document.

Actual core export produced6816bytes from the existing native GLB/geometry fixture;
public OEX validation and exact1516byteGLB SHA passed, with CMS bindings removed and
geometry/placement preserved. Actual staleETag412, no-export-rights403, isolated-client401,
and missing-directory file-write refusal passed without changing the qualified source.
Loopback transport exercised wrong MIME, redirect and both declared/streamed64MiB bounds.

Actual r23 beginner GUI passed dirty-title gate, save revision2, native save-sheet cancel,
retry from preview, new5360byteOEX save, completed-path/Finder affordance and independent
public-contract/GLB-byte verification. Expert r22 observation crashed SkyComputerUseService
(Array.remove assertion), with expert preference/source retained; a fresh beginner profile
isolated the GUI test. Expert/full accessibility remains unqualified. All owned PG/Node
runs stopped normally. Output-location/hash feedback is memory-only; static publication,
full mode/relaunch/recovery and final release/distribution acceptance remain incomplete.


## Optional exhibition lighting settings (pending shared runtime)

The native `space-settings-v1` adapter edits the shared, exhibition-wide
`org.exhibitos.studio/space-lighting` namespace through the negotiated
`window.exhibitosViewport.spaceSettings` command. It is disabled when the shared
viewport does not advertise that capability. The existing `light-inspector-v1`
contract remains limited to its original light-row change.

General and Expert share the same model-owned raw input and baseline value;
Expert also exposes reflection directly. General can disclose the same reflection
field. Existing unset documents retain their previous rendering until an explicit
edit. Both values must be entered when creating an override or selected through an
explicit preset button. The shared D1 comparison qualified calm (0.15/0.2) and
bright (0.2/0.45) baseline/reflection mappings; these are display-response
conveniences, not photometry or human visual acceptance. Preset names are derived
from the same two values, never persisted. No legacy-to-new default mapping is
assumed. Unsetting submits `settings:null`.
Transient preview and cancel preserve the saved document, and the existing one-item
history restores the original namespace absence and template provenance.

This native source does not establish shared runtime capability, renderer changes,
GUI, OEX/static acceptance, mobile performance or visual quality. Those
boundaries require the matching shared implementation and scoped actual evidence.

### Built-in furnishing catalog

A Manager installation with the matching native runtime includes four CC0
furnishings and their license notices. In the exhibition editor, select a bench,
light, opening, or artwork placement, then open **비품 외형**.
Select the compatible furnishing, preview it, and apply it. Save the exhibition
to keep the selection when you reopen it.

| Catalog ID | Version | Select in the editor |
| --- | --- | --- |
| `backless-oak-bench` | 1 | Bench |
| `museum-track-spot` | 1 | Light |
| `gallery-open-portal` | 1 | Opening |
| `rectangular-plinth` | 1 | Artwork placement (for a pedestal visual) |

The bundled runtime serves these files to Manager over its authenticated local
connection. They are available without downloading assets or importing GLBs
into the workspace. The Manager catalog uses its existing localized labels.

With a Platform producer that supports `defaultFurnishingsRoot`, stage the
[default furnishings v1.0.0 release](https://github.com/ExhibitOS/default-furnishings/releases/tag/v1.0.0)
at commit `4ac7d23c1b557d8e303ad04d78379bcefab50b40` and pass the staged
checkout as that option.
The release manifest SHA-256 is
`f11ba6f4407e31531230cdb94921d7d7baef3caca49b1859f8696fa1a39c4530`.
Retain the producer receipt with the packaged runtime. The four models and four
notices match the existing catalog identities and bytes.

### Workspace furnishing registration (source integration)

The Manager client has a separate workspace furnishing catalog for pedestal,
bench and, on supported runtimes, stool GLBs. It requires an exact qualified native payload and an authenticated
ordinary session advertising `workspace-furnishing-intake-v1`. The current
source has an empty payload admission list. The existing bundled catalog
continues to use its own IDs and files.

In a qualified workspace, open **작업 공간 비품 등록·배치** from the exhibition
settings menu. Choose a GLB up to 32 MiB and enter its name, provenance and rights.
Reopen the registration records to resume the same request. If a receipt is
missing, reselect the file with the same byte count and SHA-256. A receipt allows
validation or a permitted retry with the server's copy. After registration,
select the catalog version and room, enter the room-local position, angle and
scale, then save the exhibition. The placement records a visual model and pose.

The GUI offers **스툴** when the authenticated session also advertises
`workspaceFurnishingStoolProfile: workspace-furnishing-stool-v1`. The first
stool placement upgrades that scene's workspace furnishing extension to
version 2 while retaining existing descriptors and placements. A scene using
only the existing furnishing kinds can continue using extension version 1.
The descriptor version stays 1. Export a version 2 scene as OEX draft.14.
Selected import retains its existing draft.12 format.

When the same session also advertises
`workspaceFurnishingRoomVisualProfile: workspace-furnishing-room-visual-v1`,
the GUI and CLI accept `partition`, `info-stand`, `barrier` and `display-case`.
Placing the first of these visual models upgrades the scene extension to
version 3 and retains existing descriptors and poses. Existing version 1 and
2 scenes keep their versions until a new kind is placed, and later edits to a
version 3 scene retain version 3. Version 3 scenes export as OEX draft.15.

In that authenticated workspace, the 3D editor can read the registered
version's display check and model on the current loopback origin and tenant.
The viewport keeps these same two URL exceptions during saved-scene capture.
Platform's read routes check session ownership, display rights and model
integrity. Intake and catalog writes remain outside the viewport.

The private native automation command uses the same intake and catalog API:

- `workspace-furnishing-intake-create` takes `kind`, `name`, `rights`,
  `provenance` and `model` (`mime`, `bytes`, `sha256`). The envelope request
  ID is the idempotency key. Reuse the same declaration if the create reply is lost.
  Use `kind: "stool"` only in an authenticated session with the stool capability.
  The four additional kinds require the room visual capability.
- `workspace-furnishing-intake-list` and `workspace-furnishing-list` take
  `cursor` (null for the first page). Intake `status`, `upload`, `complete`,
  `cancel` and `retry` take `intakeId`. Upload also takes the configured
  digest-pinned `inputPath` in the automation envelope.
- `workspace-furnishing-read` and `workspace-furnishing-model-check` take
  `id` and `version`. The model check reports byte count and SHA-256.
- `workspace-furnishing-place` takes `id`, `roomId`, `furnishingId`,
  `furnishingVersion`, `position` (metres), `yaw` (radians) and `scale`.
  Supply the expected document revision and edit version, then save normally.

The planned live qualification uses a newly created workspace and a separate
fresh EXB recovery workspace on the same paired payload. An existing V7 profile
keeps its current schema and payload. Viewer, OEX and static output qualification
are separate from these intake and placement source checks.

The dormant native path follows the private v3 owner hello with one bounded v1
writer lease and acknowledgment. Each frame authenticates the mode, payload,
workspace and runtime identities, and the root and lock file device and inode.
The writer descriptor is probed before inheritance. Native readiness and
credential issuance require the authenticated acknowledgment and live channel.
Existing selected OEX frames use their existing protocol. Platform validator
inheritance and the opt-in claim recovery schema require separate qualification
before a furnishing payload can be admitted.

### Optional furnishing catalog (source integration)

`furnishing-visuals-v1` adds one protected catalog entry, the CC0 canted display
pedestal, to an existing selected artwork placement. The native inspector keeps
position, angle and uniform scale as raw text shared by General and Expert modes.
Browsing does not edit the exhibition. An explicit shared `furnishingVisuals`
preview is checked for its exact request binding and permitted delta before Apply
creates one existing native history entry. Removing the visual keeps its legacy
conservative box; Undo restores the prior namespace, including absence.

Catalog metadata is checked against the immutable source/version/hash/rights.
`furnishingThumbnail` has its own catalog/hash identity and bounded 256px PNG;
artwork thumbnail and import authority are unchanged. Full GLB, room/world hull,
support, OEX/static inventory and rights admission remain with Platform. The
native adapter does not advertise support, ship a new renderer or generate asset
quality tiers. A runtime without the negotiated capability retains the legacy
editor. The selected visual locks legacy pedestal configuration until removal;
existing separate center/on-top actions use shared measured support. Chair/bench
visuals and automatic artwork movement are unsupported in this slice.

Source tests cover correspondence and raw input only. Matching qualified runtime
artifacts and native save/reopen/output/consumer actual evidence are required
before claiming this feature complete.
