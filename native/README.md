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
headless preview. Supply the debug Swift build and native helper. The preview
keeps the Swift binary without execute permission and leaves the app launch
entry unset. Use the bundled helper for isolated headless work.

For all local Manager GUI validation, including recipient source builds, use
`scripts/manager-validation-gui.py` under the normal desktop resource lease.
Supply the intended existing profile and the reviewed `--gui-sha`,
`--helper-sha`, and `--runtime-manifest-sha` pins to every action. Use
`prepare` with the original `--gui`, `--helper`, and `--runtime` paths
when the fixed bundle needs updating. The launcher retains the previous
bundle and verifies the app and native helper against their approved
designated requirements before reuse.

To include a reviewed Help/source tree, pass `--legal-resources DIR`,
`--legal-inventory FILE`, and `--legal-inventory-sha SHA` to `prepare`. The
inventory lists each `path`, `bytes`, and `sha256`, including `index.html`.
Preparation checks the exact tree and copies only listed files. The signed
bundle binds the inventory SHA alongside its build, runtime and profile pins.
Pass that same `--legal-inventory-sha` to `verify`, `launch`, and `attach` for
the prepared bundle. The inventory is limited to 2 MiB, 4,096 files and 1 GiB
of resources. Source correspondence and distribution review remain separate.

Run `launch` once. It verifies the fixed bundle before opening it through
Launch Services and returns the owned PID. For CUA, run `attach --pid <PID>`
with the same pins and confirm that the exact bundle is already running in
the app inventory before selecting it. If the process disappeared, inspect
the result and use the launcher for any next start. Confirm the same PID
again after attaching. Stop owned work and quit normally before another
prepare or launch.

An authenticated existing-workspace automation session can stage one artwork
rotation with the version 1 `artwork-placement-rotate` operation. Supply the
current document and edit revisions in the ordinary envelope. The command
accepts the placement ID, the saved ETag and a normalized xyzw quaternion.
Studio checks the candidate against that ETag before the native journal stages
the edit. Use the existing `save` operation to commit it, then
`reopen-saved` to read it back. This operation adds no fields to the draft or
journal formats.

## Authenticated scene and thumbnail CLI

The installed app exposes `ExhibitOSManager --authoring-cli --profile
ABSOLUTE_PATH --output-root ABSOLUTE_PATH`. Quit the ordinary app first. Use an
existing managed personal profile and a separate owner-only output directory
on the same volume. The entry uses its bundled helper and verified runtime;
it does not accept an arbitrary helper, runtime, document or session token.
Send one version1 JSON request per line and wait for its response. Reuse the
workspace/tenant/principal/capability from `ready`, and the current document
revision and edit version for every modifying request. EOF closes and drains
the owned runtime.

On an empty personal workspace, `exhibition-new` with `command: {title}` stages
the existing typed blank exhibition. It requires a current authenticated native
create role and refuses any existing draft, saved head, pending save or saved
edit marker. It accepts no raw draft or imported selection. Use a distinct
stable `save` request UUID to create the server exhibition; creation and Save
remain separate. This does not replace an existing exhibition or waive the
selected/bounded GUI's new-document guard.

`pedestal-prepare` accepts `command: {placementId, pedestal}` using the existing
closed installation pedestal fields. `light-prepare` accepts either
`command: {light, settings?}` for creation (a full existing light row with a
caller-stable UUID), or `{lightId, type, color, intensity, position,
beamAngle?, areaSize?, colorIntent?}` for update. Updates preserve existing
fixture/aim/range settings and the supported color intent. New scene proposals
require the live `saved-head-scene-v1` capability. `plaque-prepare` retains its
existing typed command and separate `saved-head-v1` capability.

`fixture-prepare` takes `command: {lightId, pose}` for an existing spot light
and the built-in `museum-track-spot` catalogue version 1. Pose contains
`position`, `yaw` and `scale`; optical alignment requires yaw zero and position
equal to the negative catalogue aperture centre multiplied by scale (at unit
scale, `[0, 0.15, 0.0755]`). `light-attachment-prepare` takes
`command: {lightId, attachment: {surfaceId, anchor: [x, y], gap}}` for an explicit
ceiling in the same room and an existing point, spot or area light. Anchor uses
surface coordinates; gap is 0.01–1 metre along surface +Z. Attachment recomputes
the light pose from the surface transform while preserving its aim/settings.
Both operations require the distinct live `saved-head-fixture-v1` profile and
the existing `saved-head-scene-v1` profile. Their matching `-apply` and `-cancel`
operations take the envelope `proposalId`.

Prepare validates the complete saved head and returned candidate. Apply stages
the local draft; explicitly Save each change before preparing the next one.
Attach to the ceiling before adding the visual when possible. Reversing that
order is supported only when the complete result, including fixture bounds,
remains valid. An exact existing binding returns the unchanged candidate and
apply preserves the original native document; skip Save for that no-op. This
slice accepts no wall attachment, removal, passive rail or arbitrary catalogue.

Each prepare returns a checked candidate and `proposalId`; the corresponding
`pedestal-apply`, `light-apply` or `plaque-apply` stages it in the native journal.
Pass `proposalId` at the envelope level. Cancel uses the corresponding `-cancel`
operation. Ordinary `save` commits the staged edit; `reopen-saved` reads the
durable result. A proposal expires when its session/base changes. No command
replaces a raw draft or writes the database directly.

A pedestal position is its base center; setting it does not move the artwork.
Model placement uses the centered metric artifact, not the original GLB's base
origin. In the identity-pose, unit-scale case, place the model center at
`pedestalTopY - normalizedModelMinY`. For a support top of `0.2` and measured
normalized model minimum Y of `-1.5225000381469727`, the center Y is
`1.7225000381469726`. Use actual metric geometry bounds, not descriptive catalogue
dimensions or a new unit assumption. The existing generic SDK's
`artwork-placement-move` changes the placement separately, followed by explicit
Save; pedestal configuration and artwork positioning remain separate edits.

`thumbnail-prepare` takes `command: {artworkId, artworkRevision}` on a saved
exhibition. A temporary, app-managed WebKit host invokes the existing product
256px model-card renderer and returns `candidatePath`, `previewPath` and
`pngSha256`. This requires macOS with the supported WebKit/GPU host and a live
rendered-thumbnail capability; it requires no manual GUI input or capture flag.
Preparation does not select a thumbnail or save the exhibition. Review the PNG,
then explicitly send `thumbnail-select` with
`command: {candidatePath, expectedPngSha256}`. `thumbnail-status` accepts the
same command and reads the current server selection. Candidate files bind the
workspace, owner, source and rendered bytes and can be reopened from the same
private output directory after restart. Partial/missing previews are recovered
only from those verified candidate bytes; foreign files are refused.

For an uncertain selection response, retain the same operation request UUID,
candidate and selection payload. After restarting the CLI, refresh the outer
ready capability and current document revision/edit version; do not reuse an
expired session envelope. The server performs the existing source/revision
and selection-version comparison and request replay checks. Selection returns
the receipt and checked binding readback, separately from exhibition Save or
export. Source tests and compilation alone do not qualify a packaged runtime,
actual WebKit rendering/cancellation, durable consumption or Viewer output.

## Rectangular portal registration

The native furnishing form can explicitly request `rectangular-portal-v1` for
an untextured rectangular portal. The control requires the qualified ordinary
portal runtime, the existing source-bound fit capability and the distinct live
`workspacePortalRectangularFitProfile` advertisement. A modular advertisement
alone does not authorize this profile. Older sessions keep the existing path.

The request, resumed intake and registered source-fit receipt retain the same
profile, request identity and original model hash/bytes. Connecting the registered
model stages one opening attachment; ordinary exhibition Save remains separate.
The version7 descriptor reuses the existing geometry fields and source receipt.
The pinned default1.6 library also exposes its pale mineral and warm oak portals
through the existing local file selection. Their source files and CC0 notices
are checked before selection; registration and connection still use the same
capability-gated workflow. Earlier1.2/1.4/1.5 catalogs remain supported.

Source tests and compilation do not qualify a new runtime, GUI execution,
clean installation or exported Viewer geometry.

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

The same saved-owner maintenance entry admits the exact catalog 045 transition
from manifest `f738f33c777de39629b6a0ab2dd3a477b45d078b738189e173d00d2cd7ab652a`
to `1209d5d95417d1e29f637d8ce327e78cf38ed1b32eabcb5ab6fd3e2a62a10203`.
Both packages use the same SQL ledger. This pair requires a clean source and
uses a 4 GiB protected checkpoint ceiling. The target can reopen through
the existing ordinary saved-workspace entry after a successful activation.

The isolated V5 wall preview validation also admits a directed saved-profile
transition from the exact `cfe1` manifest to the reviewed `6be2` manifest.
It uses the same authenticated checkpoint and saved-draft trial. The new
payload is admitted for ordinary reopen after the transition.

The same debug maintenance commands support two directed catalog 40 frame
validation steps. The first goes from manifest
`df6efc23ea2d9aab790fc25fab9833db9f5b48b3cb0c5417a1bd7c5ef609ac2a`
to `c42e9657015695601ffb0c4ea493ae9ae6df9c954fce032db3212a1b20d309ee`.
From that manifest, the second step goes to
`2645f013b8dcef5bb33f138dc0fac9789bbb508306cca722112f222a7476e871`.
For each step both payloads must match the exact pinned source commit and producer and
have the same catalog 40 SQL ledger. The source must have a saved draft,
an unconsumed clean writer completion and the original profile root,
writer lock and logical owner. Pending edits, imports, uploads, publication
and restore must settle before staging. Use the target runtime for
`--maintain-workspace-v5-transition` and the source runtime for
`stage_saved` as above. The trial uses the normal owner admission and
shutdown, and verifies the saved Studio draft before committing the target
binding. After successful activation, use the regular helper with the
exact target runtime for subsequent starts of this profile.

If a drained trial fails, use `recover` with the same staging directory and
key. Recovery restores the source's data, runtime binding and writer
completion while retaining the trial run for diagnosis. A restart before
the first possible spawn can recover from the durable starting record.
A failed trial that ran processes needs the target run's drain record and
no live writer or database process. After recovery, retry by staging again
from the clean source. An unconfirmed stop keeps the transition fenced. This path is for the two reviewed local validation
payloads for each step and the current catalog. It does not support
reversing or skipping a step.

The debug helper has a separate occupied-catalog upgrade path for 039 to
040. It uses the same `stage_saved`, `activate` and `recover` commands,
with the reviewed local validation runtime manifests
`caf4d4d25245cfddbae8db2b80595250b3f08baceb715c66855a46537a88ef63`
as source and
`bdfc8202d851f9f8983440b9475da3fc3ddc6ee1f0bf4cf2c0d4bf1995dde338`
as target. Catalog 40-to-40 transitions use their existing unchanged-ledger check.

The occupied upgrade pins the stopped source and its saved Studio draft
in the checkpoint. The trial checks the full 039 ledger before starting
Platform, which upgrades the existing database under the writer lock.
Activation requires a verified 040 ledger, the same owner and saved draft,
and normal drain and shutdown. A completed upgrade opens normally using
the exact target runtime. If activation is interrupted, the pending
checkpoint blocks ordinary start. Recover the authenticated 039
before-images only after proving no process was started or the trial
drained safely. Retry from the restored 039 source in a new trial.
An unproved stop or a 040 database with a pending checkpoint is not
eligible for another upgrade attempt.

The same debug maintenance commands admit one explicit occupied upgrade from
the Wheat catalog 040 runtime manifest
`074719930770137f954302e1d27f0035b4d25f6489fa4e61391d96f6d25d90f1`
to the ordinary catalog 044 runtime manifest
`c0565112569fc1b1b95f8ada72b2b7cacabc8ea3e24b58a61dc9aa138c8bb406`.
Staging requires the exact 040 ledger, clean writer completion, saved draft
and original owner and writer lock. The trial uses separate authenticated
furnishing and Hayes owner channels. Activation verifies the complete 044
ledger, saved draft and unchanged blob and config inventories. After the trial
stops cleanly, reopen with the exact 044 runtime through the ordinary helper
or Manager app. The trial starts no Hayes import. Use the same checkpoint and
key for recovery if activation fails and the trial has drained.

### Ordinary Hayes original intake

Use the exact ordinary 044 runtime above with a saved owner workspace.
Mount the existing qualified Hayes worker image read-only. Manager discovers
the single verified worker on a mounted volume before the job is claimed.
Select the original GLB in the artwork's existing file controls, register it,
and confirm the saved artwork and rights before preparing the display file.
Artwork approval is a separate action after Manager verifies release of the
original and display file custody.

The native automation commands `hayes-original-queue` and
`hayes-original-prepare` use the same upload journal and owned operation.
`hayes-original-prepare` accepts an explicit `workerRuntime` path for a
verified read-only mount when the caller supplies one. On reopening the
same workspace, use `hayes-original-release` to read the confirmed receipt
for the persisted job before `artwork-approve`. The app performs that read
when it restores an approved file job. If the release is unconfirmed, keep
the job and its journal for inspection.

For current catalog 040 service EXB, the same saved-workspace commands
admit a directed refresh from
`bdfc8202d851f9f8983440b9475da3fc3ddc6ee1f0bf4cf2c0d4bf1995dde338`
to
`ebbf0aeabf013f5071151580495c9c7e5ff6812cbd995769b5e347e01ae92c0b`.
Reopen and stop the source normally before staging if a prior backup tool
failed. Both payloads have the same verified 040 SQL ledger. Stage the
source, activate the reviewed target, and reopen with that target before
creating a service EXB. Restore the authenticated archive using the same
target payload and a fresh physical profile.

An occupied 040 source on the reviewed `ebbf0aea` runtime can refresh to
`7f76b19894fe3a9824a640cf1856ba7c828d5bd3201ede0da5d1c53a501bd1e7`
using the same checkpoint commands and unchanged SQL ledger. Use the new
payload to create and restore a service EXB. If the authenticated backup's
import provenance all belongs to its source database, the activated target
can reopen those imported exhibitions across restarts. New imports use the
target's current database identity. An archive with mixed historical
database identities remains outside this provenance admission.

### Compact variant of an approved model

The local-validation runtime with manifest
`a7cab3643e4b6554c70ef4c1fda7bfb2ce5234273dead44711be9f72df9ff34c`
admits a checkpointed upgrade from the retained Hayes catalog 044 payload
`af9c141f071d84ba4e9397798759e1cb13b2cde4d350f2fcf0a4b852ce3a09b5`
and a same-catalog refresh from the retained required-model 045 payload
`af4e787a5fa5c714d60bf24cba23a1b2e7015946d1f4bea9a5777a3af3c8b2d0`.
Use the exact source and target with a clean existing owner workspace. The
normal checkpoint operation verifies the source ledger and saved document
before activating the target. Stop the trial cleanly and reopen the target
before preparing or replacing a model.

An authenticated owner session must advertise `exhibitos-budget-geometry-pair-v1`.
In the approved artwork’s file controls, start compact preparation from its
current full model. The runtime reads the already approved bytes and uses its
packaged compact producer. Manager retains the exact request and job in its
local transfer journal, reads status again after reopening, and offers retry
after a failed job drains or cancellation before approval. Approving a ready,
drained job is a separate action. It creates the next artwork revision with the
same full primary asset and one coarse model. Clearing the local transfer
requires confirmed cancellation and drain or fresh approval readback.

In native automation, use a clean saved document context. Each command supplies
`artworkId` and `input` with the exact approved `revision`, `assetId`,
`sha256`, `bytes`, and caller-retained `idempotencyKey` and `generation`
UUIDs. `compact-model-prepare` durably binds that request to the returned job.
`compact-model-status`, `compact-model-retry`, `compact-model-cancel`, and
`compact-model-approve` also require its `jobId` and the identical request.
These commands take no local file paths and perform no client upload.

In the exhibition editor, select the existing placement and apply the approved
compact variant. Automation uses `compact-model-refresh-placement` with the
same request, job and `placementId`. Manager stages the next approved snapshot
and keeps the placement ID, room, transform and full asset. Use the ordinary
exhibition save, reopen and export controls to persist and verify it. Render
quality and the Viewer GPU budgets are verified separately.

### Larger GLB originals in the isolated 046 workspace

A freshly prepared personal workspace with the exact selected 046 validation
runtime can prepare GLB sources over 32 MiB and up to 64 MiB. Manager uses the
authenticated v2 model capability and its dedicated upload route. PNG, JPEG
and the existing v1 GLB path keep their existing 32 MiB source limit. The
v2 geometry limits are 400,000 triangles and 262,144 vertices, with up to
48 MiB of embedded image bytes. The runtime validates the encoding and retains
the exact source while producing an approved-display candidate of up to 8 MiB.
The artwork still requires separate rights acknowledgment and approval.

The clean 046 packet can reopen the same isolated workspace after a normal
shutdown. A lost runtime with a retained worker claim needs the separate
interruption recovery binding.

### Original GLBs with unknown source units

In a session advertising `bounded-compact-source-inspection-v1`, choose
**단위 미확인 GLB 검사…** in the artwork’s provenance controls. The runtime
checks the selected file’s structure and coordinate bounds. Enter an authored
provisional display height and apply it before saving artwork information.
Keep the original units unknown. Structure inspection makes no claim about
physical scale, orientation, floor contact or rendered appearance.

Automation uses `artwork-create` with the usual `artistId` and `metadata`,
plus `provisionalHeightMeters` and an allowed, digest-pinned GLB `inputPath`.
Set metadata source units to `unknown` and `scaleApplied` to false. Manager
constructs the reference from the inspection and chosen height. Continue
with `import-glb` using that same file, saved revision and `scaleMeters: 1`.
Wait for a ready, drained job before the separate `artwork-approve` command.
Approve using the source revision and actual prepared asset ID. When the
prepared bytes differ, the result advances the artwork revision and binds the
reference to the prepared asset hash. Use the returned revision when
registering the artwork or saving and exporting a placement.

For a failed or cancelled, drained required-model import, generic automation
can use `artwork-model-preparation-discard` with `{artworkId, jobId}` and the
current document revision envelope. It cancels a failed job through the normal
API, verifies cancellation and worker drain, then clears only the matching local
transfer journal. Server records and artwork identities remain available. A
replacement file uses a new import request ID; retry continues the original file.

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

### Stage finalized macOS release inputs

For ordinary image preparation with catalog 047, build `exhibitos-native` in
release mode with `EXHIBITOS_FRESH_IMAGE_RELEASE_PRODUCER_SHA256` and
`EXHIBITOS_FRESH_IMAGE_RELEASE_MANIFEST_SHA256` set from the reviewed Platform
finalizer success receipt. Verify its `sourceCommit`, `producerSHA256` and
`manifestSHA256` against the finalized runtime manifest before building. Cargo
compiles the two hashes into the helper. The app obtains admission through the
helper's private owner session and requires the live API capability before
enabling image preparation. The helper still validates the official app and
helper signatures, runtime inventory and managed owner binding at launch.

Use Python 3.11 or newer. On the clean Manager checkout used for the release
build, assemble the already-built non-debug app and helper with
`scripts/package-native-preview.py --release-receipt RECEIPT --app APP
--swift-executable APP_BINARY --rust-helper HELPER_BINARY --runtime-payload
RUNTIME --legal-resources SOURCES --legal-inventory SOURCES_INVENTORY`.
Use absolute paths and a new `.app` destination. The v1 JSON receipt records
`managerCommit`, `managerTree`, `appSHA256`, `helperSHA256`,
`runtimeManifestSHA256` and `legalInventorySHA256` alongside `version: 1`.
Use the independently recorded build and finalized runtime digests. The legal
inventory stays outside the copied tree and lists `path`, `bytes` and `sha256`
for every resource, including `index.html`.

The assembler verifies both inventories before and after copying and emits a
receipt for the unsealed app. The 1.25 GiB stage limit includes the finalized
runtime, bundled source resources, executables and app metadata. The assembler
also checks available disk space before copying. The development DMG limit is
separate. The existing Developer ID app/helper and full-resource validation
still applies at installed launch. The Help item uses the bundled index.

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

After editing and saving an exhibition, use a new `export-static` request ID
with the current revision, edit version and an unused output path. A finished
export to another destination does not prevent the new export. Repeating the
same request in its session reuses its result. A different active export must
finish before the new export can start.

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

The installed [image CLI](../docs/native-image-cli.md) uses the same personal
workspace, artwork approval and exhibition save. It requires a bundled runtime
that admits ordinary image preparation.

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
Artwork plaques support a 150 × 100 mm minimum. Room and exhibition plaques
keep the 200 × 150 mm minimum. Existing version 3 artwork caption translations
can carry optional single-line `medium` and `dimensions` fields, up to 96 UTF-16
code units each. `dimensions` holds the museum-recorded original dimensions
separately from the scene size. Existing caption rows can omit both fields.
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

### Room audio

In the native exhibition workbench, save the exhibition and open **공간 소리**.
Select a room, then choose a WAV from the exhibition's approved audio catalog,
**없음** to clear its soundtrack, or **기본값** to leave the room track unset.
Set the first-visit defaults for background music, footsteps and room reverb
independently. Apply the room settings and save the exhibition. A visitor's
previous choices still take precedence.

The private automation operation `room-audio-apply` stages the same edit on a
clean, ordinary saved draft. Pass `expectedRevision` and
`expectedEditVersion` in the request envelope, and `etag`, `roomId` and
`settings` in its command. Include the room's existing `reverb` value in
`settings` (zero for a room with no prior row). An asset selection is
`"bgm": {"assetId": "<approved audio ID>"}`, and `"bgm": null` means no
soundtrack. Omit `bgm` to leave it unset. Each of `bgmOn`, `footstepsOn`
and `reverbOn` accepts a boolean or can be omitted to inherit the default.
After applying, use `save`, `reopen-saved` and the normal export command.

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

When the qualified session advertises
`workspaceArtworkFramePreparationProfile: workspace-artwork-frame-preparation-v1`,
selecting one of the three reviewed original artwork-frame GLBs enables server
preparation during registration. The source GLB stays the upload and resume
file. Registration history shows the recipe and both SHA-256 digests. Use the
registered model's digest when attaching the frame. Other GLBs follow the
ordinary registration flow.

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
  In a session with artwork-frame preparation, add
  `"preparationPolicy": "omit-unused-invalid-tangent-r1"` to the command for a
  reviewed original `kind: "artwork-frame"` GLB. Declare the original model and
  upload the same digest-pinned original. Keep the policy and declaration when
  replaying the request ID. A registered response includes
  `preparation.sourceModel`, `preparation.recipe` and
  `preparation.renderableModel`. The registered `furnishing.model` matches the
  renderable model. Omit the policy for direct registration.
- `workspace-furnishing-intake-list` and `workspace-furnishing-list` take
  `cursor` (null for the first page). Intake `status`, `upload`, `complete`,
  `cancel` and `retry` take `intakeId`. Upload also takes the configured
  digest-pinned `inputPath` in the automation envelope.
- `workspace-furnishing-read` and `workspace-furnishing-model-check` take
  `id` and `version`. The model check reports byte count and SHA-256.
- `workspace-furnishing-place` takes `id`, `roomId`, `furnishingId`,
  `furnishingVersion`, `position` (metres), `yaw` (radians) and `scale`.
  Supply the expected document revision and edit version, then save normally.
- When the authenticated saved exhibition policy advertises
  `workspace-bench-seat-v1`, select a registered bench and its existing
  unit-scale placement in **작업 공간 비품**. Enter the seat dimensions and
  choose a same-room viewpoint or author a new one. Apply the seat and save
  the exhibition. The seat uses the existing model and placement.
  The equivalent `workspace-bench-seat-attach` command takes `etag`, `benchId`,
  `placementId`, `furnishingId`, `furnishingVersion`, `modelSha256`,
  `dimensions` (`width`, `depth`, `height` in metres), and `viewpointId`.
  To add a viewpoint in the same edit, supply the optional `viewpoint` with
  that ID, `name`, `roomId`, `position`, `target` and `fov`. Supply the
  expected document revision and edit version in the envelope. The command
  returns `changed` and the local document state after server validation.
  Reuse the bench ID and any fresh viewpoint ID for the same intent. An
  identical saved seat returns `changed: false`. Run `save` and `reopen-saved`
  before exporting. After an uncertain save, use the existing pending
  request ID and journal to recover the same save.
- In a qualified session with the artwork frame capability, register a
  supported GLB as `kind: "artwork-frame"`. The original capability admits
  thin brass. The additional `workspace-artwork-frame-models-v1` profile
  admits the ordinary models recognized by Platform. Save the draft with
  the approved artwork already placed. `workspace-frame-attach` takes
  `placementId`, `artworkRevisionId`, `assetId`, `furnishingId`,
  `furnishingVersion` and the registered descriptor's `modelSha256`. It can
  replace the attachment at the same placement. `workspace-frame-detach`
  takes `placementId`.
  Supply the expected document revision and edit version. After the server
  validates the change against the saved ETag, run `save` and `reopen-saved`.
  In the native app, select the placed artwork and open the workspace
  furnishings panel to make the same change, then save the exhibition. The
  first attachment or replacement with an additional model upgrades the
  furnishing extension to v5 and selects OEX draft.17 on export. Later edits
  retain v5 while the draft has furnishing rows.

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

The authenticated automation session exposes `plaque-prepare` and
`wall-finish-prepare` for the existing inspector commands. Each requires a
clean local draft matching the authenticated saved Studio head, the current
local revision and edit version, and a `command`. The server validates the
proposal against that saved head without writing it. The session retains the
checked preview and returns its proposal ID and candidate for inspection.
`plaque-apply` or `wall-finish-apply` accepts only the proposal ID and current
revisions; it rechecks the saved head and stages the retained preview. Use
`save` and `reopen-saved` before preparing another edit. `plaque-cancel` and
`wall-finish-cancel` discard a pending proposal without changing the draft.
Caller-supplied producer replies are not accepted.

## Rendered artwork thumbnail validation payload

The debug owner-validation helper and ordinary Swift authoring entry admit the
exact rendered artwork thumbnail payload `3e05e41286c517d5304299bdd987b8e5b6fe08ab8396dee3804feeb0f0c4f670`
from Platform source `77bf7d1435a9ee248eeb19c164b6062db5fbaa5d`. Its only new
saved-workspace transition starts at the qualified terminal-disposition payload
`e41df146d7097fd6964564de3d2f82b4bb766dba5377ec7f938896955fea39b0`.
The transition preserves the physical installation, authenticated owner and
original carried-restore proof; it grants no new direct restore or reverse
transition. The runtime remains local validation, not a signed release.

The existing artwork thumbnail action prepares one bound rendered candidate,
shows its PNG for explicit confirmation and retains that candidate for uncertain
retries. Runtime admission and source checks are separate from actual official
workspace transition, thumbnail selection, reopen and export qualification.

The corrected canonical-model/optional-thumbnail composition payload
eac64ed106bdac283c1aa0c6c3dee5541c98919ed4a67bd9e5509b21496b5bce
from Platform b0c02301c39b85a75ca576e3dca56274ccd92c28 has one new transition
from preceding payload 3e05e41286c517d5304299bdd987b8e5b6fe08ab8396dee3804feeb0f0c4f670
with the same carried-restore protections. Original-export cancellation does not
reset a consumed export claim: the existing successor path requires a distinct
request and new destination while retaining the saved head and prior attempt.
Exact admission is separate from actual successor-output qualification.


## Atomic workspace info-stand replacement (source slice)

The generic automation SDK adds `workspace-info-stand-replace` for an explicitly
selected existing stand and artwork plaque in the same room. Its closed command
is `{etag, modelSha256, intakeId, placement, plaque}`. `placement` has the existing
room-placement fields (`id`, `roomId`, `furnishingId`, `furnishingVersion`,
`position`, `yaw`, `scale`); `plaque` has only `id`, `position`, `rotation` and
`dimensions: {width, height}`. The existing placement/plaque IDs and room stay
fixed; plaque source, caption and visibility are preserved. No stand/plaque
relationship is inferred from proximity.

`workspace-info-stand-check` accepts the same closed command and revision/CAS
envelope. It runs the same current authorization, descriptor, model and viewport
validation, rechecks that the local document stayed exact, and returns
`{changed, document, prospectiveDraft}` without staging or saving. Replaying a
check repeats current validation; its result is not a durable admission receipt.
This generic SDK operation is not added to the installed authoring CLI allowlist.
It does not establish camera visibility, legibility or physical fit.

The operation requires a clean saved head and the live
`workspaceInfoStandReplacementProfile: workspace-info-stand-replacement-v1`
marker alongside existing workspace room-visual admission. It rechecks the
registered descriptor, intake, applicable preparation approval, actual model
bytes and current server viewport validation before staging one local edit.
Use ordinary explicit `save` with current revision/CAS afterward. Exact no-ops
stage nothing. Old server sessions without the marker refuse this operation.
Only a now-unused old descriptor is removed from the scene snapshot; registered
versions in the workspace remain untouched. Shared descriptors and unrelated
scene rows retain their order and values.

Synthetic source tests do not qualify a new bundled runtime, actual intake,
registration thumbnail, replacement Save, viewport rendering or physical mount.
The existing GUI/WK furnishing registration thumbnail route remains separate;
this operation does not generate or select a thumbnail.


## Registered furnishing thumbnail capture (source adapter)

The generic SDK `workspace-furnishing-thumbnail-render` command accepts exactly
`{id, version: 1, modelSha256}` plus the ordinary document revision/edit-version
scope. It uses a clean saved frame, the exact current workspace session and
registered descriptor, and the product's furnishing-specific transient WK
renderer. It rechecks the saved frame, session, descriptor and time-bounded
display rights before returning a canonical 256×256 PNG as bounded base64 with
its digest and source/request binding. It does not upload/select an artwork
thumbnail, change the draft or save an exhibition. Furnishing authority does
not require the unrelated rendered-artwork-thumbnail profile.

The isolated renderer allows only existing bound-tenant furnishing reads under
the viewport resource policy. Cancellation/timeout requests stop, page reset
and teardown; they do not prove asynchronous WebKit callback or GPU-process
drain. Synthetic tests and source compilation do not qualify an actual WK
capture or release of a live rendering resource lease. Actual capture must use
the qualified app/runtime identity and the ordinary resource/owner handoff.


## Default furnishing 1.7 runtime recipient (source admission)

The debug owner-validation helper and Native client recognize only the exact
Server source `0de658cd2ce28e766ad4d9a05a912eb43c082691`, producer
`7b6890fae09bc3d40963a16d8bab2a040cc431023a7952ff2bb4f429da83543c`
and manifest `1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738`
tuple. The new sealed transition admits only the existing fixture runtime
`5b3aadfe5332bdaf084741f2b9a20b0e9c6733c44be36cec7f4d1ddb8ba338f2`
as predecessor, preserving the current saved document and exact carried restore
provenance under the existing 6 GiB checkpoint scope. Reverse, skipped and
same-manifest transitions are not added.

The runtime pin does not replace live scene, fixture or info-stand replacement
profiles. Source admission and offline opening do not qualify a live transition,
registered furnishing intake/thumbnail/replacement, the corrected painting
lighting output or public viewing. The previously failed export remains failed;
a later observed export of its already frozen publication is a distinct scoped
operation with preserved failure evidence and the existing safe API trace.

### Registered furnishing thumbnail candidates

The optional `workspaceFurnishingThumbnailProfile` session value
`workspace-furnishing-thumbnail-v1` adds reviewed thumbnail candidates and a
separate representative-image selection. The Native 050 consumer source keeps
one exact local-validation tuple: source `55e8ad6c861eab44326ee8db60db2692e3f7baf7`,
producer `02a2e7eb9630eedad593edc04e6365ab263ee9f637d1aff994ba7dc093c356c5`,
manifest `2c29a8ffe2a56395dd1cdc1b0acd93e4a1623e8aac8b0e10aca30369e60b4626`.
The assembled payload and reader delta have independent review. This does not
qualify an official runtime or authorize a workspace transition.

The scoped authoring adapter exposes `workspace-furnishing-thumbnail-prepare`,
`-candidates`, `-candidate`, `-image`, `-selection`, `-select`, and
`-selected-image` (each with the full prefix). Prepare accepts the closed command
`{capture, pngInputPath}`: `capture` is the existing six-field renderer capture,
and the PNG path must be present with its SHA-256 in the automation input map.
The captured Native revision, edit version and ETag are provenance; unrelated
scene edits do not require rendering another identical registered-model image.
Live owner, registered descriptor, model bytes, render recipe and display rights
are rechecked. Preparation does not select, stage a scene edit, or save one.

Read commands use `{id, version:1}`; candidate/image add `candidateId`. Selection
adds `candidateId`, `receiptSha256` and `expectedSelectionVersion`; the envelope
request ID is the selection nonce. A successful recorded POST is exactly the
requested selection version plus one. A later GET supplies current selection,
which can differ during replay. Preserve the same request, CAS and PNG after an
uncertain response; do not invent a replacement nonce as an automatic retry.

When admitted, the ordinary furnishing-card menu opens **대표 이미지 확인·선택**.
Ready images can be reviewed before explicit selection. Selected cards load the
server-selected, hash-checked PNG after reopening; ordinary card reads do not
create candidates. Existing runtimes retain their transient preview behavior.
Actual candidate admission, selection/reopen and GUI qualification remain
separate checks against the assembled payload.

The stand inspector also accepts a one-purpose closed replacement proposal
`{etag, modelSha256, intakeId, placement, plaque}`. It focuses the target artwork
before showing a cached prospective stand/plaque frame. Cancel discards that
preview; Apply stages one checked candidate, and exhibition Save remains a
separate action. The saved-scene snapshot guard is unchanged. Physical fit and
ordinary GUI acceptance require actual target-camera checks before applying.

Before dispatching a GUI thumbnail mutation, the existing Native owner store
persists one bounded private intent per personal workspace. It retains the
original capture metadata and canonical PNG, or the issued candidate and original
selection nonce/CAS. The journal uses its own revision and does not change the
exhibition document. Reopening the sheet or app reads that intent and refreshes
registered descriptor and display authority before continuing the same request.
A ready capture can be reconciled by its original renderer request ID; an
uncertain selection replays its original mutation nonce. A known result whose
local acknowledgement failed only retries the local journal completion.

The journal is optional in existing backups and is restored byte-for-byte only
for the same owner; absent legacy journals stay absent. Pending intent blocks
personal-runtime transition. These source paths and focused tests do not claim
actual application-termination, current-data transition or backup/restore
qualification against the assembled runtime.

## Coupled light-object source correspondence

The source negotiates `nativeLightObjectAuthoringProposalProfile:
saved-head-light-object-v1` alongside the existing saved-head scene profile and
the editable viewport capability `light-object-v1`. Older receivers retain the
existing light-list inspector. A `{kind: "light", id}` viewport selection opens
that same inspector; generic transform controls are not used for light objects.

The closed shared commands are `set-light-object` (whole light/settings and an
optional attachment), `set-light-fixture` (a known catalogue visual or explicit
null), `duplicate-light-object`, and `remove-light-object`. Bindings use the
existing flattened six-field request envelope. Replies contain those exact six
fields plus the complete candidate and the authoritative `lightId`; clone IDs
come from the server. The consumer checks the whole graph delta, retained order,
shared assets, no-ops and unrelated metadata before accepting a candidate.
Changing a procedural `fixture` setting to `none` does not remove an existing
GLB visual. Explicit `set-light-fixture` with null is the invisible-light action.

The installed authoring SDK source adds `light-object-prepare`,
`light-object-apply`, and `light-object-cancel`, using the existing proposal
cache, expiry, saved-head comparison and revision checks. Preparation and GUI
preview require a clean saved head. Apply stages one complete local edit with
ordinary undo; exhibition Save remains explicit. GUI clone/delete, numerical
mount/detach and catalogue selection use this same bounded proposal path.
No aim/fit optimizer, automatic relocation or contextual artwork fitting is
included in this unit.

The exact five-record catalogue snapshot is generated from Platform source
`48daa7b5033c20b2b3aaa0df09a08c45df68c985`. The washer retains original source
`d85d9d1823c49bef412a23a59e238104f5b8ad342d8e8149efaf8d1110dc3290`,
independently produced model
`a4b66294c77e206a574905e12680148129f914d368add947f1c8831c64bf3f4d`,
and the current distribution notice. Public provenance uses the existing
`omit-unused-tangent-v1` omission recipe; the immutable producer receipt retains
the stricter preparation policy and tool/source/output binding. Existing four
catalogue/asset records are unchanged. Explicit catalogue IDs distinguish the
washer from the track spot and reject an unknown or wrong-host descriptor.

This source slice does not update the default 1.7 runtime admission above or
establish a matched 1.8 runtime, actual GUI application, physical aperture/mount
fit, Save/reopen/export or Viewer qualification. Those require their own exact
runtime and observed consumer evidence.


## Room base-light placement intent (source consumer)

The optional `basePlacement` light-details field records `auto` or `manual`
only on an existing `purpose: "room-base"` binding. Its absence remains
unknown and is never inferred from position, brightness or colour. New
canonical creation and the explicit placement action require the authenticated
`roomBaseLightPlacementProfile: "room-base-light-placement-v1"` marker in
addition to the existing room-base profile; this source adds no runtime pins.

The installed authoring CLI reuses `light-object-prepare`, `light-object-apply`
and `light-object-cancel`. Its closed `command` is
`{"type":"set-room-base-light-placement","lightId":"UUID","basePlacement":"auto"}`
(or `manual`), inside the existing owner/revision envelope. Preparation is not
Apply or Save. Automatic restoration clears that light's surface attachment,
aim and placement target, and uses the room-local point recipe. Brightness,
including zero, colour, identity and unrelated graph fields remain exact.
Manual or unknown lights and missing/removed base lights are not reflowed by
ordinary room edits. No per-room copy or new generic room-edit CLI is added.

The native inspector shows automatic, manual or unrecorded placement separately
from brightness and colour. Placement changes use the existing transient
light-object preview, explicit Apply, Undo and separate exhibition Save. The
ordinary room-row editor checks the exact automatic-light reflow alongside its
existing geometry correspondence; the six-surface envelope operation remains a
separate shared command.

Range is the existing50m-capped shared recipe
`min(50,max(0.1,Math.hypot(width/2,depth/2,height*0.9)))`. Source tests pin
Node24.21.0 golden values, using scaled compensated summation in Swift.
Quaternion representation equivalence follows the shared semantic helper:
exact position/scale and normalized quaternion sign equivalence within1e-12;
no tolerance applies to the returned candidate or other fields.

This is source correspondence only. Native compilation, tests, matching runtime
admission, GUI lifecycle/Undo, Save/reopen and output qualification must be
reported from their actual runs. No existing profile, database or runtime is
migrated by opening a document with this field.

### Corrected current-image thumbnail catalog051

The qualified debug runtime adds only `051_current_image_thumbnail.sql` to the
existing45 migrations (46 ledger rows, catalog identity51). Native admits its
exact source/producer/manifest tuple through an explicit Frame74→051 saved
workspace transition. The sealed owner helper uses lease mode11 and admission
kind10, preserves earlier wire codes and carried original-model custody, checks
the exact ledger before and after migration, then drains and exits before a
fresh ordinary reopen. Ordinary reopen never applies this migration implicitly.
The existing thumbnail request, ready receipt and separate reviewed selection
operations remain unchanged. Source and package qualification do not establish
an actual profile transition or successful thumbnail jobs.

### Source-bound miter frame intake

The debug SDK admits `straight-miter-ring-v1` only for the qualified Frame74
runtime manifest and an authenticated furnishing editor advertising the existing
frame-model and `source-bound-fit-v1` profiles. Intake sends the original model
and the requested profile. Platform measures and validates the mesh; Native
checks the returned fit's source identity, closed geometry and Spec draft.31
bounds, without rewriting the model or using a model hash allowlist.

The existing frame attach/detach commands carry that registered descriptor in
furnishing envelope9, retaining room placements and portal attachments. Removing
the last miter frame returns the remaining graph to the applicable legacy
5/6/7 envelope. Reserved envelope8 remains unsupported. Explicit save, reopen
and bounded OEX download use the existing operations; Platform selects OEX
draft.21 for envelope9. Selected archive import support is unchanged. These
source paths require separate actual intake/save/reopen/export and Viewer
qualification before claiming acceptance of a real frame.
